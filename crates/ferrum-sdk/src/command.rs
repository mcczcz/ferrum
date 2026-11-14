use crate::{MainThreadToken, PlayerRef, Result, firewall, host, player};
use ferrum_abi::{
    CommandCallbackV1, CommandDefinitionV1, CommandParamV1, CommandScopeHandle, FerrumStatus,
    FfiBool, FfiSlice, FfiStr, ModContextHandle, RawMainThreadToken,
};
use std::cell::{Cell, UnsafeCell};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::MaybeUninit;

pub use ferrum_abi::{CommandHandle, CommandParamKind, CommandPermission};

/// 命令注册入口。所有注册和注销显式要求主线程令牌。
#[derive(Copy, Clone)]
pub struct Commands {
    pub(crate) ctx: ModContextHandle,
}

impl Commands {
    pub fn register(
        &self,
        name: impl Into<String>,
        description: impl Into<String>,
        permission: CommandPermission,
    ) -> CommandBuilder {
        CommandBuilder {
            ctx: self.ctx,
            name: name.into(),
            description: description.into(),
            permission,
            params: Vec::new(),
        }
    }
    pub fn unregister(&self, token: &MainThreadToken<'_>, handle: CommandHandle) -> Result<()> {
        let _ = token;
        host::check((host::command_api()?.unregister)(self.ctx, handle))
    }
}

pub struct CommandBuilder {
    ctx: ModContextHandle,
    name: String,
    description: String,
    permission: CommandPermission,
    params: Vec<(String, CommandParamKind, bool)>,
}

impl CommandBuilder {
    pub fn required(mut self, name: impl Into<String>, kind: CommandParamKind) -> Self {
        self.params.push((name.into(), kind, false));
        self
    }
    pub fn optional(mut self, name: impl Into<String>, kind: CommandParamKind) -> Self {
        self.params.push((name.into(), kind, true));
        self
    }

    /// 最低 ABI 1.1。非 Send 闭包只在主线程执行及析构；模组停用自动注销。
    pub fn execute<F>(self, token: &MainThreadToken<'_>, callback: F) -> Result<CommandHandle>
    where
        F: for<'tick> FnMut(&CommandInvocation<'tick>, &'tick MainThreadToken<'tick>) -> Result<()>
            + 'static,
    {
        let _ = token;
        let api = host::command_api()?;
        let params: Vec<_> = self
            .params
            .iter()
            .map(|(name, kind, optional)| CommandParamV1 {
                name: FfiStr::new(name),
                kind: *kind,
                optional: (*optional).into(),
                reserved: [0; 3],
            })
            .collect();
        let definition = CommandDefinitionV1 {
            struct_size: size_of::<CommandDefinitionV1<'_>>() as u32,
            name: FfiStr::new(&self.name),
            description: FfiStr::new(&self.description),
            permission: self.permission,
            params: FfiSlice::new(&params),
        };
        let env = Box::into_raw(Box::new(CallbackEnv {
            ctx: self.ctx,
            in_call: Cell::new(false),
            poisoned: Cell::new(false),
            callback: UnsafeCell::new(callback),
        }));
        let callback = CommandCallbackV1 {
            env: env.cast(),
            invoke_fn: invoke::<F>,
            drop_fn: drop_callback::<F>,
        };
        let mut out = MaybeUninit::<CommandHandle>::uninit();
        let status = (api.register)(self.ctx, &definition, callback, out.as_mut_ptr());
        if !status.is_ok() {
            let error = host::error_from_status(status);
            drop_callback::<F>(env.cast());
            return Err(error);
        }
        // SAFETY: register 返回 OK 保证完整写入 CommandHandle，并接管 env。
        Ok(unsafe { out.assume_init() })
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
    scope: CommandScopeHandle,
    _raw_token: RawMainThreadToken,
) -> FerrumStatus
where
    F: for<'tick> FnMut(&CommandInvocation<'tick>, &'tick MainThreadToken<'tick>) -> Result<()>
        + 'static,
{
    // SAFETY: 宿主托管 SDK 分配的 env，活跃派发持有强引用，且入口仅在主线程。
    let env = unsafe { &*raw.cast::<CallbackEnv<F>>() };
    if env.poisoned.get() {
        return FerrumStatus::CALLBACK_POISONED;
    }
    if env.in_call.replace(true) {
        return FerrumStatus::REENTRANT_CALL;
    }
    let status = firewall::firewall(env.ctx, || {
        let token = MainThreadToken::assume_main_thread();
        let invocation = CommandInvocation {
            ctx: env.ctx,
            scope,
            token: &token,
            _scope: PhantomData,
        };
        // SAFETY: in_call 拒绝同一闭包重入，主线程串行执行，当前 env 不会被提前析构。
        (unsafe { &mut *env.callback.get() })(&invocation, &token)
    });
    env.in_call.set(false);
    if status == FerrumStatus::MOD_PANICKED {
        env.poisoned.set(true);
    }
    status
}

