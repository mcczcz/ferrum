//! LeviLamina 同步事件。监听器由模组托管，丢弃句柄不会注销监听。

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
    pub(crate) fn to_abi(self) -> ferrum_abi::EventPriority {
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

/// 在 LL 触发线程同步派发，可并发调用。
pub trait EmitterThreadEvent: sealed::Sealed {
    #[doc(hidden)]
    const KIND: EventKind;
}
pub trait ConsoleEvent: EmitterThreadEvent {}
pub trait PlayerEvent: MainThreadEvent {}
pub trait CancellableEvent: MainThreadEvent {}

pub trait ActorEvent: MainThreadEvent {}
pub trait MobEvent: ActorEvent {}

macro_rules! event {
    ($ty:ident, $kind:ident $(, $capability:ident)*) => {
        pub struct $ty;
        impl sealed::Sealed for $ty {}
        impl MainThreadEvent for $ty {
            const KIND: EventKind = EventKind::$kind;
        }
        $(impl $capability for $ty {})*
    };
}
event!(
    PlayerJoinEvent,
    PLAYER_JOIN,
    ActorEvent,
    MobEvent,
    PlayerEvent,
    CancellableEvent
);
event!(
    PlayerDisconnectEvent,
    PLAYER_DISCONNECT,
    ActorEvent,
    MobEvent,
    PlayerEvent
);
event!(
    PlayerChatEvent,
    PLAYER_CHAT,
    ActorEvent,
    MobEvent,
    PlayerEvent,
    CancellableEvent
);
event!(ExecutingCommandEvent, EXECUTING_COMMAND, CancellableEvent);
event!(ExecutedCommandEvent, EXECUTED_COMMAND);
event!(ServerCommandRegisterEvent, SERVER_COMMAND_REGISTER);
event!(ActorHurtEvent, ACTOR_HURT, ActorEvent, CancellableEvent);
event!(MobDieEvent, MOB_DIE, ActorEvent, MobEvent);
pub struct ConsoleOutputtingEvent;
impl sealed::Sealed for ConsoleOutputtingEvent {}
impl EmitterThreadEvent for ConsoleOutputtingEvent {
    const KIND: EventKind = EventKind::CONSOLE_OUTPUTTING;
}
impl ConsoleEvent for ConsoleOutputtingEvent {}
pub struct ConsoleOutputtedEvent;
impl sealed::Sealed for ConsoleOutputtedEvent {}
impl EmitterThreadEvent for ConsoleOutputtedEvent {
    const KIND: EventKind = EventKind::CONSOLE_OUTPUTTED;
}
impl ConsoleEvent for ConsoleOutputtedEvent {}
event!(
    PlayerAddExperienceEvent,
    PLAYER_ADD_EXPERIENCE,
    ActorEvent,
    MobEvent,
    PlayerEvent,
    CancellableEvent
);
event!(
    PlayerAttackEvent,
    PLAYER_ATTACK,
    ActorEvent,
    MobEvent,
    PlayerEvent,
    CancellableEvent
);
event!(
    PlayerChangePermEvent,
    PLAYER_CHANGE_PERM,
    ActorEvent,
    MobEvent,
    PlayerEvent,
    CancellableEvent
);
event!(
    PlayerConnectEvent,
    PLAYER_CONNECT,
    ActorEvent,
    MobEvent,
    PlayerEvent,
    CancellableEvent
);
event!(
    PlayerDestroyBlockEvent,
    PLAYER_DESTROY_BLOCK,
    ActorEvent,
    MobEvent,
    PlayerEvent,
    CancellableEvent
);
event!(
    PlayerDieEvent,
    PLAYER_DIE,
    ActorEvent,
    MobEvent,
    PlayerEvent
);
event!(
    PlayerInteractBlockEvent,
    PLAYER_INTERACT_BLOCK,
    ActorEvent,
    MobEvent,
    PlayerEvent,
    CancellableEvent
);
event!(
    PlayerJumpEvent,
    PLAYER_JUMP,
    ActorEvent,
    MobEvent,
    PlayerEvent,
    CancellableEvent
);
event!(
    PlayerPickUpItemEvent,
    PLAYER_PICK_UP_ITEM,
    ActorEvent,
    MobEvent,
    PlayerEvent,
    CancellableEvent
);
event!(
    PlayerPlacingBlockEvent,
    PLAYER_PLACING_BLOCK,
    ActorEvent,
    MobEvent,
    PlayerEvent,
    CancellableEvent
);
event!(
    PlayerPlacedBlockEvent,
    PLAYER_PLACED_BLOCK,
    ActorEvent,
    MobEvent,
    PlayerEvent
);
event!(
    PlayerRespawnEvent,
    PLAYER_RESPAWN,
    ActorEvent,
    MobEvent,
    PlayerEvent
);
event!(
    PlayerSneakingEvent,
    PLAYER_SNEAKING,
    ActorEvent,
    MobEvent,
    PlayerEvent,
    CancellableEvent
);
event!(
    PlayerSneakedEvent,
    PLAYER_SNEAKED,
    ActorEvent,
    MobEvent,
    PlayerEvent,
    CancellableEvent
);
event!(
    PlayerSprintingEvent,
    PLAYER_SPRINTING,
    ActorEvent,
    MobEvent,
    PlayerEvent
);
event!(
    PlayerSprintedEvent,
    PLAYER_SPRINTED,
    ActorEvent,
    MobEvent,
    PlayerEvent
);
event!(
    PlayerSwingEvent,
    PLAYER_SWING,
    ActorEvent,
    MobEvent,
    PlayerEvent
);
event!(
    PlayerUseItemEvent,
    PLAYER_USE_ITEM,
    ActorEvent,
    MobEvent,
    PlayerEvent,
    CancellableEvent
);
event!(ServerStartedEvent, SERVER_STARTED);
event!(ServerStoppingEvent, SERVER_STOPPING);
pub struct ServiceRegisterEvent;
impl sealed::Sealed for ServiceRegisterEvent {}
impl EmitterThreadEvent for ServiceRegisterEvent {
    const KIND: EventKind = EventKind::SERVICE_REGISTER;
}
pub struct ServiceUnregisterEvent;
impl sealed::Sealed for ServiceUnregisterEvent {}
impl EmitterThreadEvent for ServiceUnregisterEvent {
    const KIND: EventKind = EventKind::SERVICE_UNREGISTER;
}
event!(BlockChangedEvent, BLOCK_CHANGED);
event!(FireSpreadEvent, FIRE_SPREAD, CancellableEvent);
event!(ServerLevelTickEvent, SERVER_LEVEL_TICK);
event!(SpawningMobEvent, SPAWNING_MOB, CancellableEvent);
event!(SpawnedMobEvent, SPAWNED_MOB);

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
        self.subscribe_inner::<E, _, true>(priority, callback)
    }

    /// 只采集类型化字段，跳过事件 NBT 快照。
    pub fn subscribe_light<E: MainThreadEvent>(
        &self,
        _token: &MainThreadToken<'_>,
        priority: EventPriority,
        callback: impl for<'tick> FnMut(
            &LightEventCtx<'tick, E>,
            &'tick MainThreadToken<'tick>,
        ) -> Result<()> + 'static,
    ) -> Result<ListenerHandle> {
        self.subscribe_inner::<E, _, false>(priority, callback)
    }

    fn subscribe_inner<E, F, const WITH_PAYLOAD: bool>(&self, priority: EventPriority, callback: F) -> Result<ListenerHandle>
    where
        E: MainThreadEvent,
        F: for<'tick> FnMut(&EventCtx<'tick, E, WITH_PAYLOAD>, &'tick MainThreadToken<'tick>) -> Result<()>
            + 'static,
    {
        let subscribe = host::event_api()?.subscribe()?;
        let subscription = EventSubscriptionV1 {
            struct_size: size_of::<EventSubscriptionV1>() as u32,
            kind: E::KIND,
            priority: priority.to_abi(),
            include_payload: WITH_PAYLOAD.into(),
        };
        let env = Box::into_raw(Box::new(CallbackEnv {
            ctx: self.ctx,
            in_call: Cell::new(false),
            poisoned: Cell::new(false),
            callback: UnsafeCell::new(callback),
        }));
        let callback = EventCallbackV1 {
            env: env.cast(),
            invoke_fn: invoke::<E, F, WITH_PAYLOAD>,
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

/// 借用的数据在本次回调期间有效。此接口只提供值数据。
pub struct ThreadEventCtx<'a, E: EmitterThreadEvent> {
    payload: &'a str,
    message: &'a str,
    cancelled: bool,
    cancel_requested: bool,
    _event: PhantomData<E>,
}
impl<E: EmitterThreadEvent> ThreadEventCtx<'_, E> {
    pub fn payload_snbt(&self) -> &str {
        self.payload
    }
}
impl<E: ConsoleEvent> ThreadEventCtx<'_, E> {
    pub fn message(&self) -> &str {
        self.message
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled || self.cancel_requested
    }
}
impl ThreadEventCtx<'_, ConsoleOutputtingEvent> {
    pub fn cancel(&mut self) {
        self.cancel_requested = true;
    }
}

