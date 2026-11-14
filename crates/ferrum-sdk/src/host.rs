//! 宿主根表的保存与底层调用辅助。

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;

use ferrum_abi::{CoreApiV1, FerrumHostApi, FerrumStatus, FfiByteWriter, LogApiV1};

use crate::error::Error;

/// 宿主根表的 `'static` 引用（握手成功后设置一次）。
#[derive(Copy, Clone)]
pub(crate) struct HostRef(pub(crate) &'static FerrumHostApi);

// SAFETY: 宿主保证根表及其子表在进程生命周期内不可变且可从任意线程读取，
// 其中的函数指针按 ABI 契约线程安全（主线程专属函数由令牌在类型层约束）。
unsafe impl Send for HostRef {}
// SAFETY: 同上。
unsafe impl Sync for HostRef {}

static HOST: OnceLock<HostRef> = OnceLock::new();

/// 记录宿主根表；同一镜像被再次加载时宿主指针不变，重复设置被忽略。
pub(crate) fn install(api: &'static FerrumHostApi) {
    let _ = HOST.set(HostRef(api));
}

pub(crate) fn try_api() -> Option<&'static FerrumHostApi> {
    HOST.get().map(|h| h.0)
}

/// 握手之后才可能被调用，因此根表必定存在。
pub(crate) fn api() -> &'static FerrumHostApi {
    try_api().expect("ferrum-sdk: host api used before ferrum_mod_entry_v1 handshake")
}

pub(crate) fn core() -> &'static CoreApiV1 {
    // SAFETY: 握手时已校验 core_v1 非空且 struct_size 覆盖 v1.0；子表为 'static。
    unsafe { &*api().core_v1 }
}

pub(crate) fn log() -> &'static LogApiV1 {
    // SAFETY: 握手时已校验 log_v1 非空且 struct_size 覆盖 v1.0；子表为 'static。
    unsafe { &*api().log_v1 }
}

pub(crate) fn check(status: FerrumStatus) -> Result<(), Error> {
    if status.is_ok() {
        Ok(())
    } else {
        Err(error_from_status(status))
    }
}

pub(crate) fn player_api() -> Result<&'static ferrum_abi::PlayerApiV1, Error> {
    let ptr =
        (api().query_interface)("ferrum.player.v1".into(), 0).cast::<ferrum_abi::PlayerApiV1>();
    if ptr.is_null() {
        return Err(Error::new(
            FerrumStatus::UNSUPPORTED,
            "宿主未提供玩家 API（需要 ABI 1.1）",
        ));
    }
    // SAFETY: query_interface 返回宿主静态子表，先只读取冻结的 struct_size 前缀。
    if unsafe { ptr.cast::<u32>().read() } < size_of::<ferrum_abi::PlayerApiV1>() as u32 {
        return Err(Error::new(FerrumStatus::UNSUPPORTED, "玩家 API 子表不完整"));
    }
    // SAFETY: 已确认子表非空且大小覆盖 v1 全部槽位，宿主保证静态且对齐。
    Ok(unsafe { &*ptr })
}

pub(crate) fn command_api() -> Result<&'static ferrum_abi::CommandApiV1, Error> {
    let ptr =
        (api().query_interface)("ferrum.command.v1".into(), 0).cast::<ferrum_abi::CommandApiV1>();
    if ptr.is_null() {
        return Err(Error::new(
            FerrumStatus::UNSUPPORTED,
            "宿主未提供命令 API（需要 ABI 1.1）",
        ));
    }
    // SAFETY: query_interface 返回宿主静态子表，先只读取冻结的 struct_size 前缀。
    if unsafe { ptr.cast::<u32>().read() } < size_of::<ferrum_abi::CommandApiV1>() as u32 {
        return Err(Error::new(FerrumStatus::UNSUPPORTED, "命令 API 子表不完整"));
    }
    // SAFETY: 已确认子表非空且大小覆盖 v1 全部槽位，宿主保证静态且对齐。
    Ok(unsafe { &*ptr })
}

