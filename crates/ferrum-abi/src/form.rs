use core::ffi::c_void;

use crate::{ActorRuntimeId, FerrumStatus, FfiStr, ModContextHandle, RawMainThreadToken};

/// 0..=255 保留给基岩版的取消原因。
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormResponseKind(pub u32);

impl FormResponseKind {
    pub const USER_CLOSED: Self = Self(0);
    pub const USER_BUSY: Self = Self(1);
    pub const CANCELLED: Self = Self(256);
    pub const SUBMITTED: Self = Self(257);
    pub const DISCONNECTED: Self = Self(258);
}

/// 发送成功后由宿主接管 env。回调最多调用一次，响应视图仅在回调期间有效。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FormCallbackV1 {
    pub env: *mut c_void,
    pub invoke_fn: extern "C" fn(
        *mut c_void,
        RawMainThreadToken,
        ActorRuntimeId,
        FormResponseKind,
        FfiStr<'_>,
    ) -> FerrumStatus,
    pub drop_fn: extern "C" fn(*mut c_void),
}

#[repr(C)]
pub struct FormApiV1 {
    pub struct_size: u32,
    /// 主线程发送一张表单，json 在本次调用内借用。
    pub send:
        extern "C" fn(ModContextHandle, ActorRuntimeId, FfiStr<'_>, FormCallbackV1) -> FerrumStatus,
}
