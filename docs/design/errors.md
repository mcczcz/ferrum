# 错误、Panic 与熔断

C++ 异常与 Rust panic 展开跨 `extern "C"` 边界均为 UB，因此两层边界分别设有隔离防火墙。

```text
LeviLamina / BDS (C++) <--[A: cpp-shim]--> ferrum-host (Rust) <--[B: SDK 跳板]--> 模组 (Rust)
```

## 1. 防火墙 A（`cpp-shim` 与 `ferrum-host`）

- `cpp-shim` 导出函数全部标注 `noexcept`，外层 `try / catch (...)` 捕获异常并通过 `ll::error_utils::makeExceptionString` 写入 last-error，返回 `CPP_EXCEPTION`。
- `ferrum-host` 导出的 `extern "C"` 入口全部包裹 `vtable::guard`（`catch_unwind` + `last_error::scope`）：拦截宿主内部 panic 返回 `INTERNAL_ERROR`，并在未写入具体详情的失败路径补充上下文，隔离历次调用的错误槽。

## 2. 防火墙 B（`ferrum-sdk` 跳板）

- `ferrum-sdk` 对 `panic = "abort"` 使用 `compile_error!` 强制拦截。
- 所有进入模组代码的路径（入口、生命周期、事件/命令/自定义事件/表单/调度回调、`drop_fn`、`write_fn`）均包裹 `catch_unwind`（统一经 `crates/ferrum-sdk/src/firewall.rs`）。
- 捕获 panic 后提取消息与源码位置，调用 `CoreApiV1::report_panic` 上报宿主日志，`mem::forget(payload)` 防止 payload 析构二次 panic，最终返回 `MOD_PANICKED`。

**不可拦截边界**：`Drop` 内的二次 panic（abort）、栈溢出、OOM、模组 `unsafe` 引发的访问违例及主动 `process::abort`。

## 3. 回调毒化与熔断

- **毒化**：SDK 回调 `env` 在捕获 panic 后置 `poisoned = true`，后续调用直接返回 `CALLBACK_POISONED`。宿主收到 `MOD_PANICKED` 或 `CALLBACK_POISONED` 后摘除该回调；`on_enable` panic 则使模组保持 `Disabled`。
- **熔断器**（`crates/ferrum-host/src/breaker.rs`）：每个模组维护 10 秒滑动窗口；窗口内 `report_panic` 超过 16 次（第 17 次）时写 `FATAL` 日志并调用 `shim.request_disable`，在下一个主线程切片停用该模组；重新 `enable` 时复位。

## 4. `FerrumStatus` 与 `LastErrorSlot`

`FerrumStatus(pub i32)`（`0` 为 `OK`）取值发布后永久冻结；SDK 将未知状态码映射为 `ErrorKind::Unknown(i32)`。

| 类别 | 状态码 |
| :--- | :--- |
| 通用 | `OK` (0)、`INVALID_ARGUMENT` (1)、`NULL_POINTER` (2)、`INVALID_UTF8` (3)、`BUFFER_TOO_SMALL` (4)、`OUT_OF_MEMORY` (5)、`UNSUPPORTED` (6)、`FAILED` (7) |
| ABI 与生命周期 | `ABI_INCOMPATIBLE` (100)、`MOD_DISABLED` (101)、`WRONG_THREAD` (102)、`REENTRANT_CALL` (103) |
| 句柄与对象 | `STALE_HANDLE` (200)、`PERMISSION_DENIED` (201)、`NOT_FOUND` (202)、`SERVICE_UNAVAILABLE` (203)、`ALREADY_EXISTS` (204) |
| 异常隔离 | `CPP_EXCEPTION` (500)、`MOD_PANICKED` (501)、`CALLBACK_POISONED` (502)、`INTERNAL_ERROR` (599) |

**`LastErrorSlot`**（`crates/ferrum-host/src/last_error.rs`）：成功路径零分配仅返回状态码；失败时写入当前线程错误槽，SDK 在同一线程立即调用 `take_last_error` 取回并清空。模组回调返回 `Err` 时通过 `set_last_error` 传回文本。
