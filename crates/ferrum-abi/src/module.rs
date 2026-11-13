use crate::{AbiVersion, FerrumHostApi, FerrumStatus, ModContextHandle, RawMainThreadToken};

/// 模组 DLL 唯一导出的入口符号名（后缀即 ABI 主版本）。
pub const FERRUM_MOD_ENTRY_SYMBOL: &str = "ferrum_mod_entry_v1";

#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct FerrumModFlags(pub u32);

impl FerrumModFlags {
    pub const NONE: Self = Self(0);
    /// 模组声明自身可被 `FreeLibrary`（见线程规范 §4.4）。
    pub const HOT_UNLOAD_SAFE: Self = Self(1 << 0);

    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

pub type OnLoadFn = extern "C" fn(mod_ctx: ModContextHandle) -> FerrumStatus;
pub type OnEnableFn =
    extern "C" fn(mod_ctx: ModContextHandle, token: RawMainThreadToken) -> FerrumStatus;
pub type OnDisableFn =
    extern "C" fn(mod_ctx: ModContextHandle, token: RawMainThreadToken) -> FerrumStatus;
pub type OnUnloadFn = extern "C" fn(mod_ctx: ModContextHandle) -> FerrumStatus;

/// 模组向宿主回填的生命周期回调描述符。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct FerrumModDescriptor {
    pub struct_size: u32,
    pub flags: FerrumModFlags,
    /// 模组运行所需的最低宿主 ABI 版本（按需取值，而非编译所用 SDK 的版本）。
    pub required_abi_version: AbiVersion,
    /// 编译该模组所用的 ferrum-sdk 版本，仅用于诊断日志。
    pub sdk_version: AbiVersion,
    // 以下签名与 OnLoadFn 等别名一致；此处展开书写，是为了让 cbindgen
    // 把 Option<extern "C" fn> 识别为可空函数指针。
    pub on_load: extern "C" fn(mod_ctx: ModContextHandle) -> FerrumStatus,
    pub on_enable:
        Option<extern "C" fn(mod_ctx: ModContextHandle, token: RawMainThreadToken) -> FerrumStatus>,
    pub on_disable:
        Option<extern "C" fn(mod_ctx: ModContextHandle, token: RawMainThreadToken) -> FerrumStatus>,
    pub on_unload: Option<extern "C" fn(mod_ctx: ModContextHandle) -> FerrumStatus>,
}

/// ABI v1.0 时 [`FerrumModDescriptor`] 的大小。
pub const FERRUM_MOD_DESCRIPTOR_MIN_SIZE_V1_0: u32 = 56;

/// `ferrum_mod_entry_v1` 的签名。
///
/// 模组校验 `host_api` 后把描述符写入 `out_descriptor`。
pub type ModEntryFnV1 = unsafe extern "C" fn(
    host_api: *const FerrumHostApi,
    mod_ctx: ModContextHandle,
    out_descriptor: *mut FerrumModDescriptor,
) -> FerrumStatus;
