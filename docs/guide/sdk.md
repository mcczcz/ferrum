# Rust SDK 手册

`ferrum-sdk` 公开 API 速查。以下示例默认已导入 `use ferrum_sdk::prelude::*;`。

当前宿主 ABI 为 **`1.5`**。宿主缺少某项接口时，对应调用返回 `ErrorKind::Unsupported`；如需在加载阶段就校验最低宿主版本，使用 `#[ferrum_sdk::mod_entry(min_abi = "1.x")]`。

| 能力 | 最低 ABI |
| :--- | :--- |
| 生命周期、`ModContext`、日志、目录 | 1.0 |
| 玩家、实体、生物、方块、命令、基础玩家事件、自定义事件 | 1.1 |
| 主线程调度与异步等待、扩展实体/玩家属性、全部 LL 官方事件与同步订阅 | 1.2 |
| 表单（Simple / Modal / Custom）、LegacyMoney 经济接口 | 1.3 |
| 玩家快照（`PlayerRef::snapshot`）、轻量事件订阅（`subscribe_light`） | 1.4 |
| 实体与方块 NBT 快照（`nbt` / `block_entity_nbt`） | 1.5 |

## 入口与生命周期

```rust
pub struct MyMod {
    logger: Logger,
}

#[ferrum_sdk::mod_entry]
impl FerrumMod for MyMod {
    fn on_load(ctx: &ModContext) -> Result<Self> {
        Ok(Self { logger: ctx.logger() })
    }
}
```

| 阶段 | 签名 | 调用线程 | 失败行为 |
| :--- | :--- | :--- | :--- |
| `on_load` | `fn(ctx: &ModContext) -> Result<Self>` | 加载线程 | 加载失败，无实例 |
| `on_enable` | `fn(&mut self, ctx, token) -> Result<()>` | 主线程 | 保持停用，已注册资源撤销 |
| `on_disable` | `fn(&mut self, ctx, token) -> Result<()>` | 主线程 | 记录日志，继续停用与清理 |
| `on_unload` | `fn(self, ctx) -> Result<()>` | 任意线程 | 记录日志，继续卸载 |

- 除 `on_load` 外其余回调均有默认空实现。`FerrumMod` 要求 `Sized + Send + 'static`。
- 服务器运行期间执行 `ll mod load` 时，`on_enable` 会紧跟 `on_load` 调用。依赖世界或命令的操作请放在 `on_enable`。
- 每个 DLL 只能有一个 `#[mod_entry]`，类型不能带泛型。编译期必须保持 `panic = "unwind"`。
- 宏参数：`min_abi = "1.x"`（默认 `"1.0"`）；`hot_unload_safe`（预留标记）。

## ModContext 与日志

`ModContext` 为 `Copy`，可移入后台线程。模组卸载后其上的调用返回 `ErrorKind::StaleHandle`。

| 方法 | 说明 |
| :--- | :--- |
| `name()` / `version()` | `manifest.json` 中的 `name` / `version`（缺省 `"0.0.0"`） |
| `mod_dir()` / `data_dir()` / `config_dir()` / `lang_dir()` | `mods/<name>/` 及其子目录（或 `dir(ModDir::*)`） |
| `logger()` / `commands()` / `events()` / `custom_events()` / `scheduler()` | 各子系统入口 |

写入文件前请先用 `std::fs::create_dir_all` 确保目录存在。`Logger` 同样为 `Copy` 且线程安全，支持 `fatal / error / warn / info / debug / trace`、`enabled(Level)` 与 `log_fmt`。

## 主线程令牌

`MainThreadToken<'tick>` 是 `!Send + !Sync` 的零大小令牌，只能由 `on_enable`、`on_disable`、命令、主线程事件、表单或调度回调的形参获得，无法自行构造，也不能存入结构体或 `thread_local!`。所有世界、实体、方块、命令与经济 API 均要求传入它。

`ferrum_sdk::is_main_thread()` 可在任意线程检测当前是否为主线程。

## 错误与 Panic 隔离

`ferrum_sdk::Result<T>` 的错误类型为 `Error`，可通过 `kind()` 判断 `ErrorKind`（如 `NotFound`、`InvalidArgument`、`Unsupported`、`AlreadyExists`、`WrongThread`、`ReentrantCall`、`StaleHandle`、`ModDisabled`、`ServiceUnavailable`、`CallbackPoisoned`）。支持从 `&'static str`、`String`、`io::Error`、`Utf8Error`、`FromUtf8Error` 使用 `?` 自动转换。

