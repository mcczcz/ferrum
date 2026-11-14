use core::ffi::c_void;

use crate::{
    ActorRuntimeId, FerrumStatus, FfiBool, FfiByteWriter, GenerationalHandle, ModContextHandle,
    RawMainThreadToken,
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
}
