//! 原生自定义事件协议；真实 shim 与 Rust 假 shim 均经由本宿主核心。

use crate::arena::Arena;
use ferrum_abi::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::OnceLock;

pub(crate) struct Hooks {
    pub main_thread: fn() -> bool,
    pub can_subscribe: fn(ModContextHandle) -> FerrumStatus,
    pub begin: fn(ModContextHandle) -> FerrumStatus,
    pub finish: fn(ModContextHandle),
    pub track_listener: fn(ModContextHandle, bool),
    pub track_provider: fn(ModContextHandle, bool),
    pub publisher_name: fn(ModContextHandle) -> Result<String, FerrumStatus>,
    pub enabled: fn(ModContextHandle) -> bool,
    pub publish_ll: Option<fn(EventScopeHandle, *mut FfiBool) -> FerrumStatus>,
    pub attach: fn(ModContextHandle, CustomListenerHandle, &str, EventPriority) -> FerrumStatus,
    pub detach: fn(CustomListenerHandle),
    pub log: fn(ModContextHandle, LogLevel, &str),
    pub set_error: fn(String),
    pub take_error: fn() -> Option<String>,
}
static HOOKS: OnceLock<Hooks> = OnceLock::new();
pub(crate) fn install(hooks: Hooks) {
    let _ = HOOKS.set(hooks);
}

struct Provider {
    owner: Option<ModContextHandle>,
    name: String,
    events: Vec<CustomEventHandle>,
}
struct Definition {
    provider: EventProviderHandle,
    name: String,
    cancellable: bool,
    fields: BTreeMap<String, EventValueKind>,
}
struct Listener {
    owner: ModContextHandle,
    name: String,
    priority: EventPriority,
    order: u64,
    callback: EventCallbackV1,
    warned: Cell<bool>,
    owned: Cell<bool>,
}
impl Drop for Listener {
    fn drop(&mut self) {
        // 非 Send 闭包只在主线程析构；线程退出时能力已失效则保留 env。
        if self.owned.get() && HOOKS.get().is_some_and(|h| (h.main_thread)()) {
            (self.callback.drop_fn)(self.callback.env);
        }
    }
}
#[derive(Clone)]
enum Value {
    I64(i64),
    U64(u64),
    F64(f64),
    Bool(bool),
    String(String),
    Bytes(Vec<u8>),
    Uuid(PlayerUuid),
}
struct Payload {
    event: CustomEventHandle,
    name: String,
    values: BTreeMap<String, Value>,
    cancellable: bool,
    cancelled: Cell<bool>,
}
struct Scope {
    owner: Option<ModContextHandle>,
    payload: Rc<Payload>,
}
struct Registry {
    providers: Arena<Provider>,
    events: Arena<Rc<Definition>>,
    names: BTreeMap<String, CustomEventHandle>,
    listeners: Arena<Rc<Listener>>,
    scopes: Arena<Scope>,
    next_order: u64,
}
impl Registry {
    const fn new() -> Self {
        Self {
            providers: Arena::new(),
            events: Arena::new(),
            names: BTreeMap::new(),
            listeners: Arena::new(),
            scopes: Arena::new(),
            next_order: 0,
        }
    }
}
thread_local! { static REGISTRY: RefCell<Registry> = const { RefCell::new(Registry::new()) }; }