回调中的普通 Rust `panic!` 会被 SDK 捕获并上报日志，不会使 BDS 崩溃：

- `on_enable` / `on_disable` panic 后实例被毒化，再次调用返回 `CallbackPoisoned`。
- 命令、事件或调度回调 panic 后，该回调会被毒化并被自动摘除。
- 短时间内（10 秒内超过 16 次）连续 panic 会触发自动停用保护。
- 无法拦截的情况：`panic = "abort"`、`Drop` 内二次 panic、栈溢出、OOM 或 `unsafe` 引发的未定义行为。

## 玩家、实体、方块与 NBT

`ActorRef<'t>`、`MobRef<'t>`、`PlayerRef<'t>` 与 `BlockRef<'t>` 均为 `!Send + !Sync` 瞬态引用，只在当次回调内有效。底层每次访问按 ID 重新解析；目标消失返回 `NotFound`。需要跨 Tick 或跨线程保留身份时，请保存 `PlayerUuid` 或 `ActorUniqueId`，回到主线程后再重新查找。

### 查找

| API | 返回 |
| :--- | :--- |
| `world::get_player(token, uuid)` | `Result<Option<PlayerRef>>` |
| `world::get_player_by_name(token, name)` | `Result<Option<PlayerRef>>` |
| `world::online_players(token)` | `Result<Vec<PlayerRef>>` |
| `world::get_actor(token, ActorUniqueId)` | `Result<Option<ActorRef>>`（不加载未加载实体） |
| `world::get_actor_by_runtime_id(token, id)` | 同上（`ActorRuntimeId` 仅限本次会话有效） |
| `world::get_block(token, dim, BlockPos)` | `Result<BlockRef>`（仅限已加载区块） |

### 实体与玩家层级

`ActorRef` -> `MobRef` -> `PlayerRef` 逐层包含上层能力：

| 类型 | Trait / 固有 | 方法 |
| :--- | :--- | :--- |
| `ActorRef` / `MobRef` / `PlayerRef` | `ActorOps` | `runtime_id`、`unique_id`、`type_name`、`kind`、`position`、`dimension`、`teleport`、`name_tag`、`set_name_tag`、`is_on_ground`、`is_in_water`、`nbt` |
| `MobRef` / `PlayerRef` | `MobOps` | `health`、`max_health`、`set_health`、`is_sprinting`、`set_sprinting` |
| `PlayerRef` | 固有方法 | `snapshot`、`real_name`、`uuid`、`send_message`、`is_operator`、`game_mode`、`set_game_mode`、`is_hungry`、`xp_earned_at_current_level`、`xp_needed_for_next_level`、`add_experience` |

- 向上转换：`as_mob()` / `as_actor()`；向下转换：`try_as_mob()` / `try_as_player()`（类型不符返回 `Ok(None)`）。
- 常用值类型：`DimensionId::{OVERWORLD, NETHER, THE_END}`、`GameMode::{SURVIVAL, CREATIVE, ADVENTURE, SPECTATOR, UNKNOWN}`、`ActorKind::{ACTOR, MOB, PLAYER}`。
- `set_health` 要求有限值且 `0.0 <= v <= max_health`，直接写入属性，不触发伤害/治疗事件。

### 方块

```rust
let block = world::get_block(token, DimensionId::OVERWORLD, BlockPos::new(0, 64, 0))?;
if !block.is_air()? {
    let name = block.type_name()?;
    block.set_type("minecraft:stone")?; // 放置默认状态并同步客户端，不产生掉落
}
```

只访问已存在维度的已加载区块，缺失维度/区块或未知方块名返回 `NotFound`，高度越界返回 `InvalidArgument`。`runtime_id()` 仅当次进程有效，持久化请保存方块类型名与坐标。

### NBT 快照

实体（`ActorOps::nbt()`）和方块主层（`BlockRef::nbt()`）返回模组拥有的 `ferrum_sdk::nbt::NbtCompound` 快照；方块实体存档用 `BlockRef::block_entity_nbt()` 读取（已加载位置无方块实体时返回 `Ok(None)`）。快照可在回调外保存和本地修改，可用 `ferrum_sdk::nbt::snbt::parse` 从 SNBT 构造。

