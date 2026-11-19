# Rust SDK 参考

`ferrum-sdk` 公开 API 速查。示例默认已 `use ferrum_sdk::prelude::*;`。

当前 ABI 为 `1.2`。下表列出各项能力所需的最低宿主 ABI。宿主缺少所请求的接口时，SDK 返回 `Unsupported`。

| 能力 | ABI |
| --- | --- |
| 生命周期、日志、目录 | 1.0 |
| 玩家、实体、方块、命令、事件 | 1.1 |
| 主线程投递、Tick 定时任务、异步等待主线程结果 | 1.2 |

## 入口与生命周期

```rust
pub struct MyMod { logger: Logger }

#[ferrum_sdk::mod_entry]
impl FerrumMod for MyMod {
    fn on_load(ctx: &ModContext) -> Result<Self> {
        Ok(Self { logger: ctx.logger() })
    }
}
```

`#[mod_entry]` 导出 `ferrum_mod_entry_v1`，给四个回调包上 `catch_unwind` 和重入守卫，并为类型实现 `ModEntryPoint`（供 mock 加载）。

| 阶段 | 签名 | 线程 | 失败时 |
| --- | --- | --- | --- |
| `on_load` | `fn(ctx: &ModContext) -> Result<Self>` | 加载线程 | 加载失败，无实例 |
| `on_enable` | `fn(&mut self, ctx, token) -> Result<()>` | 主线程 | 保持停用，实例保留 |
| `on_disable` | `fn(&mut self, ctx, token) -> Result<()>` | 主线程 | 仍进入停用，记录日志 |
| `on_unload` | `fn(self, ctx) -> Result<()>` | 任意线程 | 记录日志 |

除 `on_load` 外，其余回调都有默认空实现。

- 服务器已运行时用 `ll mod load` 加载，`on_enable` 会紧跟 `on_load` 调用。`on_load` 里不要访问世界。
- `FerrumMod: Sized + Send + 'static`。
- 一个 dll 只能有一个 `#[mod_entry]`，类型不能带泛型。

宏参数：

| 参数 | 作用 |
| --- | --- |
| `min_abi = "1.1"` | 要求宿主 ABI 至少为此版本，不满足则加载失败。默认 `"1.0"`。格式 `"主.次"` |
| `hot_unload_safe` | 声明卸载后无残留线程、TLS 析构、系统回调。当前宿主仍保留镜像，不 `FreeLibrary`（规划中，见 [../roadmap.md](../roadmap.md)） |

`Cargo.toml` 要求见 [quick-start.md](./quick-start.md)。`panic = "abort"` 会编译失败。

## ModContext 与目录

`ModContext` 是 `Copy` 句柄，可移入后台线程。模组卸载后调用返回 `ErrorKind::StaleHandle`。

| 方法 | 说明 |
| --- | --- |
| `name()` / `version()` | manifest 中的 `name` / `version`（缺省 `"0.0.0"`） |
| `mod_dir()` | `mods/<name>/` |
| `data_dir()` | `mods/<name>/data/` |
| `config_dir()` | `mods/<name>/config/` |
| `lang_dir()` | `mods/<name>/lang/` |
| `dir(ModDir::Config)` | 同上，`ModDir::{Mod, Data, Config, Lang}` |
| `logger()` / `commands()` / `events()` / `custom_events()` | 子系统入口 |
| `raw_handle()` | 原始句柄，给直接使用 `ferrum-abi` 的扩展 crate |

写入前用 `std::fs::create_dir_all` 确保目录存在。

## 日志

`ctx.logger()` 返回 `Logger`（`Copy`，任意线程可用），写入 LL 为本模组分配的 logger。卸载后残留线程调用会被静默丢弃。

```rust
let log = ctx.logger();
log.info("加载完成");           // fatal / error / warn / info / debug / trace
if log.enabled(Level::Debug) {
    log.debug(format!("state = {state:?}"));
}
log.log_fmt(Level::Debug, format_args!("tick = {tick}"));
```

## 主线程令牌

