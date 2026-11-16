//! 线程局部 `LastErrorSlot`，见 `docs/design/errors.md`。

use std::cell::RefCell;

use ferrum_abi::{FerrumStatus, FfiByteWriter};

thread_local! {
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// 覆盖当前线程的错误详情。
pub fn set(message: impl Into<String>) {
    let message = message.into();
    LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(message));
}

/// 写入错误详情并返回 `status`，便于 `return last_error::fail(..)`。
pub fn fail(status: FerrumStatus, message: impl Into<String>) -> FerrumStatus {
    set(message);
    status
}

/// 取回并清空当前线程的错误详情。
pub fn take() -> Option<String> {
    LAST_ERROR.with(|slot| slot.borrow_mut().take())
}

/// 隔离一次 API 调用的错误详情；失败时不能误用此前调用留下的消息。
pub(crate) fn scope(
    operation: impl std::fmt::Display,
    f: impl FnOnce() -> FerrumStatus,
) -> FerrumStatus {
    let previous = take();
    let status = f();
    LAST_ERROR.with(|slot| {
        let mut slot = slot.borrow_mut();
        if status.is_ok() {
            if slot.is_none() {
                *slot = previous;
            }
        } else if slot.as_ref().is_none_or(|text| text.is_empty()) {
            let detail = match status {
                FerrumStatus::NULL_POINTER => "必需的输入或输出指针为空",
                FerrumStatus::INVALID_ARGUMENT => "参数值、类型或当前作用域不符合要求",
                FerrumStatus::ABI_INCOMPATIBLE => "ABI 版本或结构体大小不兼容",
                FerrumStatus::MOD_DISABLED => "模组当前状态不允许此操作",
                FerrumStatus::STALE_HANDLE => "句柄已失效或资源已销毁",
                FerrumStatus::PERMISSION_DENIED => "无权访问该资源",
                FerrumStatus::NOT_FOUND => "请求的对象或资源不存在",
                FerrumStatus::SERVICE_UNAVAILABLE => "shim 或世界服务尚不可用",
                FerrumStatus::UNSUPPORTED => "宿主不支持请求的操作",
                FerrumStatus::WRONG_THREAD => "此操作必须在服务端主线程调用",
                FerrumStatus::REENTRANT_CALL => "当前回调不允许重入",
                FerrumStatus::CALLBACK_POISONED => "回调此前 panic，已被毒化",
                FerrumStatus::MOD_PANICKED => "模组回调发生 panic",
                FerrumStatus::ALREADY_EXISTS => "同名资源已注册",
                _ => "操作未能完成",
            };
            *slot = Some(format!("{operation}: {status}，{detail}"));
        }
    });
    status
}

/// 将 `text` 写入调用方提供的写入器；`writer` 为空时返回 `NULL_POINTER`。
///
/// # Safety
/// `writer` 为空或指向一个在本次调用期间有效的 [`FfiByteWriter`]。
pub unsafe fn write_to(writer: *mut FfiByteWriter, text: &str) -> FerrumStatus {
    // SAFETY: 调用方保证 writer 为空或在本次调用期间有效。
    let Some(writer) = (unsafe { writer.as_ref() }) else {
        return FerrumStatus::NULL_POINTER;
    };
    if text.is_empty() {
        return FerrumStatus::OK;
    }
    writer.write(text.as_bytes())
}