extern "C" fn drop_callback<F>(raw: *mut c_void) {
    // SAFETY: 宿主仅在主线程且无活跃调用后归还一次 env；注册失败由 SDK 自行归还。
    let ctx = unsafe { (*raw.cast::<CallbackEnv<F>>()).ctx };
    let _ = firewall::firewall(ctx, || {
        // SAFETY: raw 来自同一 DLL 的 Box::into_raw，所有权已独占归还。
        drop(unsafe { Box::from_raw(raw.cast::<CallbackEnv<F>>()) });
        Ok(())
    });
}

/// 一次命令调用的参数、来源与输出；不能跨线程或离开本次回调。
pub struct CommandInvocation<'tick> {
    ctx: ModContextHandle,
    scope: CommandScopeHandle,
    token: &'tick MainThreadToken<'tick>,
    _scope: PhantomData<*mut ()>,
}

fn optional<T>(status: FerrumStatus, value: T) -> Result<Option<T>> {
    if status == FerrumStatus::NOT_FOUND {
        Ok(None)
    } else {
        host::check(status)?;
        Ok(Some(value))
    }
}

impl<'tick> CommandInvocation<'tick> {
    pub fn player(&self) -> Result<Option<PlayerRef<'tick>>> {
        let mut id = ferrum_abi::ActorRuntimeId::default();
        optional(
            (host::command_api()?.origin_player)(self.ctx, self.scope, &mut id),
            id,
        )
        .map(|p| p.map(|id| PlayerRef::new(id, self.token)))
    }
    pub fn int(&self, name: &str) -> Result<Option<i32>> {
        let mut out = 0;
        let s = (host::command_api()?.get_int)(self.ctx, self.scope, name.into(), &mut out);
        optional(s, out)
    }
    pub fn float(&self, name: &str) -> Result<Option<f32>> {
        let mut out = 0.;
        let s = (host::command_api()?.get_float)(self.ctx, self.scope, name.into(), &mut out);
        optional(s, out)
    }
    pub fn bool(&self, name: &str) -> Result<Option<bool>> {
        let mut out = FfiBool::FALSE;
        let s = (host::command_api()?.get_bool)(self.ctx, self.scope, name.into(), &mut out);
        optional(s, out.get())
    }
    pub fn string(&self, name: &str) -> Result<Option<String>> {
        let api = host::command_api()?;
        match host::read_bytes(|w| (api.get_string)(self.ctx, self.scope, name.into(), w)) {
            Ok(bytes) => Ok(Some(String::from_utf8(bytes)?)),
            Err(FerrumStatus::NOT_FOUND) => Ok(None),
            Err(s) => Err(host::error_from_status(s)),
        }
    }
    pub fn players(&self, name: &str) -> Result<Option<Vec<PlayerRef<'tick>>>> {
        let api = host::command_api()?;
        match host::read_bytes(|w| (api.get_players)(self.ctx, self.scope, name.into(), w)) {
            Ok(bytes) => player::decode_players(bytes, self.token).map(Some),
            Err(FerrumStatus::NOT_FOUND) => Ok(None),
            Err(s) => Err(host::error_from_status(s)),
        }
    }
    pub fn success(&self, text: impl AsRef<str>) -> Result<()> {
        self.output(false, text.as_ref())
    }
    pub fn error(&self, text: impl AsRef<str>) -> Result<()> {
        self.output(true, text.as_ref())
    }
    fn output(&self, error: bool, text: &str) -> Result<()> {
        host::check((host::command_api()?.output)(
            self.ctx,
            self.scope,
            error.into(),
            text.into(),
        ))
    }
}