`MainThreadToken<'tick>` 是 `!Send + !Sync`，无公开构造函数。只能从 `on_enable` / `on_disable` 及命令、事件回调拿到 `&MainThreadToken`，不能存进结构体或 `thread_local!`。所有世界、实体、命令、事件 API 都要求传入它。

`ferrum_sdk::is_main_thread()` 可在任意线程判断当前是否主线程。

## 错误与 panic

`ferrum_sdk::Result<T>` 的错误类型是 `Error`。

```rust
fn load(ctx: &ModContext) -> Result<String> {
    let path = ctx.config_dir()?.join("settings.toml");
    if !path.exists() {
        return Err(Error::msg("缺少 settings.toml"));
    }
    Ok(std::fs::read_to_string(&path)?) // io::Error 按 kind 映射
}

match ctx.config_dir() {
    Err(e) if e.kind() == ErrorKind::StaleHandle => { /* 模组已卸载 */ }
    Err(e) => ctx.logger().error(format!("{e}")),
    Ok(_) => {}
}
```

- `Error::msg(text)` 对应 `FAILED`；`Error::new(status, text)` 指定状态码。
- `From` 已实现：`&'static str`、`String`、`io::Error`、`Utf8Error`、`FromUtf8Error`。
- `ErrorKind` 是 `#[non_exhaustive]`，常见值：`NotFound`、`InvalidArgument`、`Unsupported`、`AlreadyExists`、`WrongThread`、`ReentrantCall`、`StaleHandle`、`CallbackPoisoned`。

panic 行为：

- 回调里的 `panic!` 被捕获并上报宿主，返回 `MOD_PANICKED`，不会使 BDS 崩溃。
- `on_enable` / `on_disable` panic 后实例被毒化，再次调用返回 `CallbackPoisoned`，直到卸载。
- 10 秒内 panic 超过 16 次，宿主熔断并自动停用该模组。
- 捕获不了：`panic = "abort"`、double panic、栈溢出、OOM、`unsafe` 导致的未定义行为。

细节见 [../design/errors.md](../design/errors.md)。

## 玩家、实体、方块

引用都是瞬态的：`!Send + !Sync`，只在当次回调内有效，不要保存。每次调用按 ID 重新解析，目标消失返回 `NotFound`。跨 tick 保存 `PlayerUuid` 或 `ActorUniqueId`，下次用新令牌重新查找。见 [../design/handles.md](../design/handles.md)。

### 查找

| API | 返回 |
| --- | --- |
| `world::get_player(token, uuid)` | `Result<Option<PlayerRef>>` |
| `world::get_player_by_name(token, name)` | `Result<Option<PlayerRef>>` |
| `world::online_players(token)` | `Result<Vec<PlayerRef>>` |
| `world::get_actor(token, ActorUniqueId)` | `Result<Option<ActorRef>>`，不加载未加载实体 |
| `world::get_actor_by_runtime_id(token, id)` | 同上，仅限本次会话 |
| `world::get_block(token, dim, BlockPos)` | `Result<BlockRef>` |

### 引用层级

`ActorRef` -> `MobRef` -> `PlayerRef`，每层包含上一层。方法由 trait 提供，已在 prelude 中：

| 引用 | trait | 方法 |
| --- | --- | --- |
| `ActorRef` / `MobRef` / `PlayerRef` | `ActorOps` | `runtime_id()`、`unique_id()`、`type_name()`、`kind()`、`position()`、`dimension()`、`teleport(pos, dim)` |
| `MobRef` / `PlayerRef` | `MobOps` | `health()`、`max_health()`、`set_health(v)` |
| `PlayerRef` | 固有方法 | `real_name()`、`uuid()`、`send_message(text)`、`is_operator()`、`game_mode()`、`set_game_mode(m)` |

转换：

- 向上：`as_mob()` / `as_actor()`，只复制 ID。
- 向下：`try_as_mob()` / `try_as_player()`，返回 `Result<Option<_>>`；类别不符为 `Ok(None)`。
- 泛型写法：`fn f<'t>(e: impl ActorOps<'t>)`、`impl MobOps<'t>`。这两个 trait 是 sealed 的，模组不能自行实现。