fn fail(status: FerrumStatus, text: impl Into<String>) -> FerrumStatus {
    if let Some(h) = HOOKS.get() {
        (h.set_error)(text.into());
    }
    status
}
fn guard(f: impl FnOnce(&Hooks) -> FerrumStatus) -> FerrumStatus {
    let Some(h) = HOOKS.get() else {
        return crate::last_error::fail(
            FerrumStatus::SERVICE_UNAVAILABLE,
            "自定义事件 hooks 尚未安装",
        );
    };
    if !(h.main_thread)() {
        return fail(FerrumStatus::WRONG_THREAD, "通用事件必须在主线程调用");
    }
    crate::vtable::guard_api("自定义事件 API", || f(h))
}
/// `begin` / `finish` 的 RAII 配对：宿主代码 panic 展开时同样调用 `finish`。
struct Active<'h> {
    hooks: &'h Hooks,
    owner: ModContextHandle,
}
impl<'h> Active<'h> {
    fn begin(hooks: &'h Hooks, owner: ModContextHandle) -> Result<Self, FerrumStatus> {
        let status = (hooks.begin)(owner);
        if status.is_ok() {
            Ok(Self { hooks, owner })
        } else {
            Err(status)
        }
    }
}
impl Drop for Active<'_> {
    fn drop(&mut self) {
        (self.hooks.finish)(self.owner);
    }
}
fn text(value: FfiStr<'_>) -> Result<String, FerrumStatus> {
    if value.ptr.is_null() && value.len != 0 {
        return Err(FerrumStatus::NULL_POINTER);
    }
    // SAFETY: ABI 调用方提供本次同步调用内有效的字符串字节；空视图由辅助函数处理。
    unsafe { value.to_str() }
        .map(str::to_owned)
        .map_err(|_| FerrumStatus::INVALID_UTF8)
}
fn name(value: FfiStr<'_>) -> Result<String, FerrumStatus> {
    let value = text(value)?;
    if value.is_empty()
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-' | b':'))
    {
        return Err(fail(
            FerrumStatus::INVALID_ARGUMENT,
            "事件或字段名称必须是非空 ASCII 标识符",
        ));
    }
    Ok(value)
}
fn put<T>(out: *mut T, value: T) -> FerrumStatus {
    if out.is_null() {
        return FerrumStatus::NULL_POINTER;
    }
    // SAFETY: 非空 out 由 ABI 调用方提供，指向独占可写且匹配类型的内存。
    unsafe { out.write(value) };
    FerrumStatus::OK
}
fn write(out: *mut FfiByteWriter, bytes: &[u8]) -> FerrumStatus {
    // SAFETY: ABI 调用方提供有效的 writer 或空指针，仅在同步调用内借用。
    match unsafe { out.as_ref() } {
        Some(w) => w.write(bytes),
        None => FerrumStatus::NULL_POINTER,
    }
}
fn valid_priority(priority: EventPriority) -> bool {
    matches!(
        priority,
        EventPriority::HIGHEST
            | EventPriority::HIGH
            | EventPriority::NORMAL
            | EventPriority::LOW
            | EventPriority::LOWEST
    )
}

