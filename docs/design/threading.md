# 线程模型与停用卸载

## 1. 线程与主线程令牌

- **主线程约束**：世界、实体、方块、命令、主线程事件、表单、经济及 `on_enable`/`on_disable` 仅限 BDS 主线程；其他线程返回 `WRONG_THREAD`。日志、元数据与 `is_main_thread()` 任意线程可用。
- **`MAIN_THREAD`**：生产构建在启用阶段由 shim 登记一次 `OnceLock<ThreadId>`；`ferrum-host/mock` 仅允许在上一会话全部卸载后重新绑定测试线程。
- **触发线程事件**：`Console*` 与 `Service*` 经 `subscribe_sync` 在 LL 触发线程同步调用（回调须 `Fn + Send + Sync`），不授予主线程令牌。
- **主线程调度**：loader 监听 `ServerLevelTickEvent` 驱动 `Scheduler` 队列；`Send` 任务可由后台投递，非 `Send` 闭包仅在主线程注册、调用与析构。
- **`MainThreadToken<'tick>` 与 HRTB**：零大小 `!Send + !Sync`，外部不可构造。接收令牌的回调签名必须使用 HRTB（`for<'tick> FnMut(..., &'tick MainThreadToken<'tick>)`），防止借由外层生命周期参数把令牌逃逸到 `thread_local!`。

## 2. 锁与重入规则

- **严禁持锁回调**：调用任何模组函数指针前必须释放 `ferrum-host` 全部锁（持锁克隆 `Arc`，放锁后再调）。
- **重入守卫**：SDK 的 `FnMut` 跳板通过 `in_call` 标志拦截同闭包重入，直接返回 `REENTRANT_CALL`，避免生成两个 `&mut F` 导致 UB。

## 3. 状态机与停用协议

`ModState`：`Loading` -> `Loaded` -> `Enabling` -> `Enabled` -> `Disabling` -> `Disabled`，以及 `Unloading`。注册命令/监听器/任务仅在 `Enabling` / `Enabled` 允许；回调仅在 `Enabled` 派发。

停用流程（`ll mod disable`、熔断、关服，仅主线程）：
1. **关闸**：状态置为 `Disabling`（新派发与注册返回 `MOD_DISABLED`）。
2. **通知**：调用 `on_disable`（失败只记日志，不中断停用）。
3. **撤销与排空**：按 `ModOwnership`（`commands`、`listeners`、`custom_listeners`、`custom_providers` 及调度任务）从路由表移除，唤醒等待中的异步 Future（返回 `MOD_DISABLED`），并释放 `Arc` 触发 `drop_fn`。
4. **落定**：状态置为 `Disabled`。

### 回调栈内停用与 RAII 守卫

若停用触发时当前正处于该模组回调内（`active_invocations > 0`），第 1 步仅置 `pending_disable = true`，第 2–4 步推迟到最外层回调返回后完成；栈内同步自卸载直接返回 `REENTRANT_CALL`。

- 所有宿主调模组派发路径必须使用 `loader::Invocation::begin(ctx, entry)`（或 `custom_event_core.rs` 的 `Active`）守卫，严禁手动增减 `active_invocations`。
- **析构顺序硬要求**：回调对象（`Arc`）必须在守卫**之后**声明或重新绑定（`let cb = cb;`），确保无论是正常返回还是宿主 panic 展开，都**先释放回调触发 `drop_fn`，再配平 `active_invocations`**。

## 4. 卸载与影子副本

- **卸载**：若处于 `Enabled` 先走停用；置 `Unloading` 后调用 `on_unload`，移除 `ModEntry` 并递增 `ModContextHandle` 代数。
- **保留镜像**：默认不调用 `FreeLibrary`，防止模组自建线程、TLS 析构或未清理回调引发进程崩溃（`HOT_UNLOAD_SAFE` 目前仅作预留标志）。
- **影子重载**：加载时按内容哈希将 DLL 与同名 `.pdb` 复制到 `<mod_dir>/.ferrum-shadow/<name>-<hash>.dll` 后加载（并清理未被映射的旧副本），从而允许开发期直接覆盖原 DLL 并执行 `ll mod reload`。
