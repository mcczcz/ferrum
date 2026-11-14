# Ferrum Rust SDK 开发手册（`ferrum-sdk` 参考）

本文档面向使用 Rust 编写 `type = "ferrum"` 模组的开发者，介绍当前已实现的全部 `ferrum-sdk` / `ferrum-macros` 接口、惯用写法与安全约束。

> 当前开发版对应 **ABI `1.1.0`**，已提供命令注册、Actor/Mob/Player 分层接口和基础方块读写。仅使用生命周期与日志的模组仍默认要求 ABI 1.0；事件、异步调度与表单仍在规划中。

## 命令与玩家 API（ABI 1.1）

在 `on_enable` 中通过 `ctx.commands()` 构建命令，`execute(token, callback)` 完成注册并返回 `CommandHandle`。相同命令名可注册不同参数签名的重载；重复签名返回 `AlreadyExists`，其他模组或原生命令的名称不能接管。

```rust
ctx.commands()
    .register("greet", "向玩家发送问候", CommandPermission::ANY)
    .required("target", CommandParamKind::PLAYER)
    .optional("message", CommandParamKind::STRING)
    .execute(token, |command, token| {
        let message = command.string("message")?.unwrap_or_else(|| "你好！".into());
        for player in command.players("target")?.unwrap_or_default() {
            player.send_message(&message)?;
        }
        command.success(format!("当前在线 {} 人", world::online_players(token)?.len()))?;
        Ok(())
    })?;
```

- 参数类型：`INT` / `FLOAT` / `BOOL` / `STRING` / `PLAYER`，支持必选和可选；必选参数必须排在可选参数前。`STRING` 是单个命令字符串参数，不是剩余整行 RawText。
- 读取参数：`int` / `float` / `bool` / `string` / `players`，返回 `Result<Option<T>>`；未提供可选参数为 `None`，未知参数名或类型不符为 `InvalidArgument`。玩家选择器每次读取时重新解析。
- 执行来源：`command.player()?`，控制台及非玩家来源返回 `None`。输出通过 `success` / `error`。
- 权限：`ANY` / `GAME_DIRECTORS` / `ADMIN` / `HOST` / `OWNER`，分别对应 LL 权限等级 0–4。
- 注销：`ctx.commands().unregister(token, handle)?`；模组停用、启用失败、卸载时自动撤销并释放回调。手动注销会关掉该重载的执行路由，命令语法可能仍保留在 LL 注册表，直到 LL 停用模组。
- 回调允许捕获 `Rc` 等非 `Send` 状态，仅在主线程调用及析构；同一 `FnMut` 重入返回 `ReentrantCall`。panic 后回调被毒化并从路由移除。
- 在自身命令回调里触发模组停用时，先拒绝后续派发，待最外层回调返回后执行 `on_disable` 和资源排空；同步卸载自身返回 `ReentrantCall`。

玩家使用瞬态 `PlayerRef<'tick>`，不可跨线程或跨 Tick 保存：

| API | 返回值与用途 |
| --- | --- |
| `world::get_player(token, uuid)` | `Result<Option<PlayerRef>>`，按 UUID 查找 |
| `world::get_player_by_name(token, name)` | `Result<Option<PlayerRef>>`，按名称查找 |
| `world::online_players(token)` | `Result<Vec<PlayerRef>>`，在线玩家快照 |
| `player.real_name()` | `Result<String>`，真实名称 |
| `player.uuid()` | `Result<PlayerUuid>`，跨重连保存的 UUID（标准网络字节序） |
| `player.runtime_id()` | `ActorRuntimeId`，当前会话标识 |
| `player.send_message(text)` | `Result<()>`，发送消息 |
| `player.is_operator()` | `Result<bool>`，OP 判断 |

每次玩家调用都在 shim 内按 runtime ID 重新解析，玩家离线后返回 `NotFound`。若模组必须依赖这些能力，可声明 `#[ferrum_sdk::mod_entry(min_abi = "1.1")]`；默认最低版本仍为 1.0，旧宿主调用新接口返回 `Unsupported`。

`ferrum-host-mock` 提供 `MockPlayer`、`add_player` / `remove_player` / `player_messages`，以及接收已解析参数 `MockCommandValue` 的 `execute_command`；它不模拟 Bedrock 命令文本解析和选择器语法。

### 玩家状态与传送（ABI 1.1）

