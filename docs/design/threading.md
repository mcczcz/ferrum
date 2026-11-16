# 线程与停用卸载

## 线程分类

| 类别 | 内容 | 约束 |
| :--- | :--- | :--- |
| 主线程 | 世界、实体、方块、命令、事件、enable/disable | 需要 `&MainThreadToken<'_>` |
| 任意线程 | 日志、读模组元数据、`is_main_thread()` | 无 |

主线程 ID 由 shim 在启用时登记。宿主所有领域 API 入口都检查它，失败返回 `WRONG_THREAD`。shim 在派发事件和命令前也检查，不通过就不授予令牌。

测试时，`ferrum-host-mock` 通过 `ferrum-host/mock` feature 使用同一套宿主逻辑。shim 函数表、SDK 根表和自定义事件 hooks 保持固定；互斥的 mock 会话只在旧模组全部卸载后重新绑定主线程并清理假世界。生产构建仍使用只初始化一次的主线程 ID。

调度器与 `async` 桥接：规划中，见 [../roadmap.md](../roadmap.md)。

## MainThreadToken

`MainThreadToken<'tick>` 是 `!Send + !Sync` 的零大小类型，外部不能构造。回调只拿到 `&MainThreadToken`。

接收令牌的回调必须用 HRTB，让 `'tick` 由宿主每次调用重选：

```rust
F: for<'tick> FnMut(&EventCtx<'tick, E>, &'tick MainThreadToken<'tick>) -> Result<()> + 'static
```

不要写 `fn foo<'tick>(f: impl FnMut(&MainThreadToken<'tick>))`。否则模组能把令牌存进 `thread_local!` 在 Tick 外使用。

回调只要 `'static`，不要求 `Send`，可以捕获 `Rc`。它们只在主线程调用和析构。

## 锁规则

- 调用任何模组函数指针前，先释放宿主所有锁。做法：持锁克隆 `Arc`，放锁后再调。
- 回调被注销时，`drop_fn` 随最后一个 `Arc` 释放，不会在栈上还在运行时触发。
- SDK 的 `FnMut` 跳板带 `in_call` 标志。重入同一闭包时直接返回 `REENTRANT_CALL`，不进用户代码。两个 `&mut F` 同时存在是 UB，与宿主锁无关。
- panic 后设 `poisoned`，之后返回 `CALLBACK_POISONED`。见 [errors.md](errors.md)。

## ModOwnership

每个模组一份，在 `crates/ferrum-host/src/command.rs`：

| 字段 | 用途 |
| :--- | :--- |
| `commands` / `listeners` | 已注册的命令、原生事件监听器 |
| `custom_listeners` / `custom_providers` | 自定义事件监听器、发布者计数 |
| `active_invocations` | 正在执行的宿主到模组回调数 |
| `pending_disable` | 停用被推迟 |

## 状态机

`ModState`：`Loading`、`Loaded`、`Enabling`、`Enabled`、`Disabling`、`Disabled`、`Unloading`。

```text
Loading -on_load OK-> Loaded -enable-> Enabling -on_enable OK-> Enabled
Enabled -disable-> Disabling -> Disabled -enable-> Enabling
Enabling -on_enable 失败/panic-> Disabled
Loading -on_load 失败/panic-> 记录移除
任意 -unload-> Unloading -> 句柄失效
```

- 注册命令和监听器只在 `Enabling` / `Enabled` 允许，否则 `MOD_DISABLED`。
- 回调只在 `Enabled` 派发。

## 停用协议

`ll mod disable`、熔断、关服都走这里，只在主线程执行。

1. 关闸：状态设为 `Disabling`。新注册和新派发得到 `MOD_DISABLED`。
2. 通知：调用 `on_disable`。失败只记日志，仍继续。
3. 撤销：移除命令、事件监听器、自定义事件。
4. 排空：释放被撤销回调，调用 `drop_fn`。模组 DLL 仍在内存。
5. 状态设为 `Disabled`。

### 自身回调内停用

回调栈上有本模组代码时，`active_invocations > 0`，不能原地等待。只执行第 1 步并置 `pending_disable`。第 2 至 5 步在最外层回调返回时完成。

### 计数用 RAII

计数由 `crates/ferrum-host/src/loader.rs` 的 `enter` / `leave` 维护，对外用守卫：

| 路径 | 守卫 |
| :--- | :--- |
| `command.rs`、`event.rs` 派发 | `Invocation::begin(ctx, entry)` |
| `custom_event_core.rs` 的 `hooks.begin/finish` | `Active` |

守卫析构时配平计数。宿主代码在派发中途 panic 也一样，推迟的停用会在最外层回调结束时完成，模组不会卡在 `Disabling`。

新增派发路径必须用守卫，不要手写 `+= 1` / `-= 1`。

守卫必须最后析构：先释放回调（可能执行 `drop_fn`），再配平计数。局部变量按声明逆序析构，末尾的显式 `drop` 只管正常路径；panic 展开时只看声明顺序。所以回调对象要在守卫**之后**声明，已在前面取到的就重新绑定：

```rust
let _invocation = match Invocation::begin(ctx, entry.clone()) {
    Ok(g) => g,
    Err(s) => return s,
};
let listener = listener; // 晚于守卫声明，所以早于守卫析构
```

## 卸载

1. `Enabled` 先走停用。回调栈上有本模组代码时返回 `REENTRANT_CALL`。
2. 状态设为 `Unloading`，撤销资源，调用 `on_unload`。
3. 从注册表移除，`ModContextHandle` 代数递增。残留线程之后的调用得到 `STALE_HANDLE`。

镜像始终保留，不调用 `FreeLibrary`。原因：模组自建线程、`thread_local!` 析构、依赖注册的 OS 回调，作者无法保证全部回收。`HOT_UNLOAD_SAFE` 标志目前只记一条 debug 日志，真正的热卸载在 M5。

## 影子副本重载

Windows 锁定已加载的 DLL，同路径重复加载只返回同一镜像。所以 `load` 先把 `entry` 复制到 `<mod_dir>/.ferrum-shadow/<name>-<hash>.dll`，同名 `.pdb` 一并复制。

- 可以直接覆盖 `mods/<name>/<name>.dll`，再 `ll mod reload <name>`。
- 旧镜像保留，每次 reload 多占一份代码段。
- 加载时顺带删除其他旧影子文件，仍被映射的会删除失败，忽略。

## 模组作者

自建 `std::thread` 或异步运行时，必须在 `on_disable` 发停止信号，在 `on_unload` 返回前 `join` / `shutdown`。否则线程在模组卸载后继续运行，只会得到 `MOD_DISABLED` 或 `STALE_HANDLE`。
