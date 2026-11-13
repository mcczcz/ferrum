# 三层隔离架构与生命周期映射

## 1. 全局拓扑与物理产物边界

Ferrum 在物理产物上严格切分为**宿主加载器（`ferrum-loader.dll`）**与**下游 Rust 模组（`<mod_name>.dll`）**两大独立编译单元，中间仅通过纯 C 内存布局的 `ferrum-abi` 通信：

```text
+-----------------------------------------------------------------------------------+
| 进程空间: bedrock_server_mod.exe + LeviLamina.dll                                  |
|                                                                                   |
|  +-----------------------------------------------------------------------------+  |
|  | 一级 Native 插件: ferrum-loader.dll (type = "native", preload/loadBefore)    |  |
|  |                                                                             |  |
|  |  +-----------------------------------+   extern "C"   +------------------+  |  |
|  |  | Layer 1: cpp-shim (C++20)         | <============> | Layer 2:         |  |  |
|  |  | - 唯一包含 ll/api/* 与 mc/* 头文件 |   内部静态链接  | ferrum-host      |  |  |
|  |  | - 继承 ll::mod::ModManager        |                | (Rust staticlib) |  |  |
|  |  | - 对接 EventBus / CommandRegistrar|                | - 代数句柄池     |  |  |
|  |  | - 对接 ServerThreadExecutor       |                | - VTable 组装分发|  |  |
|  |  | - C++ try/catch <-> 状态码转换    |                | - ModOwnership   |  |  |
|  |  +-----------------------------------+                +--------+---------+  |  |
|  +----------------------------------------------------------------|------------+  |
|                                                                   |               |
|                     纯 C ABI 边界 (ferrum-abi: #[repr(C)] VTable + 不透明句柄)      |
|                     无符号导入链接，加载时通过入口函数单点注入 FerrumHostApi*         |
|                                                                   |               |
|  +----------------------------------------------------------------v------------+  |
|  | 二级 Ferrum 模组: my_rust_mod.dll (manifest.json: type = "ferrum")          |  |
|  |                                                                             |  |
|  |  +-----------------------------------------------------------------------+  |  |
|  |  | Layer 3: ferrum-sdk + ferrum-macros                                   |  |  |
|  |  | - #[ferrum_sdk::mod_entry] 展开导出 ferrum_mod_entry_v1               |  |  |
|  |  | - catch_unwind 异常防火墙                                             |  |  |
|  |  | - MainThreadToken<'tick> / 安全 RAII 包装 / async 调度器封装           |  |  |
|  |  +-----------------------------------------------------------------------+  |  |
|  +-----------------------------------------------------------------------------+  |
+-----------------------------------------------------------------------------------+
```

### 各层职责硬边界

| 层级 / Crate | 语言与产物类型 | 允许的依赖 | 核心职责与禁令 |
| :--- | :--- | :--- | :--- |
| **`cpp-shim`** | C++20 源码（由 xmake 编译并链接进 `ferrum-loader.dll`） | `LeviLamina`、`src/mc`、C++ 标准库 | **唯一允许 `#include "ll/..."` 和 `"mc/..."` 的模块**。负责将 C++ 类方法、模板、虚函数与异常翻译为平坦的 `extern "C"` 垫片函数；导出 `ll_memory_operator_overrided` 与 `LL_REGISTER_MOD`。严禁包含复杂业务状态。 |
| **`ferrum-host`** | Rust `staticlib`（静态链接进 `ferrum-loader.dll`） | `ferrum-abi`、`libloading` 等纯 Rust 库 | 管理二级 `.dll` 加载（影子复制 + `libloading`）、ABI 版本协商、全局 VTable 实例构造、代数槽位句柄池（Generational Arena）、模组资源所有权表（`ModOwnership`，M2）与任务调度队列（M3）。严禁直接绑定 C++ 符号。 |
| **`ferrum-abi`** | Rust `rlib`（`#![no_std]` 兼容设计） | **零外部依赖** | 仅定义 `#[repr(C)]` 结构体、固定底层类型枚举、函数指针类型与常量。宿主与 SDK 共同依赖此 Crate。 |
| **`ferrum-sdk`** | Rust `rlib` | `ferrum-abi`、`ferrum-macros` | 面向模组开发者的安全高级 API。将底层句柄与 VTable 调用封装为符合 Rust 人体工学的类型、Trait 与 Future。 |
| **`ferrum-macros`** | Rust `proc-macro` | `syn`、`quote` | 生成 `ferrum_mod_entry_v1` C 导出符号、自动注入 ABI 握手校验与 `catch_unwind` 包装代码。 |
| **`ferrum-host-mock`** | Rust `rlib`（仅 `dev-dependencies`） | `ferrum-abi` | 纯 Rust 实现的假宿主 VTable，让下游模组与 `ferrum-sdk` 在不启动 BDS 的情况下 `cargo test`。 |
| **`xtask`** | Rust 二进制（不发布） | `cbindgen`、`ferrum-abi` | 生成 `cpp-shim/include/ferrum/ferrum_abi.h` 与 `layout_lock.rs` 追加断言，CI 校验生成物未过期。 |

