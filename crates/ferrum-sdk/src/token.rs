use std::marker::PhantomData;

use crate::host;

/// 证明当前调用栈正处于 BDS 主线程同步切片内的零大小令牌。
///
/// 只以 `&MainThreadToken<'tick>` 形式借给回调；`!Send + !Sync`，且外部无法构造。
/// 所有非线程安全的世界/实体 API 都会要求它作为参数。
pub struct MainThreadToken<'tick> {
    _not_send_sync: PhantomData<(&'tick (), *mut ())>,
}

impl MainThreadToken<'_> {
    /// 仅供 SDK 跳板在宿主授予 `RawMainThreadToken` 后调用。
    pub(crate) fn assume_main_thread() -> Self {
        Self {
            _not_send_sync: PhantomData,
        }
    }
}

impl std::fmt::Debug for MainThreadToken<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MainThreadToken")
    }
}

/// 当前线程是否为 BDS 服务端主线程（运行时检查，可在任意线程调用）。
pub fn is_main_thread() -> bool {
    match host::try_api() {
        Some(_) => (host::core().is_main_thread)().get(),
        None => false,
    }
}
