# 线程模型、并发与热卸载安全规范

## 1. BDS 线程现实与 `MainThreadToken` 编译期约束

Minecraft Bedrock Dedicated Server 的核心世界模拟（`ServerLevel::tick`、实体 AI、区块读写、容器操作、命令执行、大部分同步事件）严格运行在**单一服务端逻辑线程（Server Thread，下文简称“主线程”）**上；与此同时，RakNet/NetherNet 网络收发、DBStorage 区块压缩以及模组自建的 `tokio`/工作线程池运行在后台线程。

为了在**零运行时锁开销**的前提下从根源杜绝跨线程数据竞争，Ferrum 引入编译期主线程能力令牌（Capability Token）：

```rust
/// 证明当前调用栈正处于 BDS 主线程同步切片内的零大小令牌
pub struct MainThreadToken<'tick> {
    /// *mut () 强制令此类型为 !Send 且 !Sync
    _not_send_sync: core::marker::PhantomData<(&'tick (), *mut ())>,
}
```

### 1.1 线程分类契约

1. **主线程专属 API（要求 `MainThreadToken<'_>`）**：
   - 所有涉及 `Level`、`Dimension`、`BlockSource`、`Actor`、`Player` 属性修改、物品栏变更、生成实体、方块读写、执行命令的 SDK 函数，**必须在函数签名中接收 `token: &MainThreadToken<'_>`**（或只能由令牌派生的瞬态引用如 `PlayerRef<'_>`）。
   - 因为 `MainThreadToken` 是 `!Send + !Sync` 且外部无法自行构造（私有字段），后台线程在**编译期**就无法直接调用任何非线程安全的游戏接口。
   - **防御性运行时兜底**：`ferrum_sdk::is_main_thread()` 由宿主比对记录的主线程 ID 做 `O(1)` 校验（规划中的 `MainThreadToken::try_acquire_current()` 逃生舱口同理，非主线程严格返回 `None`）。`ferrum-host` 对所有主线程专属入口（如 `enable` / `disable` 及后续领域 API）做同样断言，失败返回 `FerrumStatus::WRONG_THREAD`。

2. **任意线程安全 API（`Send + Sync`，无需 Token）**：
   - 日志输出（`Logger::info/warn/error`——底层 `ll::io::Logger` 线程安全）。
   - 向主线程投递闭包或异步等待主线程结果（`scheduler::on_main_thread`、`ctx.scheduler().post(...)`）。
   - 读取不可变模组元数据（版本、路径配置）。
   - 纯内存数据结构构造（如在后台线程组装 `Form` 描述结构或序列化 `Nbt` 缓冲，组装完毕后再投递回主线程发送给玩家）。

### 1.2 令牌作用域必须由 HRTB 锁死

所有接收令牌的回调签名**必须**使用高阶生命周期约束（HRTB），使 `'tick` 由宿主每次调用时重新选定，模组无法把它推断为 `'static`：

```rust
pub fn subscribe<E, F>(&self, priority: EventPriority, f: F) -> Result<ListenerHandle>
where
    E: MainThreadEvent,
    F: for<'tick> FnMut(&EventCtx<'tick, E>, &MainThreadToken<'tick>) + 'static;
```

- **严禁**提供 `fn foo<'tick>(f: impl FnMut(&MainThreadToken<'tick>))` 这类由调用方选定生命周期的签名，否则模组可将令牌或 `PlayerRef<'static>` 存入 `thread_local!` 后在后续 Tick 外使用。
- 令牌只以 `&MainThreadToken<'tick>` 形式借出，SDK 不提供按值获取的公开途径。

### 1.3 事件线程归属核验

- SDK 中每种事件类型声明其线程归属（实现 `MainThreadEvent` 或 `AnyThreadEvent` 标记 trait）。只有 `MainThreadEvent` 的回调会收到令牌。
- `cpp-shim` 在向主线程事件回调授予 `RawMainThreadToken` **之前**必须断言当前线程为主线程。若因上游 LeviLamina 行为变化导致事件在其他线程触发，宿主记录 `ERROR` 日志并跳过本次派发（返回 `WRONG_THREAD`），**绝不**伪造令牌。
- `AnyThreadEvent`（如部分网络包相关事件）的回调签名不含令牌，且闭包必须满足 `Send + Sync`。