### 依赖方向（单向，禁止反向）

```text
ferrum-abi  <──  ferrum-host  <──(extern "C")──  cpp-shim  ──>  LeviLamina / mc
    ^
    └────────  ferrum-sdk (+ ferrum-macros)  <──  下游模组
```

- 下游模组与 `ferrum-sdk` 永远不依赖 `ferrum-host` 或 `cpp-shim`；它们之间唯一的接触面是运行时传入的 `*const FerrumHostApi`。
- `ferrum-host` 不依赖 `ferrum-sdk`。宿主侧与模组侧各自从 `ferrum-abi` 出发实现自己的安全封装。

---

## 2. 对接 LeviLamina 模组管理器的精确机制

LeviLamina 原生设计了多语言/二级加载器扩展点 `ll::mod::ModManager` 与 `ll::mod::ModManagerRegistry`（参见 `ll/api/mod/ModManagerRegistry.h` 与 `ll/core/mod/ModRegistrar.cpp`）。Ferrum 完全遵循该官方机制接入，不做任何私有 Hack：

### 2.1 加载顺序保障（Bootstrapping）
在 `ModRegistrar::loadAllMods()` 执行期间：
1. LeviLamina 首先注册 `NativeModManager`（`type = "native"`），随后一次性扫描 `mods/` 下所有子目录的 `manifest.json`，并通过 `ModLoadPlanner::resolve` 构建全服统一的拓扑加载计划（跨 `type` 统一解析 `dependencies`、`optionalDependencies`、`conflicts`、`loadBefore`）。
2. **`ferrum-loader` 自身的 `manifest.json`**：
   - `name`: `"ferrum-loader"`
   - `type`: `"native"`
   - `entry`: `"ferrum-loader.dll"`
3. **所有下游 Rust 模组的 `manifest.json`**：
   - `type`: `"ferrum"`
   - `entry`: `"<mod_name>.dll"`
   - `dependencies`: **必须显式声明** `{ "name": "ferrum-loader", "version": "^0.1.0" }`。
4. 因为下游模组依赖 `ferrum-loader`，`ModLoadPlanner` 拓扑排序**严格保证** `ferrum-loader` 先于所有 `type: "ferrum"` 模组执行 `load()`。
5. 在 `ferrum-loader` 的 `ll_mod_load` 阶段，`cpp-shim` 实例化 `FerrumModManager : public ll::mod::ModManager("ferrum")`，调用 `ModManagerRegistry::getInstance().addManager(...)` 完成注册。紧接着当 `ModRegistrar` 遍历到后续 `type: "ferrum"` 模组时，`ModManagerRegistry::loadMod` 即可识别 `"ferrum"` 类型并无缝委派给 `FerrumModManager::load(Manifest)`。

### 2.2 复用 LeviLamina 原生设施的收益
由于每个 Rust 模组在 `FerrumModManager` 内都对应一个真实的 `std::shared_ptr<ll::mod::Mod>`（或其派生类 `FerrumMod`）实例并注册进 `ModManager`:
- **命令管理全兼容**：控制台 `ll list`、`ll mod load <name>`、`ll mod enable <name>`、`ll mod disable <name>`、`ll mod unload <name>`、`ll mod reload <name>` 对 Rust 模组直接生效，级联依赖检查（`cascade`）由 `ModRegistrar` 自动处理。
- **目录与资源包自动挂载**：`Mod::getModDir()`、`getDataDir()`、`getConfigDir()`、`getLangDir()` 直接可用；且 LeviLamina 在 `InitPacksHook` 中遍历 `ModManagerRegistry::getInstance().mods()`，Rust 模组目录下的 `resource_packs/` 与 `behavior_packs/` 会自动挂载到 BDS 世界中。
- **独立具名日志器**：每个 Rust 模组天然获得独立的 `ll::io::Logger&`（`Mod::getLogger()`），日志前缀、控制台着色与文件轮转策略与原生 C++ 模组完全一致。
- **所有权弱引用锚点**：向 LeviLamina 的 `EventBus::addListener`、`CommandHandle::runtimeOverload` 注册资源时，显式传入该二级模组对应的 `std::weak_ptr<ll::mod::Mod>`（而非默认的 `NativeMod::current()`，后者会错误指向 `ferrum-loader` 自身！），从而让 LeviLamina 内置的按模组禁用/清理逻辑与 Ferrum 的 `ModOwnership` 形成双重保险。