pub(crate) fn block_api() -> Result<&'static ferrum_abi::BlockApiV1, Error> {
    let ptr = (api().query_interface)("ferrum.block.v1".into(), 0).cast::<ferrum_abi::BlockApiV1>();
    if ptr.is_null() {
        return Err(Error::new(FerrumStatus::UNSUPPORTED, "宿主未提供方块 API"));
    }
    // SAFETY: 宿主返回对齐且静态有效的子表，先仅读取冻结的 struct_size 字段。
    if unsafe { ptr.cast::<u32>().read() } < size_of::<ferrum_abi::BlockApiV1>() as u32 {
        return Err(Error::new(FerrumStatus::UNSUPPORTED, "方块 API 子表不完整"));
    }
    // SAFETY: 已检查非空和完整大小，宿主保证对齐、静态且函数指针有效。
    Ok(unsafe { &*ptr })
}

pub(crate) fn actor_api() -> Result<&'static ferrum_abi::ActorApiV1, Error> {
    let ptr = (api().query_interface)("ferrum.actor.v1".into(), 0).cast::<ferrum_abi::ActorApiV1>();
    if ptr.is_null() {
        return Err(Error::new(FerrumStatus::UNSUPPORTED, "宿主未提供实体 API"));
    }
    // SAFETY: 宿主返回对齐的静态子表，先读取固定的 struct_size 前缀。
    if unsafe { ptr.cast::<u32>().read() } < size_of::<ferrum_abi::ActorApiV1>() as u32 {
        return Err(Error::new(FerrumStatus::UNSUPPORTED, "实体 API 子表不完整"));
    }
    // SAFETY: 子表非空、完整且对齐，宿主保证进程生命周期内有效。
    Ok(unsafe { &*ptr })
}
pub(crate) fn mob_api() -> Result<&'static ferrum_abi::MobApiV1, Error> {
    let ptr = (api().query_interface)("ferrum.mob.v1".into(), 0).cast::<ferrum_abi::MobApiV1>();
    if ptr.is_null() {
        return Err(Error::new(FerrumStatus::UNSUPPORTED, "宿主未提供生物 API"));
    }
    // SAFETY: 宿主返回对齐的静态子表，先读取固定的 struct_size 前缀。
    if unsafe { ptr.cast::<u32>().read() } < size_of::<ferrum_abi::MobApiV1>() as u32 {
        return Err(Error::new(FerrumStatus::UNSUPPORTED, "生物 API 子表不完整"));
    }
    // SAFETY: 子表非空、完整且对齐，宿主保证进程生命周期内有效。
    Ok(unsafe { &*ptr })
}

/// 宿主调用失败后立即取回 last-error，组装为 [`Error`]。
pub(crate) fn error_from_status(status: FerrumStatus) -> Error {
    let message = read_bytes(|w| (core().take_last_error)(w))
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    Error::new(status, message)
}

extern "C" fn vec_write(context: *mut c_void, bytes: *const u8, len: usize) -> FerrumStatus {
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: context 是 read_bytes 在栈上传入的 &mut Vec<u8>，仅在本次同步调用内使用。
        let buf = unsafe { &mut *(context as *mut Vec<u8>) };
        if len == 0 || bytes.is_null() {
            return FerrumStatus::OK;
        }
        if buf.try_reserve(len).is_err() {
            return FerrumStatus::OUT_OF_MEMORY;
        }
        // SAFETY: 宿主保证 bytes 指向 len 个本次调用内有效的字节，且已排除空指针。
        buf.extend_from_slice(unsafe { std::slice::from_raw_parts(bytes, len) });
        FerrumStatus::OK
    }));
    result.unwrap_or(FerrumStatus::MOD_PANICKED)
}

/// 用一个写入模组自身堆的 [`FfiByteWriter`] 调用宿主，收集输出字节（不检查 UTF-8）。
///
/// 返回非 `OK` 时不会读取 last-error（避免递归）；需要富错误时用 [`read_string`]。
pub(crate) fn read_bytes(
    f: impl FnOnce(*mut FfiByteWriter) -> FerrumStatus,
) -> Result<Vec<u8>, FerrumStatus> {
    let mut buf = Vec::<u8>::new();
    let mut writer = FfiByteWriter {
        context: &mut buf as *mut Vec<u8> as *mut c_void,
        write_fn: vec_write,
    };
    let status = f(&mut writer);
    if status.is_ok() { Ok(buf) } else { Err(status) }
}

/// 调用宿主并把输出解码为 UTF-8 字符串。
pub(crate) fn read_string(
    f: impl FnOnce(*mut FfiByteWriter) -> FerrumStatus,
) -> Result<String, Error> {
    let bytes = read_bytes(f).map_err(error_from_status)?;
    Ok(String::from_utf8(bytes)?)
}