extern "C" fn open_provider(owner: FfiStr<'_>, out: *mut EventProviderHandle) -> FerrumStatus {
    guard(|_| {
        if out.is_null() {
            return FerrumStatus::NULL_POINTER;
        }
        let owner = match name(owner) {
            Ok(v) => v,
            Err(s) => return s,
        };
        REGISTRY.with(|r| {
            let mut r = r.borrow_mut();
            if r.providers.iter().any(|(_, p)| p.name == owner) {
                return FerrumStatus::ALREADY_EXISTS;
            }
            let h = EventProviderHandle(r.providers.insert(Provider {
                owner: None,
                name: owner,
                events: Vec::new(),
            }));
            put(out, h)
        })
    })
}
fn remove_event(r: &mut Registry, handle: CustomEventHandle) {
    if let Some(event) = r.events.remove(handle.0) {
        r.names.remove(&event.name);
    }
}
extern "C" fn close_provider(provider: EventProviderHandle) -> FerrumStatus {
    guard(|_| {
        REGISTRY.with(|r| {
            let mut r = r.borrow_mut();
            let Some(p) = r.providers.remove(provider.0) else {
                return FerrumStatus::STALE_HANDLE;
            };
            for handle in p.events {
                remove_event(&mut r, handle);
            }
            FerrumStatus::OK
        })
    })
}
extern "C" fn register_event(
    provider: EventProviderHandle,
    raw: *const CustomEventDefinitionV1<'_>,
    out: *mut CustomEventHandle,
) -> FerrumStatus {
    guard(|_| {
        if raw.is_null() || out.is_null() {
            return FerrumStatus::NULL_POINTER;
        }
        // SAFETY: ABI 调用方保证描述指针至少可读取 struct_size。
        if unsafe { (*raw).struct_size } < size_of::<CustomEventDefinitionV1<'_>>() as u32 {
            return FerrumStatus::ABI_INCOMPATIBLE;
        }
        // SAFETY: 已确认描述大小，调用方保证描述和借用字段在同步调用内有效。
        let raw = unsafe { &*raw };
        if raw.fields.ptr.is_null() && raw.fields.len != 0 {
            return FerrumStatus::NULL_POINTER;
        }
        let event_name = match name(raw.name) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let mut fields = BTreeMap::new();
        // SAFETY: fields 为调用方提供的已初始化字段数组，空数组由辅助函数处理。
        for field in unsafe { raw.fields.as_slice() } {
            let field_name = match name(field.name) {
                Ok(v) => v,
                Err(s) => return s,
            };
            if field.kind.0 > EventValueKind::UUID.0 {
                return FerrumStatus::UNSUPPORTED;
            }
            if field.reserved != 0 || fields.insert(field_name, field.kind).is_some() {
                return FerrumStatus::INVALID_ARGUMENT;
            }
        }
        REGISTRY.with(|r| {
            let mut r = r.borrow_mut();
            let Some(p) = r.providers.get(provider.0) else {
                return FerrumStatus::STALE_HANDLE;
            };
            if !event_name.starts_with(&format!("{}.", p.name))
                || event_name.len() <= p.name.len() + 1
            {
                return fail(
                    FerrumStatus::PERMISSION_DENIED,
                    "事件名称必须以发布者模组名加点号开头",
                );
            }
            if r.names.contains_key(&event_name) {
                return FerrumStatus::ALREADY_EXISTS;
            }
            let event = Definition {
                provider,
                name: event_name.clone(),
                cancellable: raw.cancellable.get(),
                fields,
            };
            let handle = CustomEventHandle(r.events.insert(Rc::new(event)));
            r.names.insert(event_name, handle);
            r.providers
                .get_mut(provider.0)
                .expect("checked provider")
                .events
                .push(handle);
            put(out, handle)
        })
    })
}
extern "C" fn unregister_event(
    provider: EventProviderHandle,
    handle: CustomEventHandle,
) -> FerrumStatus {
    guard(|_| {
        REGISTRY.with(|r| {
            let mut r = r.borrow_mut();
            if r.providers.get(provider.0).is_none() {
                return FerrumStatus::STALE_HANDLE;
            }
            match r.events.get(handle.0) {
                None => return FerrumStatus::STALE_HANDLE,
                Some(e) if e.provider != provider => return FerrumStatus::PERMISSION_DENIED,
                _ => {}
            }
            remove_event(&mut r, handle);
            r.providers
                .get_mut(provider.0)
                .expect("checked provider")
                .events
                .retain(|h| *h != handle);
            FerrumStatus::OK
        })
    })
}