> **架构红线**：在 `cpp-shim` 调用任何带有默认参数 `std::weak_ptr<mod::Mod> mod = mod::NativeMod::current()` 的 LeviLamina API 时，**必须显式传入目标二级 `FerrumMod` 的 `shared_ptr`/`weak_ptr`**，严禁省略该参数！

---

## 3. 全生命周期状态机与时序映射

BDS 启动、运行与关服过程中，LeviLamina 状态与 Ferrum 二级模组回调的精确映射如下：

```text
BDS 进程阶段                 LeviLamina 动作                        Ferrum Host & 二级模组动作
============                 ===============                        =========================
[1] main() 早期              LeviLaminaMainHook
    GamingStatus::Starting   └─ ModRegistrar::loadAllMods()
                                ├─ 加载 ferrum-loader (native)  --> 初始化 ferrum-host 全局运行时与 VTable
                                │                                   注册 FerrumModManager("ferrum")
                                └─ 按拓扑序加载 ferrum 模组     --> FerrumModManager::load(manifest):
                                                                    1. ferrum-host 复制 .dll 到 .ferrum-shadow/ 后加载
                                                                    2. 查找 ferrum_mod_entry_v1，双向握手校验 ABI
                                                                    3. 创建 ModContext，调用模组 on_load(&ctx)
                                                                    (注: 此时 Level/CommandRegistry 尚未就绪)

[2] 资源包初始化             InitPacksHook                      --> 自动挂载各 ferrum 模组的 behavior/resource_packs

[3] 服务端线程就绪           EnableAllModsHook
    ServerScriptManager::    ├─ ModRegistrar::enableAllMods()   --> FerrumModManager::enable(name):
    $onServerThreadStarted   │                                      获取 MainThreadToken，调用模组 on_enable(&ctx, token)
                             │                                      (此时 Level / CommandRegistry 已就绪，可注册命令与世界逻辑)
                             └─ GamingStatus::Running

[4] 运行期热操作 (可选)      执行 `ll mod disable <name>`       --> FerrumModManager::disable(name)（停用协议）:
    GamingStatus::Running                                           1. 关闸：拒绝新注册/新投递
                                                                    2. 调用模组 on_disable(&ctx, token)
                                                                    3. 撤销监听、命令、定时任务、表单、服务
                                                                    4. 在主线程排空执行全部 drop_fn
                             执行 `ll mod unload <name>`        --> FerrumModManager::unload(name):
                                                                    1. 若处于 Enabled 先执行停用协议
                                                                    2. 调用模组 on_unload(ctx)，模组 join 自建线程
                                                                    3. 递增残留代数句柄，全部失效
                                                                    4. 默认保留 .dll 镜像；仅 HOT_UNLOAD_SAFE 时 FreeLibrary

[5] 关服阶段                 DisableAllModsHook
    ServerInstance::         ├─ GamingStatus::Stopping
    leaveGameSync            └─ ModRegistrar::disableAllMods()  --> 按加载逆序对各模组执行停用协议
```

停用与卸载的完整协议见《线程模型、并发与热卸载安全规范》§4。

### 动态热加载边缘情况处理
当服务器已处于 `GamingStatus::Running` 状态，管理员通过 `ll mod load <rust_mod>` 动态加载新模组时：
- LeviLamina 的 `Mod::onLoad()` 内部在成功执行 `mImpl->onLoad(*this)` 后，检测到 `getGamingStatus() == GamingStatus::Running` 会**立即自动串联调用 `onEnable()`**（参见 `ll/api/mod/Mod.cpp`）。
- 因此 `FerrumMod` 的 `onLoad` 与 `onEnable` 回调必须保持职责分离：`onLoad` 仅做配置读取、内存结构初始化与加载期钩子准备；依赖 `Level` 或 `CommandRegistry` 的操作统一放在 `onEnable` 中执行（或使用事件监听延迟触发）。