### 1.4 回调闭包的 `Send` 要求

| 回调类别 | 注册线程 | 触发与析构线程 | 闭包约束 |
| :--- | :--- | :--- | :--- |
| 主线程事件 / 命令 / Tick 定时器 / 表单回调 | 主线程（需令牌） | 主线程 | `'static`（**不要求** `Send`，可捕获 `Rc`） |
| `on_main_thread` / `post` 投递闭包 | 任意线程 | 主线程 | `Send + 'static` |
| `AnyThreadEvent` 回调 | 主线程 | 任意线程 | `Send + Sync + 'static` |

- 宿主保证非 `Send` 闭包的 `invoke_fn` 与 `drop_fn` **只在主线程执行**。若某条清理路径不在主线程（如进程退出时的非主线程析构），宿主将析构工作投递到主线程；若主线程已不可用，**宁可泄漏，也不跨线程 drop**。

---

## 2. 主线程调度器与异步桥接（对接 `ServerThreadExecutor`）

LeviLamina 提供了成熟的 `ll::thread::ServerThreadExecutor::getDefault()`（参见 `ll/api/thread/ServerThreadExecutor.cpp`），其内部基于无锁队列 `ConcurrentQueue` 与 `ServerLevelTickEvent` 驱动，支持按帧/Tick 定时调度（`execute` / `executeAfter`）及单帧耗时预算控制（默认 30ms 分片防卡服）。

Ferrum 的 `SchedulerApiV1` 直接桥接至该执行器，并在其上增加**模组生命周期取消门控（Cancellation Gate）**：每个任务在执行前检查所属模组状态，模组非 `Enabled` 时不调用 `invoke_fn`，直接 `drop_fn`。

### 2.1 核心调度原语

| ABI 函数（`SchedulerApiV1`） | SDK 封装 | 说明 |
| :--- | :--- | :--- |
| `post(mod_ctx, callback)` | `ctx.scheduler().post(...)` / `scheduler::on_main_thread(...)` | 任意线程调用，排入下一主线程切片执行并授予令牌 |
| `after_ticks(mod_ctx, ticks: u64, callback) -> TaskHandle` | `ctx.scheduler().after_ticks(...)` | 延迟指定游戏刻执行一次，返回可取消的 `TaskHandle` |
| `every_ticks(mod_ctx, delay_ticks: u64, interval_ticks: u64, callback) -> TaskHandle` | `ctx.scheduler().every_ticks(...)` | 周期性主线程任务 |
| `cancel(mod_ctx, task: TaskHandle)` | `TaskHandle::cancel` / `ctx.scheduler().cancel(...)` | 取消未触发任务并在主线程 `drop_fn` |

### 2.2 与 `async/await`（如 `tokio`）的零摩擦协作
`ferrum-sdk` 提供与运行时无关（Runtime-Agnostic，兼容 `tokio`、`async-std`、`smol`）的 `Future` 桥接器 `on_main_thread`：

```rust
// 在模组自建的 tokio 后台任务中安全执行：
pub async fn reward_player_from_db(uuid: PlayerUuid) -> ferrum_sdk::Result<()> {
    // 1. 后台线程执行耗时网络/SQL 查询，绝不阻塞 BDS 主线程 20 TPS
    let coins = sqlx_query_coins(uuid).await?;

    // 2. 挂起当前 Future，排队到 BDS 下一个 Tick 执行主线程闭包并拿回返回值
    let applied = ferrum_sdk::scheduler::on_main_thread(move |token| {
        if let Some(player) = ferrum_sdk::world::get_player(token, uuid) {
            let _ = player.send_message(format!("已到账 {coins} 金币！"));
            true
        } else {
            false // 玩家在此期间已离线，安全放弃
        }
    })
    .await?; // 若等待期间模组被停用，返回 Err，其 status() 为 MOD_DISABLED

    let _ = applied;
    Ok(())
}
```

- 桥接 `Future` 被唤醒所需的 `Waker` 属于模组自身的异步运行时；宿主只持有一个由 SDK 构造的 `FfiCallback`（内部完成 `oneshot` 发送），**宿主不持有任何 `Waker` 或 `dyn` 对象**。
- 模组停用时，宿主对所有未执行的投递闭包调用 `drop_fn`，SDK 在 `drop_fn` 中以 `MOD_DISABLED` 错误完成对应 `oneshot`，等待方因此得到 `Err` 而非永久挂起。