fn decode(raw: &EventValueV1<'_>) -> Result<Value, FerrumStatus> {
    if raw.reserved != 0 || (raw.bytes.ptr.is_null() && raw.bytes.len != 0) {
        return Err(FerrumStatus::INVALID_ARGUMENT);
    }
    Ok(match raw.kind {
        EventValueKind::I64 => Value::I64(raw.scalar as i64),
        EventValueKind::U64 => Value::U64(raw.scalar),
        EventValueKind::F64 => Value::F64(f64::from_bits(raw.scalar)),
        EventValueKind::BOOL => Value::Bool(raw.scalar != 0),
        EventValueKind::STRING | EventValueKind::BYTES | EventValueKind::UUID => {
            // SAFETY: 发布者保证 bytes 在 publish 返回前有效，宿主在派发前复制。
            let bytes = unsafe { raw.bytes.as_slice() };
            match raw.kind {
                EventValueKind::STRING => Value::String(
                    std::str::from_utf8(bytes)
                        .map_err(|_| FerrumStatus::INVALID_UTF8)?
                        .to_owned(),
                ),
                EventValueKind::BYTES => Value::Bytes(bytes.to_vec()),
                _ => Value::Uuid(PlayerUuid {
                    bytes: bytes
                        .try_into()
                        .map_err(|_| FerrumStatus::INVALID_ARGUMENT)?,
                }),
            }
        }
        _ => return Err(FerrumStatus::UNSUPPORTED),
    })
}
extern "C" fn publish(
    provider: EventProviderHandle,
    event: CustomEventHandle,
    raw: FfiSlice<'_, EventValueV1<'_>>,
    out: *mut FfiBool,
) -> FerrumStatus {
    guard(|hooks| {
        if out.is_null() || (raw.ptr.is_null() && raw.len != 0) {
            return FerrumStatus::NULL_POINTER;
        }
        let def = REGISTRY.with(|r| {
            let r = r.borrow();
            if r.providers.get(provider.0).is_none() {
                return Err(FerrumStatus::STALE_HANDLE);
            }
            let def = r.events.get(event.0).ok_or(FerrumStatus::STALE_HANDLE)?;
            if def.provider != provider {
                return Err(FerrumStatus::PERMISSION_DENIED);
            }
            Ok(def.clone())
        });
        let def = match def {
            Ok(v) => v,
            Err(s) => return s,
        };
        if raw.len != def.fields.len() {
            return fail(
                FerrumStatus::INVALID_ARGUMENT,
                "发布字段必须完整匹配已注册描述",
            );
        }
        let mut values = BTreeMap::new();
        // SAFETY: ABI 发布者提供已初始化且有效的字段数组，空视图由辅助函数处理。
        for field in unsafe { raw.as_slice() } {
            let key = match name(field.name) {
                Ok(v) => v,
                Err(s) => return s,
            };
            if def.fields.get(&key) != Some(&field.kind) || values.contains_key(&key) {
                return fail(
                    FerrumStatus::INVALID_ARGUMENT,
                    "事件字段缺失、重复或类型不匹配",
                );
            }
            let value = match decode(field) {
                Ok(v) => v,
                Err(s) => return s,
            };
            values.insert(key, value);
        }
        let payload = Rc::new(Payload {
            event,
            name: def.name.clone(),
            values,
            cancellable: def.cancellable,
            cancelled: Cell::new(false),
        });
        if let Some(publish_ll) = hooks.publish_ll {
            let scope = REGISTRY.with(|r| {
                EventScopeHandle(r.borrow_mut().scopes.insert(Scope {
                    owner: None,
                    payload: payload.clone(),
                }))
            });
            let mut cancelled = FfiBool::FALSE;
            let status = publish_ll(scope, &mut cancelled);
            REGISTRY.with(|r| r.borrow_mut().scopes.remove(scope.0));
            if !status.is_ok() {
                return status;
            }
            if cancelled.get() && payload.cancellable {
                payload.cancelled.set(true);
            }
        } else {
            let mut snapshot = REGISTRY.with(|r| {
                r.borrow()
                    .listeners
                    .iter()
                    .filter(|(_, l)| l.name == def.name)
                    .map(|(h, l)| (l.priority.0, l.order, h))
                    .collect::<Vec<_>>()
            });
            snapshot.sort_unstable_by_key(|(p, order, _)| (*p, *order));
            for (_, _, handle) in snapshot {
                if !payload_active(&payload) {
                    break;
                }
                invoke_listener(hooks, handle, payload.clone());
                if payload.cancelled.get() {
                    break;
                }
            }
        }
        // 发布者在回调中停用或撤销事件时，禁止调用方继续原操作。
        if !REGISTRY.with(|r| r.borrow().events.get(event.0).is_some()) {
            return FerrumStatus::MOD_DISABLED;
        }
        put(out, FfiBool::from(payload.cancelled.get()))
    })
}

