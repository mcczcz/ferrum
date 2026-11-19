use crate::{
    arena::Arena,
    last_error,
    loader::Invocation,
    runtime::{self, ModState},
    shim, vtable,
};
use ferrum_abi::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy)]
enum Callback {
    Main(EventCallbackV1),
    Thread(ThreadEventCallbackV1),
}

struct Listener {
    owner: ModContextHandle,
    kind: EventKind,
    callback: Callback,
    owned: AtomicBool,
    warned_reentrant: AtomicBool,
}

// SAFETY: Main 闭包仅在主线程调用和析构。Thread 闭包满足 Send + Sync。
unsafe impl Send for Listener {}
// SAFETY: 工作线程仅在 Invocation 守卫内取得 Thread 回调的强引用。
unsafe impl Sync for Listener {}

impl Drop for Listener {
    fn drop(&mut self) {
        if self.owned.load(Ordering::Relaxed) {
            match self.callback {
                Callback::Main(cb) => (cb.drop_fn)(cb.env),
                Callback::Thread(cb) => (cb.drop_fn)(cb.env),
            }
        }
    }
}

struct Scope {
    owner: ModContextHandle,
    kind: EventKind,
    shim_id: u64,
}
static LISTENERS: Mutex<Arena<Arc<Listener>>> = Mutex::new(Arena::new());
static SCOPES: Mutex<Arena<Scope>> = Mutex::new(Arena::new());

fn main_thread() -> Result<(), FerrumStatus> {
    if vtable::is_main_thread() {
        Ok(())
    } else {
        Err(last_error::fail(
            FerrumStatus::WRONG_THREAD,
            "事件 API 必须在主线程调用",
        ))
    }
}

extern "C" fn subscribe(
    ctx: ModContextHandle,
    subscription: *const EventSubscriptionV1,
    callback: EventCallbackV1,
    out: *mut ListenerHandle,
) -> FerrumStatus {
    register(ctx, subscription, Callback::Main(callback), out)
}

extern "C" fn subscribe_sync(
    ctx: ModContextHandle,
    subscription: *const EventSubscriptionV1,
    callback: ThreadEventCallbackV1,
    out: *mut ListenerHandle,
) -> FerrumStatus {
    register(ctx, subscription, Callback::Thread(callback), out)
}

fn register(
    ctx: ModContextHandle,
    subscription: *const EventSubscriptionV1,
    callback: Callback,
    out: *mut ListenerHandle,
) -> FerrumStatus {
    vtable::guard_api("event::subscribe", || {
        if let Err(s) = main_thread() {
            return s;
        }
        if subscription.is_null() || out.is_null() {
            return last_error::fail(FerrumStatus::NULL_POINTER, "事件订阅描述或输出句柄指针为空");
        }
        // SAFETY: 调用方保证非空指针至少指向可读的结构头。
        if unsafe { (*subscription).struct_size } < size_of::<EventSubscriptionV1>() as u32 {
            return last_error::fail(FerrumStatus::ABI_INCOMPATIBLE, "事件订阅描述大小不足");
        }
        // SAFETY: 已确认结构大小，调用方保证其在同步调用内有效。
        let subscription = unsafe { &*subscription };
        if subscription.kind.0 > EventKind::SPAWNED_MOB.0 {
            return last_error::fail(FerrumStatus::UNSUPPORTED, "不支持的原生事件类型");
        }
        if subscription.kind.is_emitter_thread_event() != matches!(callback, Callback::Thread(_)) {
            return FerrumStatus::INVALID_ARGUMENT;
        }
        if !matches!(
            subscription.priority,
            EventPriority::HIGHEST
                | EventPriority::HIGH
                | EventPriority::NORMAL
                | EventPriority::LOW
                | EventPriority::LOWEST
        ) {
            return last_error::fail(FerrumStatus::INVALID_ARGUMENT, "无效的事件优先级");
        }
        let entry = match runtime::lookup(ctx) {
            Ok(e) => e,
            Err(s) => return s,
        };
        if !matches!(entry.state(), ModState::Enabling | ModState::Enabled) {
            return last_error::fail(FerrumStatus::MOD_DISABLED, "只能在启用阶段订阅事件");
        }
        let Some(api) = shim::get() else {
            return last_error::fail(FerrumStatus::SERVICE_UNAVAILABLE, "事件 shim 尚未安装");
        };
        let listener = Arc::new(Listener {
            owner: ctx,
            kind: subscription.kind,
            callback,
            owned: AtomicBool::new(false),
            warned_reentrant: AtomicBool::new(false),
        });
        let handle = ListenerHandle(
            LISTENERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(listener.clone()),
        );
        let status = (api.event_subscribe)(entry.info.cookie, handle, subscription);
        if status.is_ok() {
            entry
                .ownership
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .listeners
                .push(handle);
            listener.owned.store(true, Ordering::Release);
            // SAFETY: 调用方提供独占可写的 ListenerHandle 内存，注册已完成。
            unsafe { out.write(handle) };
        } else {
            let removed = LISTENERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(handle.0);
            drop(removed);
        }
        status
    })
}

