# 项目愿景与非目标声明

## 1. 背景与核心痛点

在 Minecraft Bedrock Dedicated Server (BDS) 与 LeviLamina 生态中，直接用 Rust 编写一级 `native` 模组面临四道结构性障碍：

1. **C++ ABI 极度脆弱**：BDS 与 LeviLamina 重度依赖 MSVC C++ 类继承、模板、`std::string`/`std::function`/`ll::Expected` 以及随每个小版本重排的 `src/mc` 内存布局。Rust 的 `cxx` 或 `bindgen` 一旦直接绑定底层头文件，每次 MC 更新都会导致所有下游模组必须全量重新编译。
2. **统一分配器强校验与双堆冲突**：LeviLamina 的 `NativeModManager::load` 强制要求一级 `native` 模组导出 `ll_memory_operator_overrided` 并接入宿主统一分配器；而 Rust 标准库与第三方生态（`tokio`、`serde`、`reqwest` 等）默认使用独立全局分配器，混用指针极易引发跨堆释放崩溃。
3. **跨 FFI 异常与展开（Unwind）未定义行为**：Rust `panic!` 跨越 `extern "C"` 进入 C++ 栈帧，或 C++ 异常跨越边界进入 Rust 栈帧，均属立即崩服的未定义行为（UB）。
4. **游戏主线程并发失控**：BDS 核心状态（`Level`、`BlockSource`、`Actor`、`Container`）完全非线程安全。Rust 生态极易开启后台线程或异步任务，缺乏编译期约束会导致隐蔽的数据竞争。

---

## 2. 核心设计目标

Ferrum 作为运行在 LeviLamina 之上的**二级加载器（Secondary Mod Loader）与稳定抽象运行时**，确立以下四大目标：

### 2.1 跨版本二进制兼容（Compile Once, Run Across Versions）
- 将所有与 BDS 符号偏移、C++ 类布局、LeviLamina 模板接口相关的耦合全部收敛在单一宿主插件 `ferrum-loader.dll` 的 `cpp-shim` 层内。
- 下游 Rust 模组（`type: "ferrum"`）仅链接零外部依赖的纯 C 契约层 `ferrum-abi` 与安全包装层 `ferrum-sdk`，不链接 `LeviLamina.lib`、不接触任何 C++ 头文件。
- 当 Minecraft 或 LeviLamina 版本升级时，仅需发布新版 `ferrum-loader`，遵守 ABI 规范的下游 Rust `.dll` 模组无需重编译即可直接加载运行。
- **升级成本与 API 暴露面成正比**：ABI 稳定只保证“下游不用改”，`cpp-shim` 仍需随上游适配。因此 Ferrum 只暴露经过领域抽象、语义稳定的少量高价值 API，并按路线图逐步扩展，而不是追求覆盖面。

### 2.2 严格的堆内存隔离与零悬垂引用
- `ferrum-loader`（C++ Shim 侧）遵循 LeviLamina 统一内存分配器要求；下游 Rust 模组保留各自独立的 Rust 默认分配器，无需导出 `ll_memory_operator_overrided`。
- 跨边界数据交换严格执行“谁分配谁释放”、借用切片视图（`FfiStr` / `FfiSlice`）与回调写入器（Writer Callback）机制，根除跨 DLL `free` 崩溃。
- 游戏对象指针永不离开 `cpp-shim`：模组侧只持有 ID 与宿主代数句柄，每次调用由 shim 重新解析，实体在任何时刻被销毁都只会得到 `NotFound`，而不是悬垂指针。
- 目标是**安全 Rust 模组代码无法通过 Ferrum API 触发 UB**；模组自身 `unsafe` 代码的正确性不在保证范围内。

### 2.3 编译期主线程安全与人体工学异步
- 利用 Rust 类型系统构造零运行时开销的 `MainThreadToken<'tick>`（`!Send + !Sync`），所有非线程安全的世界、方块、实体读写 API 在签名层强制要求传入该令牌。
- 内置与 LeviLamina `ServerThreadExecutor` 对接的任务桥接器，允许模组自由使用 `tokio` 等异步生态处理网络/数据库 IO，再安全切回主线程提交游戏状态变更。

### 2.4 模组级故障围栏与可靠停用
- 所有宿主回调入口内置 `std::panic::catch_unwind` 围栏；单个 Rust 模组发生普通 `panic!` 仅中断当次回调，panic 过的回调被毒化，持续出错的模组被熔断停用，不会连带宿主崩服。（double panic、栈溢出、模组 `unsafe` UB 等无法拦截的情形见《状态码、异常与 Panic 隔离规范》§1.4。）
- 所有向 LeviLamina 注册的资源（`EventBus` 监听器、`RuntimeOverload` 命令、Tick 调度器、表单回调、跨模组服务）均绑定 `ModId` 所有权，支持运行期 `ll mod disable/unload/reload` 全自动回收。
- **稳定优先于内存回收**：卸载默认保留模组 `.dll` 镜像，不调用 `FreeLibrary`，从根本上杜绝“跳入已卸载代码段”类崩溃；只有显式声明 `hot_unload_safe` 的模组才真正释放镜像。

---

## 3. 明确的非目标声明（Non-Goals）

为防止架构腐化与维护成本失控，Ferrum **明确不做**以下事项：

1. **不做 `src/mc` 全量 C++ 头文件的 1:1 自动映射**
   - 严禁将 BDS 内部成千上万个不稳定 C++ 类通过自动生成器裸露给下游模组。一旦暴露底层布局，跨版本二进制稳定性即刻破产。Ferrum 仅提供经过领域抽象的高内聚语义 API（玩家、实体、方块、容器、世界、事件、命令、表单、NBT、网络等）。
2. **不支持向无源码/裸函数地址打底层 Inline Hook**
   - 需要使用 `LL_AUTO_TYPE_INSTANCE_HOOK` 或底层汇编补丁修改未公开游戏逻辑的场景，本身就是强版本耦合操作，应当直接编写 LeviLamina 一级 C++ `native` 插件，或向 Ferrum 上游提交标准领域 API 提案。
3. **不在 ABI 层依赖 Rust 编译器内部布局**
   - 绝不采用 `extern "Rust"`、`dyn Trait` 胖指针、`String`、`Vec<T>`、`Result<T, E>` 跨越 DLL 边界。即便宿主与模组使用不同版本的 `rustc` 编译，ABI 依然保持二进制兼容。
4. **不接管或替代 LeviLamina 的核心职责**
   - 模组目录扫描、拓扑依赖排序（`ModLoadPlanner`）、生命周期命令（`ll mod ...`）、日志汇聚（`io::Logger`）与基础崩溃转储均完全复用 LeviLamina 原生设施，不重复造轮子。
5. **不提供进程级沙箱**
   - 下游模组是与 BDS 同进程的原生代码，Ferrum 无法阻止恶意或含 UB 的 `unsafe` 代码破坏进程。需要强隔离（不可信第三方脚本）的场景应考虑 WASM 等沙箱方案，不在 Ferrum 范围内。
6. **不保证 `FreeLibrary` 级热卸载**
   - 热重载开发体验通过“保留旧镜像 + 影子副本加载新镜像”实现；真正释放镜像只作为模组显式声明的可选能力。