---

## 3. 重入安全（Re-entrancy）与锁序规范

在事件回调或主线程任务执行期间，模组代码极有可能在同一调用栈内触发递归操作（例如：在 `PlayerChatEvent` 监听器里动态注销自身监听器、注册新监听器，或调用 `execute_command` 同步触发另一个命令事件）。

### 3.1 宿主侧（`ferrum-host`）锁序规范
1. **严禁持锁跨越 FFI 回调**：在调用任何模组侧函数指针（`invoke_fn`、`drop_fn`、`on_load`、`on_enable` 等）之前，**必须先释放 `ferrum-host` 内部的互斥锁/读写锁**。标准做法是持短锁克隆 `Arc<CallbackEntry>` 快照，释放锁后再调用模组代码。
2. **延迟墓碑回收**：若模组在回调执行栈中注销该回调自身，将条目标记为 `cancelled = AtomicBool(true)` 并从路由表摘除；真正的 `drop_fn(env)` 延迟到外层 `Arc` 引用计数归零时触发，避免对正在栈上运行的闭包环境提前释放（Use-After-Free during self-unregister）。

### 3.2 模组侧（`ferrum-sdk`）`FnMut` 重入守卫
`FnMut` 闭包的跳板需要从 `env` 取得 `&mut F`。若回调 A 执行中调用的 API 同步触发了事件 B，而 B 又路由回同一个闭包，就会同时存在两个 `&mut F`——这是**未定义行为**，与宿主锁无关。

- SDK 为每个 `FnMut` 回调的 `env` 附带重入标志：
  ```rust
  struct CallbackEnv<F> {
      in_call: core::cell::Cell<bool>,
      poisoned: core::cell::Cell<bool>,
      f: core::cell::UnsafeCell<F>,
  }
  ```
- 跳板入口若发现 `in_call == true`，**不进入用户代码**，直接返回 `FerrumStatus::REENTRANT_CALL`；宿主记录限频 `WARN` 日志（指出模组与回调类型），本次派发对该回调视为跳过。
- `Fn` 闭包只需 `&F`，不受此限制（但用户若在内部使用 `RefCell`，其 `borrow_mut` 冲突会以 panic 形式被防火墙拦截）。
- `poisoned` 标志用于 panic 后熔断，见《状态码、异常与 Panic 隔离规范》§1.3。

---

## 4. `ModOwnership` 与停用 / 卸载协议

### 4.1 宿主全资源归属账本（`ModOwnership`）
`ferrum-host` 为每个已加载的二级模组维护一个独立的 `ModOwnership` 实例，集中追踪该模组申请的一切宿主侧资源：
- 已向 `ll::event::EventBus` 注册的所有 `ListenerPtr`；
- 已通过 `RuntimeOverload` 注册的所有动态命令重载回调；
- 所有尚未触发或周期运行的定时任务与投递闭包；
- 所有等待玩家响应的挂起表单回调（Pending Form Callbacks）；
- 该模组注册的跨模组服务（`ServiceHandle`，见《清单文件与依赖管理》§2）；
- 当前正在执行的宿主→模组回调计数 `active_invocations: AtomicUsize`。

### 4.2 模组状态机

```text
            on_load OK              on_enable OK
 [Loading] ───────────> [Loaded] ───────────────> [Enabled]
                           ^                        │  ▲
                           │      disable 协议      │  │ on_enable OK
                           │                        ▼  │
                           │                     [Disabled]
                           │                        │
                           │                        │ unload 协议
                           │                        ▼
                           │                 [Unloaded: image retained]  (默认)
                           │                 [Unloaded: image released]  (仅 HOT_UNLOAD_SAFE)
```

### 4.3 停用协议（Disable：Fence → Revoke → Drain）
适用于 `ll mod disable`、熔断自动停用、关服 `disableAllMods`，以及卸载的前置阶段。全程在主线程执行：