先导入 `use ferrum_sdk::prelude::*;`，其中已包含 `ActorOps` 和 `MobOps`。位置、维度和传送由 `ActorOps` 提供，生命值由 `MobOps` 提供；`PlayerRef` 实现这两个 Trait，因此可直接调用下表方法。游戏模式仍是 `PlayerRef` 的专属固有方法。若按类型单独导入 SDK，也需将相应 Trait 导入作用域。

| API | 返回值与用途 |
| --- | --- |
| `player.position()` | `Result<Vec3>`，世界坐标，`x/y/z` 为 `f32`；返回 Actor 位置，不保证等于脚底方块坐标 |
| `player.dimension()` | `Result<DimensionId>`，当前维度，保留自定义维度 ID |
| `player.teleport(Vec3::new(x, y, z), dimension)` | `Result<()>`，跨维度传送并保留朝向 |
| `player.health()` | `Result<f32>`，当前生命值，不截断小数 |
| `player.max_health()` | `Result<f32>`，当前有效最大生命值 |
| `player.set_health(value)` | `Result<()>`，直接设置当前生命属性，不模拟受伤、治疗事件 |
| `player.game_mode()` | `Result<GameMode>`，读取游戏模式 |
| `player.set_game_mode(mode)` | `Result<()>`，设置生存、创造、冒险或旁观 |

`DimensionId::OVERWORLD` / `NETHER` / `THE_END` 对应 0/1/2，自定义维度可使用 `DimensionId(id)`。传送要求坐标有限（拒绝 NaN/无穷），目标维度已存在；目标不存在返回 `NotFound`。成功表示已提交底层传送，不表示客户端已完成加载。

生命值单位为点，2 点为 1 颗心。设置值必须有限且满足 `0 <= value <= max_health`，越界返回 `InvalidArgument`；不自动截断或修改最大生命值。设置 0 直接写入属性，不等同于带伤害来源的击杀操作。

游戏模式使用 `GameMode::SURVIVAL` / `CREATIVE` / `ADVENTURE` / `SPECTATOR`；底层遇到其他模式返回 `GameMode::UNKNOWN`，设置未知值返回 `InvalidArgument`。SDK 不暴露当前 MC 的枚举数值。

```rust
// 在已有的主线程命令回调内：
if let Some(player) = command.player()? {
    let position = player.position()?;
    let dimension = player.dimension()?;
    let health = player.health()?;
    let max_health = player.max_health()?;
    let game_mode = player.game_mode()?;
    command.success(format!("坐标 {position:?}，维度 {dimension:?}，生命 {health}/{max_health}，模式 {game_mode:?}"))?;
}
```

玩家专属方法通过 ABI 1.1 的 `PlayerApiV1`（`ferrum.player.v1`）调用，`ActorOps` 默认方法走 `ActorApiV1`，`MobOps` 默认方法走 `MobApiV1`。导入 Trait 后，`player.position()` 等调用方式继续可用，由 Trait 统一提供实现，`PlayerRef` 无需逐个编写父层方法的转发；玩家 ABI 里已有的公共槽位仍转发到同一底层实现。SDK 在使用前检查完整子表大小；硬依赖时声明 `#[ferrum_sdk::mod_entry(min_abi = "1.1")]`。当前仍处开发阶段，宿主和模组需使用同一开发快照同步重新编译。

mock 可通过 `MockPlayerState`、`player_state(uuid)` / `set_player_state(uuid, state)` 配置和查看这些属性；默认生命 20/20，生存模式，主世界原点。内置三种原版维度，可用 `add_dimension(DimensionId(id))` 注册自定义维度。

### Actor、Mob 与 Player 的继承语义

游戏中的实体基础类就是 Actor，SDK 使用组合与密封 Trait（Sealed Trait）表达 `Actor → Mob → Player` 的能力关系：`MobRef` 内含 `ActorRef`，`PlayerRef` 内含 `MobRef`。公共实体能力由 `ActorOps<'tick>` 的默认方法提供，生物能力由继承 `ActorOps<'tick>` 的 `MobOps<'tick>` 提供；两个 Trait 都已包含在 `prelude` 中。私有的 `sealed::Sealed` 超 Trait 限制下游实现，模组无法自行实现这些 Trait 来伪造实体引用类型。C ABI 继续使用三个独立的平坦函数表，不传递 C++ 对象或复制其继承布局。

