//! 第三方原生模组主动注册和发布的主线程事件，独立于 LL 内建事件。
use crate::events::EventPriority;
use crate::{MainThreadToken, PlayerUuid, Result, firewall, host};
pub use ferrum_abi::CustomListenerHandle;
use ferrum_abi::{
    EventCallbackV1, EventScopeHandle, FerrumStatus, FfiBool, FfiStr, ModContextHandle,
    RawMainThreadToken,
};
use std::cell::{Cell, UnsafeCell};
use std::ffi::c_void;
use std::mem::MaybeUninit;

#[derive(Copy, Clone)]
pub struct CustomEvents {
    pub(crate) ctx: ModContextHandle,
}
impl CustomEvents {
    /// 注册当前模组的事件；name 必须以 manifest 模组名加点号开头。
    /// 描述被宿主复制；停用时撤销，重新启用必须重新注册。
    pub fn register(
        &self,
        token: &MainThreadToken<'_>,
        name: &str,
        cancellable: bool,
        fields: &[(&str, EventFieldType)],
    ) -> Result<RegisteredEvent> {
        let _ = token;
        let register = host::custom_event_publisher_api()?.register_event()?;
        let fields: Vec<_> = fields
            .iter()
            .map(|(name, kind)| ferrum_abi::EventFieldV1 {
                name: FfiStr::new(name),
                kind: kind.to_abi(),
                reserved: 0,
            })
            .collect();
        let definition = ferrum_abi::CustomEventDefinitionV1 {
            struct_size: size_of::<ferrum_abi::CustomEventDefinitionV1<'_>>() as u32,
            name: FfiStr::new(name),
            cancellable: cancellable.into(),
            fields: ferrum_abi::FfiSlice::new(&fields),
        };
        let mut out = MaybeUninit::uninit();
        host::check(register(self.ctx, &definition, out.as_mut_ptr()))?;
        // SAFETY: register 返回 OK 保证写出完整句柄。
        let handle = unsafe { out.assume_init() };
        Ok(RegisteredEvent {
            ctx: self.ctx,
            handle,
        })
    }