struct ThreadCallback<F> {
    ctx: ModContextHandle,
    callback: F,
    poisoned: std::sync::atomic::AtomicBool,
}

impl Events {
    /// 在主线程注册；LL 在事件产生的线程同步调用，可以从多个线程同时进入。
    pub fn subscribe_sync<E: EmitterThreadEvent>(
        &self,
        _token: &MainThreadToken<'_>,
        priority: EventPriority,
        callback: impl Fn(&mut ThreadEventCtx<'_, E>) -> Result<()> + Send + Sync + 'static,
    ) -> Result<ListenerHandle> {
        fn register<E, F>(
            ctx: ModContextHandle,
            priority: EventPriority,
            callback: F,
        ) -> Result<ListenerHandle>
        where
            E: EmitterThreadEvent,
            F: Fn(&mut ThreadEventCtx<'_, E>) -> Result<()> + Send + Sync + 'static,
        {
            let subscribe = host::event_api()?.subscribe_sync()?;
            let sub = EventSubscriptionV1 {
                struct_size: size_of::<EventSubscriptionV1>() as u32,
                kind: E::KIND,
                priority: priority.to_abi(),
                include_payload: FfiBool::TRUE,
            };
            let env = Box::into_raw(Box::new(ThreadCallback {
                ctx,
                callback,
                poisoned: false.into(),
            }));
            let callback = ferrum_abi::ThreadEventCallbackV1 {
                env: env.cast(),
                invoke_fn: invoke_thread::<E, F>,
                drop_fn: drop_thread::<F>,
            };
            let mut out = MaybeUninit::uninit();
            let status = subscribe(ctx, &sub, callback, out.as_mut_ptr());
            if !status.is_ok() {
                let error = host::error_from_status(status);
                drop_thread::<F>(env.cast());
                return Err(error);
            }
            // SAFETY: 注册成功时宿主初始化句柄并接管 env。
            Ok(unsafe { out.assume_init() })
        }
        register::<E, _>(self.ctx, priority, callback)
    }
}
extern "C" fn invoke_thread<E, F>(
    raw: *mut c_void,
    payload: ferrum_abi::FfiStr<'_>,
    message: ferrum_abi::FfiStr<'_>,
    cancelled: FfiBool,
    out: *mut FfiBool,
) -> FerrumStatus
where
    E: EmitterThreadEvent,
    F: Fn(&mut ThreadEventCtx<'_, E>) -> Result<()> + Send + Sync,
{
    use std::sync::atomic::Ordering;
    // SAFETY: 活跃派发保有 env，F 为 Sync，仅通过共享引用调用。
    let env = unsafe { &*raw.cast::<ThreadCallback<F>>() };
    if env.poisoned.load(Ordering::Acquire) {
        return FerrumStatus::CALLBACK_POISONED;
    }
    let status = firewall::firewall(env.ctx, || {
        // SAFETY: 两个视图都由宿主借出至本次调用返回。
        let (payload, message) = unsafe { (payload.to_str(), message.to_str()) };
        let utf8 = |_| crate::Error::new(FerrumStatus::INVALID_UTF8, "事件文本不是 UTF-8");
        let mut event = ThreadEventCtx {
            payload: payload.map_err(utf8)?,
            message: message.map_err(utf8)?,
            cancelled: cancelled.get(),
            cancel_requested: false,
            _event: PhantomData,
        };
        let result = (env.callback)(&mut event);
        // SAFETY: 宿主提供本次调用独占的可写 FfiBool。
        unsafe { out.write(event.cancel_requested.into()) };
        result
    });
    if status == FerrumStatus::MOD_PANICKED {
        env.poisoned.store(true, Ordering::Release);
    }
    status
}

extern "C" fn drop_thread<F>(raw: *mut c_void) {
    // SAFETY: 注册失败或最后一个活跃派发结束后，宿主只归还一次所有权。
    let env = unsafe { Box::from_raw(raw.cast::<ThreadCallback<F>>()) };
    let _ = firewall::firewall(env.ctx, || {
        drop(env);
        Ok(())
    });
}

struct CallbackEnv<F> {
    ctx: ModContextHandle,
    in_call: Cell<bool>,
    poisoned: Cell<bool>,
    callback: UnsafeCell<F>,
}

extern "C" fn invoke<E, F, const WITH_PAYLOAD: bool>(
    raw: *mut c_void,
    scope: EventScopeHandle,
    _token: RawMainThreadToken,
) -> FerrumStatus
where
    E: MainThreadEvent,
    F: for<'tick> FnMut(&EventCtx<'tick, E, WITH_PAYLOAD>, &'tick MainThreadToken<'tick>) -> Result<()> + 'static,
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
pub struct EventCtx<'tick, E: MainThreadEvent, const WITH_PAYLOAD: bool = true> {
    ctx: ModContextHandle,
    scope: EventScopeHandle,
    token: &'tick MainThreadToken<'tick>,
    _event: PhantomData<E>,
}

/// 轻量订阅提供事件类型对应的字段和取消操作。
pub type LightEventCtx<'tick, E> = EventCtx<'tick, E, false>;

impl<E: MainThreadEvent> EventCtx<'_, E, true> {
    /// LL 在本监听器收到事件时序列化的 SNBT 快照。字段取决于 LL 版本。
    pub fn payload_snbt(&self) -> Result<String> {
        let read = host::event_api()?.payload_snbt()?;
        host::read_string(|out| read(self.ctx, self.scope, out))
    }
}

