//! 交给模组的 `FerrumHostApi` 根表及 v1.0 子表实现。
//!
//! 每个 `extern "C"` 实现都以 [`guard`] 包裹：宿主自身 panic 记为 `INTERNAL_ERROR`，
//! 绝不让展开越过 FFI 边界。

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;
use std::thread::ThreadId;
use std::time::Instant;

use ferrum_abi::{
    AbiVersion, CoreApiV1, FERRUM_ABI_MAGIC, FerrumHostApi, FerrumStatus, FfiBool, FfiByteWriter,
    FfiStr, LogApiV1, LogLevel, ModContextHandle, ModDirKind,
};

use crate::last_error;
use crate::runtime::{self, ModState};
use crate::shim;

/// 在宿主侧 FFI 入口兜底捕获 panic。
pub(crate) fn guard(f: impl FnOnce() -> FerrumStatus) -> FerrumStatus {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(status) => status,
        Err(payload) => {
            let msg = panic_message(payload.as_ref());
            std::mem::forget(payload);
            last_error::fail(
                FerrumStatus::INTERNAL_ERROR,
                format!("ferrum-host internal panic: {msg}"),
            )
        }
    }
}

pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_owned()
    }
}

// ---------------------------------------------------------------------------
// 主线程识别
// ---------------------------------------------------------------------------

static MAIN_THREAD: OnceLock<ThreadId> = OnceLock::new();

/// 由 shim 在确认处于 BDS 服务端线程时调用一次。
pub fn mark_main_thread() {
    let _ = MAIN_THREAD.set(std::thread::current().id());
}

pub fn is_main_thread() -> bool {
    MAIN_THREAD.get() == Some(&std::thread::current().id())
}

// ---------------------------------------------------------------------------
// CoreApiV1
// ---------------------------------------------------------------------------

extern "C" fn core_take_last_error(out: *mut FfiByteWriter) -> FerrumStatus {
    // 这里不能用 guard：guard 失败路径会写 LastErrorSlot。
    let text = last_error::take().unwrap_or_default();
    // SAFETY: ABI 契约要求 out 为空或指向调用方栈上的有效写入器。
    unsafe { last_error::write_to(out, &text) }
}

extern "C" fn core_set_last_error(message: FfiStr<'_>) -> FerrumStatus {
    guard(|| {
        // SAFETY: 模组按 ABI 契约传入本次调用内有效的借用视图。
        let bytes = unsafe { message.as_bytes() };
        last_error::set(String::from_utf8_lossy(bytes));
        FerrumStatus::OK
    })
}

extern "C" fn core_report_panic(
    ctx: ModContextHandle,
    message: FfiStr<'_>,
    location: FfiStr<'_>,
) -> FerrumStatus {
    guard(|| {
        let entry = match runtime::lookup(ctx) {
            Ok(e) => e,
            Err(s) => return s,
        };
        // SAFETY: 模组按 ABI 契约传入本次调用内有效的借用视图。
        let message = String::from_utf8_lossy(unsafe { message.as_bytes() });
        // SAFETY: 同上。
        let location = String::from_utf8_lossy(unsafe { location.as_bytes() });
        let text = if location.is_empty() {
            format!("模组发生 panic: {message}")
        } else {
            format!("模组发生 panic: {message} (at {location})")
        };
        mod_log(&entry.info.cookie, LogLevel::ERROR, &text);

        let tripped = entry
            .breaker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record(Instant::now());
        if tripped && entry.state() == ModState::Enabled {
            mod_log(
                &entry.info.cookie,
                LogLevel::FATAL,
                &format!(
                    "模组 {} 在短时间内反复 panic，已触发熔断，将自动停用",
                    entry.info.name
                ),
            );
            if let Some(shim) = shim::get() {
                (shim.request_disable)(entry.info.cookie);
            }
        }
        FerrumStatus::OK
    })
}

extern "C" fn core_get_mod_name(ctx: ModContextHandle, out: *mut FfiByteWriter) -> FerrumStatus {
    guard(|| match runtime::lookup(ctx) {
        // SAFETY: ABI 契约要求 out 为空或指向有效写入器。
        Ok(entry) => unsafe { last_error::write_to(out, &entry.info.name) },
        Err(s) => s,
    })
}

extern "C" fn core_get_mod_version(ctx: ModContextHandle, out: *mut FfiByteWriter) -> FerrumStatus {
    guard(|| match runtime::lookup(ctx) {
        // SAFETY: ABI 契约要求 out 为空或指向有效写入器。
        Ok(entry) => unsafe { last_error::write_to(out, &entry.info.version) },
        Err(s) => s,
    })
}