```rust
if let Some(player) = command.player()? {
    let pos = player.position()?;
    player.teleport(Vec3::new(0.0, 80.0, 0.0), DimensionId::OVERWORLD)?;
    player.set_game_mode(GameMode::CREATIVE)?;
}
```

语义：

| 项 | 规则 |
| --- | --- |
| `Vec3` | `x/y/z: f32`，是 Actor 位置，不一定等于脚下方块坐标 |
| `DimensionId` | `OVERWORLD`=0、`NETHER`=1、`THE_END`=2，自定义维度用 `DimensionId(id)` |
| `teleport` | 坐标必须有限；目标维度须已存在，否则 `NotFound`。成功仅表示已提交，不表示客户端加载完成 |
| 生命值 | 单位为点，2 点 = 1 心。`set_health` 要求有限且 `0 <= v <= max_health`，否则 `InvalidArgument`。直接写属性，不触发伤害或治疗事件 |
| `GameMode` | `SURVIVAL` / `CREATIVE` / `ADVENTURE` / `SPECTATOR`；读到其他值为 `UNKNOWN`，设置 `UNKNOWN` 返回 `InvalidArgument` |
| `ActorKind` | `ACTOR` / `MOB` / `PLAYER`；未知类别保留原值，不当作 Mob 或 Player |

### 方块

```rust
let block = world::get_block(token, DimensionId::OVERWORLD, BlockPos::new(0, 64, 0))?;
block.type_name()?;   // "minecraft:air"
block.is_air()?;
block.set_type("minecraft:stone")?;
block.set_air()?;
```

- `BlockRef` 只存维度和坐标，每次调用重新读取。`position()` / `dimension()` 不访问宿主。
- `runtime_id()` 是当前进程的注册表 ID，不要跨重启或版本保存；持久化用类型名加坐标。
- 只访问已存在维度的已加载区块，不强制加载。缺失维度、区块或未知方块名返回 `NotFound`；高度越界返回 `InvalidArgument`。
- `set_type` 放置默认状态，触发邻居更新和客户端同步。不模拟玩家放置/挖掘，不产生掉落。不支持方块状态、NBT、容器。

## 命令

在 `on_enable` 中注册：

```rust
ctx.commands()
    .register("greet", "向玩家问候", CommandPermission::ANY)
    .required("target", CommandParamKind::PLAYER)
    .optional("message", CommandParamKind::STRING)
    .execute(token, |command, token| {
        let msg = command.string("message")?.unwrap_or_else(|| "你好！".into());
        for p in command.players("target")?.unwrap_or_default() {
            p.send_message(&msg)?;
        }
        command.success(format!("在线 {} 人", world::online_players(token)?.len()))
    })?;
```

| 项 | 说明 |
| --- | --- |
| 参数类型 | `INT`、`FLOAT`、`BOOL`、`STRING`、`PLAYER`。必选参数须在可选参数之前。`STRING` 是单个字符串参数，不是整行文本 |
| 读参数 | `int` / `float` / `bool` / `string` / `players`，返回 `Result<Option<T>>`。未提供的可选参数为 `None`；参数名未知或类型不符为 `InvalidArgument` |
| 来源 | `command.player()?`；控制台等非玩家来源为 `None` |
| 输出 | `command.success(text)` / `command.error(text)` |
| 权限 | `ANY`、`GAME_DIRECTORS`、`ADMIN`、`HOST`、`OWNER`，对应 LL 权限 0-4 |
| 重载 | 同名命令可注册不同参数签名；签名重复返回 `AlreadyExists`；不能接管其他模组或原生命令 |
| 注销 | `ctx.commands().unregister(token, handle)`。停用、启用失败、卸载时自动注销。手动注销后执行路由关闭，命令语法可能仍留在 LL 注册表 |

回调规则（命令与事件相同）：