| 引用 | 能力 Trait 与专属方法 | 与其他层的关系 |
| --- | --- | --- |
| `ActorRef<'tick>` | `ActorOps`：runtime ID、unique ID、类型名、能力类别、位置、维度、传送 | `as_actor()` 返回自身；`try_as_mob()` / `try_as_player()` 返回 `Result<Option<...>>` |
| `MobRef<'tick>` | `ActorOps` + `MobOps`：所有公共实体方法，以及当前/最大生命值、设置生命值 | 可直接调用 `position()` 等父层方法；`as_actor()` 向上转换，`try_as_player()` 向下转换 |
| `PlayerRef<'tick>` | `ActorOps` + `MobOps`；专属名称、UUID、消息、OP、游戏模式方法 | 可直接调用全部公共实体与生物方法；`as_mob()` / `as_actor()` 向上转换 |

`MobRef` / `PlayerRef` 通过实现能力 Trait 复用默认方法，无需手写重复转发，也无需先向上转换才能调用父层方法。向上转换只复制 ID；`ActorRef::try_as_mob()`、`ActorRef::try_as_player()` 和 `MobRef::try_as_player()` 向下转换时先在主线程按 ID 解析并验证类别，类别不匹配返回 `Ok(None)`，宿主错误保留为 `Err`。所有引用保持 `!Send + !Sync` 且受本次令牌生命周期限制，后续每次操作仍会重新解析和检查目标，离线/销毁得到 `NotFound`。未知 `ActorKind` 保留原始数值，SDK 不把未知类别当作 Mob/Player。

`world::get_actor(token, ActorUniqueId)` 按长期逻辑 ID 重新查找当前已存在的实体；`world::get_actor_by_runtime_id(token, ActorRuntimeId)` 仅用于当前会话。它们都返回 `Result<Option<ActorRef>>`，不会加载未加载实体。

```rust
use ferrum_sdk::prelude::*;

// ActorRef、MobRef、PlayerRef 都可传入。
fn describe<'tick>(entity: impl ActorOps<'tick>) -> Result<String> {
    Ok(format!("{}: {:?}", entity.type_name()?, entity.position()?))
}

// MobRef、PlayerRef 都可传入；同时可使用 ActorOps 的方法。
fn describe_mob<'tick>(entity: impl MobOps<'tick>) -> Result<String> {
    Ok(format!("{}，生命 {}/{}", describe(entity)?, entity.health()?, entity.max_health()?))
}

// 在已有的主线程命令回调内：
if let Some(player) = command.player()? {
    command.success(describe(player)?)?;
    command.success(describe_mob(player)?)?;
    let mob = player.as_mob();
    if let Some(player_again) = mob.try_as_player()? {
        player_again.send_message("已验证玩家能力")?;
    }
    let saved_id = player.unique_id()?;
    // saved_id 可保存；之后持新 token 调 world::get_actor(token, saved_id) 重新解析。
    let _ = saved_id;
}
```

更具体的生物或实体（如僵尸、掉落物、载具）后续按需求增加专属 Ref 与独立子表。专属 Ref 组合对应父层并提供显式转换；只有确实需要该子类的稳定能力才添加新接口。当前没有自动导出 MC 全部类、反射任意成员或通用强制转换。

mock 中 `MockActor::new(type_name, ActorKind::ACTOR/MOB)` 配合 `add_actor` / `remove_actor` / `actor_state` 可模拟非玩家实体；`ActorKind::PLAYER` 通过原有 `add_player` 创建。mock 的 unique ID 为 `ActorUniqueId(runtime_id.0 as i64)`，仅模拟本次 MockHost 生命周期内的重新解析。

### 方块基础读写

```rust
let block = world::get_block(token, DimensionId::OVERWORLD, BlockPos::new(0, 64, 0))?;
let name = block.type_name()?;
let runtime_id = block.runtime_id()?;
let is_air = block.is_air()?;
block.set_type("minecraft:stone")?;
// 同一个 BlockRef 代表该坐标，后续查询观察新放置的石头。
block.set_air()?;
let _ = (name, runtime_id, is_air);
```

`BlockRef<'tick>` 仅保存维度 ID 和整数 `BlockPos`，实际操作每次在 shim 中重新查找，不保存 `Block*` / `BlockSource*`。读取返回真实类型名、当前注册表 runtime ID、是否空气。runtime ID 不应跨重启/跨版本保存；持久记录用类型名和坐标。