fn invoke_listener(hooks: &Hooks, handle: GenerationalHandle, payload: Rc<Payload>) {
    let listener = REGISTRY.with(|r| r.borrow().listeners.get(handle).cloned());
    let Some(listener) = listener else {
        return;
    };
    if listener.name != payload.name || !payload_active(&payload) {
        return;
    }
    let Ok(active) = Active::begin(hooks, listener.owner) else {
        return;
    };
    // 在守卫之后重新绑定：局部变量逆序析构，panic 展开时也先释放监听器
    // （可能执行 drop_fn），再结束活跃调用。
    let listener = listener;
    let scope = REGISTRY.with(|r| {
        EventScopeHandle(r.borrow_mut().scopes.insert(Scope {
            owner: Some(listener.owner),
            payload: payload.clone(),
        }))
    });
    let status =
        (listener.callback.invoke_fn)(listener.callback.env, scope, RawMainThreadToken::ASSERTED);
    REGISTRY.with(|r| r.borrow_mut().scopes.remove(scope.0));
    match status {
        FerrumStatus::OK | FerrumStatus::MOD_PANICKED => {}
        FerrumStatus::REENTRANT_CALL => {
            if !listener.warned.replace(true) {
                (hooks.log)(
                    listener.owner,
                    LogLevel::WARN,
                    "通用事件回调重入，已跳过本次调用",
                );
            }
        }
        FerrumStatus::CALLBACK_POISONED => (hooks.log)(
            listener.owner,
            LogLevel::ERROR,
            "通用事件回调已毒化，移除监听器",
        ),
        _ => {
            let detail = (hooks.take_error)().unwrap_or_default();
            let message = if detail.is_empty() {
                format!("通用事件回调 {status}")
            } else {
                format!("通用事件回调 {status}: {detail}")
            };
            (hooks.log)(listener.owner, LogLevel::ERROR, &message);
        }
    }
    if matches!(
        status,
        FerrumStatus::MOD_PANICKED | FerrumStatus::CALLBACK_POISONED
    ) {
        let removed = REGISTRY.with(|r| r.borrow_mut().listeners.remove(handle));
        if removed.is_some() {
            (hooks.detach)(CustomListenerHandle(handle));
            (hooks.track_listener)(listener.owner, false);
        }
        drop(removed);
    }
    // 先释放监听器，再结束活跃调用（可能触发推迟的停用）。
    drop(listener);
    drop(active);
}
fn payload_active(payload: &Payload) -> bool {
    REGISTRY.with(|r| {
        let r = r.borrow();
        let Some(def) = r.events.get(payload.event.0) else {
            return false;
        };
        let Some(provider) = r.providers.get(def.provider.0) else {
            return false;
        };
        provider
            .owner
            .is_none_or(|owner| HOOKS.get().is_some_and(|h| (h.enabled)(owner)))
    })
}
pub(crate) fn dispatch_listener(raw: u64, scope: EventScopeHandle) -> FerrumStatus {
    guard(|hooks| {
        let Some(handle) = GenerationalHandle::from_raw(raw) else {
            return FerrumStatus::STALE_HANDLE;
        };
        let payload = match view_payload(scope) {
            Ok(p) => p,
            Err(s) => return s,
        };
        invoke_listener(hooks, handle, payload);
        FerrumStatus::OK
    })
}
extern "C" fn take_error(out: *mut FfiByteWriter) -> FerrumStatus {
    let Some(h) = HOOKS.get() else {
        return FerrumStatus::SERVICE_UNAVAILABLE;
    };
    let text = (h.take_error)().unwrap_or_default();
    write(out, text.as_bytes())
}