```rust
if let Some(data) = block.block_entity_nbt()? {
    logger.info(data.to_snbt());
}
```

## 命令

在 `on_enable` 或主线程回调中注册，模组停用时自动注销：

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

- 参数类型：`INT`、`FLOAT`、`BOOL`、`STRING`（单个字符串参数）、`PLAYER`。必选参数必须放在可选参数之前。
- 读取参数：`int` / `float` / `bool` / `string` / `players` 返回 `Result<Option<T>>`。
- 来源与输出：`command.player()?`（非玩家来源为 `None`）、`command.success(text)` / `command.error(text)`。
- 权限：`ANY`、`GAME_DIRECTORS`、`ADMIN`、`HOST`、`OWNER`。同名命令可注册不同参数签名，签名冲突返回 `AlreadyExists`。
- 手动注销：`ctx.commands().unregister(token, handle)`。命令与事件回调均支持捕获 `Rc` 等非 `Send` 状态，且只在主线程调用与析构。

## 表单

`ferrum_sdk::form` 提供按钮菜单 `SimpleForm`、双按钮确认框 `ModalForm` 与自定义表单 `CustomForm`，支持一次回调 `send` 与异步 `send_async`：

```rust
use ferrum_sdk::form::{CustomForm, FormOutcome};

CustomForm::new("偏好设置")
    .input("name", "显示名称", "请输入名称", "")
    .toggle("enabled", "接收提醒", true)
    .send(ctx, player, |player, outcome, _token| {
        if let FormOutcome::Submitted(resp) = outcome {
            player.send_message(format!("你好，{}！", resp.text("name")?))?;
        }
        Ok(())
    })?;
```

- `SimpleForm` 返回按钮索引（`usize`）；`ModalForm` 返回 `ModalChoice::{First, Second}`；`CustomForm` 通过 `text`、`toggle`、`slider`、`choice` 按字段名读取结果。
- 玩家关闭或客户端忙时收到 `FormOutcome::Cancelled(reason)`。
- `send_async(ctx, player)?` 立即发送并返回 `Send` Future；等待期间玩家断线返回 `NotFound`，模组停用返回 `ModDisabled`。

## 经济（LegacyMoney）

`ferrum_sdk::llmoney` 对接可选插件 LegacyMoney（按玩家 XUID 操作，需主线程令牌）：

- `get(token, xuid)` / `set(token, xuid, amount)` / `add(token, xuid, amount)` / `reduce(token, xuid, amount)`
- `transfer(token, from, to, amount, note)`
- `history(token, xuid, seconds)` / `clear_history(token, seconds)` / `ranking(token, limit)`

服务端未安装或已停用 LegacyMoney 时返回 `ErrorKind::ServiceUnavailable`。

## 主线程调度与异步

通过 `ctx.scheduler()` 投递主线程任务：

| 方法 | 提交线程 | 说明 |
| :--- | :--- | :--- |
| `post(f)` | 任意（需 `Send`） | 下一 Tick 执行一次 |
| `after_ticks(token, ticks, f)` | 主线程 | 延迟指定 Tick（`0` 为下一 Tick），可捕获非 `Send` 状态 |
| `every_ticks(token, interval, f)` | 主线程 | 每隔 `interval` Tick 执行（`>= 1`）；返回 `Err` 或 panic 时停止 |
| `cancel(token, handle)` | 主线程 | 取消本模组的任务 |
| `on_main_thread(f).await` | 任意异步任务 | 在主线程执行闭包并传回 `Send` 结果；停用时返回 `ModDisabled` |

```rust
async fn online_count(ctx: ModContext) -> Result<usize> {
    ctx.scheduler()
        .on_main_thread(|token| Ok(world::online_players(token)?.len()))
        .await
}
```

不要在当前主线程 Tick 内同步阻塞等待 `on_main_thread`，否则会造成死锁。若模组自建 `std::thread` 或 Tokio 运行时，请在 `on_disable` 发送停止信号，并在 `on_unload` 返回前完成 `join` 或 `shutdown_timeout`。

## 扩展阅读

- [events.md](./events.md)：LL 官方事件与跨语言自定义事件
- [testing.md](./testing.md)：`MockHost` 单元测试与 BDS 联调