只访问主方块层、已存在维度和已加载区块，缺失维度/区块或未知方块名返回 `NotFound`，高度超出该维度范围返回 `InvalidArgument`。查询和设置均不强制加载区块；普通空气不等同于区块未加载。

`set_type` 放置类型的默认状态，并触发邻居更新与客户端同步；`set_air` 等价于设置 `minecraft:air`。这是世界直接修改，不模拟玩家放置/挖掘，不生成破坏掉落，可能触发游戏内部连锁更新。方块状态自定义、NBT、容器、液体额外层、自然破坏及掉落不在本批接口范围。

mock 默认注册 `minecraft:air` / `minecraft:stone`，没有已加载区块。先用 `set_chunk_loaded(dim, chunk_x, chunk_z, true)` 标记可访问区块，再用 `set_block(dim, pos, name)` 设置；其他类型用 `register_block_type(name, BlockRuntimeId(id))` 注册。已加载区块内未设置的位置视为空气。mock 的高度范围为主世界 `[-64, 320)`、下界 `[0, 128)`、末地/自定义维度 `[0, 256)`，不模拟邻居更新、掉落或客户端网络同步。

---

## 1. 引入与 Prelude

在模组 `Cargo.toml` 中将 crate 类型设为 `["cdylib"]`（若需要写单元测试或让其他 crate 引用内部模块，可写 `["cdylib", "rlib"]`），并确保保持 `panic = "unwind"`：

```toml
[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
ferrum-sdk = "0.1"

[dev-dependencies]
ferrum-host-mock = "0.1"

[profile.release]
panic = "unwind" # ferrum-sdk 会在 panic = "abort" 时拒绝编译
```

在代码中使用 `prelude` 导入常用类型：

```rust
use ferrum_sdk::prelude::*;
// 包含：Error, ErrorKind, FerrumMod, Level, Logger, MainThreadToken, ModContext, ModDir, Result
```

---

## 2. 模组入口与生命周期（`FerrumMod` & `#[mod_entry]`）

### 2.1 `#[ferrum_sdk::mod_entry]` 属性宏

将 `#[ferrum_sdk::mod_entry]` 标注在 `impl FerrumMod for YourMod { ... }` 上：

```rust
pub struct MyMod {
    logger: Logger,
}

#[ferrum_sdk::mod_entry]
impl FerrumMod for MyMod {
    fn on_load(ctx: &ModContext) -> Result<Self> {
        let logger = ctx.logger();
        Ok(Self { logger })
    }
}
```

宏展开会自动完成以下工作：
1. 导出 C 入口符号 `ferrum_mod_entry_v1`；
2. 安装 panic hook，并在 `on_load` / `on_enable` / `on_disable` / `on_unload` 外层包裹 `catch_unwind` 防火墙与重入守卫；
3. 为 `MyMod` 实现 `ModEntryPoint` trait（供 `ferrum-host-mock` 单元测试直接加载）。

#### 可选宏参数
- `#[ferrum_sdk::mod_entry(min_abi = "1.0")]`：声明模组所需的最低宿主 ABI 版本（格式 `"主版本.次版本"`）。只有当模组硬依赖更高 minor 才引入的新接口、且希望在加载时立即失败而不是运行时降级时才需调高；默认值为 `"1.0"`。
- `#[ferrum_sdk::mod_entry(hot_unload_safe)]`：向宿主声明本模组及其依赖在 `on_unload` 返回后不留任何活跃后台线程、`thread_local!` 析构或系统回调。**注意**：当前版本（M1）为保证安全仍默认保留镜像，真正 `FreeLibrary` 路径将在 M5 启用。

#### 约束与限制
- **每个 `.dll` 只能有一个 `#[mod_entry]`**：因为宏会导出固定符号 `ferrum_mod_entry_v1`。同一 crate 的单元测试若需要测试多个不同的 `FerrumMod` 类型，请放在各自独立的集成测试文件 `tests/*.rs` 中。
- **不支持泛型模组类型**：`MyMod` 不能带泛型参数。
- **`Send + 'static` 约束**：`FerrumMod` 要求 `Sized + Send + 'static`，因为 `on_load`（进程启动线程）与 `on_enable`/`on_disable`（BDS 服务端逻辑线程）可能在不同线程上执行。

### 2.2 四阶段生命周期职责

