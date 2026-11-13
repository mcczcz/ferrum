# AGENTS.md

Ferrum 是构建在 LeviLamina 之上的 Rust 二级模组加载器与跨版本稳定运行时（Windows x64，MSVC ABI）。

## 目录布局与分层硬边界

| 路径 | 职责与隔离边界 |
| :--- | :--- |
| `crates/ferrum-abi` | 零外部依赖的纯 `#[repr(C)]` ABI 契约与 VTable 定义（兼容 `#![no_std]`），由宿主与 SDK 共同依赖。 |
| `crates/ferrum-host` | 宿主侧 Rust `staticlib` 运行时（代数槽位句柄池、VTable 分发、`ModOwnership`、调度器）。严禁直接绑定 C++ 符号。 |
| `cpp-shim` | 极薄 C++20 垫片层（`clang-cl` + xmake）。**唯一允许 `#include` `ll/api/*` 或 `mc/*` 的目录**，与 `ferrum-host` 静态链接为 `ferrum-loader.dll`（`type = "native"`）。 |
| `crates/ferrum-sdk` | 面向下游模组（`type = "ferrum"`，`cdylib`）的安全 Rust SDK。零 C++ 头文件、零 `LeviLamina.lib` 链接。 |
| `crates/ferrum-macros` | `#[ferrum_sdk::mod_entry]` 等过程宏，生成 `ferrum_mod_entry_v1` 握手与 `catch_unwind` 隔离跳板。 |
| `crates/ferrum-host-mock` | 纯 Rust 假宿主 VTable，供 `ferrum-sdk` 与下游模组在无 BDS 环境下 `cargo test`。 |
| `crates/cargo-ferrum` | 下游模组脚手架与打包 CLI 工具。 |
| `xtask` | 生成 `cpp-shim/include/ferrum/ferrum_abi.h`（cbindgen）与 `layout_lock.rs` 断言；生成物严禁手改。 |
| `docs` | 权威架构与工程规范。修改 ABI、FFI 或生命周期逻辑前，必须先阅读 `docs/01-specs/`。 |

## 不可违背的架构铁律

1. **纯 C ABI 与 VTable 尾追加铁律（`docs/01-specs/abi-stability-spec.md`）**
   - 严禁跨 FFI 传递任何 `repr(Rust)` 类型、`String`、`Vec`、`&str`、`&[T]`、`Result`、`&dyn Trait` 或 C++ STL/MC 对象。
   - 所有公开的 `#[repr(C)]` VTable 与可扩展配置结构体首字段必须是 `pub struct_size: u32`。
   - 同一主版本内严禁删除、重排或修改既有 VTable 字段；新增函数指针只能追加在结构体末尾，递增 `AbiVersion.minor`，并在 `crates/ferrum-abi/tests/layout_lock.rs` 中追加 `offset_of!` 静态断言。
   - 跨 FFI 状态/枚举值统一使用 `#[repr(transparent)] pub struct FooKind(pub u32);`，布尔值统一使用 `FfiBool(u8)`，严禁 Rust `bool` 与封闭 `enum` 跨边界。
   - C++ 侧结构体定义只能来自 cbindgen 生成的 `ferrum_abi.h`，严禁手写镜像。
   - 模组描述符中的 `required_abi_version` 取模组实际所需的最低版本，而非编译所用 SDK 的版本。

2. **双堆隔离与内存所有权（`docs/01-specs/memory-and-ownership.md`）**
   - `ferrum-loader.dll` 接入 LeviLamina 统一分配器（导出 `ll_memory_operator_overrided`）；下游 Rust 模组使用各自独立的 Rust 分配器。**严禁跨 DLL 边界释放对方分配的内存。**
   - 输入统一使用零拷贝借用视图 `FfiStr<'a>` / `FfiSlice<'a, T>`（还原 Rust 切片前必须防御 `len == 0 && ptr.is_null()`）；变长输出统一使用调用方提供的 `FfiByteWriter` 回调写入。
   - 回调使用强类型结构体（`env` + 调用函数 + `drop_fn`）；模组镜像释放前必须排空并执行完该模组名下所有 `drop_fn`。

3. **句柄生命周期、主线程令牌与 LeviLamina 对接（`docs/01-specs/handle-and-id-design.md`, `threading-and-safety.md`）**
   - **裸指针永不出 `cpp-shim`**：`ferrum-abi` 中不得出现任何游戏对象指针。实体以 `ActorRuntimeId` 跨边界，shim 每次调用通过 `Level::getRuntimeEntity` 重新解析，不存在则返回 `NOT_FOUND`。
   - 非线程安全的世界与实体 API 必须受 `!Send + !Sync` 的 `MainThreadToken<'tick>` 或瞬态引用（如 `PlayerRef<'t>`）约束；接收令牌的回调签名必须使用 HRTB（`for<'tick> FnMut(..., &MainThreadToken<'tick>)`）。跨 Tick / 异步任务只允许保存 `PlayerUuid` / `ActorUniqueId` 并在主线程重新解析。
   - shim 授予主线程令牌前必须断言当前确为主线程；非 `Send` 闭包的调用与析构只能在主线程执行。
   - 调用任何模组 FFI 回调前，必须先释放 `ferrum-host` 内部锁，严禁持锁回调。SDK 的 `FnMut` 跳板必须带重入守卫。
   - 卸载默认**保留模组镜像、不调用 `FreeLibrary`**；只有声明 `HOT_UNLOAD_SAFE` 的模组才释放镜像。跨模组服务通过 `ServiceHandle` 代数句柄访问，严禁交出 `&'static` VTable。
   - 在 `cpp-shim` 调用任何带默认参数 `std::weak_ptr<mod::Mod> = NativeMod::current()` 的 LeviLamina 接口（`EventBus`、`CommandHandle::runtimeOverload` 等）时，**必须显式传入目标二级 `FerrumMod` 的 `weak_ptr`**，严禁省略（默认参数会错误归属给 `ferrum-loader` 自身）。

4. **双向异常与 Panic 防火墙（`docs/01-specs/error-handling-spec.md`）**
   - `cpp-shim` 所有 `extern "C"` 函数必须标注 `noexcept`，转换 `ll::Expected<T>` 并以 `try / catch (...)` 兜底写入线程局部 `LastErrorSlot`。
   - SDK 所有进入模组代码的回调跳板（含 `drop_fn`、`write_fn`）必须包裹 `std::panic::catch_unwind`；SDK 对 `panic = "abort"` 以 `compile_error!` 拦截。panic 过的回调被毒化，不再调用。

5. **Unsafe 注释与代码风格**
   - Rust 中每一处 `unsafe` 块上方必须附带 `// SAFETY:` 注释，明确列出所满足的安全不变式。
   - `cpp-shim` 遵循 LeviLamina C++20 规范：禁止 `dynamic_cast`，禁止匿名命名空间（文件局部符号用 `static`），统一使用 `/MD` 动态 CRT（`ferrum-host` 严禁开启 `+crt-static`）。

## 构建与验证命令

纯 Rust 层检查与测试（无需 BDS / C++ 环境）：

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask cbindgen --check
cargo xtask layout-lock --check
```

完整构建宿主插件 `ferrum-loader.dll`（`cpp-shim` + `ferrum-host`）：

```powershell
xmake f -p windows -a x64 -m release -y --target_type=server
xmake
```
