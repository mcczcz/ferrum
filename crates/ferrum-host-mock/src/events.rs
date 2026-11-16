//! 假 LL EventBus：只保存路由和原生作用域，闭包由真实宿主拥有。
use super::domain::{guard, player_exists, put, text, write};
use super::{MockHost, check};
use ferrum_abi::*;
use ferrum_host::shim::ShimModCookie;
use ferrum_sdk::Error;
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Clone)]
struct Route {
    cookie: ShimModCookie,
    kind: EventKind,
    priority: EventPriority,
}
struct Scope {
    player: ActorRuntimeId,
    message: Option<String>,
    cancelled: bool,
}
#[derive(Clone)]
struct CustomRoute {
    name: String,
    priority: EventPriority,
    order: u64,
}
#[derive(Default)]
struct Bus {
    routes: BTreeMap<u64, Route>,
    scopes: BTreeMap<u64, Scope>,
    custom: BTreeMap<u64, CustomRoute>,
    serial: u64,
}
thread_local! { static BUS: RefCell<Bus> = RefCell::new(Bus::default()); }
pub(super) fn reset() {
    BUS.with(|b| *b.borrow_mut() = Bus::default());
}

pub(super) extern "C" fn subscribe(
    cookie: ShimModCookie,
    handle: ListenerHandle,
    sub: *const EventSubscriptionV1,
) -> FerrumStatus {
    guard(|| {
        // SAFETY: 真实宿主已校验订阅描述，其在当前同步调用内有效。
        let sub = unsafe { &*sub };
        BUS.with(|b| {
            b.borrow_mut().routes.insert(
                handle.0.to_raw(),
                Route {
                    cookie,
                    kind: sub.kind,
                    priority: sub.priority,
                },
            )
        });
        FerrumStatus::OK
    })
}
pub(super) extern "C" fn unsubscribe(
    cookie: ShimModCookie,
    handle: ListenerHandle,
) -> FerrumStatus {
    guard(|| {
        BUS.with(|b| {
            let mut b = b.borrow_mut();
            match b.routes.get(&handle.0.to_raw()) {
                Some(r) if r.cookie != cookie => FerrumStatus::PERMISSION_DENIED,
                None => FerrumStatus::STALE_HANDLE,
                _ => {
                    b.routes.remove(&handle.0.to_raw());
                    FerrumStatus::OK
                }
            }
        })
    })
}
fn scope<T>(raw: u64, f: impl FnOnce(&mut Scope) -> T) -> Result<T, FerrumStatus> {
    BUS.with(|b| {
        b.borrow_mut()
            .scopes
            .get_mut(&raw)
            .map(f)
            .ok_or(FerrumStatus::STALE_HANDLE)
    })
}
pub(super) extern "C" fn player(raw: u64, out: *mut ActorRuntimeId) -> FerrumStatus {
    guard(
        || match scope(raw, |s| s.player).and_then(|id| player_exists(id).map(|_| id)) {
            Ok(id) => put(out, id),
            Err(s) => s,
        },
    )
}
pub(super) extern "C" fn cancelled(raw: u64, out: *mut FfiBool) -> FerrumStatus {
    guard(|| match scope(raw, |s| s.cancelled) {
        Ok(value) => put(out, value.into()),
        Err(s) => s,
    })
}
pub(super) extern "C" fn cancel(raw: u64) -> FerrumStatus {
    guard(|| scope(raw, |s| s.cancelled = true).map_or_else(|s| s, |_| FerrumStatus::OK))
}
pub(super) extern "C" fn message(raw: u64, out: *mut FfiByteWriter) -> FerrumStatus {
    guard(|| match scope(raw, |s| s.message.clone()) {
        Ok(Some(message)) => write(out, message.as_bytes()),
        Ok(None) => FerrumStatus::INVALID_ARGUMENT,
        Err(s) => s,
    })
}
pub(super) extern "C" fn subscribe_custom(
    _cookie: ShimModCookie,
    handle: CustomListenerHandle,
    name: FfiStr<'_>,
    priority: EventPriority,
) -> FerrumStatus {
    guard(|| {
        let name = text(name);
        BUS.with(|b| {
            let mut b = b.borrow_mut();
            b.serial += 1;
            let order = b.serial;
            b.custom.insert(
                handle.0.to_raw(),
                CustomRoute {
                    name,
                    priority,
                    order,
                },
            );
        });
        FerrumStatus::OK
    })
}
pub(super) extern "C" fn unsubscribe_custom(handle: CustomListenerHandle) -> FerrumStatus {
    guard(|| {
        BUS.with(|b| b.borrow_mut().custom.remove(&handle.0.to_raw()));
        FerrumStatus::OK
    })
}
struct TextOut(String);
extern "C" fn collect_text(env: *mut std::ffi::c_void, ptr: *const u8, len: usize) -> FerrumStatus {
    guard(|| {
        // SAFETY: writer 指向本次 publish 栈内的独占 TextOut；非空输入由宿主保证有效。
        let out = unsafe { &mut *env.cast::<TextOut>() };
        if len != 0 {
            // SAFETY: 宿主 writer 协议保证 ptr 指向 len 字节，且当前已排除空输入。
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
            out.0.push_str(&String::from_utf8_lossy(bytes));
        }
        FerrumStatus::OK
    })
}
pub(super) extern "C" fn publish_custom(
    scope: EventScopeHandle,
    out: *mut FfiBool,
) -> FerrumStatus {
    guard(|| {
        let ptr = ferrum_host::custom_event::ferrum_host_custom_event_view_api();
        // SAFETY: 真实宿主返回进程生命周期内有效的不可变作用域访问表。
        let view = unsafe { &*ptr };
        let mut name = TextOut(String::new());
        let mut writer = FfiByteWriter {
            context: (&raw mut name).cast(),
            write_fn: collect_text,
        };
        let status = (view.name)(scope, &mut writer);
        if !status.is_ok() {
            return status;
        }
        let mut routes = BUS.with(|b| {
            b.borrow()
                .custom
                .iter()
                .filter(|(_, r)| r.name == name.0)
                .map(|(h, r)| (r.priority.0, r.order, *h))
                .collect::<Vec<_>>()
        });
        routes.sort_unstable();
        let mut cancelled = FfiBool::FALSE;
        for (_, _, raw) in routes {
            if !(view.is_active)(scope).is_ok() {
                break;
            }
            if !BUS.with(|b| b.borrow().custom.contains_key(&raw)) {
                continue;
            }
            let status =
                ferrum_host::custom_event::ferrum_host_dispatch_custom_listener(raw, scope);
            if !status.is_ok() {
                return status;
            }
            let status = (view.is_cancelled)(scope, &mut cancelled);
            if !status.is_ok() {
                return status;
            }
            if cancelled.get() {
                break;
            }
        }
        put(out, cancelled)
    })
}
impl MockHost {
    /// 返回是否取消；不自动添加玩家或改变在线状态。
    pub fn emit_player_join(&self, player: ActorRuntimeId) -> Result<bool, Error> {
        self.emit_event(EventKind::PLAYER_JOIN, player, None)
    }
    /// 在移除玩家前调用；不自动改变在线状态。
    pub fn emit_player_disconnect(&self, player: ActorRuntimeId) -> Result<(), Error> {
        self.emit_event(EventKind::PLAYER_DISCONNECT, player, None)
            .map(|_| ())
    }
    pub fn emit_player_chat(
        &self,
        player: ActorRuntimeId,
        message: impl Into<String>,
    ) -> Result<bool, Error> {
        self.emit_event(EventKind::PLAYER_CHAT, player, Some(message.into()))
    }
    fn emit_event(
        &self,
        kind: EventKind,
        player: ActorRuntimeId,
        message: Option<String>,
    ) -> Result<bool, Error> {
        check(guard(|| FerrumStatus::OK))?;
        self.flush_disables();
        player_exists(player).map_err(|s| Error::new(s, "mock 玩家不存在"))?;
        let (raw_scope, mut routes) = BUS.with(|b| {
            let mut b = b.borrow_mut();
            b.serial += 1;
            let raw = b.serial;
            b.scopes.insert(
                raw,
                Scope {
                    player,
                    message,
                    cancelled: false,
                },
            );
            let routes = b
                .routes
                .iter()
                .filter(|(_, r)| r.kind == kind)
                .map(|(h, r)| (r.priority.0, *h))
                .collect::<Vec<_>>();
            (raw, routes)
        });
        routes.sort_unstable();
        for (_, raw) in routes {
            if !BUS.with(|b| b.borrow().routes.contains_key(&raw)) {
                continue;
            }
            let _ = ferrum_host::event::ferrum_host_dispatch_event(raw, raw_scope);
            self.flush_disables();
            if BUS.with(|b| {
                b.borrow()
                    .scopes
                    .get(&raw_scope)
                    .is_some_and(|s| s.cancelled)
            }) {
                break;
            }
        }
        Ok(BUS.with(|b| {
            b.borrow_mut()
                .scopes
                .remove(&raw_scope)
                .expect("active shim event scope")
                .cancelled
        }))
    }
}