fn rust_provider(
    owner: ModContextHandle,
    create: bool,
) -> Result<EventProviderHandle, FerrumStatus> {
    let hooks = HOOKS.get().ok_or(FerrumStatus::SERVICE_UNAVAILABLE)?;
    let status = (hooks.can_subscribe)(owner);
    if !status.is_ok() {
        return Err(status);
    }
    let existing = REGISTRY.with(|r| {
        r.borrow()
            .providers
            .iter()
            .find(|(_, p)| p.owner == Some(owner))
            .map(|(h, _)| EventProviderHandle(h))
    });
    if let Some(handle) = existing {
        return Ok(handle);
    }
    if !create {
        return Err(FerrumStatus::STALE_HANDLE);
    }
    let provider_name = (hooks.publisher_name)(owner)?;
    let mut out = std::mem::MaybeUninit::uninit();
    let status = open_provider(FfiStr::new(&provider_name), out.as_mut_ptr());
    if !status.is_ok() {
        return Err(status);
    }
    // SAFETY: open_provider 返回 OK 保证完整写出句柄。
    let provider: EventProviderHandle = unsafe { out.assume_init() };
    REGISTRY.with(|r| {
        r.borrow_mut()
            .providers
            .get_mut(provider.0)
            .expect("new provider")
            .owner = Some(owner)
    });
    (hooks.track_provider)(owner, true);
    Ok(provider)
}
extern "C" fn rust_register(
    owner: ModContextHandle,
    def: *const CustomEventDefinitionV1<'_>,
    out: *mut CustomEventHandle,
) -> FerrumStatus {
    guard(|_| match rust_provider(owner, true) {
        Ok(p) => register_event(p, def, out),
        Err(s) => s,
    })
}
extern "C" fn rust_unregister(owner: ModContextHandle, event: CustomEventHandle) -> FerrumStatus {
    guard(|_| match rust_provider(owner, false) {
        Ok(p) => unregister_event(p, event),
        Err(s) => s,
    })
}
extern "C" fn rust_publish(
    owner: ModContextHandle,
    event: CustomEventHandle,
    values: FfiSlice<'_, EventValueV1<'_>>,
    out: *mut FfiBool,
) -> FerrumStatus {
    guard(|hooks| {
        let provider = match rust_provider(owner, false) {
            Ok(p) => p,
            Err(s) => return s,
        };
        // 计入发布者的活跃调用，native 监听间接停用/卸载 Rust 发布者时遵循延迟排空。
        let _active = match Active::begin(hooks, owner) {
            Ok(a) => a,
            Err(s) => return s,
        };
        let status = publish(provider, event, values, out);
        if !(hooks.enabled)(owner) {
            return FerrumStatus::MOD_DISABLED;
        }
        status
    })
}
fn view_payload(scope: EventScopeHandle) -> Result<Rc<Payload>, FerrumStatus> {
    REGISTRY.with(|r| {
        let r = r.borrow();
        match r.scopes.get(scope.0) {
            Some(s) if s.owner.is_none() => Ok(s.payload.clone()),
            Some(_) => Err(FerrumStatus::PERMISSION_DENIED),
            None => Err(FerrumStatus::STALE_HANDLE),
        }
    })
}
fn view_field(scope: EventScopeHandle, key: FfiStr<'_>) -> Result<Value, FerrumStatus> {
    view_payload(scope)?
        .values
        .get(&text(key)?)
        .cloned()
        .ok_or(FerrumStatus::NOT_FOUND)
}
extern "C" fn view_name(scope: EventScopeHandle, out: *mut FfiByteWriter) -> FerrumStatus {
    guard(|_| match view_payload(scope) {
        Ok(p) => write(out, p.name.as_bytes()),
        Err(s) => s,
    })
}
macro_rules! view_getter {
    ($fn:ident, $variant:ident, $ty:ty) => {
        extern "C" fn $fn(scope: EventScopeHandle, key: FfiStr<'_>, out: *mut $ty) -> FerrumStatus {
            guard(|_| match view_field(scope, key) {
                Ok(Value::$variant(v)) => put(out, v.into()),
                Ok(_) => FerrumStatus::INVALID_ARGUMENT,
                Err(s) => s,
            })
        }
    };
}
view_getter!(view_i64, I64, i64);
view_getter!(view_u64, U64, u64);
view_getter!(view_f64, F64, f64);
view_getter!(view_bool, Bool, FfiBool);
view_getter!(view_uuid, Uuid, PlayerUuid);
extern "C" fn view_string(
    scope: EventScopeHandle,
    key: FfiStr<'_>,
    out: *mut FfiByteWriter,
) -> FerrumStatus {
    guard(|_| match view_field(scope, key) {
        Ok(Value::String(v)) => write(out, v.as_bytes()),
        Ok(_) => FerrumStatus::INVALID_ARGUMENT,
        Err(s) => s,
    })
}
extern "C" fn view_bytes(
    scope: EventScopeHandle,
    key: FfiStr<'_>,
    out: *mut FfiByteWriter,
) -> FerrumStatus {
    guard(|_| match view_field(scope, key) {
        Ok(Value::Bytes(v)) => write(out, &v),
        Ok(_) => FerrumStatus::INVALID_ARGUMENT,
        Err(s) => s,
    })
}
extern "C" fn view_cancelled(scope: EventScopeHandle, out: *mut FfiBool) -> FerrumStatus {
    guard(|_| match view_payload(scope) {
        Ok(p) => put(out, FfiBool::from(p.cancelled.get())),
        Err(s) => s,
    })
}
extern "C" fn view_cancel(scope: EventScopeHandle) -> FerrumStatus {
    guard(|_| match view_payload(scope) {
        Ok(p) if p.cancellable => {
            p.cancelled.set(true);
            FerrumStatus::OK
        }
        Ok(_) => FerrumStatus::UNSUPPORTED,
        Err(s) => s,
    })
}
extern "C" fn view_active(scope: EventScopeHandle) -> FerrumStatus {
    guard(|_| match view_payload(scope) {
        Ok(p) if payload_active(&p) => FerrumStatus::OK,
        Ok(_) => FerrumStatus::MOD_DISABLED,
        Err(s) => s,
    })
}
pub(crate) static VIEW_API: CustomEventViewApiV1 = CustomEventViewApiV1 {
    struct_size: size_of::<CustomEventViewApiV1>() as u32,
    name: view_name,
    get_i64: view_i64,
    get_u64: view_u64,
    get_f64: view_f64,
    get_bool: view_bool,
    get_string: view_string,
    get_bytes: view_bytes,
    get_uuid: view_uuid,
    is_cancelled: view_cancelled,
    cancel: view_cancel,
    is_active: view_active,
};

