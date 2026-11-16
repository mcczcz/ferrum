# 错误、Panic 与熔断

C++ 异常和 Rust panic 展开跨过 `extern "C"` 边界都是 UB。所以两个边界各设一道防火墙。

```text
LeviLamina/BDS (C++) <-[A: cpp-shim]-> ferrum-host (Rust) <-[B: SDK 跳板]-> 模组 (Rust)
```

## 防火墙 A：cpp-shim

- 导出给 `ferrum-host` 的函数标 `noexcept`。
- 外层 `try { ... } catch (...)`。捕获后用 `ll::error_utils::makeExceptionString` 取描述，写入 last-error，返回 `CPP_EXCEPTION`。
- `ferrum-host` 自己导出的 `extern "C"` 函数用 `catch_unwind` 包裹（`vtable::guard`）。宿主内部 panic 返回 `INTERNAL_ERROR`。
- 状态码 API 的 guard 隔离每次调用的错误槽；普通失败未提供详情时，按操作位置和状态补充说明，避免返回此前调用遗留的错误。shim 或模组提供的具体详情优先保留。

## 防火墙 B：SDK 跳板

- 模组必须 `panic = "unwind"`。`ferrum-sdk` 对 `panic = "abort"` 用 `compile_error!` 拦截。
- 进入模组代码的跳板都包 `catch_unwind`：`ferrum_mod_entry_v1`、四个生命周期钩子、事件/命令/自定义事件回调、`drop_fn`、`FfiByteWriter::write_fn`。除入口和 `write_fn` 外都经过 `firewall()`（`crates/ferrum-sdk/src/firewall.rs`）。
- 捕获 panic 后：
  1. 取 payload 文本和位置（SDK 的 panic hook 记录位置）。
  2. 调 `CoreApiV1::report_panic` 上报宿主，宿主写模组 Logger。
  3. `mem::forget(payload)`，防止 payload 析构再 panic。
  4. 返回 `MOD_PANICKED`。

## 毒化

闭包可能在改状态中途 panic，不能再调用。

- SDK 捕获 panic 后给回调 `env` 置 `poisoned`。之后每次派发返回 `CALLBACK_POISONED`，不进用户代码。
- 宿主收到 `MOD_PANICKED` 或 `CALLBACK_POISONED`，把该回调从路由表移除（只影响这一个回调）。
- `on_load` panic：加载失败。`on_enable` panic：模组进入 `Disabled`。

## 不能保证的情况

`catch_unwind` 只拦截可展开的 panic。以下情况仍会终止整个进程：

- double panic，或 `Drop` 里 panic 导致 abort
- 栈溢出、内存耗尽（`handle_alloc_error`）
- 模组 `unsafe` 代码或依赖里的 UB、访问违例
- 模组调用 `process::abort` / `exit`

承诺的范围：模组里普通的 Rust panic 不会崩服。

## FerrumStatus

`i32` 透明包装，`0` 为成功。定义在 `crates/ferrum-abi/src/status.rs`。取值发布后冻结，只能用未分配的值新增。SDK 把未知值解码为 `Unknown(i32)`。

| 范围 | 名称 | 值 | 含义 |
| :--- | :--- | :--- | :--- |
| 0 | `OK` | 0 | 成功 |
| 通用 | `INVALID_ARGUMENT` | 1 | 参数非法 |
| | `NULL_POINTER` | 2 | 空指针 |
| | `INVALID_UTF8` | 3 | 非 UTF-8 |
| | `BUFFER_TOO_SMALL` | 4 | 缓冲区太小 |
| | `OUT_OF_MEMORY` | 5 | 内存不足 |
| | `UNSUPPORTED` | 6 | 宿主不支持，或游戏特性已移除 |
| | `FAILED` | 7 | 一般失败，详情见 last-error |
| ABI 与生命周期 | `ABI_INCOMPATIBLE` | 100 | 版本或布局不兼容 |
| | `MOD_DISABLED` | 101 | 模组已停用、停用中或已卸载 |
| | `WRONG_THREAD` | 102 | 不在主线程 |
| | `REENTRANT_CALL` | 103 | `FnMut` 重入被拦截；也用于在回调内卸载自身 |
| 句柄与对象 | `STALE_HANDLE` | 200 | 代数不匹配或已销毁 |
| | `PERMISSION_DENIED` | 201 | 操作了其他模组的句柄 |
| | `NOT_FOUND` | 202 | 实体、玩家、维度不存在或已离线 |
| | `SERVICE_UNAVAILABLE` | 203 | Level 或命令注册表未就绪 |
| | `ALREADY_EXISTS` | 204 | 重复注册 |
| 异常隔离 | `CPP_EXCEPTION` | 500 | C++ 侧异常 |
| | `MOD_PANICKED` | 501 | 模组 panic |
| | `CALLBACK_POISONED` | 502 | 回调已毒化 |
| | `INTERNAL_ERROR` | 599 | 宿主内部错误 |

## last-error

成功路径只返回整数，不分配内存。失败时：

1. `cpp-shim` 或 `ferrum-host` 返回非 `OK` 前，把详情写入当前线程的 last-error 槽（`crates/ferrum-host/src/last_error.rs`）。每次写入覆盖旧值。
2. SDK 在同一线程、下一次调用宿主之前，用 `CoreApiV1::take_last_error` 取回文本，组成 `Error { status, message }`。取后清空。
3. 模组侧错误用 `set_last_error` 写入，生命周期钩子返回 `Err` 时由宿主读出。

必须在同一线程立刻取，否则会读到别的调用的错误。

## 熔断

`crates/ferrum-host/src/breaker.rs`。每个模组一个滑动窗口计数器，每次 `report_panic` 记一次。

| 项 | 值 |
| :--- | :--- |
| 窗口 | 10 秒 |
| 阈值 | 窗口内超过 16 次（第 17 次触发） |
| 触发动作 | 写 `FATAL` 日志，调 shim 的 `request_disable`，在下一个主线程切片停用该模组 |
| 复位 | 模组重新 `enable` 时 |

每次熔断只触发一次。毒化只拦同一个回调，熔断拦的是整个模组持续出错。阈值目前写死，未开放配置。
