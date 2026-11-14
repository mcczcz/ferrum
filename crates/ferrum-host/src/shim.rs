//! `ferrum-host` ↔ `cpp-shim` 内部接口。
//!
//! 这些类型同样由 cbindgen 导出到 `ferrum_abi.h`，C++ 侧严禁手写镜像。
//! 本接口只在同一个 `ferrum-loader.dll` 内部使用，不属于对模组公开的 ABI。

use std::ffi::c_void;
use std::sync::OnceLock;

use ferrum_abi::{ActorRuntimeId, CommandDefinitionV1, CommandHandle, FfiByteWriter};
use ferrum_abi::{FfiBool, FfiStr, LogLevel};

/// `cpp-shim` 为每个 `FerrumMod` 提供的不透明 cookie（指向 C++ 侧对象）。
///
/// 宿主只原样回传，绝不解引用；模组侧永远看不到它。
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ShimModCookie(pub *mut c_void);

// SAFETY: cookie 仅作为不透明值在宿主内部传递并回传给 cpp-shim，宿主从不解引用；
// cpp-shim 保证 cookie 所指对象在模组记录被标记为 Unloaded 之前一直有效。
unsafe impl Send for ShimModCookie {}
// SAFETY: 同上，宿主对 cookie 只做按值拷贝。
unsafe impl Sync for ShimModCookie {}

/// `cpp-shim` 在 `ferrum_host_init` 时注入的函数表。全部函数必须 `noexcept`。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct FerrumShimApi {
    pub struct_size: u32,
    /// 写入模组专属 `ll::io::Logger`（任意线程）。
    pub log: extern "C" fn(cookie: ShimModCookie, level: LogLevel, message: FfiStr<'_>),
    /// 模组 Logger 是否会输出该级别。
    pub should_log: extern "C" fn(cookie: ShimModCookie, level: LogLevel) -> FfiBool,
    /// 写入 `ferrum-loader` 自身的 Logger（任意线程）。
    pub log_loader: extern "C" fn(level: LogLevel, message: FfiStr<'_>),
    /// 请求在下一个主线程切片停用该模组（熔断用，任意线程）。
    pub request_disable: extern "C" fn(cookie: ShimModCookie),
    /// 宿主不再使用该 cookie（最后一个 `Arc<ModEntry>` 释放时调用，任意线程）。
    pub release_cookie: extern "C" fn(cookie: ShimModCookie),
    pub player_v1: ferrum_abi::PlayerApiV1,
    pub command_register: extern "C" fn(
        ShimModCookie,
        CommandHandle,
        *const CommandDefinitionV1<'_>,
    ) -> ferrum_abi::FerrumStatus,
    pub command_unregister: extern "C" fn(ShimModCookie, CommandHandle) -> ferrum_abi::FerrumStatus,
    pub command_origin: extern "C" fn(u64, *mut ActorRuntimeId) -> ferrum_abi::FerrumStatus,
    pub command_int: extern "C" fn(u64, FfiStr<'_>, *mut i32) -> ferrum_abi::FerrumStatus,
    pub command_float: extern "C" fn(u64, FfiStr<'_>, *mut f32) -> ferrum_abi::FerrumStatus,
    pub command_bool: extern "C" fn(u64, FfiStr<'_>, *mut FfiBool) -> ferrum_abi::FerrumStatus,
    pub command_string:
        extern "C" fn(u64, FfiStr<'_>, *mut FfiByteWriter) -> ferrum_abi::FerrumStatus,
    pub command_players:
        extern "C" fn(u64, FfiStr<'_>, *mut FfiByteWriter) -> ferrum_abi::FerrumStatus,
    pub command_output: extern "C" fn(u64, FfiBool, FfiStr<'_>) -> ferrum_abi::FerrumStatus,
    pub block_v1: ferrum_abi::BlockApiV1,
    pub actor_v1: ferrum_abi::ActorApiV1,
    pub mob_v1: ferrum_abi::MobApiV1,
}

/// `cpp-shim` 调用 `ferrum_host_load_mod` 时提供的模组信息（所有字符串为 UTF-8，仅本次调用内有效）。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ShimModInfo<'a> {
    pub struct_size: u32,
    pub cookie: ShimModCookie,
    pub name: FfiStr<'a>,
    pub version: FfiStr<'a>,
    /// `entry` 指向的 `.dll` 绝对路径
    pub entry_path: FfiStr<'a>,
    pub mod_dir: FfiStr<'a>,
    pub data_dir: FfiStr<'a>,
    pub config_dir: FfiStr<'a>,
    pub lang_dir: FfiStr<'a>,
}

static SHIM: OnceLock<FerrumShimApi> = OnceLock::new();

/// 安装 shim 函数表；重复安装返回 `false`。
pub fn install(api: FerrumShimApi) -> bool {
    SHIM.set(api).is_ok()
}

pub fn get() -> Option<&'static FerrumShimApi> {
    SHIM.get()
}

/// 写入 `ferrum-loader` 自身日志；shim 未安装时退回 stderr（单元测试场景）。
pub fn loader_log(level: LogLevel, message: &str) {
    match get() {
        Some(shim) => (shim.log_loader)(level, FfiStr::new(message)),
        None => eprintln!("[ferrum-loader][{}] {message}", level.0),
    }
}