- 签名为 `for<'tick> FnMut(&Ctx<'tick>, &'tick MainThreadToken<'tick>) -> Result<()> + 'static`，不要求 `Send`，可捕获 `Rc`。
- 只在主线程调用和析构。
- 同一回调重入返回 `ReentrantCall`。
- 回调 panic 后被毒化并摘除。
- 回调里触发本模组停用时，`on_disable` 推迟到最外层回调返回后执行；同步卸载自身返回 `ReentrantCall`。

## 事件

玩家事件通过 `ctx.events()` 订阅，在 `on_enable` 或主线程回调中调用：

```rust
use ferrum_sdk::events::{PlayerChatEvent, PlayerJoinEvent};

let h = ctx.events().subscribe::<PlayerChatEvent>(
    token,
    EventPriority::High,
    |event, _token| {
        if event.message()?.contains("禁词") {
            event.cancel()?;
        }
        Ok(())
    },
)?;
ctx.events().unsubscribe(token, h)?; // 可选，提前移除
```

| 事件 | 取消 | 额外方法 |
| --- | --- | --- |
| `PlayerJoinEvent` | `cancel()` / `is_cancelled()`（仅取消 LL 初始化阶段处理，不会踢人） | - |
| `PlayerChatEvent` | 同上 | `message()`，只读副本 |
| `PlayerDisconnectEvent` | 无 | - |

所有事件有 `player() -> Result<PlayerRef>`。优先级 `Highest` 到 `Lowest`。回调返回 `Err` 只记日志，不隐式取消。`ListenerHandle` 丢弃时不会注销；停用、启用失败、卸载时自动清理。

跨语言自定义事件（`ctx.custom_events()`）与完整规则见 [events.md](./events.md)。

## 主线程调度与异步调用

通过 `ctx.scheduler()` 取得可跨线程复制的调度器。调度 API 需要宿主 ABI 1.2；依赖它才能启用的模组可声明 `#[ferrum_sdk::mod_entry(min_abi = "1.2")]`。

| 方法 | 提交线程 | 执行时机 |
| --- | --- | --- |
| `post(f)` | 任意，闭包需 `Send` | 下一个服务端 Tick |
| `after_ticks(token, ticks, f)` | 主线程，可捕获 `Rc` | 延迟指定 Tick，零延迟在下一 Tick 执行 |
| `every_ticks(token, interval, f)` | 主线程，可捕获 `Rc` | 每隔指定 Tick 执行一次，间隔至少为 1 |
| `cancel(token, handle)` | 主线程 | 取消本模组的任务，正在执行的一次可以完成 |

这些任务可在 `on_enable` 和启用期间注册。同一 Tick 到期的任务按入队顺序执行。并发提交以宿主取得队列锁的顺序为准，周期任务每次重新排期时领取新的序号。周期回调返回错误或 panic 会结束该任务。停用和启用失败时，宿主在主线程撤销剩余任务并析构闭包。

后台 Future 可以等待主线程计算的结果，例如读取在线人数：

```rust
use ferrum_sdk::prelude::*;

async fn online_count(ctx: ModContext) -> Result<usize> {
    ctx.scheduler()
        .on_main_thread(|token| Ok(world::online_players(token)?.len()))
        .await
}
```

首次轮询时提交任务。闭包在主线程获得本次调用的令牌，返回值必须是 `Send + 'static`。等待期间停用会返回 `ErrorKind::ModDisabled`。主线程需能继续推进 Tick，在当前 Tick 内同步阻塞等待结果会造成死锁。`MockHost::tick()` 可手动推进一次。

自行使用 `std::thread::spawn` 或 Tokio 运行时：

1. 保存停止信号（`AtomicBool`、`CancellationToken`）和 `JoinHandle`。`ModContext`、`Logger` 可直接移入线程。
2. `on_disable` 里发停止信号。
3. `on_unload` 里 `join()` 或 `runtime.shutdown_timeout(..)`。
4. 后台线程不能使用令牌和实体引用。需要访问世界时，只传 ID，回到主线程再解析。
5. 不要在模组间传递堆指针或 Rust 对象，每个模组有独立堆。

配置解析用 `serde` 加 `toml` / `serde_json` 读 `config_dir()`。

测试见 [testing.md](./testing.md)。