| 阶段 | 签名 | 调用线程 | 适用工作 | 返回 `Err` / Panic 行为 |
| :--- | :--- | :--- | :--- | :--- |
| **1. `on_load`** | `fn on_load(ctx: &ModContext) -> Result<Self>` | 进程主线程（或运行期热加载线程） | 读取 `config_dir()` 配置、初始化内存状态、校验前置资源 | 加载失败，实例未创建 |
| **2. `on_enable`** | `fn on_enable(&mut self, ctx: &ModContext, token: &MainThreadToken<'_>) -> Result<()>` | BDS 服务端主线程 | 注册世界相关逻辑、启动任务（此时世界/服务已就绪） | 模组进入 `Disabled`，实例保留待后续 `on_unload` |
| **3. `on_disable`** | `fn on_disable(&mut self, ctx: &ModContext, token: &MainThreadToken<'_>) -> Result<()>` | BDS 服务端主线程 | 向自建线程/异步运行时发送停止信号、刷新缓冲数据 | 仍继续进入 `Disabled` 并记录错误日志 |
| **4. `on_unload`** | `fn on_unload(self, ctx: &ModContext) -> Result<()>` | 任意线程（通常主线程） | **必须在此 `join()` 自建 OS 线程、`shutdown` 异步运行时**并释放外部句柄 | 句柄失效；错误记入日志 |

> **热加载串联提示**：当服务器已经在运行中、服主通过 `ll mod load <name>` 动态加载模组时，LeviLamina 会在 `on_load` 成功后立即自动串联调用 `on_enable`。因此不要假设 `on_load` 和 `on_enable` 之间一定隔着漫长等待，也不要把依赖主线程世界的操作放进 `on_load`。

---

## 3. 模组上下文与目录（`ModContext` & `ModDir`）

`ModContext` 是轻量句柄包装（实现了 `Copy + Clone`），可安全跨线程传递并在后台线程使用。模组卸载后，通过旧 `ModContext` 发起的宿主查询会返回 `Err`（`ErrorKind::StaleHandle`）。

```rust
fn init_paths(ctx: &ModContext) -> Result<()> {
    let name: String = ctx.name()?;       // manifest.json 中的 name
    let version: String = ctx.version()?; // manifest.json 中的 version（缺省为 "0.0.0"）

    let mod_dir = ctx.mod_dir()?;         // mods/<name>/
    let data_dir = ctx.data_dir()?;       // mods/<name>/data/
    let config_dir = ctx.config_dir()?;   // mods/<name>/config/
    let lang_dir = ctx.lang_dir()?;       // mods/<name>/lang/
    // 等价于 ctx.dir(ModDir::Config) 等

    std::fs::create_dir_all(&config_dir)?; // std::io::Error 可直接通过 `?` 转为 ferrum_sdk::Error
    ctx.logger().info(format!("{name} v{version} 数据目录: {}", data_dir.display()));
    Ok(())
}
```

其他方法：
- `ctx.raw_handle() -> ModContextHandle`：供需要与 `ferrum-abi` 直接交互的扩展 crate 使用。

---

## 4. 日志系统（`Logger` & `Level`）

`ctx.logger()` 返回当前模组专属的 `Logger`（`Copy + Clone`，任意线程安全），底层直接写入 LeviLamina 为该模组分配的 `ll::io::Logger`，自动带模组名称前缀与控制台/文件输出策略。

```rust
let logger = ctx.logger();

logger.fatal("严重故障");
logger.error("错误详情");
logger.warn("警告");
logger.info("常规信息");
logger.debug("调试信息");
logger.trace("跟踪信息");

// 昂贵格式化前先判定日志级别，避免无效字符串分配：
if logger.enabled(Level::Debug) {
    logger.debug(format!("当前内部状态: {:?}", expensive_state));
}

// 或使用 log_fmt（内部自动检查 enabled 并在字面量时零分配）：
logger.log_fmt(Level::Debug, format_args!("tick = {tick}"));
```

模组卸载后，残留线程继续调用 `logger` 会被宿主安全静默丢弃，不会崩溃。

---

## 5. 主线程令牌（`MainThreadToken` & `is_main_thread`）

BDS 游戏世界状态不是线程安全的。Ferrum 用零大小的 `MainThreadToken<'tick>` 在编译期隔离主线程与后台线程：

