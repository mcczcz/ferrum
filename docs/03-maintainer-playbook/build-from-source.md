# 源码联编与调试指南

本手册面向 Ferrum 引擎维护者，说明如何从源码构建宿主加载器 `ferrum-loader.dll` 及 Workspace 内所有 Rust Crates。

---

## 1. 构建管线架构（xmake + Cargo 混合联编）

`ferrum-loader.dll` 由两部分静态链接合并为一个标准 LeviLamina `native` 插件：
1. **Rust 核心运行时（`crates/ferrum-host`）**：由 Cargo 编译为静态库 `ferrum_host.lib`（`crate-type = ["staticlib", "rlib"]`，后者供单元测试使用）。
2. **C++20 适配垫片（`cpp-shim/`）**：由 xmake 使用 `clang-cl` 编译，链接 `LeviLamina`（通过 `levibuildscript` / `xmake-repo` 拉取）以及刚才生成的 `ferrum_host.lib`（加上 Windows 系统依赖库 `kernel32`、`ntdll`、`userenv`、`ws2_32`、`dbghelp`、`synchronization`、`advapi32`、`bcrypt`）。

```text
[Cargo Build]                                 [xmake Build (clang-cl, C++20)]
crates/ferrum-abi ─┐                          cpp-shim/*.cpp  +  LeviLamina SDK
                   ├─> ferrum_host.lib ───────────────────────┼─────────────────> ferrum-loader.dll
crates/ferrum-host ┘                                          │                   (导出 ll_mod_load 与
                                                              └─ 链接系统导入库      ll_memory_operator_overrided)
```

---

## 2. 开发环境前置要求

- **操作系统**：Windows 10 / 11 x64
- **Visual Studio 2022 Build Tools**：包含 C++ 桌面开发组件、Windows 10/11 SDK 与 LLVM (`clang-cl`)
- **xmake**：`>= 2.9.x`（用于构建 `cpp-shim` 并拉取固定版本的 `levilamina` 包）
- **Rust**：符合仓库根 `rust-toolchain.toml` 指定的工具链（`x86_64-pc-windows-msvc`）

### CRT 链接模式一致性铁律（`/MD` 动态 CRT）
LeviLamina 发布版与 BDS 统一使用动态多线程运行时（MSVC `MD` CRT）。因此：
- `crates/ferrum-host` 在编译时**严禁设置 `+crt-static`**（保持默认的动态 `msvcrt` 链接），确保与 `cpp-shim` 链接进同一个 DLL 时不会发生 CRT 符号重定义冲突（`LNK2005`）。
- 即使在 Debug 调试宿主时，由于预编译的 BDS 与 Release 版 LeviLamina 使用非调试 CRT（`/MD` 而非 `/MDd`），xmake 中也必须遵循 LeviLamina 插件标准配置（`runtimes = "MD"`，开启符号 `-g` / `/Zi` 即可源码级断点调试）。

---

## 3. 标准联编命令

### 3.1 检查与测试纯 Rust 层（无需 C++ 环境即可跑通单元测试）
`ferrum-abi`、`ferrum-sdk`、`ferrum-macros` 与 `ferrum-host`（通过 Mock Shim 接口）支持纯 Cargo 极速测试：
```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# 生成物校验：C 头文件与布局锁必须与 ferrum-abi 一致
cargo xtask cbindgen --check
cargo xtask layout-lock --check
```

### 3.2 完整构建 `ferrum-loader.dll`
`xmake.lua` 内置 `before_build` 钩子自动触发对应模式（debug/release）的 `cargo build -p ferrum-host --locked` 并执行 `cargo xtask cbindgen --check`：
```powershell
# 配置构建目标（server 或 client）
xmake f -p windows -a x64 -m release -y --target_type=server

# 执行编译与打包输出至 bin/ferrum-loader/
xmake
```

---

## 4. 源码级联合断点调试（LLDB / VS Code `codelldb` / Visual Studio）

因为 `clang-cl` 和 `rustc`（MSVC target）全部生成标准 **PDB 调试符号文件**：
1. 编译带调试信息的产物（`xmake f -m releasedbg` 或保留 PDB），确保 `ferrum-loader.pdb` 与 `<example_mod>.pdb` 与各自 `.dll` 位于同一 `mods/.../` 目录。
2. 使用 VS Code `vadimcn.vscode-lldb` 或 Visual Studio 直接启动或附加（Attach）到 `bedrock_server_mod.exe`。
3. 可在同一个调用栈中无缝单步跨越：
   `BDS C++ Hook` -> `cpp-shim (C++)` -> `ferrum-host (Rust)` -> `example_mod (Rust)`！
4. **验证卸载行为**：在调试器模块窗口（Modules View）观察执行 `ll mod unload <example_mod>` 后的模块列表：
   - 当前（M1）所有模组的影子副本（`.ferrum-shadow/<name>-<hash>.dll`）均会**保留镜像**（声明 `hot_unload_safe` 仅记录 debug 提示，真正 `FreeLibrary` 路径在 M5 实现）。