extern "C" fn unsubscribe(ctx: ModContextHandle, handle: ListenerHandle) -> FerrumStatus {
    vtable::guard_api("event::unsubscribe", || {
        if let Err(s) = main_thread() {
            return s;
        }
        let entry = match runtime::lookup(ctx) {
            Ok(e) => e,
            Err(s) => return s,
        };
        let removed = {
            let mut listeners = LISTENERS.lock().unwrap_or_else(|e| e.into_inner());
            match listeners.get(handle.0) {
                None => return FerrumStatus::STALE_HANDLE,
                Some(l) if l.owner != ctx => return FerrumStatus::PERMISSION_DENIED,
                _ => {}
            }
            listeners.remove(handle.0)
        };
        entry
            .ownership
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .listeners
            .retain(|h| *h != handle);
        let status = shim::get().map_or(FerrumStatus::SERVICE_UNAVAILABLE, |a| {
            (a.event_unsubscribe)(entry.info.cookie, handle)
        });
        // 活跃派发持有另一份 Arc，自注销不会提前释放闭包；此处不持有宿主锁。
        drop(removed);
        status
    })
}

pub fn revoke(ctx: ModContextHandle) {
    let Ok(entry) = runtime::lookup(ctx) else {
        return;
    };
    let handles = std::mem::take(
        &mut entry
            .ownership
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .listeners,
    );
    for handle in handles {
        let removed = LISTENERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(handle.0);
        if let Some(api) = shim::get() {
            (api.event_unsubscribe)(entry.info.cookie, handle);
        }
        drop(removed);
    }
}

fn scope_call(
    ctx: ModContextHandle,
    scope: EventScopeHandle,
    kind: Option<EventKind>,
    cancellable: bool,
    f: impl FnOnce(&shim::FerrumShimApi, u64) -> FerrumStatus,
) -> FerrumStatus {
    vtable::guard_api("event::scope_call", || {
        if let Err(s) = main_thread() {
            return s;
        }
        let id = {
            let scopes = SCOPES.lock().unwrap_or_else(|e| e.into_inner());
            let s = match scopes.get(scope.0) {
                Some(s) if s.owner == ctx => s,
                Some(_) => return FerrumStatus::PERMISSION_DENIED,
                None => return FerrumStatus::STALE_HANDLE,
            };
            if kind.is_some_and(|k| k != s.kind) || (cancellable && !s.kind.is_cancellable()) {
                return FerrumStatus::INVALID_ARGUMENT;
            }
            s.shim_id
        };
        match shim::get() {
            Some(a) => f(a, id),
            None => FerrumStatus::SERVICE_UNAVAILABLE,
        }
    })
}
extern "C" fn player(
    c: ModContextHandle,
    s: EventScopeHandle,
    out: *mut ActorRuntimeId,
) -> FerrumStatus {
    scope_call(c, s, None, false, |a, id| (a.event_player)(id, out))
}
extern "C" fn is_cancelled(
    c: ModContextHandle,
    s: EventScopeHandle,
    out: *mut FfiBool,
) -> FerrumStatus {
    scope_call(c, s, None, true, |a, id| (a.event_is_cancelled)(id, out))
}
extern "C" fn cancel(c: ModContextHandle, s: EventScopeHandle) -> FerrumStatus {
    scope_call(c, s, None, true, |a, id| (a.event_cancel)(id))
}
extern "C" fn chat_message(
    c: ModContextHandle,
    s: EventScopeHandle,
    out: *mut FfiByteWriter,
) -> FerrumStatus {
    scope_call(c, s, Some(EventKind::PLAYER_CHAT), false, |a, id| {
        (a.event_chat_message)(id, out)
    })
}

