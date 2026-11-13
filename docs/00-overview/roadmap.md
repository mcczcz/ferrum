# 实施路线图

本路线图按“先打通端到端最小链路、再扩展 API 暴露面”的原则安排。每个里程碑结束时都必须有可在真实 BDS 上运行的产物与对应测试；在前一个里程碑的验收项全部通过之前，不开始下一个里程碑的 API 扩展。

---

## M0：仓库骨架与生成管线（已完成）

- Cargo workspace：`ferrum-abi`、`ferrum-host`、`ferrum-sdk`、`ferrum-macros`、`ferrum-host-mock`、`examples/hello`、`xtask`。
- 将模板中的 C++ 目标改造为 `cpp-shim/`，xmake target 更名为 `ferrum-loader`，`before_build` 调用 `cargo build -p ferrum-host --locked` 与 `cargo xtask cbindgen --check`。
- `cargo xtask cbindgen` 生成 `cpp-shim/include/ferrum/ferrum_abi.h`；`cargo xtask layout-lock` 生成布局断言。
- 本地/CI 校验：`fmt` / `clippy -D warnings` / `test` / 生成物 `--check` / xmake 完整构建。

**验收**：空的 `ferrum-loader.dll` 能被 LeviLamina 加载并打印版本日志。

## M1：加载器最小闭环（已完成）

- `ferrum-abi`：`AbiVersion`、`FerrumStatus`、`FfiStr`、`FfiSlice`、`FfiBool`、`FfiByteWriter`、`FerrumHostApi`（`core_v1` + `log_v1` + `query_interface`）、`FerrumModDescriptor`、`ferrum_mod_entry_v1`。
- `cpp-shim`：注册 `FerrumModManager("ferrum")`，转发 `onLoad/onEnable/onDisable/onUnload`。
- `ferrum-host`：影子复制（`.ferrum-shadow/`）与 `libloading` 加载、双向握手、`ModContextHandle` 代数槽位池、`LastErrorSlot`、panic 上报与滑动窗口熔断计数。
- `ferrum-sdk` + `ferrum-macros`：`#[mod_entry]`、`catch_unwind` 跳板、`panic = "abort"` 编译期拦截、`Logger`、`ModContext`、`MainThreadToken`。
- 卸载**只实现保留镜像模式**（`hot_unload_safe` 仅记录提示，不调用 `FreeLibrary`）。

**验收**：`examples/hello` 在 BDS 中完成 load → enable → disable → reload → 关服；在 `on_enable` 中故意 panic 不崩服且模组进入 `Disabled`。

## M2：回调基础设施与第一批领域 API（下一步 / 未开始）

- 强类型回调结构体、`ModOwnership`、停用协议（关闸 → 通知 → 撤销 → 排空）、自我停用的延迟执行。
- SDK：`FnMut` 重入守卫、panic 毒化、HRTB 令牌签名。
- 事件：`PlayerJoinEvent`、`PlayerLeaveEvent`、`PlayerChatEvent`（可拦截）。
- 玩家：按 `ActorRuntimeId` 解析的 `PlayerRef`：`real_name`、`uuid`、`send_message`、`is_operator`；`world::get_player(token, uuid)`。
- 命令：基于 `RuntimeOverload` 的 Builder（先支持 `Int`/`Float`/`Bool`/`String`/`Player`）。
- `ferrum-host-mock` 覆盖以上全部 API。

**验收**：
- 在聊天回调中注销自身、在命令回调中 `ll mod disable <self>`、在回调中踢出玩家后继续访问该玩家，三种场景均不崩服，并返回预期错误。
- `compat_test_mod_v1_0.dll` 作为首个二进制兼容基准发布。

## M3：调度与异步（未开始）

- `SchedulerApiV1`：`post`、`after_ticks`、`every_ticks`、`cancel`，以及模组停用时的取消门控。
- SDK：运行时无关的 `on_main_thread(...).await`；停用时等待方收到 `MOD_DISABLED`。
- 非 `Send` 闭包只在主线程析构的保证与测试。

**验收**：`tokio` 示例模组反复 reload 100 次，无崩溃、无挂起的 future；内存增长只来自保留的镜像。

## M4：表单、跨模组服务与扩展领域（未开始）

- 表单（Simple / Modal / Custom）及其异步封装。
- 服务注册总线：`ServiceHandle` + `ServiceGuard`，提供方停用时撤销并等待 guard 释放。
- 按社区需求逐个通过 `query_interface` 增加领域子表（计分板、容器、方块、实体属性等），每个都遵循《新增领域 API 标准流程》。

## M5（可选）：真正的热卸载（未开始）

- 实现 `HOT_UNLOAD_SAFE` 路径：排空后检测线程是否仍在模组镜像内，安全时 `FreeLibrary`。
- 只在 M1–M4 稳定运行一段时间、且确有内存回收需求时再做。
