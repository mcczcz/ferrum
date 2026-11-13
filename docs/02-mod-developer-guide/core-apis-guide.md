# 事件、命令、表单与异步调度指南（设计规划）

> **实现状态提示**：本文档描述的四大领域子系统目前处于**规划阶段**（事件、玩家、命令在 **M2** 落地；异步调度在 **M3** 落地；表单在 **M4** 落地）。
> 当前（**M1**）已可用的 SDK 能力为生命周期（`FerrumMod`）、日志（`Logger`）、元数据与目录（`ModContext`）与 `MainThreadToken`，可参考 `examples/hello/`。

本文档概述 `ferrum-sdk` 四大高频子系统的底层桥接原理与目标用法。

---

## 1. 事件系统（Events）

### 底层机制
`cpp-shim` 为每个受支持的 LeviLamina 事件（如 `PlayerJoinEvent`、`PlayerChatEvent`、`PlayerDestroyBlockEvent`、`MobDieEvent` 等）实现统一的 `ll::event::ListenerBase` 适配器：
- 注册监听时，显式将二级模组的 `std::weak_ptr<ll::mod::Mod>` 传入 `ListenerBase(priority, weakMod)`，使 `EventBus` 原生认领该监听器归属于对应的二级 Rust 模组。
- 事件触发时，`ferrum-host` 为本次派发分配一个事件作用域句柄（`EventScopeHandle`），连同 `RawMainThreadToken`（仅主线程事件）调用模组回调；派发结束后句柄立即失效。事件中的实体以 `ActorRuntimeId` 提供，访问时由 shim 按 ID 重新解析（见《句柄与游戏对象标识规范》§3.1）。
- 若为可拦截事件（继承自 `Cancellable`），模组调用 `event.cancel()` 即同步映射为底层 `event.cancel()`。
- 每种事件类型标注线程归属：`MainThreadEvent` 回调会收到令牌；`AnyThreadEvent` 回调不含令牌且闭包须为 `Send + Sync`。

### SDK 用法示例
```rust
let listener = ctx.events().subscribe::<PlayerDestroyBlockEvent>(
    EventPriority::High,
    |event, token| {
        let player = event.player();
        let pos = event.pos();
        if pos.y > 200 && !player.is_operator(token).unwrap_or(false) {
            let _ = player.send_message("高空保护区禁止破坏方块！");
            event.cancel(); // 拦截破坏行为
        }
    },
)?;
// listener 为 ListenerHandle，既可提前调用 ctx.events().unsubscribe(listener) 手动移除，
// 也可交由模组停用时由宿主自动清理。
```

- 回调签名由 HRTB 约束为 `for<'tick> FnMut(&EventCtx<'tick, E>, &MainThreadToken<'tick>)`，`event`、`player`、`token` 均无法被保存到回调之外。
- 回调内对游戏对象的操作返回 `Result`：实体可能已在本回调中被其他操作销毁，此时得到 `Err(NotFound)` 而不是崩溃。
- 若回调中的操作同步触发了**同一个** `FnMut` 回调（重入），内层派发会被 SDK 跳过并记录警告。

---

## 2. 运行时动态命令系统（Runtime Commands）

### 底层机制
由于下游 Rust 模组在编译期不接触 C++ 模板 `CommandHandle::overload<Params>`，Ferrum 统一对接 LeviLamina 的**运行时动态命令接口 `ll::command::RuntimeOverload`**（参见 `ll/api/command/runtime/RuntimeOverload.h`）：
- 支持全部 `ParamKind`（`Int`, `Float`, `Bool`, `String`, `RawText`, `BlockPos`, `Vec3`, `Actor`, `Player`, `Enum`, `SoftEnum` 等）；
- 命令注册必须在 `on_enable`（`CommandRegistry` 已就绪）阶段进行；
- 向 `CommandHandle::runtimeOverload(weakMod)` 显式绑定目标二级模组的 `weak_ptr<Mod>`，当执行 `ll mod disable <mod>` 时，LeviLamina `CommandRegistrar::disableModCommands` 与 Ferrum `ModOwnership` 自动停用该模组注册的所有命令重载。