extern "C" fn payload_snbt(
    ctx: ModContextHandle,
    scope: EventScopeHandle,
    out: *mut FfiByteWriter,
) -> FerrumStatus {
    scope_call(ctx, scope, None, false, |api, id| {
        (api.event_payload_snbt)(id, out)
    })
}

extern "C" fn actor(
    ctx: ModContextHandle,
    scope: EventScopeHandle,
    out: *mut ActorRuntimeId,
) -> FerrumStatus {
    scope_call(ctx, scope, None, false, |api, id| {
        (api.event_actor)(id, out)
    })
}

/// shim 仅传递栈上事件的数字索引，C++ 对象指针不会进入 Rust。
#[unsafe(no_mangle)]
pub extern "C" fn ferrum_host_dispatch_event(raw: u64, shim_scope: u64) -> FerrumStatus {
    vtable::guard_api("event::ferrum_host_dispatch_event", || {
        if let Err(s) = main_thread() {
            return s;
        }
        let Some(handle) = GenerationalHandle::from_raw(raw) else {
            return FerrumStatus::STALE_HANDLE;
        };
        let listener = LISTENERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(handle)
            .cloned();
        let Some(listener) = listener else {
            return FerrumStatus::STALE_HANDLE;
        };
        let ctx = listener.owner;
        let entry = match runtime::lookup(ctx) {
            Ok(e) => e,
            Err(s) => return s,
        };
        let _invocation = match Invocation::begin(ctx, entry.clone()) {
            Ok(g) => g,
            Err(s) => return s,
        };
        // 在守卫之后重新绑定：局部变量逆序析构，panic 展开时也先释放监听器
        // （可能执行 drop_fn），再结束活跃调用并处理推迟的停用。
        let listener = listener;
        let Callback::Main(callback) = listener.callback else {
            return FerrumStatus::INVALID_ARGUMENT;
        };
        let scope = EventScopeHandle(SCOPES.lock().unwrap_or_else(|e| e.into_inner()).insert(
            Scope {
                owner: ctx,
                kind: listener.kind,
                shim_id: shim_scope,
            },
        ));
        let status = (callback.invoke_fn)(callback.env, scope, RawMainThreadToken::ASSERTED);
        SCOPES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(scope.0);
        if !status.is_ok() {
            let diagnostic = match status {
                // panic 已由 report_panic 输出详情，不再重复记录或读取无关的 last-error。
                FerrumStatus::MOD_PANICKED => None,
                FerrumStatus::REENTRANT_CALL => {
                    if listener.warned_reentrant.swap(true, Ordering::Relaxed) {
                        None
                    } else {
                        Some((LogLevel::WARN, "事件回调重入，已跳过本次调用".to_owned()))
                    }
                }
                FerrumStatus::CALLBACK_POISONED => {
                    Some((LogLevel::ERROR, "事件回调已被毒化，移除监听器".to_owned()))
                }
                _ => {
                    let detail = last_error::take().unwrap_or_default();
                    let text = if detail.is_empty() {
                        format!("事件回调 {status}")
                    } else {
                        format!("事件回调 {status}: {detail}")
                    };
                    Some((LogLevel::ERROR, text))
                }
            };
            if let Some((level, text)) = diagnostic {
                if let Some(api) = shim::get() {
                    (api.log)(entry.info.cookie, level, FfiStr::new(&text));
                }
            }
            if status == FerrumStatus::MOD_PANICKED || status == FerrumStatus::CALLBACK_POISONED {
                unsubscribe(ctx, ListenerHandle(handle));
            }
        }
        drop(listener);
        status
    })
}

