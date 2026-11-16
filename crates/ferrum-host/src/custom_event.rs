//! 自定义事件与宿主模组生命周期的连接。
use crate::{
    custom_event_core as core, last_error, loader,
    runtime::{self, ModState},
    shim, vtable,
};
use ferrum_abi::*;

fn can_subscribe(ctx: ModContextHandle) -> FerrumStatus {
    match runtime::lookup(ctx) {
        Ok(e) if matches!(e.state(), ModState::Enabling | ModState::Enabled) => FerrumStatus::OK,
        Ok(_) => FerrumStatus::MOD_DISABLED,
        Err(s) => s,
    }
}
fn begin(ctx: ModContextHandle) -> FerrumStatus {
    match runtime::lookup(ctx).and_then(|e| loader::enter(&e)) {
        Ok(()) => FerrumStatus::OK,
        Err(s) => s,
    }
}
fn finish(ctx: ModContextHandle) {
    if let Ok(entry) = runtime::lookup(ctx) {
        loader::leave(ctx, &entry);
    }
}
fn log(ctx: ModContextHandle, level: LogLevel, text: &str) {
    if let (Ok(entry), Some(api)) = (runtime::lookup(ctx), shim::get()) {
        (api.log)(entry.info.cookie, level, FfiStr::new(text));
    }
}
fn install() {
    core::install(core::Hooks {
        main_thread: vtable::is_main_thread,
        can_subscribe,
        begin,
        finish,
        log,
        publisher_name: |ctx| runtime::lookup(ctx).map(|e| e.info.name.clone()),
        enabled: |ctx| runtime::lookup(ctx).is_ok_and(|e| e.state() == ModState::Enabled),
        publish_ll: Some(|scope, out| {
            shim::get().map_or(FerrumStatus::SERVICE_UNAVAILABLE, |a| {
                (a.custom_event_publish)(scope, out)
            })
        }),
        attach: |ctx, listener, name, priority| {
            let entry = match runtime::lookup(ctx) {
                Ok(e) => e,
                Err(s) => return s,
            };
            shim::get().map_or(FerrumStatus::SERVICE_UNAVAILABLE, |a| {
                (a.custom_event_subscribe)(entry.info.cookie, listener, FfiStr::new(name), priority)
            })
        },
        detach: |listener| {
            if let Some(a) = shim::get() {
                (a.custom_event_unsubscribe)(listener);
            }
        },
        track_provider: |ctx, added| {
            if let Ok(e) = runtime::lookup(ctx) {
                let mut own = e.ownership.lock().unwrap_or_else(|e| e.into_inner());
                if added {
                    own.custom_providers += 1;
                } else {
                    own.custom_providers -= 1;
                }
            }
        },
        track_listener: |ctx, added| {
            if let Ok(e) = runtime::lookup(ctx) {
                let mut own = e.ownership.lock().unwrap_or_else(|e| e.into_inner());
                if added {
                    own.custom_listeners += 1;
                } else {
                    own.custom_listeners -= 1;
                }
            }
        },
        set_error: |text| last_error::set(text),
        take_error: last_error::take,
    });
}
pub(crate) fn api() -> &'static CustomEventApiV1 {
    install();
    &core::API
}
pub(crate) fn publisher_api() -> &'static CustomEventPublisherApiV1 {
    install();
    &core::PUBLISHER_API
}
pub(crate) fn revoke(ctx: ModContextHandle) {
    core::revoke(ctx);
}

/// 仅供 shim 获取原生发布端的内部表；公开入口由 C++ shim 做原生模组身份与状态检查。
#[unsafe(no_mangle)]
pub extern "C" fn ferrum_host_native_event_api() -> *const NativeEventApiV1 {
    install();
    &core::NATIVE_API
}

#[unsafe(no_mangle)]
pub extern "C" fn ferrum_host_custom_event_view_api() -> *const CustomEventViewApiV1 {
    install();
    &core::VIEW_API
}

#[unsafe(no_mangle)]
pub extern "C" fn ferrum_host_dispatch_custom_listener(
    raw: u64,
    scope: EventScopeHandle,
) -> FerrumStatus {
    core::dispatch_listener(raw, scope)
}
