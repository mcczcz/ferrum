#include "ferrum/NativeEvents.h"
#include "ferrum/CustomEvent.h"
#include "ferrum/FerrumModManager.h"
#include "ll/api/event/EmitterBase.h"
#include "ll/api/event/EventBus.h"
#include "ll/api/mod/ModManagerRegistry.h"
#include "ll/api/mod/NativeMod.h"
#include "ll/api/utils/ErrorUtils.h"
#include "ll/api/utils/SystemUtils.h"
#include <map>
#include <memory>
#include <string>

namespace ferrum {
struct NativeProvider {
    std::string                 mName;
    std::weak_ptr<ll::mod::Mod> mMod;
};
static std::map<EventProviderHandle, NativeProvider>          sProviders;
static bool                                                   sReady = false;
static std::map<CustomListenerHandle, ll::event::ListenerPtr> sCustomListeners;

template <class F>
static FerrumStatus nativeGuard(F&& f) noexcept {
    try {
        if (!ferrum_host_is_main_thread()) return FerrumStatus_WRONG_THREAD;
        if (!sReady) return FerrumStatus_SERVICE_UNAVAILABLE;
        return f();
    } catch (...) {
        auto text = ll::error_utils::makeExceptionString(std::current_exception());
        ferrum_host_set_last_error({reinterpret_cast<uint8_t const*>(text.data()), text.size()});
        return FerrumStatus_CPP_EXCEPTION;
    }
}
static FerrumStatus openProvider(FfiStr name, EventProviderHandle* out) noexcept {
    auto caller = LL_RETURN_ADDRESS;
    return nativeGuard([&] {
        if (!out || (!name.ptr && name.len)) return FerrumStatus_NULL_POINTER;
        auto text = name.len ? std::string{reinterpret_cast<char const*>(name.ptr), name.len} : std::string{};
        auto mod  = ll::mod::ModManagerRegistry::getInstance().getMod(text);
        // 热加载的 onEnable 早于 registry 登记，按调用方 DLL 查找 LL 当前正在加载的模组。
        if (!mod) {
            auto loading = ll::mod::NativeMod::getByHandle(ll::sys_utils::getModuleHandle(caller));
            if (loading && loading->getName() == text) mod = std::move(loading);
        }
        if (!mod || mod->getType() != ll::mod::NativeModManagerName) return FerrumStatus_NOT_FOUND;
        EventProviderHandle handle{};
        auto                api    = ferrum_host_native_event_api();
        auto                status = api->open_provider(name, &handle);
        if (status != FerrumStatus_OK) return status;
        try {
            sProviders.emplace(handle, NativeProvider{text, mod});
        } catch (...) {
            api->close_provider(handle);
            throw;
        }
        *out = handle;
        return FerrumStatus_OK;
    });
}
static FerrumStatus closeProvider(EventProviderHandle provider) noexcept {
    return nativeGuard([&] {
        if (!sProviders.contains(provider)) return FerrumStatus_STALE_HANDLE;
        auto status = ferrum_host_native_event_api()->close_provider(provider);
        sProviders.erase(provider);
        return status;
    });
}
static FerrumStatus providerStatus(EventProviderHandle provider, bool publishing) {
    auto it = sProviders.find(provider);
    if (it == sProviders.end()) return FerrumStatus_STALE_HANDLE;
    auto mod = it->second.mMod.lock();
    auto registered = ll::mod::ModManagerRegistry::getInstance().getMod(it->second.mName);
    // openProvider 已确认 native 类型；注册期间仍须接受尚未登记的同一个模组实例。
    if (!registered && mod) {
        auto native = std::static_pointer_cast<ll::mod::NativeMod>(mod);
        registered  = ll::mod::NativeMod::getByHandle(native->getHandle());
    }
    if (!mod || registered != mod) {
        ferrum_host_native_event_api()->close_provider(provider);
        sProviders.erase(it);
        return FerrumStatus_STALE_HANDLE;
    }
    // 允许 onEnable 阶段注册；只有 LL 已确认 Enabled 的发布者才能派发。
    return publishing && !mod->isEnabled() ? FerrumStatus_MOD_DISABLED : FerrumStatus_OK;
}
static FerrumStatus registerEvent(
    EventProviderHandle            provider,
    CustomEventDefinitionV1 const* definition,
    CustomEventHandle*             out
) noexcept {
    return nativeGuard([&] {
        auto status = providerStatus(provider, false);
        return status == FerrumStatus_OK ? ferrum_host_native_event_api()->register_event(provider, definition, out)
                                         : status;
    });
}
static FerrumStatus unregisterEvent(EventProviderHandle provider, CustomEventHandle event) noexcept {
    return nativeGuard([&] {
        auto status = providerStatus(provider, false);
        return status == FerrumStatus_OK ? ferrum_host_native_event_api()->unregister_event(provider, event) : status;
    });
}
static FerrumStatus publishEvent(
    EventProviderHandle   provider,
    CustomEventHandle     event,
    FfiSlice_EventValueV1 values,
    FfiBool*              out
) noexcept {
    return nativeGuard([&] {
        auto status = providerStatus(provider, true);
        if (status != FerrumStatus_OK) return status;
        status = ferrum_host_native_event_api()->publish(provider, event, values, out);
        if (status != FerrumStatus_OK) return status;
        // 回调可能停用或卸载发布者；返回后禁止调用方再执行原业务操作。
        return providerStatus(provider, true) == FerrumStatus_OK ? FerrumStatus_OK : FerrumStatus_MOD_DISABLED;
    });
}
static FerrumStatus takeError(FfiByteWriter* out) noexcept {
    try {
        return ferrum_host_take_last_error(out);
    } catch (...) {
        return FerrumStatus_CPP_EXCEPTION;
    }
}
static NativeEventApiV1 const sApi{
    sizeof(NativeEventApiV1),
    openProvider,
    closeProvider,
    registerEvent,
    unregisterEvent,
    publishEvent,
    takeError
};

static void revokeMod(std::string_view name) noexcept {
    (void)nativeGuard([&] {
        for (auto it = sProviders.begin(); it != sProviders.end();) {
            if (it->second.mName == name) {
                ferrum_host_native_event_api()->close_provider(it->first);
                it = sProviders.erase(it);
            } else ++it;
        }
        return FerrumStatus_OK;
    });
}
static FerrumStatus bridgePublish(EventScopeHandle scope, FfiBool* out) noexcept {
    return nativeGuard([&] {
        if (!out) return FerrumStatus_NULL_POINTER;
        FerrumCustomEvent event{ferrum_host_custom_event_view_api(), scope};
        ll::event::EventBus::getInstance().publish(event);
        event.syncCancelled();
        *out = event.isCancelled() ? FfiBool_TRUE : FfiBool_FALSE;
        return event.active() ? FerrumStatus_OK : FerrumStatus_MOD_DISABLED;
    });
}
static FerrumStatus
bridgeSubscribe(ShimModCookie cookie, CustomListenerHandle handle, FfiStr name, EventPriority priority) noexcept {
    return nativeGuard([&] {
        auto owner    = *static_cast<std::shared_ptr<FerrumMod>*>(cookie);
        auto text     = std::string{reinterpret_cast<char const*>(name.ptr), name.len};
        auto listener = ll::event::Listener<FerrumCustomEvent>::create(
            [handle, text = std::move(text)](FerrumCustomEvent& event) {
                (void)nativeGuard([&] {
                    if (!event.active()) return FerrumStatus_MOD_DISABLED;
                    std::string eventName;
                    if (event.name(eventName) != FerrumStatus_OK || eventName != text) return FerrumStatus_OK;
                    event.syncCancelled();
                    if (!event.isCancelled()) ferrum_host_dispatch_custom_listener(handle, event.scope());
                    event.syncCancelled();
                    return FerrumStatus_OK;
                });
            },
            static_cast<ll::event::EventPriority>(priority),
            owner
        );
        auto [it, inserted] = sCustomListeners.emplace(handle, listener);
        if (!inserted) return FerrumStatus_ALREADY_EXISTS;
        try {
            if (!ll::event::EventBus::getInstance().addListener<FerrumCustomEvent>(listener)) {
                sCustomListeners.erase(it);
                return FerrumStatus_SERVICE_UNAVAILABLE;
            }
        } catch (...) {
            ll::event::EventBus::getInstance().removeListener(listener);
            sCustomListeners.erase(handle);
            throw;
        }
        return FerrumStatus_OK;
    });
}
static FerrumStatus bridgeUnsubscribe(CustomListenerHandle handle) noexcept {
    return nativeGuard([&] {
        auto it = sCustomListeners.find(handle);
        if (it == sCustomListeners.end()) return FerrumStatus_STALE_HANDLE;
        ll::event::EventBus::getInstance().removeListener(it->second);
        sCustomListeners.erase(it);
        return FerrumStatus_OK;
    });
}
void installCustomEventBridge(FerrumShimApi& api) {
    api.custom_event_publish     = bridgePublish;
    api.custom_event_subscribe   = bridgeSubscribe;
    api.custom_event_unsubscribe = bridgeUnsubscribe;
}
FerrumStatus installNativeEvents() noexcept {
    try {
        if (sReady) return FerrumStatus_OK;
        auto  owner    = ll::mod::NativeMod::current();
        auto& registry = ll::mod::ModManagerRegistry::getInstance();
        auto& bus      = ll::event::EventBus::getInstance();
        if (!bus.hasEvent(ll::event::getEventId<FerrumCustomEvent>)
            && !bus.setEventEmitter<FerrumCustomEvent>(
                [] { return std::make_unique<ll::event::EmitterBase>(); },
                owner
            ))
            return FerrumStatus_FAILED;
        registry.executeOnModDisable([](std::string_view name) { revokeMod(name); }, owner);
        registry.executeOnModUnload([](std::string_view name) { revokeMod(name); }, owner);
        sReady = true;
        return FerrumStatus_OK;
    } catch (...) {
        auto text = ll::error_utils::makeExceptionString(std::current_exception());
        ferrum_host_set_last_error({reinterpret_cast<uint8_t const*>(text.data()), text.size()});
        return FerrumStatus_CPP_EXCEPTION;
    }
}
void closeNativeEvents() noexcept {
    (void)nativeGuard([&] {
        for (auto const& [provider, _] : sProviders) ferrum_host_native_event_api()->close_provider(provider);
        sProviders.clear();
        return FerrumStatus_OK;
    });
}
} // namespace ferrum

extern "C" __declspec(dllexport) NativeEventApiV1 const* ferrum_native_event_api_v1() noexcept { return &ferrum::sApi; }
