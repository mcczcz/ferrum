# 状态码、异常与 Panic 隔离规范

## 1. 跨 FFI 异常零传播铁律

在 Windows x64 下，C++ 异常与 Rust 栈展开（Unwind）跨越 `extern "C"` 函数边界均属**未定义行为（UB）**（Rust 1.81 起，`extern "C"` 函数内的 panic 展开到边界会直接 `abort` 整个进程）。LeviLamina 自身以 `ll::Expected<T>` 作为主要错误模型，但部分底层或第三方库仍可能抛出异常。

因此，Ferrum 在架构上设立**双向双层异常防火墙**：

```text
+-----------------------+       extern "C"        +-----------------------+       extern "C"        +----------------------+
| LeviLamina / BDS (C++)| <=====================> | ferrum-host (Rust)    | <=====================> | 下游 Rust 模组 (.dll) |
|                       |  [防火墙 A: cpp-shim]   |                       |  [防火墙 B: SDK 跳板]   |                      |
| - ll::Expected<T>     |  try { ... }            | - catch_unwind 兜底   |  catch_unwind(|| {      | - Result<T, E>       |
| - C++ Exceptions      |  catch (...) -> Status  | - 状态码路由与熔断统计 |      user_closure()     | - panic!(...)        |
+-----------------------+                         +-----------------------+  }) -> FerrumStatus     +----------------------+
```

### 1.1 防火墙 A（`cpp-shim` 边界）
- 每一个由 `ferrum-host` 调用的 C++ 垫片函数必须标注 `noexcept`。
- 若内部调用返回 `ll::Expected<T>`，检查 `!res` 并将 `res.error().message()` 写入宿主线程局部错误槽（`LastErrorSlot`），返回对应 `FerrumStatus`。
- 最外层必须包裹 `try { ... } catch (...)`，捕获任何意外 C++ 异常并通过 `ll::error_utils` 提取异常描述写入 `LastErrorSlot`，返回 `FerrumStatus::CPP_EXCEPTION`。
- `ferrum-host` 自身导出给 `cpp-shim` 的 `extern "C"` 函数同样以 `catch_unwind` 包裹，宿主内部 panic 记为 `INTERNAL_ERROR`。

### 1.2 防火墙 B（模组回调边界）
- **下游模组必须编译为 `panic = "unwind"`**。`ferrum-sdk` 内置编译期拦截，配置为 `abort` 时直接编译失败：
  ```rust
  #[cfg(panic = "abort")]
  compile_error!("Ferrum mods must be built with panic = \"unwind\"; see docs/01-specs/error-handling-spec.md");
  ```
- 所有由 `ferrum-macros` 与 `ferrum-sdk` 生成的 `extern "C"` 跳板（`ferrum_mod_entry_v1`、`on_load`/`on_enable`/`on_disable`/`on_unload`、事件回调、命令回调、调度闭包、`drop_fn`、`FfiByteWriter::write_fn`），在进入用户代码最外层**强制包裹 `std::panic::catch_unwind(AssertUnwindSafe(...))`**。
- 一旦捕获到 Panic：
  1. 在模组侧提取 Panic Payload（`&str` 或 `String`）及源码位置（SDK 安装的 panic hook 记录），通过 `CoreApiV1::report_panic` 写入宿主；
  2. **Payload 本身的析构也可能 panic**：SDK 对 payload 调用 `core::mem::forget`（或再次以 `catch_unwind` 包裹其 drop），绝不让二次 panic 越过边界；
  3. 返回 `FerrumStatus::MOD_PANICKED` 给 `ferrum-host`；
  4. 宿主使用**该模组专属的 `ll::io::Logger`** 打印结构化错误，当前事件或命令安全跳过。

### 1.3 Panic 后的回调毒化（Poisoning）
Panic 可能发生在用户闭包修改状态的中途，之后再次调用同一闭包可能观察到不一致状态。
- SDK 在捕获 panic 后设置该回调 `env` 的 `poisoned` 标志；之后该回调的每次派发直接返回 `FerrumStatus::CALLBACK_POISONED`，不进入用户代码。
- 宿主收到 `CALLBACK_POISONED` 时从路由表摘除该回调（与模组其余回调无关），并记录一次 `ERROR`。
- 生命周期钩子（`on_load`/`on_enable`）panic 视为该阶段失败：`on_load` panic 则加载失败；`on_enable` panic 则模组进入 `Disabled`。

### 1.4 防火墙的覆盖边界（不能保证的情况）
`catch_unwind` 只拦截**可展开的 Rust panic**。以下情况依然会终止整个 BDS 进程，Ferrum 不对其做出保证，文档与宣传中**严禁**使用“任何错误都不会崩服”的表述：
- Panic 期间再次 panic（double panic），或 Drop 实现中 panic 导致的 `abort`；
- 栈溢出（Rust 运行时直接 `abort`）、内存耗尽时的 `handle_alloc_error`；
- 模组 `unsafe` 代码或其依赖 crate 中的 UB、访问违例（Access Violation）；
- 模组显式调用 `std::process::abort` / `exit`。

