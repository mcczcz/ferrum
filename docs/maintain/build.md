# 构建

`ferrum-loader.dll` = `cpp-shim`（C++20，xmake + clang-cl）静态链接 `ferrum-host`（Rust `staticlib`）。`xmake.lua` 的 `before_build` 先执行 `cargo build -p ferrum-host --locked`（非 debug 模式加 `--release`），再执行 `cargo xtask cbindgen --check`。

## 前置要求

- Windows x64
- Visual Studio 2022 Build Tools（C++ 桌面开发、Windows SDK）
- LLVM（`clang-cl`），CI 用 22
- xmake
- Rust stable，`x86_64-pc-windows-msvc`，带 `rustfmt`、`clippy`（见 `rust-toolchain.toml`）

链接约束：统一 `/MD` 动态 CRT。`ferrum-host` 不能开 `+crt-static`，否则会 `LNK2005`。

## 纯 Rust 检查

不需要 C++ 环境。提交前跑：

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask cbindgen --check
cargo xtask layout-lock --check
```

- `ferrum-host/tests/load_real_dll.rs` 会真实加载 `hello_ferrum.dll`。DLL 不存在就跳过。要跑它，先 `cargo build -p hello-ferrum`。CI 会先构建再测试。
- `cbindgen --check` 对比 `cpp-shim/include/ferrum/ferrum_abi.h`。`layout-lock --check` 对比 `crates/ferrum-abi/tests/layout_lock.rs`。过期时去掉 `--check` 重新生成。这两个文件不要手改。

## 完整构建

```powershell
xmake f -p windows -a x64 -m release -y --target_type=server
xmake
```

- `--target_type` 取 `server` 或 `client`，CI 两个都构建。
- `-m` 取 `debug` 或 `release`，决定链接 `target/debug` 还是 `target/release`。
- 产物在 `bin/ferrum-loader/`，含 `config.jsonc` 和 `lang/`。

## 调试

- `clang-cl` 和 `rustc` 都输出 PDB。`ferrum-loader.pdb` 和模组 `.pdb` 放在对应 `.dll` 旁边。release profile 带 `line-tables-only`，够看行号和调用栈。
- 用 Visual Studio 或 VS Code `codelldb` 附加到 `bedrock_server_mod.exe`，可在同一调用栈里跨 C++ 与 Rust 单步。
- 模组 DLL 会先复制到 `<mod_dir>/.ferrum-shadow/` 再加载，调试器里是影子副本路径。
- `ll mod unload` 后影子副本仍在模块列表里。镜像不释放是预期行为，见 [../design/threading.md](../design/threading.md)。
