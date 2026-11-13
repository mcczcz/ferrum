//! Ferrum 模组开发 SDK。
//!
//! ```ignore
//! use ferrum_sdk::prelude::*;
//!
//! pub struct MyMod {
//!     logger: Logger,
//! }
//!
//! #[ferrum_sdk::mod_entry]
//! impl FerrumMod for MyMod {
//!     fn on_load(ctx: &ModContext) -> Result<Self> {
//!         let logger = ctx.logger();
//!         logger.info("加载中");
//!         Ok(Self { logger })
//!     }
//! }
//! ```
//!
//! 模组必须编译为 `cdylib` 且保持 `panic = "unwind"`。

#[cfg(panic = "abort")]
compile_error!(
    "Ferrum mods must be built with panic = \"unwind\"; see docs/01-specs/error-handling-spec.md"
);

mod context;
mod error;
mod firewall;
mod host;
mod lifecycle;
mod logger;
mod token;

pub use context::{ModContext, ModDir};
pub use error::{Error, ErrorKind, Result};
pub use ferrum_macros::mod_entry;
pub use lifecycle::{FerrumMod, ModEntryPoint};
pub use logger::{Level, Logger};
pub use token::{MainThreadToken, is_main_thread};

/// 本 SDK 的版本（写入模组描述符，仅用于诊断）。
pub const SDK_VERSION: ferrum_abi::AbiVersion = ferrum_abi::AbiVersion::new(
    parse_u16(env!("CARGO_PKG_VERSION_MAJOR")),
    parse_u16(env!("CARGO_PKG_VERSION_MINOR")),
    parse_u16(env!("CARGO_PKG_VERSION_PATCH")) as u32,
);

const fn parse_u16(s: &str) -> u16 {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut value: u16 = 0;
    while i < bytes.len() {
        value = value * 10 + (bytes[i] - b'0') as u16;
        i += 1;
    }
    value
}

pub mod prelude {
    pub use crate::{
        Error, ErrorKind, FerrumMod, Level, Logger, MainThreadToken, ModContext, ModDir, Result,
    };
}

/// 宏展开与 `ferrum-host-mock` 使用的内部接口，不属于稳定 API。
#[doc(hidden)]
pub mod __private {
    pub use crate::lifecycle::{
        EntryCallbacks, ModState, entry, on_disable, on_enable, on_load, on_unload,
    };
    pub use ferrum_abi as abi;
}