extern "C" fn core_get_mod_dir(
    ctx: ModContextHandle,
    kind: ModDirKind,
    out: *mut FfiByteWriter,
) -> FerrumStatus {
    guard(|| {
        let entry = match runtime::lookup(ctx) {
            Ok(e) => e,
            Err(s) => return s,
        };
        match entry.info.dir(kind) {
            // SAFETY: ABI 契约要求 out 为空或指向有效写入器。
            Some(dir) => unsafe { last_error::write_to(out, dir) },
            None => last_error::fail(
                FerrumStatus::INVALID_ARGUMENT,
                format!("unknown ModDirKind({})", kind.0),
            ),
        }
    })
}

extern "C" fn core_is_main_thread() -> FfiBool {
    is_main_thread().into()
}

// ---------------------------------------------------------------------------
// LogApiV1
// ---------------------------------------------------------------------------

fn mod_log(cookie: &shim::ShimModCookie, level: LogLevel, message: &str) {
    match shim::get() {
        Some(shim) => (shim.log)(*cookie, level, FfiStr::new(message)),
        None => eprintln!("[mod][{}] {message}", level.0),
    }
}

extern "C" fn log_log(ctx: ModContextHandle, level: LogLevel, message: FfiStr<'_>) -> FerrumStatus {
    guard(|| {
        if level.0 > LogLevel::TRACE.0 {
            return last_error::fail(
                FerrumStatus::INVALID_ARGUMENT,
                format!("unknown LogLevel({})", level.0),
            );
        }
        let entry = match runtime::lookup(ctx) {
            Ok(e) => e,
            Err(s) => return s,
        };
        // SAFETY: 模组按 ABI 契约传入本次调用内有效的借用视图。
        let bytes = unsafe { message.as_bytes() };
        mod_log(&entry.info.cookie, level, &String::from_utf8_lossy(bytes));
        FerrumStatus::OK
    })
}

extern "C" fn log_should_log(ctx: ModContextHandle, level: LogLevel) -> FfiBool {
    let result = catch_unwind(|| {
        let entry = runtime::lookup(ctx).ok()?;
        let shim = shim::get()?;
        Some((shim.should_log)(entry.info.cookie, level))
    });
    match result {
        Ok(Some(v)) => v,
        Ok(None) => FfiBool::TRUE,
        Err(payload) => {
            std::mem::forget(payload);
            FfiBool::FALSE
        }
    }
}

// ---------------------------------------------------------------------------
// 根表
// ---------------------------------------------------------------------------

extern "C" fn query_interface(id: FfiStr<'_>, min_minor: u16) -> *const std::ffi::c_void {
    // SAFETY: ABI 保证 id 为本次同步调用内的有效借用视图。
    match unsafe { id.as_bytes() } {
        b"ferrum.actor.v1" if min_minor == 0 => {
            (&crate::actor::ACTOR_V1 as *const ferrum_abi::ActorApiV1).cast()
        }
        b"ferrum.mob.v1" if min_minor == 0 => {
            (&crate::mob::MOB_V1 as *const ferrum_abi::MobApiV1).cast()
        }
        b"ferrum.block.v1" if min_minor == 0 => {
            (&crate::block::BLOCK_V1 as *const ferrum_abi::BlockApiV1).cast()
        }
        b"ferrum.player.v1" if min_minor == 0 => {
            (&crate::player::PLAYER_V1 as *const ferrum_abi::PlayerApiV1).cast()
        }
        b"ferrum.command.v1" if min_minor == 0 => {
            (&crate::command::COMMAND_V1 as *const ferrum_abi::CommandApiV1).cast()
        }
        _ => std::ptr::null(),
    }
}

pub static CORE_V1: CoreApiV1 = CoreApiV1 {
    struct_size: size_of::<CoreApiV1>() as u32,
    take_last_error: core_take_last_error,
    set_last_error: core_set_last_error,
    report_panic: core_report_panic,
    get_mod_name: core_get_mod_name,
    get_mod_version: core_get_mod_version,
    get_mod_dir: core_get_mod_dir,
    is_main_thread: core_is_main_thread,
};

pub static LOG_V1: LogApiV1 = LogApiV1 {
    struct_size: size_of::<LogApiV1>() as u32,
    log: log_log,
    should_log: log_should_log,
};

/// 让包含裸指针的根表可以放进 `static`。
pub struct HostApiCell(pub FerrumHostApi);

// SAFETY: 根表及其指向的子表均为不可变的 'static 数据，函数指针本身线程安全。
unsafe impl Sync for HostApiCell {}

pub static HOST_API: HostApiCell = HostApiCell(FerrumHostApi {
    struct_size: size_of::<FerrumHostApi>() as u32,
    reserved_flags: 0,
    magic: FERRUM_ABI_MAGIC,
    host_abi_version: AbiVersion::CURRENT,
    core_v1: &CORE_V1,
    log_v1: &LOG_V1,
    query_interface,
});

pub fn host_api() -> &'static FerrumHostApi {
    &HOST_API.0
}
