use std::fmt;

use ferrum_abi::{FfiStr, LogLevel, ModContextHandle};

use crate::host;

/// 日志级别。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Level {
    Fatal,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Level {
    fn to_abi(self) -> LogLevel {
        match self {
            Self::Fatal => LogLevel::FATAL,
            Self::Error => LogLevel::ERROR,
            Self::Warn => LogLevel::WARN,
            Self::Info => LogLevel::INFO,
            Self::Debug => LogLevel::DEBUG,
            Self::Trace => LogLevel::TRACE,
        }
    }
}

/// 模组专属日志器，输出到 LeviLamina 为该模组创建的 `ll::io::Logger`。
///
/// 可在任意线程使用；模组卸载后的调用被宿主静默丢弃。
#[derive(Copy, Clone)]
pub struct Logger {
    ctx: ModContextHandle,
}

impl Logger {
    pub(crate) fn new(ctx: ModContextHandle) -> Self {
        Self { ctx }
    }

    /// 宿主是否会输出该级别；可用于跳过昂贵的格式化。
    pub fn enabled(&self, level: Level) -> bool {
        (host::log().should_log)(self.ctx, level.to_abi()).get()
    }

    pub fn log(&self, level: Level, message: impl AsRef<str>) {
        let _ = (host::log().log)(self.ctx, level.to_abi(), FfiStr::new(message.as_ref()));
    }

    /// 按需格式化后输出：`logger.log_fmt(Level::Info, format_args!("x = {x}"))`。
    pub fn log_fmt(&self, level: Level, args: fmt::Arguments<'_>) {
        match args.as_str() {
            Some(s) => self.log(level, s),
            None if self.enabled(level) => self.log(level, args.to_string()),
            None => {}
        }
    }

    pub fn fatal(&self, message: impl AsRef<str>) {
        self.log(Level::Fatal, message);
    }

    pub fn error(&self, message: impl AsRef<str>) {
        self.log(Level::Error, message);
    }

    pub fn warn(&self, message: impl AsRef<str>) {
        self.log(Level::Warn, message);
    }

    pub fn info(&self, message: impl AsRef<str>) {
        self.log(Level::Info, message);
    }

    pub fn debug(&self, message: impl AsRef<str>) {
        self.log(Level::Debug, message);
    }

    pub fn trace(&self, message: impl AsRef<str>) {
        self.log(Level::Trace, message);
    }
}

impl fmt::Debug for Logger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Logger").finish_non_exhaustive()
    }
}