### SDK 用法示例（支持 Builder 与派生宏）
```rust
#[derive(ferrum_sdk::CommandArgs)]
struct PayArgs {
    target: PlayerSelector,
    amount: i32,
    #[cmd(optional)]
    reason: Option<String>,
}

ctx.commands()
    .register("pay", "向其他玩家转账", CommandPermission::Any)?
    .overload::<PayArgs>(|origin, args, output, token| {
        let Some(sender) = origin.as_player(token) else {
            output.error("该命令只能由玩家执行");
            return;
        };
        if args.amount <= 0 {
            output.error("转账金额必须为正整数");
            return;
        }
        let sender_name = sender.real_name().unwrap_or_default();
        for target in args.target.resolve(token, &origin) {
            let target_name = target.real_name().unwrap_or_default();
            output.success(format!("{sender_name} -> {target_name} : {}", args.amount));
        }
    })?;
```

---

## 3. 表单交互系统（Forms: Simple / Modal / Custom）

### 底层机制
对接 `ll::form::SimpleForm`、`ModalForm` 与 `CustomForm`：
- 模组可在任意线程构造纯数据描述的表单结构（或通过句柄流式追加元素）；
- 调用 `form.send_to(player, callback)`（或异步 `form.send_async(player).await`）必须在持有 `MainThreadToken`（或 `PlayerRef`）时发起；
- 表单响应回调在主线程触发，携带玩家选项或 `FormCancelReason`（如 `UserClosed`、`UserBusy`），同时注册进 `ModOwnership`，等待期间若模组被停用，闭包会被安全释放，异步等待方收到 `MOD_DISABLED` 错误。

### SDK 异步表单示例
```rust
// 结合异步调度器编写线性无回调地狱的多级菜单：
let player_uuid = player.uuid();
ctx.scheduler().spawn_local_flow(move |flow| async move {
    let confirm = ModalForm::new("传送确认", "是否花费 10 金币回到主城？", "确认", "取消");
    let response = flow.send_modal_form(player_uuid, confirm).await?;

    if response == ModalResponse::Confirmed {
        flow.with_main_thread(move |token| {
            if let Some(p) = world::get_player(token, player_uuid) {
                let _ = p.teleport(Vec3::new(0.5, 65.0, 0.5), DimensionId::OVERWORLD);
            }
        }).await?;
    }
    Ok::<_, ferrum_sdk::Error>(())
});
```

---

## 4. Tick 定时器与跨线程异步桥接（Scheduler）

| 需求场景 | 推荐 API | 执行线程 |
| :--- | :--- | :--- |
| 下一 Tick 执行（任意线程调用） | `ctx.scheduler().post(\|token\| { ... })` | BDS 主线程 |
| 延迟 `N` 游戏刻执行单次世界操作 | `ctx.scheduler().after_ticks(20, \|token\| { ... })` | BDS 主线程 |
| 每隔 `N` 游戏刻周期巡检 | `ctx.scheduler().every_ticks(20, 100, \|token\| { ... })` | BDS 主线程 |
| HTTP 请求、数据库读写、加解密计算 | 模组内 `tokio::spawn(async { ... })` | 模组后台工作线程 |
| 后台计算完毕后修改玩家背包/发消息 | `ferrum_sdk::scheduler::on_main_thread(\|token\| { ... }).await` | 切回 BDS 主线程执行并返回结果 |

> **性能黄金法则**：永远不要在持有 `MainThreadToken` 的同步上下文（事件回调、命令回调、`on_main_thread` 闭包）中调用同步阻塞的网络请求、文件锁等待或 `thread::sleep`，单帧超过 50ms 将直接造成全服掉刻卡顿！
