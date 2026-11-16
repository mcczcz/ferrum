//! Rust 假 shim 的会话边界；生产宿主不启用此 feature。
use crate::shim::ShimModCookie;
use std::sync::OnceLock;

type PanicObserver = fn(ShimModCookie, &str, &str);
static PANIC_OBSERVER: OnceLock<PanicObserver> = OnceLock::new();

pub fn install_panic_observer(observer: PanicObserver) {
    let _ = PANIC_OBSERVER.set(observer);
}

pub(crate) fn report_panic(cookie: ShimModCookie, message: &str, location: &str) {
    if let Some(observer) = PANIC_OBSERVER.get() {
        observer(cookie, message, location);
    }
}

/// 将当前线程绑定为本次 mock 会话的主线程。
///
/// # Safety
/// 调用方必须独占宿主；前一会话的模组、回调和主线程资源已经排空。
pub unsafe fn begin_session() {
    crate::vtable::set_mock_thread(Some(std::thread::current().id()));
    crate::last_error::take();
}

/// 清理会话内的原生事件发布者，并解除主线程绑定。
///
/// # Safety
/// 必须在会话主线程调用，所有模组已卸载，且没有正在执行的回调。
pub unsafe fn end_session() {
    crate::custom_event_core::reset();
    crate::last_error::take();
    crate::vtable::set_mock_thread(None);
}