- `MainThreadToken<'tick>` 是 **`!Send + !Sync`**，且没有公开构造函数，只能以不可变引用 `&MainThreadToken<'_>` 的形式从 `on_enable` / `on_disable`（以及后续事件/命令/调度回调）中获得。
- **不要尝试把 `MainThreadToken` 存进结构体或 `thread_local!`**——Rust 的生命周期参数 `'tick` 会阻止它逃逸出当前调用栈。
- 如果在后台工作线程或通用工具函数中需要判断当前是否处在 BDS 服务端主线程，可调用：

```rust
if ferrum_sdk::is_main_thread() {
    // 当前位于 BDS 服务端主线程
}
```

---

## 6. 错误处理与 Panic 隔离（`Error`、`ErrorKind`、`Result`）

`ferrum_sdk::Result<T>` 默认错误类型为 `ferrum_sdk::Error`。

### 6.1 构造与转换错误

```rust
use ferrum_sdk::prelude::*;

fn load_config(ctx: &ModContext) -> Result<String> {
    let path = ctx.config_dir()?.join("settings.toml");
    if !path.exists() {
        // 自定义一般性错误（FerrumStatus::FAILED），详情会传回宿主日志
        return Err(Error::msg("缺少 settings.toml"));
    }
    // std::io::Error 会按 kind 自动映射为 NotFound / PermissionDenied / AlreadyExists 等
    let content = std::fs::read_to_string(&path)?;
    Ok(content)
}
```

`Error` 已实现 `From<&'static str>`、`From<String>`、`From<std::io::Error>`、`From<std::str::Utf8Error>`、`From<std::string::FromUtf8Error>`，同时实现了 `std::error::Error`。需要指定底层状态码时可用 `Error::new(status, message)`。

### 6.2 检查错误种类

不要直接匹配底层整数，使用 `err.kind()`（`#[non_exhaustive]`）：

```rust
match ctx.config_dir() {
    Ok(dir) => { /* ... */ }
    Err(err) => match err.kind() {
        ErrorKind::StaleHandle => { /* 模组已卸载 */ }
        ErrorKind::WrongThread => { /* 违反主线程约束 */ }
        ErrorKind::NotFound => { /* 资源不存在 */ }
        _ => ctx.logger().error(format!("操作失败: {err}")),
    }
}
```

### 6.3 Panic 防火墙与回调毒化机制

- 生命周期回调中发生普通 `panic!` 时，SDK 跳板会捕获展开、通过 `report_panic` 将消息与源码位置上报给宿主打印，并返回 `MOD_PANICKED`，**不会导致 BDS 进程崩溃**。
- **回调毒化**：若在 `on_load` panic，加载失败；若在 `on_enable` panic，实例锁中毒，模组进入 `Disabled`，后续再次尝试 `enable` 会直接返回 `ErrorKind::CallbackPoisoned`（直到卸载/重载），防止在受损状态上继续执行。
- **熔断保护**：若模组短时间内（默认 10 秒内超过 16 次）频繁触发 panic，宿主会打印 `FATAL` 日志并自动调度停用该模组。
- **重要边界**：`catch_unwind` **无法**捕获 `panic = "abort"`、Drop 中二次 panic（double panic）、栈溢出、内存耗尽（OOM abort）或模组自身 `unsafe` 导致的未定义行为。

---

## 7. 自建后台线程与异步运行时的注意事项

在 M3 `SchedulerApiV1` 落地之前，若你的模组在 `on_enable` 中自行启动了后台工作线程（如 `std::thread::spawn`）或独立的 `tokio::runtime::Runtime`：

1. **必须保存关闭句柄**：例如 `AtomicBool` 停止标志、`tokio_util::sync::CancellationToken` 或 `JoinHandle<()>`（注意 `ModContext` 和 `Logger` 都是 `Copy + Send + Sync`，可直接移入后台线程）。
2. **`on_disable` 通知停止**：在 `on_disable` 中触发取消信号，停止接受新任务。
3. **`on_unload` 必须等待退出**：在 `on_unload(self, _ctx)` 中完成 `runtime.shutdown_timeout(...)` 或 `handle.join()`。不要让脱离管控的后台线程在模组卸载后无限期存活。
4. **不要将其他模组的堆指针/Rust 对象跨模组传递**：每个 Ferrum 模组都是独立的 `cdylib` 与独立堆，只能通过 `std::fs` / 网络 / 约定文件或未来的标准服务总线交互。
5. **配置读取建议**：使用 `serde` + `toml`/`serde_json` 等纯 Rust crate 解析 `ctx.config_dir()` 下的配置，不要把模组配置写进 `ferrum-loader` 目录。
