# 快速开始

编写一个最小 Ferrum 模组并部署到 BDS。完整示例见 [`examples/hello`](../../examples/hello)。

## 环境准备

- Rust 稳定版（最低 `1.85`），目标架构 `x86_64-pc-windows-msvc`。
- 已安装 LeviLamina 与 `ferrum-loader`（位于 `mods/ferrum-loader/`）的 BDS。
- 编写 Ferrum 模组**不需要** C++ 编译器、`xmake` 或 BDS 头文件，只需要 `cargo`。

## 创建项目

```powershell
cargo new --lib hello-ferrum
```

`Cargo.toml`：

```toml
[package]
name = "hello-ferrum"
version = "0.1.0"
edition = "2024"

[lib]
name = "hello_ferrum"
crate-type = ["cdylib", "rlib"] # cdylib 供 BDS 加载，rlib 供 cargo test 链接

[dependencies]
ferrum-sdk = { git = "https://github.com/Yangyangzhong82/ferrum" }

[dev-dependencies]
ferrum-host-mock = { git = "https://github.com/Yangyangzhong82/ferrum" }

[profile.release]
panic = "unwind" # 必须为 unwind，设为 abort 会触发编译错误
```

`src/lib.rs`：

```rust
use ferrum_sdk::prelude::*;

pub struct HelloFerrum {
    logger: Logger,
}

#[ferrum_sdk::mod_entry]
impl FerrumMod for HelloFerrum {
    fn on_load(ctx: &ModContext) -> Result<Self> {
        let logger = ctx.logger();
        logger.info("加载中");
        Ok(Self { logger })
    }

    fn on_enable(&mut self, _ctx: &ModContext, _token: &MainThreadToken<'_>) -> Result<()> {
        self.logger.info("已启用");
        Ok(())
    }
}
```

除 `on_load` 外，`on_enable`、`on_disable`、`on_unload` 均可省略。模组停用时，宿主会自动注销其注册的命令、事件监听器和调度任务。

## 部署与重载

```powershell
cargo build --release
```

在 BDS 的 `mods/hello-ferrum/` 下放入编译产物和 `manifest.json`（字段详见 [manifest.md](./manifest.md)）：

```json
{
    "name": "hello-ferrum",
    "entry": "hello_ferrum.dll",
    "version": "0.1.0",
    "type": "ferrum",
    "dependencies": [{ "name": "ferrum-loader", "version": "^0.1.0" }]
}
```

目录结构：

```text
mods/
├── ferrum-loader/
└── hello-ferrum/
    ├── manifest.json
    ├── hello_ferrum.dll
    ├── config/    # ctx.config_dir()
    ├── data/      # ctx.data_dir()
    └── lang/      # ctx.lang_dir()
```

启动 `bedrock_server_mod.exe` 后，执行 `ll list` 确认模组已加载。开发调试时可直接覆盖 `hello_ferrum.dll`（可选附带 `.pdb`），并在控制台执行 `ll mod reload hello-ferrum` 热重载，原 DLL 文件不会被锁定。

## 下一步

- [sdk.md](./sdk.md)：API 完整参考
- [events.md](./events.md)：LL 官方事件与自定义事件
- [testing.md](./testing.md)：本地 Mock 测试与 BDS 调试
- [manifest.md](./manifest.md)：`manifest.json` 规范
