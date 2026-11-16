# 架构

## 目标与非目标

问题：直接用 Rust 写 LeviLamina `native` 模组有四个障碍。

- C++ ABI 随 MC 版本变化，绑定 `src/mc` 会让每次更新都要重编所有模组。
- `native` 模组必须接入 LL 统一分配器，Rust 生态用自己的分配器，跨堆释放会崩。
- panic 或 C++ 异常跨 FFI 是 UB。
- BDS 世界状态非线程安全，Rust 容易误开后台线程。

目标：

- 一次编译，跨 MC/LL 版本运行。版本适配只在 `ferrum-loader.dll` 内完成。
- 模组与宿主各用各的堆；游戏对象指针不出 `cpp-shim`，模组只拿 ID 和句柄。
- 世界 API 在签名上要求 `MainThreadToken`，编译期挡住非主线程调用。
- 模组 panic 只影响该模组，停用时自动回收其注册的资源。
- 目标是安全 Rust 代码不能通过 Ferrum API 触发 UB；模组自己的 `unsafe` 不在保证内。
- 只暴露少量语义稳定的领域 API。ABI 稳定不等于 `cpp-shim` 免维护。

非目标：

- 不自动映射 `src/mc` 头文件。
- 不支持 Inline Hook。需要的场景写 C++ `native` 插件，或给 Ferrum 提领域 API。
- ABI 不暴露 Rust 内部布局（`String`、`Vec`、`dyn Trait` 等）。
- 不替代 LL 的目录扫描、依赖排序、`ll mod` 命令、日志。
- 不提供进程级沙箱。
- 不保证 `FreeLibrary` 级热卸载，默认保留镜像。

## 层次

产物有两个：`ferrum-loader.dll`（LL 的 `native` 插件）和各模组 `<name>.dll`（`type = "ferrum"`）。两者之间只有纯 C ABI。

```text
bedrock_server + LeviLamina
 └─ ferrum-loader.dll (native)
     ├─ cpp-shim   (C++20)     唯一包含 ll/* 与 mc/* 的代码
     └─ ferrum-host (Rust 静态库) 句柄池、VTable、回调所有权、生命周期状态
          │  纯 C ABI：ferrum-abi (#[repr(C)] 表 + 句柄)
          │  模组入口收到 *const FerrumHostApi
 └─ my_mod.dll (ferrum)
     └─ ferrum-sdk + ferrum-macros   安全 API、catch_unwind、MainThreadToken
```

| Crate / 目录 | 产物 | 职责 | 禁止 |
| :--- | :--- | :--- | :--- |
| `cpp-shim` | C++ 源码，链入 `ferrum-loader.dll` | 把 LL/MC 的类、虚函数、异常翻译成 `extern "C"`；注册 `FerrumModManager`；导出 `LL_REGISTER_MOD` 并启用 LL 内存操作符 | 持有业务状态 |
| `ferrum-host` | Rust `staticlib` | 影子复制并加载模组 DLL、握手、全局 VTable、代数句柄池、`ModOwnership`、熔断 | 直接绑定 C++ 符号 |
| `ferrum-abi` | `#![no_std]` rlib，无外部依赖 | `#[repr(C)]` 类型、函数指针、常量 | 放逻辑 |
| `ferrum-sdk` | rlib | 模组侧安全 API | 依赖 host / shim |
| `ferrum-macros` | proc-macro | `#[mod_entry]`：导出 `ferrum_mod_entry_v1`、握手、`catch_unwind` | - |
| `ferrum-host-mock` | rlib（dev） | 真实 `ferrum-host` + Rust 假 shim，供模组 `cargo test` | 不重写宿主生命周期与回调管理 |
| `xtask` | 二进制 | 生成 C 头文件与布局锁 | - |

依赖方向：

```text
ferrum-abi <── ferrum-host <──(extern "C")── cpp-shim ──> LeviLamina / mc
    ^
    └── ferrum-sdk (+ ferrum-macros) <── 模组
```

- 只有 `cpp-shim` 可以 `#include "ll/..."` 和 `"mc/..."`。
- `ferrum-sdk` 与 `ferrum-host` 互不依赖，各自从 `ferrum-abi` 出发。
- 模组与宿主唯一的接触面是运行时传入的 `*const FerrumHostApi`。

## 接入 LeviLamina

Ferrum 使用 LL 的 `ModManager` / `ModManagerRegistry` 扩展点，没有私有 Hack。

加载顺序：

1. `ferrum-loader` 的 manifest 为 `type = "native"`。
2. 每个 Ferrum 模组的 manifest 为 `type = "ferrum"`，且 `dependencies` 必须包含 `ferrum-loader`。
3. LL 的 `ModLoadPlanner` 按依赖排序，保证 `ferrum-loader` 先 `load`。
4. `Entry::load` 里创建 `FerrumModManager("ferrum")` 并 `addManager`。必须在 `load` 阶段注册，后续 `type = "ferrum"` 的模组才会被委派给它。
5. 每个 Ferrum 模组对应一个 `FerrumMod`（`ll::mod::Mod` 派生类）。`ll list`、`ll mod enable/disable/unload/reload`、级联依赖检查、目录与日志器都直接可用。

规则：

- `cpp-shim` 为某个 Ferrum 模组向 LL 注册资源（`EventBus` 监听等）时，必须传该模组的 `weak_ptr<FerrumMod>`。省略参数会默认成 `NativeMod::current()`，即 `ferrum-loader` 自己，LL 的按模组清理就失效了。只有 loader 自身资源（如自定义事件发射器）才用 `NativeMod::current()`。

## 生命周期映射

| 阶段 | LL 动作 | Ferrum 动作 |
| :--- | :--- | :--- |
| 加载（`loadAllMods`，`Level` 未就绪） | `FerrumModManager::load` | 复制 DLL 到 `<mod_dir>/.ferrum-shadow/` 后加载；查找 `ferrum_mod_entry_v1` 并握手；调用 `on_load(ctx)`；再 `addMod` 并执行 `Mod::onLoad` |
| 启用（`enableAllMods` / `ll mod enable`） | `Mod::onEnable` 回调 | `on_enable(ctx, token)`。`Level`、`CommandRegistry` 此时可用 |
| 停用（`ll mod disable` / 关服） | `Mod::onDisable` 回调 | 关闸；`on_disable`；撤销命令与事件监听并析构回调环境（`drop_fn`） |
| 卸载（`ll mod unload`） | `Mod::onUnload` | 先停用；`on_unload`；移除模组记录，残留句柄变 `STALE_HANDLE`；保留 DLL 镜像 |

补充：

- 关服时 LL 按加载逆序停用各模组。
- 停用发生时若模组回调仍在执行，停用推迟到最外层回调返回后完成。
- `on_enable` 失败（含 panic）时模组回到 `Disabled`，已注册的资源被撤销。
- 服务器已在运行时 `ll mod load`，LL 会在 `onLoad` 后自动接着调用 `onEnable`。所以 `on_load` 只做配置读取和内存初始化，依赖 `Level` 或 `CommandRegistry` 的操作放 `on_enable`。
- `hot_unload_safe` 标志会被接受，但目前仍不 `FreeLibrary`（规划中）。

停用与卸载的线程细节见 [threading.md](threading.md)。
