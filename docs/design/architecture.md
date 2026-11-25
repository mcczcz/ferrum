# 架构

## 目标与非目标

**目标**：
- 模组一次编译即可跨 MC / LeviLamina 版本运行，版本适配收敛在 `ferrum-loader.dll` 的 `cpp-shim` 内。
- 模组与宿主隔离堆分配；游戏对象指针不出 `cpp-shim`，跨边界只传递值 ID 与代数句柄。
- 通过 `MainThreadToken<'tick>` 在编译期拦截非主线程世界访问。
- 双向隔离 C++ 异常与 Rust panic；模组停用时自动回收其注册的资源。

**非目标**：
- 不自动绑定 `src/mc` 头文件，也不支持 Inline Hook。
- 不跨 ABI 暴露 Rust 布局（`String`、`Vec`、`dyn Trait` 等）。
- 不替代 LeviLamina 的依赖排序、命令管理（`ll mod`）与日志器。
- 不保证 `FreeLibrary` 级热卸载（默认保留镜像）。

## 分层与依赖边界

```text
bedrock_server + LeviLamina
 └─ ferrum-loader.dll (type = "native")
     ├─ cpp-shim    (C++20)        唯一允许 #include <ll/*> 与 <mc/*>
     └─ ferrum-host (Rust staticlib) 影子加载、握手、VTable、句柄池、ModOwnership、调度与熔断
          │  纯 C ABI（ferrum-abi：#[repr(C)] 表 + 代数句柄）
 └─ my_mod.dll (type = "ferrum")
     └─ ferrum-sdk + ferrum-macros   安全封装、catch_unwind 防火墙、MainThreadToken
```

| 路径 | 产物 | 职责 | 禁止 |
| :--- | :--- | :--- | :--- |
| `cpp-shim` | C++ 目标 | 翻译 LL/MC 对象与异常为 `extern "C"`；注册 `FerrumModManager`；启用 LL 内存操作符 | 持有业务状态 |
| `crates/ferrum-host` | `staticlib` | 影子复制加载、握手、VTable 分发、代数句柄池、`ModOwnership`、调度队列、熔断 | 直接绑定 C++ 符号 |
| `crates/ferrum-abi` | `#![no_std]` rlib | 零外部依赖 `#[repr(C)]` 类型、函数指针、状态码与常量 | 放业务逻辑 |
| `crates/ferrum-sdk` | rlib | 模组侧安全 API 与防火墙跳板 | 依赖 `ferrum-host` / `cpp-shim` |
| `crates/ferrum-macros` | proc-macro | `#[mod_entry]` 生成 `ferrum_mod_entry_v1` 与生命周期跳板 | - |
| `crates/ferrum-host-mock` | rlib（dev） | 真实 `ferrum-host` + Rust 假 shim，供 `cargo test` | 重写宿主状态机 |
| `xtask` | CLI | 生成 `ferrum_abi.h` 与 `layout_lock.rs` 断言 | - |

依赖方向：`ferrum-abi` 为共同契约，`ferrum-host` 与 `ferrum-sdk` 互不依赖；模组与宿主唯一接触面为入口传入的 `*const FerrumHostApi`。

## LeviLamina 接入与生命周期映射

`Entry::load` 注册 `FerrumModManager("ferrum")` 到 `ModManagerRegistry`，后续 `type = "ferrum"` 且依赖 `ferrum-loader` 的模组均由其托管（每个模组对应一个 `ll::mod::Mod` 子类 `FerrumMod`）。

**硬规则**：`cpp-shim` 代表二级模组向 LL 注册资源（如 `EventBus`、`CommandHandle`）时，**必须显式传入该 `FerrumMod` 的 `weak_ptr`**；省略默认参数会归属给 `NativeMod::current()`（`ferrum-loader` 自身），导致按模组清理失效。

| 阶段 | LL 触发 | Ferrum 动作 |
| :--- | :--- | :--- |
| 加载 | `FerrumModManager::load` | 影子复制 DLL 到 `.ferrum-shadow/` 并加载；查 `ferrum_mod_entry_v1` 握手；调 `on_load`；加入注册表 |
| 启用 | `Mod::onEnable` | 主线程调用 `on_enable(ctx, token)`。运行时 `ll mod load` 会在 `onLoad` 后紧接调用 |
| 停用 | `Mod::onDisable` | 关闸；调 `on_disable`；撤销命令/监听器/事件/任务并析构 `drop_fn` |
| 卸载 | `Mod::onUnload` | 若启用则先停用；调 `on_unload`；移除记录使句柄失效（`STALE_HANDLE`）；保留 DLL 镜像 |
