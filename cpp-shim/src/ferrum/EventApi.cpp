#include "ferrum/EventApi.h"
#include "ferrum/FerrumModManager.h"
#include "ll/api/event/EventBus.h"
#include "ll/api/event/command/ExecuteCommandEvent.h"
#include "ll/api/event/command/ServerCommandRegisterEvent.h"
#include "ll/api/event/entity/ActorHurtEvent.h"
#include "ll/api/event/entity/MobDieEvent.h"
#include "ll/api/event/io/ConsoleOutputEvent.h"
#include "ll/api/event/player/PlayerAddExperienceEvent.h"
#include "ll/api/event/player/PlayerAttackEvent.h"
#include "ll/api/event/player/PlayerChangePermEvent.h"
#include "ll/api/event/player/PlayerChatEvent.h"
#include "ll/api/event/player/PlayerConnectEvent.h"
#include "ll/api/event/player/PlayerDestroyBlockEvent.h"
#include "ll/api/event/player/PlayerDieEvent.h"
#include "ll/api/event/player/PlayerDisconnectEvent.h"
#include "ll/api/event/player/PlayerInteractBlockEvent.h"
#include "ll/api/event/player/PlayerJoinEvent.h"
#include "ll/api/event/player/PlayerJumpEvent.h"
#include "ll/api/event/player/PlayerPickUpItemEvent.h"
#include "ll/api/event/player/PlayerPlaceBlockEvent.h"
#include "ll/api/event/player/PlayerRespawnEvent.h"
#include "ll/api/event/player/PlayerSneakEvent.h"
#include "ll/api/event/player/PlayerSprintEvent.h"
#include "ll/api/event/player/PlayerSwingEvent.h"
#include "ll/api/event/player/PlayerUseItemEvent.h"
#include "ll/api/event/server/ServerStartedEvent.h"
#include "ll/api/event/server/ServerStoppingEvent.h"
#include "ll/api/event/service/ServiceEvents.h"
#include "ll/api/event/world/BlockChangedEvent.h"
#include "ll/api/event/world/FireSpreadEvent.h"
#include "ll/api/event/world/ServerLevelTickEvent.h"
#include "ll/api/event/world/SpawnMobEvent.h"
#include "ll/api/service/Bedrock.h"
#include "ll/api/utils/ErrorUtils.h"
#include "mc/legacy/ActorRuntimeID.h"
#include "mc/world/level/Level.h"
#include <map>
#include "mc/deps/nbt/CompoundTag.h"
#include <optional>
#include <type_traits>

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

struct EventScope {
    std::optional<ActorRuntimeId> mActor;
    bool                          mPlayer{};
    std::optional<CompoundTag>    mTag;
    std::optional<std::string>    mPayload;
    std::optional<std::string>    mChat;
    std::optional<bool>           mCancelled;
    std::function<void()>         mCancel;
};
struct EventRoute {
    std::weak_ptr<FerrumMod> mOwner;
    ll::event::ListenerPtr   mListener;
};
static std::map<uint64_t, EventRoute>               sListeners;
static thread_local std::map<uint64_t, EventScope*> sScopes;
static thread_local uint64_t                        sNextScope = 0;

static FerrumStatus dispatch(ListenerHandle handle, EventScope& scope) {
    if (!ferrum_host_is_main_thread()) return FerrumStatus_WRONG_THREAD;
    auto id = ++sNextScope;
    sScopes.emplace(id, &scope);
    auto status = ferrum_host_dispatch_event(handle, id);
    sScopes.erase(id);
    return status;
}

template <class E>
static void capturePayload(E& event, CompoundTag& data) {
    event.serialize(data);
    if constexpr (std::is_base_of_v<ll::event::ServiceEvent, E>) {
        if (auto const& service = event.service()) {
            auto id = service->getServiceId();
            data.putString("serviceName", id.name);
            data.putInt64("serviceVersion", static_cast<int64_t>(id.version));
        }
    }
}

template <class E>
static EventScope snapshot(E& event, bool includePayload) {
    EventScope scope;
    if (includePayload) capturePayload(event, scope.mTag.emplace());
    if constexpr (std::is_base_of_v<ll::event::ActorEvent, E>) {
        scope.mActor  = event.self().getRuntimeID().rawID;
        scope.mPlayer = std::is_base_of_v<ll::event::PlayerEvent, E>;
    }
    if constexpr (std::is_same_v<ll::event::PlayerChatEvent, E>) scope.mChat = event.message();
    if constexpr (requires { event.isCancelled(); }) scope.mCancelled = event.isCancelled();
    return scope;
}

