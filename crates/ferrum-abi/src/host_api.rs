use core::ffi::c_void;

use crate::{AbiVersion, FerrumStatus, FfiBool, FfiByteWriter, FfiStr, ModContextHandle};

/// 日志级别（与 `ll::io::LogLevel` 的非 `Off` 取值一一对应）。
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct LogLevel(pub u32);

impl LogLevel {
    pub const FATAL: Self = Self(0);
    pub const ERROR: Self = Self(1);
    pub const WARN: Self = Self(2);
    pub const INFO: Self = Self(3);
    pub const DEBUG: Self = Self(4);
    pub const TRACE: Self = Self(5);
}

/// 模组目录种类（对应 `ll::mod::Mod::get*Dir`）。
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ModDirKind(pub u32);

impl ModDirKind {
    /// `mods/<name>/`
    pub const MOD: Self = Self(0);
    /// `mods/<name>/data/`
    pub const DATA: Self = Self(1);
    /// `mods/<name>/config/`
    pub const CONFIG: Self = Self(2);
    /// `mods/<name>/lang/`
    pub const LANG: Self = Self(3);
}

/// 核心子表：last-error、panic 上报、模组元数据与目录、线程判定。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct CoreApiV1 {
    pub struct_size: u32,
    /// 取回并清空当前线程的 `LastErrorSlot`；无错误时写入 0 字节并返回 `OK`。
    pub take_last_error: extern "C" fn(out: *mut FfiByteWriter) -> FerrumStatus,
    /// 模组从回调返回非 `OK` 前写入当前线程的 `LastErrorSlot`，供宿主输出详情。
    pub set_last_error: extern "C" fn(message: FfiStr<'_>) -> FerrumStatus,
    /// 模组在回调中捕获 panic 后上报（`location` 可为空）。
    pub report_panic: extern "C" fn(
        ctx: ModContextHandle,
        message: FfiStr<'_>,
        location: FfiStr<'_>,
    ) -> FerrumStatus,
    /// 模组在 `manifest.json` 中声明的 `name`。
    pub get_mod_name: extern "C" fn(ctx: ModContextHandle, out: *mut FfiByteWriter) -> FerrumStatus,
    /// 模组版本（`manifest.json` 中的 `version`，缺省为 `0.0.0`）。
    pub get_mod_version:
        extern "C" fn(ctx: ModContextHandle, out: *mut FfiByteWriter) -> FerrumStatus,
    /// 以 UTF-8 写出模组目录的绝对路径。
    pub get_mod_dir: extern "C" fn(
        ctx: ModContextHandle,
        kind: ModDirKind,
        out: *mut FfiByteWriter,
    ) -> FerrumStatus,
    /// 当前线程是否为 BDS 主线程（服务端逻辑线程）。
    pub is_main_thread: extern "C" fn() -> FfiBool,
}

/// 日志子表：写入该模组专属的 `ll::io::Logger`（任意线程安全）。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct LogApiV1 {
    pub struct_size: u32,
    pub log:
        extern "C" fn(ctx: ModContextHandle, level: LogLevel, message: FfiStr<'_>) -> FerrumStatus,
    pub should_log: extern "C" fn(ctx: ModContextHandle, level: LogLevel) -> FfiBool,
}

/// 按接口标识与最小次版本查询扩展子表；未找到返回空指针。
pub type QueryInterfaceFn =
    extern "C" fn(interface_id_utf8: FfiStr<'_>, min_minor: u16) -> *const c_void;

/// 宿主在加载模组时传入的全局根表（进程级 `'static`）。
///
/// 后续 minor 版本只允许在末尾追加字段；领域子表（事件、命令、玩家……）
/// 将按里程碑逐个追加，或通过 `query_interface` 暴露。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct FerrumHostApi {
    pub struct_size: u32,
    pub reserved_flags: u32,
    pub magic: u64,
    pub host_abi_version: AbiVersion,

    // v1.0 核心子表（指针恒非空）
    pub core_v1: *const CoreApiV1,
    pub log_v1: *const LogApiV1,

    pub query_interface: QueryInterfaceFn,
}

/// ABI v1.0 时 [`FerrumHostApi`] 的大小；模组据此判断根表是否完整。
pub const FERRUM_HOST_API_MIN_SIZE_V1_0: u32 = 48;
/// ABI v1.0 时 [`CoreApiV1`] 的大小。
pub const CORE_API_MIN_SIZE_V1_0: u32 = 64;
/// ABI v1.0 时 [`LogApiV1`] 的大小。
pub const LOG_API_MIN_SIZE_V1_0: u32 = 24;

/// 判断 `struct_size` 是否覆盖了从 `offset` 起的 `size` 字节（用于尾追加字段的存在性检查）。
#[inline]
pub const fn has_field(struct_size: u32, offset: usize, size: usize) -> bool {
    struct_size as usize >= offset + size
}