// 触发线程不能调用 LL 退订或析构主线程回调。下一个主线程 tick 按代数句柄清理。
static POISONED: Mutex<Vec<(ModContextHandle, ListenerHandle)>> = Mutex::new(Vec::new());
pub(crate) fn drain_poisoned_listeners() {
    let pending = std::mem::take(&mut *POISONED.lock().unwrap_or_else(|e| e.into_inner()));
    for (ctx, handle) in pending {
        unsubscribe(ctx, handle);
    }
}

thread_local! {
    static THREAD_CALLS: std::cell::RefCell<Vec<ListenerHandle>> = const { std::cell::RefCell::new(Vec::new()) };
}
struct ThreadCall;
impl Drop for ThreadCall {
    fn drop(&mut self) {
        THREAD_CALLS.with_borrow_mut(|calls| calls.pop());
    }
}

/// 两个文本及取消输出只在同步派发期间借用。该路径永远不发放主线程令牌。
#[unsafe(no_mangle)]
pub extern "C" fn ferrum_host_dispatch_thread_event(
    handle: ListenerHandle,
    payload: FfiStr<'_>,
    message: FfiStr<'_>,
    cancelled: FfiBool,
    out: *mut FfiBool,
) -> FerrumStatus {
    vtable::guard_api("event::dispatch_thread", || {
        if out.is_null() {
            return FerrumStatus::NULL_POINTER;
        }
        let listeners = LISTENERS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(listener) = listeners.get(handle.0) else {
            return FerrumStatus::STALE_HANDLE;
        };
        let ctx = listener.owner;
        let Callback::Thread(cb) = listener.callback else {
            return FerrumStatus::INVALID_ARGUMENT;
        };
        if !listener.owned.load(Ordering::Acquire) {
            return FerrumStatus::MOD_DISABLED;
        }
        let entry = match runtime::lookup(ctx) {
            Ok(entry) => entry,
            Err(s) => return s,
        };
        let _invocation = match Invocation::begin_on_emitter(ctx, entry.clone()) {
            Ok(guard) => guard,
            Err(status) => return status,
        };
        let listener = listener.clone();
        drop(listeners);
        if THREAD_CALLS.with_borrow(|calls| calls.contains(&handle)) {
            return FerrumStatus::REENTRANT_CALL;
        }
        THREAD_CALLS.with_borrow_mut(|calls| calls.push(handle));
        let _call = ThreadCall;
        let status = (cb.invoke_fn)(cb.env, payload, message, cancelled, out);
        if matches!(
            status,
            FerrumStatus::MOD_PANICKED | FerrumStatus::CALLBACK_POISONED
        ) {
            if !listener.warned_reentrant.swap(true, Ordering::AcqRel) {
                POISONED
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((ctx, handle));
            }
        } else if !status.is_ok() {
            if let Some(shim) = shim::get() {
                let detail = last_error::take().unwrap_or_else(|| status.to_string());
                (shim.log)(entry.info.cookie, LogLevel::ERROR, FfiStr::new(&detail));
            }
        }
        status
    })
}

pub static EVENT_V1: EventApiV1 = EventApiV1 {
    struct_size: size_of::<EventApiV1>() as u32,
    subscribe,
    unsubscribe,
    player,
    is_cancelled,
    cancel,
    chat_message,
    payload_snbt,
    actor,
    subscribe_sync,
};
