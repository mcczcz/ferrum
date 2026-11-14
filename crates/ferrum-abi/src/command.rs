use core::ffi::c_void;

use crate::{
    ActorRuntimeId, FerrumStatus, FfiBool, FfiByteWriter, FfiSlice, FfiStr, GenerationalHandle,
    ModContextHandle, RawMainThreadToken,
};

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CommandHandle(pub GenerationalHandle);

/// 只在一次同步命令执行期间有效。
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CommandScopeHandle(pub GenerationalHandle);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CommandPermission(pub u32);

impl CommandPermission {
    pub const ANY: Self = Self(0);
    pub const GAME_DIRECTORS: Self = Self(1);
    pub const ADMIN: Self = Self(2);
    pub const HOST: Self = Self(3);
    pub const OWNER: Self = Self(4);
}

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CommandParamKind(pub u32);

impl CommandParamKind {
    pub const INT: Self = Self(0);
    pub const FLOAT: Self = Self(1);
    pub const BOOL: Self = Self(2);
    pub const STRING: Self = Self(3);
    pub const PLAYER: Self = Self(4);
}

/// 冻结的参数描述；显式保留字节使切片元素无未初始化填充。
#[repr(C)]
#[derive(Copy, Clone)]
pub struct CommandParamV1<'a> {
    pub name: FfiStr<'a>,
    pub kind: CommandParamKind,
    pub optional: FfiBool,
    pub reserved: [u8; 3],
}

#[repr(C)]
pub struct CommandDefinitionV1<'a> {
    pub struct_size: u32,
    pub name: FfiStr<'a>,
    pub description: FfiStr<'a>,
    pub permission: CommandPermission,
    pub params: FfiSlice<'a, CommandParamV1<'a>>,
}

/// 多次调用；注册返回 OK 才转移 env 所有权，失败时仍由调用方释放。
/// invoke_fn 和 drop_fn 只会在主线程、宿主未持锁时执行。
#[repr(C)]
#[derive(Copy, Clone)]
pub struct CommandCallbackV1 {
    pub env: *mut c_void,
    pub invoke_fn:
        extern "C" fn(*mut c_void, CommandScopeHandle, RawMainThreadToken) -> FerrumStatus,
    pub drop_fn: extern "C" fn(*mut c_void),
}

/// ABI 1.1 命令接口；scope 的每次访问同时校验调用模组与主线程。
#[repr(C)]
pub struct CommandApiV1 {
    pub struct_size: u32,
    pub register: extern "C" fn(
        ModContextHandle,
        *const CommandDefinitionV1<'_>,
        CommandCallbackV1,
        *mut CommandHandle,
    ) -> FerrumStatus,
    pub unregister: extern "C" fn(ModContextHandle, CommandHandle) -> FerrumStatus,
    pub origin_player:
        extern "C" fn(ModContextHandle, CommandScopeHandle, *mut ActorRuntimeId) -> FerrumStatus,
    /// 未提供的可选参数返回 NOT_FOUND；类型不符返回 INVALID_ARGUMENT。
    pub get_int:
        extern "C" fn(ModContextHandle, CommandScopeHandle, FfiStr<'_>, *mut i32) -> FerrumStatus,
    pub get_float:
        extern "C" fn(ModContextHandle, CommandScopeHandle, FfiStr<'_>, *mut f32) -> FerrumStatus,
    pub get_bool: extern "C" fn(
        ModContextHandle,
        CommandScopeHandle,
        FfiStr<'_>,
        *mut FfiBool,
    ) -> FerrumStatus,
    pub get_string: extern "C" fn(
        ModContextHandle,
        CommandScopeHandle,
        FfiStr<'_>,
        *mut FfiByteWriter,
    ) -> FerrumStatus,
    /// 选择器在每次读取时重新解析，输出与 online_players 相同。
    pub get_players: extern "C" fn(
        ModContextHandle,
        CommandScopeHandle,
        FfiStr<'_>,
        *mut FfiByteWriter,
    ) -> FerrumStatus,
    pub output:
        extern "C" fn(ModContextHandle, CommandScopeHandle, FfiBool, FfiStr<'_>) -> FerrumStatus,
}
