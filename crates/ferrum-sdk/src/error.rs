use std::borrow::Cow;
use std::fmt;

use ferrum_abi::FerrumStatus;

pub type Result<T, E = Error> = core::result::Result<T, E>;

/// 错误种类；由 [`FerrumStatus`] 解码，未知值映射为 [`ErrorKind::Unknown`]。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    InvalidArgument,
    NullPointer,
    InvalidUtf8,
    BufferTooSmall,
    OutOfMemory,
    Unsupported,
    Failed,
    AbiIncompatible,
    ModDisabled,
    WrongThread,
    ReentrantCall,
    StaleHandle,
    PermissionDenied,
    NotFound,
    ServiceUnavailable,
    AlreadyExists,
    CppException,
    ModPanicked,
    CallbackPoisoned,
    Internal,
    Unknown(i32),
}

impl ErrorKind {
    pub fn from_status(status: FerrumStatus) -> Self {
        match status {
            FerrumStatus::INVALID_ARGUMENT => Self::InvalidArgument,
            FerrumStatus::NULL_POINTER => Self::NullPointer,
            FerrumStatus::INVALID_UTF8 => Self::InvalidUtf8,
            FerrumStatus::BUFFER_TOO_SMALL => Self::BufferTooSmall,
            FerrumStatus::OUT_OF_MEMORY => Self::OutOfMemory,
            FerrumStatus::UNSUPPORTED => Self::Unsupported,
            FerrumStatus::FAILED => Self::Failed,
            FerrumStatus::ABI_INCOMPATIBLE => Self::AbiIncompatible,
            FerrumStatus::MOD_DISABLED => Self::ModDisabled,
            FerrumStatus::WRONG_THREAD => Self::WrongThread,
            FerrumStatus::REENTRANT_CALL => Self::ReentrantCall,
            FerrumStatus::STALE_HANDLE => Self::StaleHandle,
            FerrumStatus::PERMISSION_DENIED => Self::PermissionDenied,
            FerrumStatus::NOT_FOUND => Self::NotFound,
            FerrumStatus::SERVICE_UNAVAILABLE => Self::ServiceUnavailable,
            FerrumStatus::ALREADY_EXISTS => Self::AlreadyExists,
            FerrumStatus::CPP_EXCEPTION => Self::CppException,
            FerrumStatus::MOD_PANICKED => Self::ModPanicked,
            FerrumStatus::CALLBACK_POISONED => Self::CallbackPoisoned,
            FerrumStatus::INTERNAL_ERROR => Self::Internal,
            FerrumStatus(other) => Self::Unknown(other),
        }
    }
}

/// SDK 统一错误：状态码 + 可选的诊断文本（来自宿主 last-error 或模组自身）。
pub struct Error {
    status: FerrumStatus,
    message: Option<Cow<'static, str>>,
}

impl Error {
    /// 以指定状态码构造；`status` 为 `OK` 时改记为 `FAILED`。
    pub fn new(status: FerrumStatus, message: impl Into<Cow<'static, str>>) -> Self {
        let message = message.into();
        Self {
            status: if status.is_ok() {
                FerrumStatus::FAILED
            } else {
                status
            },
            message: (!message.is_empty()).then_some(message),
        }
    }

    /// 模组自定义的一般性错误（状态码 `FAILED`）。
    pub fn msg(message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(FerrumStatus::FAILED, message)
    }

    pub fn status(&self) -> FerrumStatus {
        self.status
    }

    pub fn kind(&self) -> ErrorKind {
        ErrorKind::from_status(self.status)
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("status", &format_args!("{}", self.status))
            .field("message", &self.message)
            .finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.message {
            Some(m) => write!(f, "{}: {m}", self.status),
            None => write!(f, "{}", self.status),
        }
    }
}

impl std::error::Error for Error {}

impl From<&'static str> for Error {
    fn from(value: &'static str) -> Self {
        Self::msg(value)
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Self::msg(value)
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        let status = match value.kind() {
            std::io::ErrorKind::NotFound => FerrumStatus::NOT_FOUND,
            std::io::ErrorKind::PermissionDenied => FerrumStatus::PERMISSION_DENIED,
            std::io::ErrorKind::AlreadyExists => FerrumStatus::ALREADY_EXISTS,
            std::io::ErrorKind::InvalidInput => FerrumStatus::INVALID_ARGUMENT,
            _ => FerrumStatus::FAILED,
        };
        Self::new(status, value.to_string())
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(value: std::string::FromUtf8Error) -> Self {
        Self::new(FerrumStatus::INVALID_UTF8, value.to_string())
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(value: std::str::Utf8Error) -> Self {
        Self::new(FerrumStatus::INVALID_UTF8, value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_status_is_preserved() {
        let e = Error::new(FerrumStatus(4242), "");
        assert_eq!(e.kind(), ErrorKind::Unknown(4242));
        assert_eq!(e.to_string(), "UNKNOWN(4242)");
    }

    #[test]
    fn ok_status_never_escapes() {
        assert_eq!(Error::new(FerrumStatus::OK, "x").kind(), ErrorKind::Failed);
    }
}
