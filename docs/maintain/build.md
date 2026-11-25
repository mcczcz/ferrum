# 构建

`ferrum-loader.dll` 由 `cpp-shim`（C++20，xmake + `clang-cl`）静态链接 `ferrum-host`（Rust `staticlib`）构成。`xmake.lua` 的 `before_build` 会先构建 `ferrum-host` 并校验 `cbindgen --check`。

## 工具链要求

- Windows x64 + Visual Studio 2022 Build Tools（C++ 桌面开发与 Windows SDK）
- LLVM（`clang-cl`）与 xmake
- Rust stable（`x86_64-pc-windows-msvc`，含 `rustfmt` 与 `clippy`，见 `rust-toolchain.toml`）
- **链接约束**：全链路统一 `/MD` 动态 CRT；`ferrum-host` 严禁开启 `+crt-static`。

## 纯 Rust 检查（无需 C++ 环境）

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask cbindgen --check
cargo xtask layout-lock --check
```

- `ferrum-host/tests/load_real_dll.rs` 会加载 `hello_ferrum.dll`；若尚未编译会跳过，完整测试前先运行 `cargo build -p hello-ferrum`。
- `cbindgen` 与 `layout-lock` 的生成物严禁手改；过期时去掉 `--check` 重新生成。

## 完整构建

```powershell
xmake f -p windows -a x64 -m release -y --target_type=server
xmake
```

- `--target_type` 支持 `server` 与 `client`；`-m` 支持 `debug` 与 `release`。
- 产物输出在 `bin/ferrum-loader/`（含 `config.jsonc` 与 `lang/`）。

## 跨语言调试

可使用 Visual Studio 或 VS Code（`codelldb`）附加到 `bedrock_server_mod.exe`，在同一调用栈内单步 C++ 与 Rust（`release` profile 已开启 `line-tables-only` PDB）。加载器会将模组 DLL 与同名 `.pdb` 复制到 `.ferrum-shadow/` 后加载。
