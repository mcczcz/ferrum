//! Ferrum 宿主运行时。
//!
//! 以 `staticlib` 形式与 `cpp-shim` 一起链接为 `ferrum-loader.dll`：
//! - `cpp-shim` 调用本 crate 导出的 `ferrum_host_*` 函数（见 [`exports`]）；
//! - 本 crate 通过 `cpp-shim` 在初始化时注入的 [`shim::FerrumShimApi`] 函数表回调 C++ 侧，
//!   不直接绑定任何 C++ 符号。
//!
//! 锁序铁律：调用任何模组函数指针之前必须先释放本 crate 的内部锁。

pub mod arena;
pub mod breaker;
pub mod exports;
pub mod last_error;
pub mod loader;
pub mod runtime;
pub mod shim;
pub mod vtable;