1. **关闸（Fence）**：状态原子切换为 `Disabling`。此后该模组发起的新注册、新定时器、新投递请求一律返回 `FerrumStatus::MOD_DISABLED`。
2. **通知**：调用模组 `on_disable(ctx, token)`，模组应在此发出自身的取消信号（如 `CancellationToken`）。
3. **撤销（Revoke）**：遍历 `ModOwnership`：
   - 从 `EventBus` 批量移除该模组的全部 `ListenerPtr`；
   - 将命令执行回调替换为拒绝桩，并配合 LeviLamina `CommandRegistrar` 停用该模组命令；
   - 取消全部定时任务与投递闭包；
   - 关闭未决表单回调与异步桥接通道（等待方收到 `MOD_DISABLED`）；
   - 撤销该模组提供的服务，并等待所有 `ServiceGuard` 释放。
4. **排空（Drain）**：在主线程、**已释放宿主锁**的前提下对全部被撤销回调调用 `drop_fn`，模组 `.dll` 此时仍在内存中。
5. 状态切换为 `Disabled`。

> **自我停用的重入情形**：若停用请求发生在该模组自身代码仍在调用栈上时（例如模组在命令回调中执行 `ll mod disable <self>`，此时 `active_invocations > 0`），主线程无法原地等待计数归零（会自锁）。宿主只执行第 1 步关闸，并把第 2–5 步**推迟到最外层回调返回后**（或下一 Tick）执行。

### 4.4 卸载协议（Unload）
1. 若处于 `Enabled`，先执行完整停用协议。
2. 调用模组 `on_unload(ctx)`：模组 `drop` 全局状态，**并 `join` 其自建的所有 OS 线程、`shutdown` 其异步运行时**。
3. **句柄失效**：递增该模组名下所有残留代数槽位的 `generation`。
4. **镜像处置**：
   - **默认：保留镜像（image retained）**。不调用 `FreeLibrary`，模组代码段永久保持映射。代价是少量内存，换取“任何残留线程、TLS 析构函数、遗漏的函数指针都不会跳入已卸载地址”的确定性。
   - **仅当**模组描述符声明 `FerrumModFlags::HOT_UNLOAD_SAFE`（由 `#[ferrum_sdk::mod_entry(hot_unload_safe)]` 设置）时，才执行 `FreeLibrary`。宿主在释放前**应当**枚举进程线程，若发现任何线程起始地址或当前指令地址落在该模组镜像范围内，放弃释放并退回保留镜像，同时记录 `WARN`。

### 4.5 为什么默认不 `FreeLibrary`
Rust 并不正式支持卸载 `cdylib`，以下情况在实践中几乎无法由模组作者 100% 杜绝：
- 模组或其依赖创建的线程（如 `tokio` `spawn_blocking` 空闲线程默认保活 10 秒）在 `on_unload` 返回后仍存活；
- 其他线程上注册的 `thread_local!` 析构函数位于模组代码段，线程退出时跳入已卸载地址；
- 依赖 crate 向操作系统注册了回调（控制台 Ctrl 处理器、Win32 定时器、WinHTTP 回调等）；
- `static` 中的 `OnceLock` / `lazy_static` 不会被析构，若持有句柄或线程则无法回收。

### 4.6 保留镜像下的重新加载（Shadow Copy）
- Windows 会锁定已加载 `.dll` 文件，且对同一路径重复 `LoadLibraryExW` 只会返回同一镜像（`static` 状态延续）。
- 因此 `FerrumModManager::load` **应当**先将 `entry` 指向的 `.dll`（及同名 `.pdb`）复制到 `<mod_dir>/.ferrum-shadow/<name>-<content_hash>.dll` 再加载：
  - 服主/开发者可以在运行期直接覆盖 `mods/<name>/<name>.dll`，再 `ll mod reload <name>` 载入新版本；
  - 旧镜像保持映射（每次 reload 泄漏一份代码段，属于可接受的开发期成本）；
  - 启动时清理不再被任何已加载镜像引用的旧影子文件。

> **模组作者守则**：即使默认保留镜像，若模组内部自行启动了 `std::thread::spawn` 或独立 `tokio::Runtime`，仍**必须**在 `on_disable` 中发出停止信号、在 `on_unload` 返回前完成 `join`/`shutdown`，否则这些线程会在模组“已卸载”后继续运行并访问已撤销的资源（得到 `MOD_DISABLED` 错误）。只有在能证明全部线程与 OS 回调均已回收时，才可声明 `hot_unload_safe`。