extern "C" fn subscribe(
    owner: ModContextHandle,
    event_name: FfiStr<'_>,
    priority: EventPriority,
    callback: EventCallbackV1,
    out: *mut CustomListenerHandle,
) -> FerrumStatus {
    guard(|hooks| {
        if out.is_null() {
            return FerrumStatus::NULL_POINTER;
        }
        let status = (hooks.can_subscribe)(owner);
        if !status.is_ok() {
            return status;
        }
        if !valid_priority(priority) {
            return FerrumStatus::INVALID_ARGUMENT;
        }
        let name = match name(event_name) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let handle = REGISTRY.with(|r| {
            let mut r = r.borrow_mut();
            r.next_order += 1;
            let listener = Listener {
                owner,
                name,
                priority,
                callback,
                order: r.next_order,
                warned: Cell::new(false),
                owned: Cell::new(false),
            };
            CustomListenerHandle(r.listeners.insert(Rc::new(listener)))
        });
        let listener = REGISTRY
            .with(|r| r.borrow().listeners.get(handle.0).cloned())
            .expect("new listener");
        let status = (hooks.attach)(owner, handle, &listener.name, priority);
        if !status.is_ok() {
            let removed = REGISTRY.with(|r| r.borrow_mut().listeners.remove(handle.0));
            drop(removed);
            return status;
        }
        listener.owned.set(true);
        (hooks.track_listener)(owner, true);
        put(out, handle)
    })
}
extern "C" fn unsubscribe(owner: ModContextHandle, handle: CustomListenerHandle) -> FerrumStatus {
    guard(|hooks| {
        let removed = REGISTRY.with(|r| {
            let mut r = r.borrow_mut();
            match r.listeners.get(handle.0) {
                None => Err(FerrumStatus::STALE_HANDLE),
                Some(l) if l.owner != owner => Err(FerrumStatus::PERMISSION_DENIED),
                _ => Ok(r.listeners.remove(handle.0)),
            }
        });
        match removed {
            Ok(l) => {
                (hooks.detach)(handle);
                (hooks.track_listener)(owner, false);
                drop(l);
                FerrumStatus::OK
            }
            Err(s) => s,
        }
    })
}
pub(crate) fn revoke(owner: ModContextHandle) {
    let removed = REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        let providers = r
            .providers
            .iter()
            .filter(|(_, p)| p.owner == Some(owner))
            .map(|(h, _)| h)
            .collect::<Vec<_>>();
        let count = providers.len();
        for h in providers {
            if let Some(p) = r.providers.remove(h) {
                for event in p.events {
                    remove_event(&mut r, event);
                }
            }
        }
        if let Some(hooks) = HOOKS.get() {
            for _ in 0..count {
                (hooks.track_provider)(owner, false);
            }
        }
        let handles = r
            .listeners
            .iter()
            .filter(|(_, l)| l.owner == owner)
            .map(|(h, _)| h)
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .filter_map(|h| r.listeners.remove(h).map(|l| (CustomListenerHandle(h), l)))
            .collect::<Vec<_>>()
    });
    if let Some(hooks) = HOOKS.get() {
        for (handle, _) in &removed {
            (hooks.detach)(*handle);
            (hooks.track_listener)(owner, false);
        }
    }
    drop(removed);
}
/// mock 在隔离实例之间撤销资源，保留槽位代数以拒绝旧句柄。
#[allow(dead_code)]
pub(crate) fn reset() {
    let removed = REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        let handles = r.listeners.iter().map(|(h, _)| h).collect::<Vec<_>>();
        let removed = handles
            .into_iter()
            .filter_map(|h| r.listeners.remove(h))
            .collect::<Vec<_>>();
        let events = r.events.iter().map(|(h, _)| h).collect::<Vec<_>>();
        for h in events {
            r.events.remove(h);
        }
        let providers = r.providers.iter().map(|(h, _)| h).collect::<Vec<_>>();
        for h in providers {
            r.providers.remove(h);
        }
        r.names.clear();
        removed
    });
    drop(removed);
}
fn payload(owner: ModContextHandle, handle: EventScopeHandle) -> Result<Rc<Payload>, FerrumStatus> {
    REGISTRY.with(|r| {
        let r = r.borrow();
        match r.scopes.get(handle.0) {
            None => Err(FerrumStatus::STALE_HANDLE),
            Some(s) if s.owner != Some(owner) => Err(FerrumStatus::PERMISSION_DENIED),
            Some(s) => Ok(s.payload.clone()),
        }
    })
}
fn field(
    owner: ModContextHandle,
    scope: EventScopeHandle,
    key: FfiStr<'_>,
) -> Result<Value, FerrumStatus> {
    let p = payload(owner, scope)?;
    p.values
        .get(&text(key)?)
        .cloned()
        .ok_or(FerrumStatus::NOT_FOUND)
}
extern "C" fn event_name(
    c: ModContextHandle,
    s: EventScopeHandle,
    out: *mut FfiByteWriter,
) -> FerrumStatus {
    guard(|_| match payload(c, s) {
        Ok(p) => write(out, p.name.as_bytes()),
        Err(s) => s,
    })
}
macro_rules! getter {
    ($fn:ident, $variant:ident, $ty:ty) => {
        extern "C" fn $fn(
            c: ModContextHandle,
            s: EventScopeHandle,
            key: FfiStr<'_>,
            out: *mut $ty,
        ) -> FerrumStatus {
            guard(|_| match field(c, s, key) {
                Ok(Value::$variant(v)) => put(out, v.into()),
                Ok(_) => FerrumStatus::INVALID_ARGUMENT,
                Err(s) => s,
            })
        }
    };
}
getter!(get_i64, I64, i64);
getter!(get_u64, U64, u64);
getter!(get_f64, F64, f64);
getter!(get_bool, Bool, FfiBool);
getter!(get_uuid, Uuid, PlayerUuid);
extern "C" fn get_string(
    c: ModContextHandle,
    s: EventScopeHandle,
    key: FfiStr<'_>,
    out: *mut FfiByteWriter,
) -> FerrumStatus {
    guard(|_| match field(c, s, key) {
        Ok(Value::String(v)) => write(out, v.as_bytes()),
        Ok(_) => FerrumStatus::INVALID_ARGUMENT,
        Err(s) => s,
    })
}
extern "C" fn get_bytes(
    c: ModContextHandle,
    s: EventScopeHandle,
    key: FfiStr<'_>,
    out: *mut FfiByteWriter,
) -> FerrumStatus {
    guard(|_| match field(c, s, key) {
        Ok(Value::Bytes(v)) => write(out, &v),
        Ok(_) => FerrumStatus::INVALID_ARGUMENT,
        Err(s) => s,
    })
}
extern "C" fn is_cancelled(
    c: ModContextHandle,
    s: EventScopeHandle,
    out: *mut FfiBool,
) -> FerrumStatus {
    guard(|_| match payload(c, s) {
        Ok(p) => put(out, FfiBool::from(p.cancelled.get())),
        Err(s) => s,
    })
}
extern "C" fn cancel(c: ModContextHandle, s: EventScopeHandle) -> FerrumStatus {
    guard(|_| match payload(c, s) {
        Ok(p) if p.cancellable => {
            p.cancelled.set(true);
            FerrumStatus::OK
        }
        Ok(_) => FerrumStatus::UNSUPPORTED,
        Err(s) => s,
    })
}
pub(crate) static NATIVE_API: NativeEventApiV1 = NativeEventApiV1 {
    struct_size: size_of::<NativeEventApiV1>() as u32,
    open_provider,
    close_provider,
    register_event,
    unregister_event,
    publish,
    take_last_error: take_error,
};
pub(crate) static API: CustomEventApiV1 = CustomEventApiV1 {
    struct_size: size_of::<CustomEventApiV1>() as u32,
    subscribe,
    unsubscribe,
    name: event_name,
    get_i64,
    get_u64,
    get_f64,
    get_bool,
    get_string,
    get_bytes,
    get_uuid,
    is_cancelled,
    cancel,
};
pub(crate) static PUBLISHER_API: CustomEventPublisherApiV1 = CustomEventPublisherApiV1 {
    struct_size: size_of::<CustomEventPublisherApiV1>() as u32,
    register_event: rust_register,
    unregister_event: rust_unregister,
    publish: rust_publish,
};
