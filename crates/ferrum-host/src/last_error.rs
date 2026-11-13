//! 线程局部 `LastErrorSlot`，见 `docs/01-specs/error-handling-spec.md` §3。

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