    /// 允许早于发布者注册；名称应包含发布者命名空间及协议版本。
    pub fn subscribe<F>(
        &self,
        token: &MainThreadToken<'_>,
        name: &str,
        priority: EventPriority,
        callback: F,
    ) -> Result<CustomListenerHandle>
    where
        F: for<'tick> FnMut(&CustomEventCtx<'tick>, &'tick MainThreadToken<'tick>) -> Result<()>
            + 'static,
    {
        let _ = token;
        let subscribe = host::custom_event_api()?.subscribe()?;
        let env = Box::into_raw(Box::new(CallbackEnv {
            ctx: self.ctx,
            in_call: Cell::new(false),
            poisoned: Cell::new(false),
            callback: UnsafeCell::new(callback),
        }));
        let callback = EventCallbackV1 {
            env: env.cast(),
            invoke_fn: invoke::<F>,
            drop_fn: drop_callback::<F>,
        };
        let mut out = MaybeUninit::uninit();
        let status = subscribe(
            self.ctx,
            FfiStr::new(name),
            priority.to_abi(),
            callback,
            out.as_mut_ptr(),
        );
        if !status.is_ok() {
            let error = host::error_from_status(status);
            drop_callback::<F>(env.cast());
            return Err(error);
        }
        // SAFETY: OK 保证宿主完整写入监听器句柄并接管闭包。
        Ok(unsafe { out.assume_init() })
    }
    pub fn unsubscribe(
        &self,
        token: &MainThreadToken<'_>,
        listener: CustomListenerHandle,
    ) -> Result<()> {
        let _ = token;
        host::check((host::custom_event_api()?.unsubscribe()?)(
            self.ctx, listener,
        ))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EventFieldType {
    I64,
    U64,
    F64,
    Bool,
    String,
    Bytes,
    Uuid,
}
impl EventFieldType {
    fn to_abi(self) -> ferrum_abi::EventValueKind {
        use ferrum_abi::EventValueKind as K;
        match self {
            Self::I64 => K::I64,
            Self::U64 => K::U64,
            Self::F64 => K::F64,
            Self::Bool => K::BOOL,
            Self::String => K::STRING,
            Self::Bytes => K::BYTES,
            Self::Uuid => K::UUID,
        }
    }
}

/// 同步发布的字段值，宿主在调用监听器前复制所有借用数据。
#[derive(Clone, Debug)]
pub enum EventValue<'a> {
    I64(i64),
    U64(u64),
    F64(f64),
    Bool(bool),
    String(&'a str),
    Bytes(&'a [u8]),
    Uuid(PlayerUuid),
}
impl EventValue<'_> {
    fn to_abi<'a>(&'a self, name: &'a str) -> ferrum_abi::EventValueV1<'a> {
        use ferrum_abi::{EventValueKind as K, FfiSlice};
        let (kind, scalar, bytes): (_, _, &[u8]) = match self {
            Self::I64(v) => (K::I64, *v as u64, &[]),
            Self::U64(v) => (K::U64, *v, &[]),
            Self::F64(v) => (K::F64, v.to_bits(), &[]),
            Self::Bool(v) => (K::BOOL, u64::from(*v), &[]),
            Self::String(v) => (K::STRING, 0, v.as_bytes()),
            Self::Bytes(v) => (K::BYTES, 0, v),
            Self::Uuid(v) => (K::UUID, 0, &v.bytes),
        };
        ferrum_abi::EventValueV1 {
            name: FfiStr::new(name),
            kind,
            reserved: 0,
            scalar,
            bytes: FfiSlice::new(bytes),
        }
    }
}

/// 已注册事件的轻量句柄；丢弃不注销，停用时由宿主自动撤销。
#[derive(Copy, Clone, Debug)]
pub struct RegisteredEvent {
    ctx: ModContextHandle,
    handle: ferrum_abi::CustomEventHandle,
}
impl RegisteredEvent {
    /// 返回是否取消；只能在模组 Enabled 时发布，所有声明字段必须完整提供。
    pub fn publish(
        &self,
        token: &MainThreadToken<'_>,
        values: &[(&str, EventValue<'_>)],
    ) -> Result<bool> {
        let _ = token;
        let publish = host::custom_event_publisher_api()?.publish()?;
        let values = values
            .iter()
            .map(|(name, value)| value.to_abi(name))
            .collect::<Vec<_>>();
        let mut cancelled = FfiBool::FALSE;
        host::check(publish(
            self.ctx,
            self.handle,
            ferrum_abi::FfiSlice::new(&values),
            &mut cancelled,
        ))?;
        Ok(cancelled.get())
    }
    pub fn unregister(&self, token: &MainThreadToken<'_>) -> Result<()> {
        let _ = token;
        host::check((host::custom_event_publisher_api()?.unregister_event()?)(
            self.ctx,
            self.handle,
        ))
    }
}
struct CallbackEnv<F> {
    ctx: ModContextHandle,
    in_call: Cell<bool>,
    poisoned: Cell<bool>,
    callback: UnsafeCell<F>,
}
extern "C" fn invoke<F>(
    raw: *mut c_void,
    scope: EventScopeHandle,
    _token: RawMainThreadToken,
) -> FerrumStatus
where
    F: for<'tick> FnMut(&CustomEventCtx<'tick>, &'tick MainThreadToken<'tick>) -> Result<()>
        + 'static,
{
    // SAFETY: 主线程派发且宿主持有闭包强引用，raw 指向本 DLL 分配的有效 env。
    let env = unsafe { &*raw.cast::<CallbackEnv<F>>() };
    if env.poisoned.get() {
        return FerrumStatus::CALLBACK_POISONED;
    }
    if env.in_call.replace(true) {
        return FerrumStatus::REENTRANT_CALL;
    }
    let status = firewall::firewall(env.ctx, || {
        let token = MainThreadToken::assume_main_thread();
        let event = CustomEventCtx {
            ctx: env.ctx,
            scope,
            _token: &token,
        };
        // SAFETY: in_call 排除同一 FnMut 重入，主线程串行执行，闭包在调用结束前有效。
        (unsafe { &mut *env.callback.get() })(&event, &token)
    });
    env.in_call.set(false);
    if status == FerrumStatus::MOD_PANICKED {
        env.poisoned.set(true);
    }
    status
}
extern "C" fn drop_callback<F>(raw: *mut c_void) {
    // SAFETY: 注册失败由 SDK 回收，否则宿主仅在主线程且无活跃调用时归还一次所有权。
    let ctx = unsafe { (*raw.cast::<CallbackEnv<F>>()).ctx };
    let _ = firewall::firewall(ctx, || {
        // SAFETY: 来自同一 DLL 的 Box::into_raw，所有权独占归还。
        drop(unsafe { Box::from_raw(raw.cast::<CallbackEnv<F>>()) });
        Ok(())
    });
}

/// 当前监听器的作用域视图。变长字段返回模组自身拥有的副本。
pub struct CustomEventCtx<'tick> {
    ctx: ModContextHandle,
    scope: EventScopeHandle,
    _token: &'tick MainThreadToken<'tick>,
}
macro_rules! getter {
    ($fn:ident, $slot:ident, $ty:ty) => {
        pub fn $fn(&self, field: &str) -> Result<$ty> {
            let mut out = MaybeUninit::<$ty>::uninit();
            host::check((host::custom_event_api()?.$slot()?)(
                self.ctx,
                self.scope,
                FfiStr::new(field),
                out.as_mut_ptr(),
            ))?;
            // SAFETY: 对应字段 getter 返回 OK 保证写入匹配类型的完整值。
            Ok(unsafe { out.assume_init() })
        }
    };
}
impl CustomEventCtx<'_> {
    pub fn name(&self) -> Result<String> {
        let read = host::custom_event_api()?.name()?;
        host::read_string(|out| read(self.ctx, self.scope, out))
    }
    getter!(i64, get_i64, i64);
    getter!(u64, get_u64, u64);
    getter!(f64, get_f64, f64);
    getter!(uuid, get_uuid, PlayerUuid);
    pub fn bool(&self, field: &str) -> Result<bool> {
        let mut out = FfiBool::FALSE;
        host::check((host::custom_event_api()?.get_bool()?)(
            self.ctx,
            self.scope,
            FfiStr::new(field),
            &mut out,
        ))?;
        Ok(out.get())
    }
    pub fn string(&self, field: &str) -> Result<String> {
        let read = host::custom_event_api()?.get_string()?;
        host::read_string(|out| read(self.ctx, self.scope, FfiStr::new(field), out))
    }
    pub fn bytes(&self, field: &str) -> Result<Vec<u8>> {
        let read = host::custom_event_api()?.get_bytes()?;
        host::read_bytes(|out| read(self.ctx, self.scope, FfiStr::new(field), out))
            .map_err(host::error_from_status)
    }
    pub fn is_cancelled(&self) -> Result<bool> {
        let mut out = FfiBool::FALSE;
        host::check((host::custom_event_api()?.is_cancelled()?)(
            self.ctx, self.scope, &mut out,
        ))?;
        Ok(out.get())
    }
    /// 不可取消事件返回 Unsupported；取消后跳过本次后续通用监听器。
    pub fn cancel(&self) -> Result<()> {
        host::check((host::custom_event_api()?.cancel()?)(self.ctx, self.scope))
    }
}
