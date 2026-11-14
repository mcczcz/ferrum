//! 供 `cpp-shim` 调用的 `extern "C"` 入口。
//!
//! 全部以 [`guard`] 包裹；失败时详情写入当前线程的 `LastErrorSlot`，
//! shim 通过 [`ferrum_host_take_last_error`] 取回。

use ferrum_abi::{AbiVersion, FerrumStatus, FfiByteWriter, LogLevel};

use crate::last_error;
use crate::loader;
use crate::runtime::ModInfo;
use crate::shim::{self, FerrumShimApi, ShimModInfo};
use crate::vtable::{self, guard};

/// 安装 shim 函数表。必须在其他任何 `ferrum_host_*` 调用之前、且只调用一次。
///
/// # Safety
/// `api` 必须指向有效的 [`FerrumShimApi`]，其中函数在进程生命周期内保持有效。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ferrum_host_init(api: *const FerrumShimApi) -> FerrumStatus {
    guard(|| {
        // SAFETY: 调用方保证 api 为空或指向有效函数表。
        let Some(api) = (unsafe { api.as_ref() }) else {
            return last_error::fail(FerrumStatus::NULL_POINTER, "shim api is null");
        };
        if (api.struct_size as usize) < size_of::<FerrumShimApi>() {
            return last_error::fail(
                FerrumStatus::ABI_INCOMPATIBLE,
                "shim api struct_size too small",
            );
        }
        if !shim::install(*api) {
            return last_error::fail(
                FerrumStatus::ALREADY_EXISTS,
                "ferrum-host already initialized",
            );
        }
        shim::loader_log(
            LogLevel::INFO,
            &format!(
                "ferrum-host {} 已初始化（ABI {}）",
                env!("CARGO_PKG_VERSION"),
                AbiVersion::CURRENT
            ),
        );
        FerrumStatus::OK
    })
}

/// 宿主实现的 ABI 版本。
#[unsafe(no_mangle)]
pub extern "C" fn ferrum_host_abi_version() -> AbiVersion {
    AbiVersion::CURRENT
}

/// shim 将 C++ 异常详情写入同一线程的错误槽。
#[unsafe(no_mangle)]
pub extern "C" fn ferrum_host_set_last_error(message: ferrum_abi::FfiStr<'_>) -> FerrumStatus {
    guard(|| {
        // SAFETY: shim 保证 message 在本次同步调用内有效。
        last_error::set(String::from_utf8_lossy(unsafe { message.as_bytes() }));
        FerrumStatus::OK
    })
}

/// 由 shim 在服务端主线程上调用，记录主线程 ID。
#[unsafe(no_mangle)]
pub extern "C" fn ferrum_host_mark_main_thread() {
    vtable::mark_main_thread();
}

/// shim 在派发前确认当前线程，不会修改已记录的主线程。
#[unsafe(no_mangle)]
pub extern "C" fn ferrum_host_is_main_thread() -> ferrum_abi::FfiBool {
    vtable::is_main_thread().into()
}

/// 加载一个 `type = "ferrum"` 模组并执行其 `on_load`。成功时写出上下文句柄。
///
/// # Safety
/// `info` 指向有效的 [`ShimModInfo`]（字符串在本次调用内有效）；`out_ctx` 指向可写的 `u64`。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ferrum_host_load_mod(
    info: *const ShimModInfo<'_>,
    out_ctx: *mut u64,
) -> FerrumStatus {
    guard(|| {
        // SAFETY: 调用方保证 info 为空或有效。
        let Some(info) = (unsafe { info.as_ref() }) else {
            return last_error::fail(FerrumStatus::NULL_POINTER, "mod info is null");
        };
        if out_ctx.is_null() {
            return last_error::fail(FerrumStatus::NULL_POINTER, "out_ctx is null");
        }
        let text = |s: ferrum_abi::FfiStr<'_>| {
            // SAFETY: 调用方保证字符串视图在本次调用内有效。
            String::from_utf8_lossy(unsafe { s.as_bytes() }).into_owned()
        };
        let info = ModInfo {
            cookie: info.cookie,
            name: text(info.name),
            version: text(info.version),
            entry_path: text(info.entry_path),
            mod_dir: text(info.mod_dir),
            data_dir: text(info.data_dir),
            config_dir: text(info.config_dir),
            lang_dir: text(info.lang_dir),
        };
        match loader::load(info) {
            Ok(ctx) => {
                // SAFETY: 已检查非空，调用方保证可写。
                unsafe { *out_ctx = ctx.0.to_raw() };
                FerrumStatus::OK
            }
            Err(status) => status,
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn ferrum_host_enable_mod(ctx: u64) -> FerrumStatus {
    guard(|| loader::enable(ctx))
}

#[unsafe(no_mangle)]
pub extern "C" fn ferrum_host_disable_mod(ctx: u64) -> FerrumStatus {
    guard(|| loader::disable(ctx))
}

#[unsafe(no_mangle)]
pub extern "C" fn ferrum_host_unload_mod(ctx: u64) -> FerrumStatus {
    guard(|| loader::unload(ctx))
}

/// 取回并清空当前线程的错误详情。
///
/// # Safety
/// `out` 为空或指向本次调用内有效的写入器。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ferrum_host_take_last_error(out: *mut FfiByteWriter) -> FerrumStatus {
    let text = last_error::take().unwrap_or_default();
    // SAFETY: 透传调用方保证。
    unsafe { last_error::write_to(out, &text) }
}
