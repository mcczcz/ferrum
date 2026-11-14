#include "ferrum/EventApi.h"
#include "I18n/I18n.h"
#include "ferrum/FerrumModManager.h"
#include "ll/api/event/EventBus.h"
#include "ll/api/event/player/PlayerChatEvent.h"
#include "ll/api/event/player/PlayerDisconnectEvent.h"
#include "ll/api/event/player/PlayerJoinEvent.h"
#include "ll/api/service/Bedrock.h"
#include "ll/api/utils/ErrorUtils.h"
#include "mc/legacy/ActorRuntimeID.h"
#include "mc/world/level/Level.h"
#include <map>
#include <type_traits>
#include <variant>

namespace ferrum {
template <class F>
static FerrumStatus eventGuard(F&& f) noexcept {
    try {
        return f();
    } catch (...) {
        auto detail = ll::error_utils::makeExceptionString(std::current_exception());
        ferrum_host_set_last_error({reinterpret_cast<uint8_t const*>(detail.data()), detail.size()});
        return FerrumStatus_CPP_EXCEPTION;
    }
}

using EventView =
    std::variant<ll::event::PlayerJoinEvent*, ll::event::PlayerDisconnectEvent*, ll::event::PlayerChatEvent*>;
struct EventScope {
    ActorRuntimeId mPlayer;
    EventView      mEvent;
};
struct EventRoute {
    std::weak_ptr<FerrumMod> mOwner;
    ll::event::ListenerPtr   mListener;
};
static std::map<uint64_t, EventRoute>               sListeners;
static thread_local std::map<uint64_t, EventScope*> sScopes;
static thread_local uint64_t                        sNextScope = 0;

template <class E>
static FerrumStatus
attachEvent(std::shared_ptr<FerrumMod> const& owner, ListenerHandle handle, EventPriority priority) {
    auto listener = ll::event::Listener<E>::create(
        [handle, weakOwner = std::weak_ptr<FerrumMod>{owner}](E& event) {
            (void)eventGuard([&] {
                // 必须先核验主线程；非主线程既不访问事件实体，也不授予令牌。
                if (!ferrum_host_is_main_thread()) {
                    auto text = tr("event.wrong_thread");
                    if (auto mod = weakOwner.lock()) mod->getLogger().error(text);
                    ferrum_host_set_last_error({reinterpret_cast<uint8_t const*>(text.data()), text.size()});
                    return FerrumStatus_WRONG_THREAD;
                }
                EventScope scope{event.self().getRuntimeID().rawID, &event};
                auto       id = ++sNextScope;
                sScopes.emplace(id, &scope);
                auto status = ferrum_host_dispatch_event(handle, id);
                sScopes.erase(id);
                return status;
            });
        },
        static_cast<ll::event::EventPriority>(priority),
        owner
    );
    // 先保存再注册，异常时移除，避免留下不能由宿主注销的 LL 监听器。
    auto [it, inserted] = sListeners.emplace(handle, EventRoute{owner, listener});
    if (!inserted) return FerrumStatus_ALREADY_EXISTS;
    try {
        if (!ll::event::EventBus::getInstance().addListener<E>(listener)) {
            sListeners.erase(it);
            return FerrumStatus_UNSUPPORTED;
        }
    } catch (...) {
        ll::event::EventBus::getInstance().removeListener(listener);
        sListeners.erase(handle);
        throw;
    }
    return FerrumStatus_OK;
}

static FerrumStatus
subscribeEvent(ShimModCookie cookie, ListenerHandle handle, EventSubscriptionV1 const* sub) noexcept {
    return eventGuard([&] {
        if (!sub) return FerrumStatus_NULL_POINTER;
        auto owner = *static_cast<std::shared_ptr<FerrumMod>*>(cookie);
        switch (sub->kind) {
        case EventKind_PLAYER_JOIN:
            return attachEvent<ll::event::PlayerJoinEvent>(owner, handle, sub->priority);
        case EventKind_PLAYER_DISCONNECT:
            return attachEvent<ll::event::PlayerDisconnectEvent>(owner, handle, sub->priority);
        case EventKind_PLAYER_CHAT:
            return attachEvent<ll::event::PlayerChatEvent>(owner, handle, sub->priority);
        default:
            return FerrumStatus_UNSUPPORTED;
        }
    });
}
static FerrumStatus unsubscribeEvent(ShimModCookie cookie, ListenerHandle handle) noexcept {
    return eventGuard([&] {
        auto it = sListeners.find(handle);
        if (it == sListeners.end()) return FerrumStatus_STALE_HANDLE;
        auto owner = *static_cast<std::shared_ptr<FerrumMod>*>(cookie);
        if (it->second.mOwner.lock() != owner) return FerrumStatus_PERMISSION_DENIED;
        // LL 派发快照可能仍持有 listener；宿主句柄已失效，不会再进入模组闭包。
        ll::event::EventBus::getInstance().removeListener(it->second.mListener);
        sListeners.erase(it);
        return FerrumStatus_OK;
    });
}
template <class F>
static FerrumStatus withEvent(uint64_t id, F&& f) noexcept {
    return eventGuard([&] {
        auto it = sScopes.find(id);
        return it == sScopes.end() ? FerrumStatus_STALE_HANDLE : f(*it->second);
    });
}
static FerrumStatus eventPlayer(uint64_t id, ActorRuntimeId* out) noexcept {
    return withEvent(id, [&](EventScope& scope) {
        if (!out) return FerrumStatus_NULL_POINTER;
        auto level = ll::service::getLevel();
        if (!level) return FerrumStatus_SERVICE_UNAVAILABLE;
        auto* actor = level->getRuntimeEntity(::ActorRuntimeID{scope.mPlayer}, false);
        if (!actor || !actor->isPlayer()) return FerrumStatus_NOT_FOUND;
        *out = scope.mPlayer;
        return FerrumStatus_OK;
    });
}
static FerrumStatus eventCancelled(uint64_t id, FfiBool* out) noexcept {
    return withEvent(id, [&](EventScope& scope) {
        if (!out) return FerrumStatus_NULL_POINTER;
        return std::visit(
            [&](auto* event) {
                if constexpr (requires { event->isCancelled(); }) {
                    *out = event->isCancelled() ? FfiBool_TRUE : FfiBool_FALSE;
                    return FerrumStatus_OK;
                } else return FerrumStatus_INVALID_ARGUMENT;
            },
            scope.mEvent
        );
    });
}
static FerrumStatus cancelEvent(uint64_t id) noexcept {
    return withEvent(id, [&](EventScope& scope) {
        return std::visit(
            [](auto* event) {
                if constexpr (requires { event->cancel(); }) {
                    event->cancel();
                    return FerrumStatus_OK;
                } else return FerrumStatus_INVALID_ARGUMENT;
            },
            scope.mEvent
        );
    });
}
static FerrumStatus chatMessage(uint64_t id, FfiByteWriter* out) noexcept {
    return withEvent(id, [&](EventScope& scope) {
        if (!out) return FerrumStatus_NULL_POINTER;
        auto* event = std::get_if<ll::event::PlayerChatEvent*>(&scope.mEvent);
        if (!event) return FerrumStatus_INVALID_ARGUMENT;
        // 先复制，writer 回调可以重入；不持有底层字符串借用跨越模组代码。
        auto text = (*event)->message();
        return out->write_fn(out->context, reinterpret_cast<uint8_t const*>(text.data()), text.size());
    });
}
void installEventApi(FerrumShimApi& api) {
    api.event_subscribe    = subscribeEvent;
    api.event_unsubscribe  = unsubscribeEvent;
    api.event_player       = eventPlayer;
    api.event_is_cancelled = eventCancelled;
    api.event_cancel       = cancelEvent;
    api.event_chat_message = chatMessage;
}
} // namespace ferrum