> 准确的承诺是：**模组代码中的普通 Rust panic 不会导致崩服**。内存安全依赖于模组自身不写错误的 `unsafe`。

---

## 2. 统一状态码体系（`FerrumStatus`）

所有可能失败的 `ferrum-abi` 函数均以 `FerrumStatus`（`i32` 透明包装，`0` 为成功）作为返回值，输出数据通过 `out_*` 指针参数或 `FfiByteWriter` 回传。SDK 解码时**必须**把未知值映射为 `Unknown(i32)`，不得假设取值封闭。

```rust
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct FerrumStatus(pub i32);

impl FerrumStatus {
    pub const OK: Self                  = Self(0);

    // 通用参数与契约错误 (1..=99)
    pub const INVALID_ARGUMENT: Self    = Self(1);
    pub const NULL_POINTER: Self        = Self(2);
    pub const INVALID_UTF8: Self        = Self(3);
    pub const BUFFER_TOO_SMALL: Self    = Self(4);
    pub const OUT_OF_MEMORY: Self       = Self(5);
    pub const UNSUPPORTED: Self         = Self(6);   // 宿主不支持该函数/字段，或游戏特性已移除
    pub const FAILED: Self              = Self(7);   // 一般性失败（如模组生命周期钩子返回的自定义错误），详情见 last-error

    // ABI 与生命周期状态错误 (100..=199)
    pub const ABI_INCOMPATIBLE: Self    = Self(100);
    pub const MOD_DISABLED: Self        = Self(101); // 模组已停用、停用中或已卸载
    pub const WRONG_THREAD: Self        = Self(102); // 违反主线程约束
    pub const REENTRANT_CALL: Self      = Self(103); // FnMut 回调重入被守卫拦截

    // 句柄与游戏对象错误 (200..=299)
    pub const STALE_HANDLE: Self        = Self(200); // 代数不匹配或已销毁
    pub const PERMISSION_DENIED: Self   = Self(201); // 跨模组越权操作他人句柄
    pub const NOT_FOUND: Self           = Self(202); // 实体/玩家/维度/服务不存在或已离线
    pub const SERVICE_UNAVAILABLE: Self = Self(203); // Level 或 CommandRegistry 尚未初始化
    pub const ALREADY_EXISTS: Self      = Self(204); // 重复注册同名命令/枚举/服务

    // 异常与故障隔离码 (500..=599)
    pub const CPP_EXCEPTION: Self       = Self(500); // C++ 侧捕获到异常或 Expected 错误
    pub const MOD_PANICKED: Self        = Self(501); // 模组侧发生 Rust Panic
    pub const CALLBACK_POISONED: Self   = Self(502); // 回调此前 panic 过，已被毒化
    pub const INTERNAL_ERROR: Self      = Self(599);
}
```

- 状态码值一经发布**永久冻结**，新增只能使用未分配的值。

---

## 3. 零开销成功路径与 TLS 富错误上下文（Last-Error Pattern）

为兼顾高频调用性能与排错可观测性：
- **成功路径（Fast Path，`status == FerrumStatus::OK`）**：仅寄存器返回整数 `0`，**零堆分配、零字符串格式化**。
- **失败路径（Cold Path，`status != FerrumStatus::OK`）**：
  - `ferrum-host` 在当前系统线程的 Thread-Local Storage（`thread_local!`）中维护一个 `LastErrorSlot`。
  - 当 `cpp-shim` 或 `ferrum-host` 返回非 OK 状态码前，将详细原因（如 LeviLamina `Error::message()` 具体报错：`"Dependency foo requires ^1.2.0"` 或命令语法冲突详情）写入当前线程的 `LastErrorSlot`。
  - `ferrum-sdk` 检测到 `status != OK` 时，**在下一次调用任何宿主函数之前**、于同一线程调用 `core_v1.take_last_error(&mut writer)` 取回诊断文本，组装为 `ferrum_sdk::Error { status, message }` 返回给模组。
  - `LastErrorSlot` 在每次写入时覆盖旧值；`take_last_error` 读取后清空。

---

## 4. 故障熔断保护策略（Circuit Breaker）

若某个存在缺陷的下游模组在高频钩子（例如 Tick 事件或高频移动事件）中反复 panic：
- 回调毒化（§1.3）已能阻止**同一个**回调反复 panic；熔断针对的是模组整体大量回调持续出错的情形。
- **熔断机制**：`ferrum-host` 为每个模组维护滑动窗口 panic 计数器（`crates/ferrum-host/src/breaker.rs`，默认：10 秒内累计超过 `N = 16` 次 `MOD_PANICKED`；后续可在 `ferrum-loader` 配置中开放调整）：
  1. 触发阈值时，宿主输出一条 `FATAL` 级别日志，明确指出故障模组名称；
  2. 自动对该模组执行停用协议（见《线程模型、并发与热卸载安全规范》§4.3），恢复服务器正常 TPS。
- 熔断与毒化都使用限频日志，同一模组同一原因每 10 秒最多输出一次详细堆栈。