impl<'tick, E: ActorEvent, const WITH_PAYLOAD: bool> EventCtx<'tick, E, WITH_PAYLOAD> {
    pub fn actor(&self) -> Result<crate::ActorRef<'tick>> {
        let mut id = ferrum_abi::ActorRuntimeId::default();
        host::check((host::event_api()?.actor()?)(self.ctx, self.scope, &mut id))?;
        Ok(crate::ActorRef::new(id, self.token))
    }
}

impl<'tick, E: MobEvent, const WITH_PAYLOAD: bool> EventCtx<'tick, E, WITH_PAYLOAD> {
    pub fn mob(&self) -> Result<crate::MobRef<'tick>> {
        Ok(crate::MobRef::from_actor(self.actor()?))
    }
}

impl<'tick, E: PlayerEvent, const WITH_PAYLOAD: bool> EventCtx<'tick, E, WITH_PAYLOAD> {
    pub fn player(&self) -> Result<PlayerRef<'tick>> {
        let mut id = ferrum_abi::ActorRuntimeId::default();
        host::check((host::event_api()?.player()?)(
            self.ctx, self.scope, &mut id,
        ))?;
        Ok(PlayerRef::new(id, self.token))
    }
}

impl<E: CancellableEvent, const WITH_PAYLOAD: bool> EventCtx<'_, E, WITH_PAYLOAD> {
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

impl<const WITH_PAYLOAD: bool> EventCtx<'_, PlayerChatEvent, WITH_PAYLOAD> {
    pub fn message(&self) -> Result<String> {
        let read = host::event_api()?.chat_message()?;
        host::read_string(|writer| read(self.ctx, self.scope, writer))
    }
}
