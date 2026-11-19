use core::ffi::c_void;

use crate::{
    ActorRuntimeId, FerrumStatus, FfiBool, FfiByteWriter, FfiStr, GenerationalHandle,
    ModContextHandle, RawMainThreadToken,
};

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ListenerHandle(pub GenerationalHandle);

/// 仅在当前监听器的同步调用期间有效。
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct EventScopeHandle(pub GenerationalHandle);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EventKind(pub u32);

impl EventKind {
    pub const PLAYER_JOIN: Self = Self(0);
    pub const PLAYER_DISCONNECT: Self = Self(1);
    pub const PLAYER_CHAT: Self = Self(2);
    pub const EXECUTING_COMMAND: Self = Self(3);
    pub const EXECUTED_COMMAND: Self = Self(4);
    pub const SERVER_COMMAND_REGISTER: Self = Self(5);
    pub const ACTOR_HURT: Self = Self(6);
    pub const MOB_DIE: Self = Self(7);
    pub const CONSOLE_OUTPUTTING: Self = Self(8);
    pub const CONSOLE_OUTPUTTED: Self = Self(9);
    pub const PLAYER_ADD_EXPERIENCE: Self = Self(10);
    pub const PLAYER_ATTACK: Self = Self(11);
    pub const PLAYER_CHANGE_PERM: Self = Self(12);
    pub const PLAYER_CONNECT: Self = Self(13);
    pub const PLAYER_DESTROY_BLOCK: Self = Self(14);
    pub const PLAYER_DIE: Self = Self(15);
    pub const PLAYER_INTERACT_BLOCK: Self = Self(16);
    pub const PLAYER_JUMP: Self = Self(17);
    pub const PLAYER_PICK_UP_ITEM: Self = Self(18);
    pub const PLAYER_PLACING_BLOCK: Self = Self(19);
    pub const PLAYER_PLACED_BLOCK: Self = Self(20);
    pub const PLAYER_RESPAWN: Self = Self(21);
    pub const PLAYER_SNEAKING: Self = Self(22);
    pub const PLAYER_SNEAKED: Self = Self(23);
    pub const PLAYER_SPRINTING: Self = Self(24);
    pub const PLAYER_SPRINTED: Self = Self(25);
    pub const PLAYER_SWING: Self = Self(26);
    pub const PLAYER_USE_ITEM: Self = Self(27);
    pub const SERVER_STARTED: Self = Self(28);
    pub const SERVER_STOPPING: Self = Self(29);
    pub const SERVICE_REGISTER: Self = Self(30);
    pub const SERVICE_UNREGISTER: Self = Self(31);
    pub const BLOCK_CHANGED: Self = Self(32);
    pub const FIRE_SPREAD: Self = Self(33);
    pub const SERVER_LEVEL_TICK: Self = Self(34);
    pub const SPAWNING_MOB: Self = Self(35);
    pub const SPAWNED_MOB: Self = Self(36);
    pub const fn is_player_event(self) -> bool {
        matches!(self.0, 0..=2 | 10..=27)
    }

    pub const fn is_emitter_thread_event(self) -> bool {
        matches!(
            self,
            Self::CONSOLE_OUTPUTTING
                | Self::CONSOLE_OUTPUTTED
                | Self::SERVICE_REGISTER
                | Self::SERVICE_UNREGISTER
        )
    }

    pub const fn is_cancellable(self) -> bool {
        matches!(
            self,
            Self::PLAYER_JOIN
                | Self::PLAYER_CHAT
                | Self::EXECUTING_COMMAND
                | Self::ACTOR_HURT
                | Self::CONSOLE_OUTPUTTING
                | Self::PLAYER_ADD_EXPERIENCE
                | Self::PLAYER_ATTACK
                | Self::PLAYER_CHANGE_PERM
                | Self::PLAYER_CONNECT
                | Self::PLAYER_DESTROY_BLOCK
                | Self::PLAYER_INTERACT_BLOCK
                | Self::PLAYER_JUMP
                | Self::PLAYER_PICK_UP_ITEM
                | Self::PLAYER_PLACING_BLOCK
                | Self::PLAYER_SNEAKING
                | Self::PLAYER_SNEAKED
                | Self::PLAYER_USE_ITEM
                | Self::FIRE_SPREAD
                | Self::SPAWNING_MOB
        )
    }
}

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EventPriority(pub u32);

impl EventPriority {
    pub const HIGHEST: Self = Self(0);
    pub const HIGH: Self = Self(100);
    pub const NORMAL: Self = Self(200);
    pub const LOW: Self = Self(300);
    pub const LOWEST: Self = Self(400);
}

#[repr(C)]
pub struct EventSubscriptionV1 {
    pub struct_size: u32,
    pub kind: EventKind,
    pub priority: EventPriority,
}

/// 注册成功才接管 env；调用与析构都在主线程且不持有宿主锁。
#[repr(C)]
#[derive(Copy, Clone)]
pub struct EventCallbackV1 {
    pub env: *mut c_void,
    pub invoke_fn: extern "C" fn(*mut c_void, EventScopeHandle, RawMainThreadToken) -> FerrumStatus,
    pub drop_fn: extern "C" fn(*mut c_void),
}

/// 触发线程同步调用。两个字符串分别为 SNBT 和控制台消息。取消请求由调用者初始化为假。
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ThreadEventCallbackV1 {
    pub env: *mut c_void,
    pub invoke_fn:
        extern "C" fn(*mut c_void, FfiStr<'_>, FfiStr<'_>, FfiBool, *mut FfiBool) -> FerrumStatus,
    pub drop_fn: extern "C" fn(*mut c_void),
}

/// 主线程事件接口，通过 ferrum.event.v1 查询。取消会停止 LL 的后续监听。
#[repr(C)]
pub struct EventApiV1 {
    pub struct_size: u32,
    pub subscribe: extern "C" fn(
        ModContextHandle,
        *const EventSubscriptionV1,
        EventCallbackV1,
        *mut ListenerHandle,
    ) -> FerrumStatus,
    pub unsubscribe: extern "C" fn(ModContextHandle, ListenerHandle) -> FerrumStatus,
    pub player:
        extern "C" fn(ModContextHandle, EventScopeHandle, *mut ActorRuntimeId) -> FerrumStatus,
    pub is_cancelled:
        extern "C" fn(ModContextHandle, EventScopeHandle, *mut FfiBool) -> FerrumStatus,
    pub cancel: extern "C" fn(ModContextHandle, EventScopeHandle) -> FerrumStatus,
    pub chat_message:
        extern "C" fn(ModContextHandle, EventScopeHandle, *mut FfiByteWriter) -> FerrumStatus,
    pub payload_snbt:
        extern "C" fn(ModContextHandle, EventScopeHandle, *mut FfiByteWriter) -> FerrumStatus,
    pub actor:
        extern "C" fn(ModContextHandle, EventScopeHandle, *mut ActorRuntimeId) -> FerrumStatus,
    pub subscribe_sync: extern "C" fn(
        ModContextHandle,
        *const EventSubscriptionV1,
        ThreadEventCallbackV1,
        *mut ListenerHandle,
    ) -> FerrumStatus,
}
