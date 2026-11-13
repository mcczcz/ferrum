# Ferrum Rust SDK 开发手册（`ferrum-sdk` 参考）

本文档面向使用 Rust 编写 `type = "ferrum"` 模组的开发者，介绍当前已实现的全部 `ferrum-sdk` / `ferrum-macros` 接口、惯用写法与安全约束。

> 当前对应 **ABI `1.0.0`（M1）**。事件、命令、玩家、异步调度与表单等高阶领域 API 的未来设计见 [核心扩展 API 规划](./core-apis-guide.md)。

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
