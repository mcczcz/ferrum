//! 同步主线程事件。监听器由模组托管，丢弃句柄不会注销监听。

use std::cell::{Cell, UnsafeCell};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::MaybeUninit;

use ferrum_abi::{
    EventCallbackV1, EventKind, EventScopeHandle, EventSubscriptionV1, FerrumStatus, FfiBool,
    ModContextHandle, RawMainThreadToken,
};

use crate::{MainThreadToken, PlayerRef, Result, firewall, host, sealed};

pub use ferrum_abi::ListenerHandle;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum EventPriority {
    Highest,
    High,
    #[default]
    Normal,
    Low,
    Lowest,
}

impl EventPriority {
    fn to_abi(self) -> ferrum_abi::EventPriority {
        use ferrum_abi::EventPriority as P;
        match self {
            Self::Highest => P::HIGHEST,
            Self::High => P::HIGH,
            Self::Normal => P::NORMAL,
            Self::Low => P::LOW,
            Self::Lowest => P::LOWEST,
        }
    }
}

/// SDK 定义的主线程事件；回调可以捕获非 Send 状态。
pub trait MainThreadEvent: sealed::Sealed {
    #[doc(hidden)]
    const KIND: EventKind;
}

pub trait PlayerEvent: MainThreadEvent {}
pub trait CancellableEvent: MainThreadEvent {}

/// 玩家初始化阶段事件；取消不等同于踢出或拒绝登录。
pub struct PlayerJoinEvent;
/// 玩家断开连接前事件，不可取消。
pub struct PlayerDisconnectEvent;
/// 玩家聊天事件，支持读取消息和取消。
pub struct PlayerChatEvent;

macro_rules! player_event {
    ($ty:ident, $kind:ident) => {
        impl sealed::Sealed for $ty {}
        impl MainThreadEvent for $ty {
            const KIND: EventKind = EventKind::$kind;
        }
        impl PlayerEvent for $ty {}
    };
}
player_event!(PlayerJoinEvent, PLAYER_JOIN);
player_event!(PlayerDisconnectEvent, PLAYER_DISCONNECT);
player_event!(PlayerChatEvent, PLAYER_CHAT);
impl CancellableEvent for PlayerJoinEvent {}
impl CancellableEvent for PlayerChatEvent {}

#[derive(Copy, Clone)]
pub struct Events {
    pub(crate) ctx: ModContextHandle,
}

impl Events {
    /// 仅在 on_enable 或已启用模组的主线程回调中注册；停用时自动清理。
    pub fn subscribe<E: MainThreadEvent>(
        &self,
        token: &MainThreadToken<'_>,
        priority: EventPriority,
        callback: impl for<'tick> FnMut(
            &EventCtx<'tick, E>,
            &'tick MainThreadToken<'tick>,
        ) -> Result<()>
        + 'static,
    ) -> Result<ListenerHandle> {
        let _ = token;
        self.subscribe_inner::<E, _>(priority, callback)
    }

    fn subscribe_inner<E, F>(&self, priority: EventPriority, callback: F) -> Result<ListenerHandle>
    where
        E: MainThreadEvent,
        F: for<'tick> FnMut(&EventCtx<'tick, E>, &'tick MainThreadToken<'tick>) -> Result<()>
            + 'static,
    {
        let subscribe = host::event_api()?.subscribe()?;
        let subscription = EventSubscriptionV1 {
            struct_size: size_of::<EventSubscriptionV1>() as u32,
            kind: E::KIND,
            priority: priority.to_abi(),
        };
        let env = Box::into_raw(Box::new(CallbackEnv {
            ctx: self.ctx,
            in_call: Cell::new(false),
            poisoned: Cell::new(false),
            callback: UnsafeCell::new(callback),
        }));
        let callback = EventCallbackV1 {
            env: env.cast(),
            invoke_fn: invoke::<E, F>,
            drop_fn: drop_callback::<F>,
        };
        let mut out = MaybeUninit::<ListenerHandle>::uninit();
        let status = subscribe(self.ctx, &subscription, callback, out.as_mut_ptr());
        if !status.is_ok() {
            let error = host::error_from_status(status);
            drop_callback::<F>(env.cast());
            return Err(error);
        }
        // SAFETY: subscribe 返回 OK 保证写入完整句柄并接管 env。
        Ok(unsafe { out.assume_init() })
    }

    pub fn unsubscribe(&self, token: &MainThreadToken<'_>, listener: ListenerHandle) -> Result<()> {
        let _ = token;
        host::check((host::event_api()?.unsubscribe()?)(self.ctx, listener))
    }
}

