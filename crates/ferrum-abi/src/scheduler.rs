use core::ffi::c_void;

use crate::{FerrumStatus, GenerationalHandle, ModContextHandle, RawMainThreadToken};

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaskHandle(pub GenerationalHandle);

/// 注册成功后宿主接管 env。invoke_fn 和 drop_fn 仅在服务端主线程串行调用。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TaskCallbackV1 {
    pub env: *mut c_void,
    pub invoke_fn: extern "C" fn(*mut c_void, RawMainThreadToken) -> FerrumStatus,
    pub drop_fn: extern "C" fn(*mut c_void),
}

#[repr(C)]
pub struct SchedulerApiV1 {
    pub struct_size: u32,
    /// 任意线程投递，下一 Tick 执行一次。
    pub post: extern "C" fn(ModContextHandle, TaskCallbackV1, *mut TaskHandle) -> FerrumStatus,
    /// 主线程注册，零延迟表示下一 Tick。
    pub after_ticks:
        extern "C" fn(ModContextHandle, u64, TaskCallbackV1, *mut TaskHandle) -> FerrumStatus,
    /// 主线程注册，间隔必须大于零。
    pub every_ticks:
        extern "C" fn(ModContextHandle, u64, TaskCallbackV1, *mut TaskHandle) -> FerrumStatus,
    /// 主线程取消本模组的任务。正在执行的回调可以取消自身。
    pub cancel: extern "C" fn(ModContextHandle, TaskHandle) -> FerrumStatus,
}
