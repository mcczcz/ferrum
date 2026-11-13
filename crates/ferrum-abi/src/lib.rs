//! Ferrum 纯 C ABI 契约。
//!
//! 本 crate 只包含 `#[repr(C)]` / `#[repr(transparent)]` 类型、函数指针类型与常量，
//! 由 `ferrum-host`（宿主）与 `ferrum-sdk`（模组）共同依赖，并通过 cbindgen 生成
//! `cpp-shim/include/ferrum/ferrum_abi.h`。
//!
//! 修改本 crate 前必须阅读 `docs/01-specs/abi-stability-spec.md`：
//! 已发布字段只增不改、只尾追加，每次追加都要在 `tests/layout_lock.rs` 中追加断言。

#![no_std]

mod ffi;
mod handle;
mod host_api;
mod module;
mod status;
mod version;

pub use ffi::*;
pub use handle::*;
pub use host_api::*;
pub use module::*;
pub use status::*;
pub use version::*;