struct CallbackEnv<F> {
    ctx: ModContextHandle,
    in_call: Cell<bool>,
    poisoned: Cell<bool>,
    callback: UnsafeCell<F>,
}

extern "C" fn invoke<E, F>(
    raw: *mut c_void,
    scope: EventScopeHandle,
    _token: RawMainThreadToken,
) -> FerrumStatus
where
    E: MainThreadEvent,
    F: for<'tick> FnMut(&EventCtx<'tick, E>, &'tick MainThreadToken<'tick>) -> Result<()> + 'static,
{
    // SAFETY: 宿主只在主线程调用，且活跃派发保有 env 所有者的强引用。
    let env = unsafe { &*raw.cast::<CallbackEnv<F>>() };
    if env.poisoned.get() {
        return FerrumStatus::CALLBACK_POISONED;
    }
    if env.in_call.replace(true) {
        return FerrumStatus::REENTRANT_CALL;
    }
    let status = firewall::firewall(env.ctx, || {
        let token = MainThreadToken::assume_main_thread();
        let event = EventCtx {
            ctx: env.ctx,
            scope,
            token: &token,
            _event: PhantomData,
        };
        // SAFETY: 主线程串行调用，in_call 排除重入，env 在本次调用完成前有效。
        (unsafe { &mut *env.callback.get() })(&event, &token)
    });
    env.in_call.set(false);
    if status == FerrumStatus::MOD_PANICKED {
        env.poisoned.set(true);
    }
    status
}

extern "C" fn drop_callback<F>(raw: *mut c_void) {
    // SAFETY: 注册失败由 SDK 回收，否则宿主在主线程且无活跃调用后仅归还一次。
    let ctx = unsafe { (*raw.cast::<CallbackEnv<F>>()).ctx };
    let _ = firewall::firewall(ctx, || {
        // SAFETY: env 来自本 DLL 的 Box::into_raw，所有权已独占归还。
        drop(unsafe { Box::from_raw(raw.cast::<CallbackEnv<F>>()) });
        Ok(())
    });
}

/// 仅在当前回调内有效，不能跨线程或跨 Tick 保存。
pub struct EventCtx<'tick, E: MainThreadEvent> {
    ctx: ModContextHandle,
    scope: EventScopeHandle,
    token: &'tick MainThreadToken<'tick>,
    _event: PhantomData<E>,
}

impl<'tick, E: PlayerEvent> EventCtx<'tick, E> {
    pub fn player(&self) -> Result<PlayerRef<'tick>> {
        let mut id = ferrum_abi::ActorRuntimeId::default();
        host::check((host::event_api()?.player()?)(
            self.ctx, self.scope, &mut id,
        ))?;
        Ok(PlayerRef::new(id, self.token))
    }
}

impl<E: CancellableEvent> EventCtx<'_, E> {
    /// 同步取消底层行为并停止后续 LL 监听器；已完成的修改不会回滚。
    pub fn cancel(&self) -> Result<()> {
        host::check((host::event_api()?.cancel()?)(self.ctx, self.scope))
    }

    pub fn is_cancelled(&self) -> Result<bool> {
        let mut out = FfiBool::FALSE;
        host::check((host::event_api()?.is_cancelled()?)(
            self.ctx, self.scope, &mut out,
        ))?;
        Ok(out.get())
    }
}

impl EventCtx<'_, PlayerChatEvent> {
    pub fn message(&self) -> Result<String> {
        let read = host::event_api()?.chat_message()?;
        host::read_string(|writer| read(self.ctx, self.scope, writer))
    }
}
