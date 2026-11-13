//! Panic 防火墙，见 `docs/01-specs/error-handling-spec.md` §1.2。
//!
//! 所有进入模组代码的 `extern "C"` 跳板都经过 [`firewall`]：普通 Rust panic 被拦截、
//! 上报给宿主并转为 `MOD_PANICKED`，绝不展开越过 FFI 边界。

use std::cell::{Cell, RefCell};
use std::panic::{self, AssertUnwindSafe, PanicHookInfo};
use std::sync::Once;

use ferrum_abi::{FerrumStatus, FfiStr, ModContextHandle};

use crate::error::Error;
use crate::host;

thread_local! {
    /// 当前线程处于防火墙内的嵌套深度。
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    /// panic hook 记录的源码位置。
    static LOCATION: RefCell<Option<String>> = const { RefCell::new(None) };
}

static HOOK: Once = Once::new();

/// 安装 panic hook：防火墙内的 panic 只记录位置（由宿主日志统一输出），其余交给原 hook。
pub(crate) fn install_hook() {
    HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info: &PanicHookInfo<'_>| {
            if DEPTH.with(Cell::get) > 0 {
                let location = info
                    .location()
                    .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
                LOCATION.with(|slot| *slot.borrow_mut() = location);
            } else {
                previous(info);
            }
        }));
    });
}

fn payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_owned()
    }
}

/// 把模组侧错误写入宿主 last-error 并返回其状态码。
pub(crate) fn report_error(error: &Error) -> FerrumStatus {
    if let Some(core) = host::try_api().map(|_| host::core()) {
        let text = error.to_string();
        let _ = (core.set_last_error)(FfiStr::new(&text));
    }
    error.status()
}

/// 在防火墙内执行 `f`。
pub(crate) fn firewall(
    ctx: ModContextHandle,
    f: impl FnOnce() -> Result<(), Error>,
) -> FerrumStatus {
    DEPTH.with(|d| d.set(d.get() + 1));
    let result = panic::catch_unwind(AssertUnwindSafe(f));
    DEPTH.with(|d| d.set(d.get() - 1));

    match result {
        Ok(Ok(())) => FerrumStatus::OK,
        Ok(Err(error)) => report_error(&error),
        Err(payload) => {
            let message = payload_message(payload.as_ref());
            // payload 的析构也可能 panic：直接泄漏，绝不让二次 panic 越过边界。
            std::mem::forget(payload);
            let location = LOCATION
                .with(|slot| slot.borrow_mut().take())
                .unwrap_or_default();
            if host::try_api().is_some() {
                let _ =
                    (host::core().report_panic)(ctx, FfiStr::new(&message), FfiStr::new(&location));
            }
            FerrumStatus::MOD_PANICKED
        }
    }
}