template <class E>
static FerrumStatus
attachEvent(std::shared_ptr<FerrumMod> const& owner, ListenerHandle handle, EventSubscriptionV1 const& sub) {
    auto listener = ll::event::Listener<E>::create(
        [handle, includePayload = sub.include_payload != FfiBool_FALSE](E& event) {
            (void)eventGuard([&] {
                if constexpr (std::is_same_v<E, ll::event::ConsoleOutputtingEvent>
                              || std::is_same_v<E, ll::event::ConsoleOutputtedEvent>
                              || std::is_base_of_v<ll::event::ServiceEvent, E>) {
                    CompoundTag tag;
                    capturePayload(event, tag);
                    auto payload = tag.toSnbt(SnbtFormat::Minimize);
                    std::string message;
                    if constexpr (requires { event.message(); }) message = event.message();
                    auto bytes = [](std::string const& text) -> FfiStr {
                        return {reinterpret_cast<uint8_t const*>(text.data()), text.size()};
                    };
                    FfiBool cancelled = FfiBool_FALSE;
                    if constexpr (requires { event.isCancelled(); }) {
                        if (event.isCancelled()) cancelled = FfiBool_TRUE;
                    }
                    FfiBool cancel = FfiBool_FALSE;
                    auto status = ferrum_host_dispatch_thread_event(
                        handle, bytes(payload), bytes(message), cancelled, &cancel
                    );
                    if constexpr (requires { event.cancel(); }) {
                        if (cancel) event.cancel();
                    }
                    return status;
                }
                if (!ferrum_host_is_main_thread()) return FerrumStatus_WRONG_THREAD;
                auto scope = snapshot(event, includePayload);
                if constexpr (requires { event.cancel(); }) scope.mCancel = [&event] { event.cancel(); };
                return dispatch(handle, scope);
            });
        },
        static_cast<ll::event::EventPriority>(sub.priority),
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
            return attachEvent<ll::event::PlayerJoinEvent>(owner, handle, *sub);
        case EventKind_PLAYER_DISCONNECT:
            return attachEvent<ll::event::PlayerDisconnectEvent>(owner, handle, *sub);
        case EventKind_PLAYER_CHAT:
            return attachEvent<ll::event::PlayerChatEvent>(owner, handle, *sub);
        case EventKind_EXECUTING_COMMAND:
            return attachEvent<ll::event::ExecutingCommandEvent>(owner, handle, *sub);
        case EventKind_EXECUTED_COMMAND:
            return attachEvent<ll::event::ExecutedCommandEvent>(owner, handle, *sub);
        case EventKind_SERVER_COMMAND_REGISTER:
            return attachEvent<ll::event::ServerCommandRegisterEvent>(owner, handle, *sub);
        case EventKind_ACTOR_HURT:
            return attachEvent<ll::event::ActorHurtEvent>(owner, handle, *sub);
        case EventKind_MOB_DIE:
            return attachEvent<ll::event::MobDieEvent>(owner, handle, *sub);
        case EventKind_CONSOLE_OUTPUTTING:
            return attachEvent<ll::event::ConsoleOutputtingEvent>(owner, handle, *sub);
        case EventKind_CONSOLE_OUTPUTTED:
            return attachEvent<ll::event::ConsoleOutputtedEvent>(owner, handle, *sub);
        case EventKind_PLAYER_ADD_EXPERIENCE:
            return attachEvent<ll::event::PlayerAddExperienceEvent>(owner, handle, *sub);
        case EventKind_PLAYER_ATTACK:
            return attachEvent<ll::event::PlayerAttackEvent>(owner, handle, *sub);
        case EventKind_PLAYER_CHANGE_PERM:
            return attachEvent<ll::event::PlayerChangePermEvent>(owner, handle, *sub);
        case EventKind_PLAYER_CONNECT:
            return attachEvent<ll::event::PlayerConnectEvent>(owner, handle, *sub);
        case EventKind_PLAYER_DESTROY_BLOCK:
            return attachEvent<ll::event::PlayerDestroyBlockEvent>(owner, handle, *sub);
        case EventKind_PLAYER_DIE:
            return attachEvent<ll::event::PlayerDieEvent>(owner, handle, *sub);
        case EventKind_PLAYER_INTERACT_BLOCK:
            return attachEvent<ll::event::PlayerInteractBlockEvent>(owner, handle, *sub);
        case EventKind_PLAYER_JUMP:
            return attachEvent<ll::event::PlayerJumpEvent>(owner, handle, *sub);
        case EventKind_PLAYER_PICK_UP_ITEM:
            return attachEvent<ll::event::PlayerPickUpItemEvent>(owner, handle, *sub);
        case EventKind_PLAYER_PLACING_BLOCK:
            return attachEvent<ll::event::PlayerPlacingBlockEvent>(owner, handle, *sub);
        case EventKind_PLAYER_PLACED_BLOCK:
            return attachEvent<ll::event::PlayerPlacedBlockEvent>(owner, handle, *sub);
        case EventKind_PLAYER_RESPAWN:
            return attachEvent<ll::event::PlayerRespawnEvent>(owner, handle, *sub);
        case EventKind_PLAYER_SNEAKING:
            return attachEvent<ll::event::PlayerSneakingEvent>(owner, handle, *sub);
        case EventKind_PLAYER_SNEAKED:
            return attachEvent<ll::event::PlayerSneakedEvent>(owner, handle, *sub);
        case EventKind_PLAYER_SPRINTING:
            return attachEvent<ll::event::PlayerSprintingEvent>(owner, handle, *sub);
        case EventKind_PLAYER_SPRINTED:
            return attachEvent<ll::event::PlayerSprintedEvent>(owner, handle, *sub);
        case EventKind_PLAYER_SWING:
            return attachEvent<ll::event::PlayerSwingEvent>(owner, handle, *sub);
        case EventKind_PLAYER_USE_ITEM:
            return attachEvent<ll::event::PlayerUseItemEvent>(owner, handle, *sub);
        case EventKind_SERVER_STARTED:
            return attachEvent<ll::event::ServerStartedEvent>(owner, handle, *sub);
        case EventKind_SERVER_STOPPING:
            return attachEvent<ll::event::ServerStoppingEvent>(owner, handle, *sub);
        case EventKind_SERVICE_REGISTER:
            return attachEvent<ll::event::ServiceRegisterEvent>(owner, handle, *sub);
        case EventKind_SERVICE_UNREGISTER:
            return attachEvent<ll::event::ServiceUnregisterEvent>(owner, handle, *sub);
        case EventKind_BLOCK_CHANGED:
            return attachEvent<ll::event::BlockChangedEvent>(owner, handle, *sub);
        case EventKind_FIRE_SPREAD:
            return attachEvent<ll::event::FireSpreadEvent>(owner, handle, *sub);
        case EventKind_SERVER_LEVEL_TICK:
            return attachEvent<ll::event::ServerLevelTickEvent>(owner, handle, *sub);
        case EventKind_SPAWNING_MOB:
            return attachEvent<ll::event::SpawningMobEvent>(owner, handle, *sub);
        case EventKind_SPAWNED_MOB:
            return attachEvent<ll::event::SpawnedMobEvent>(owner, handle, *sub);
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
static FerrumStatus eventSubject(uint64_t id, bool playerOnly, ActorRuntimeId* out) noexcept {
    return withEvent(id, [&](EventScope& scope) {
        if (!out) return FerrumStatus_NULL_POINTER;
        if (!scope.mActor || (playerOnly && !scope.mPlayer)) return FerrumStatus_INVALID_ARGUMENT;
        auto level = ll::service::getLevel();
        if (!level) return FerrumStatus_SERVICE_UNAVAILABLE;
        auto* actor = level->getRuntimeEntity(::ActorRuntimeID{*scope.mActor}, false);
        if (!actor || (playerOnly && !actor->isPlayer())) return FerrumStatus_NOT_FOUND;
        *out = *scope.mActor;
        return FerrumStatus_OK;
    });
}
static FerrumStatus eventPlayer(uint64_t id, ActorRuntimeId* out) noexcept { return eventSubject(id, true, out); }
static FerrumStatus eventActor(uint64_t id, ActorRuntimeId* out) noexcept { return eventSubject(id, false, out); }
static FerrumStatus eventCancelled(uint64_t id, FfiBool* out) noexcept {
    return withEvent(id, [&](EventScope& scope) {
        if (!out) return FerrumStatus_NULL_POINTER;
        if (!scope.mCancelled) return FerrumStatus_INVALID_ARGUMENT;
        *out = *scope.mCancelled ? FfiBool_TRUE : FfiBool_FALSE;
        return FerrumStatus_OK;
    });
}
static FerrumStatus cancelEvent(uint64_t id) noexcept {
    return withEvent(id, [&](EventScope& scope) {
        if (!scope.mCancelled) return FerrumStatus_INVALID_ARGUMENT;
        if (!scope.mCancel) return FerrumStatus_UNSUPPORTED;
        scope.mCancel();
        scope.mCancelled = true;
        return FerrumStatus_OK;
    });
}
static FerrumStatus writeEventText(FfiByteWriter* out, std::string const& text) {
    if (!out) return FerrumStatus_NULL_POINTER;
    return out->write_fn(out->context, reinterpret_cast<uint8_t const*>(text.data()), text.size());
}
static FerrumStatus chatMessage(uint64_t id, FfiByteWriter* out) noexcept {
    return withEvent(id, [&](EventScope& scope) {
        return scope.mChat ? writeEventText(out, *scope.mChat) : FerrumStatus_INVALID_ARGUMENT;
    });
}
static FerrumStatus payloadSnbt(uint64_t id, FfiByteWriter* out) noexcept {
    return withEvent(id, [&](EventScope& scope) {
        if (!scope.mTag) return FerrumStatus_UNSUPPORTED;
        if (!scope.mPayload) scope.mPayload = scope.mTag->toSnbt(SnbtFormat::Minimize);
        return writeEventText(out, *scope.mPayload);
    });
}
void installEventApi(FerrumShimApi& api) {
    api.event_subscribe    = subscribeEvent;
    api.event_unsubscribe  = unsubscribeEvent;
    api.event_payload_snbt = payloadSnbt;
    api.event_actor        = eventActor;
    api.event_player       = eventPlayer;
    api.event_is_cancelled = eventCancelled;
    api.event_cancel       = cancelEvent;
    api.event_chat_message = chatMessage;
}
} // namespace ferrum
