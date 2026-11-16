# 快速开始

写一个最小 Ferrum 模组并部署到 BDS。完整示例见 `examples/hello/`。

## 环境

- Rust 稳定版，目标 `x86_64-pc-windows-msvc`（最低 1.85）。
- 一个装了 LeviLamina 和 `ferrum-loader` 的 BDS，`ferrum-loader` 位于 `mods/ferrum-loader/`。
- 写模组不需要 C++ 编译器和 xmake，只需要 cargo。
- `cargo-ferrum` 脚手架尚未实现，手动创建项目。

## Cargo.toml

```toml
[package]
name = "hello-ferrum"
version = "0.1.0"
edition = "2024"

[lib]
name = "hello_ferrum"
crate-type = ["cdylib", "rlib"] # cdylib 给 BDS 加载，rlib 给 cargo test 链接

[dependencies]
ferrum-sdk = "0.1"

[dev-dependencies]
ferrum-host-mock = "0.1"

[profile.release]
panic = "unwind" # 设为 "abort" 会编译失败
```

## src/lib.rs

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

    fn on_disable(&mut self, _ctx: &ModContext, _token: &MainThreadToken<'_>) -> Result<()> {
        self.logger.info("已停用");
        Ok(())
    }

    fn on_unload(self, _ctx: &ModContext) -> Result<()> {
        self.logger.info("已卸载");
        Ok(())
    }
}
```

除 `on_load` 外，其余回调可省略。停用后宿主自动撤销本模组注册的命令和事件监听。阶段与线程说明见 [sdk.md](./sdk.md)。

## manifest.json

```json
{
    "name": "hello-ferrum",
    "entry": "hello_ferrum.dll",
    "version": "0.1.0",
    "type": "ferrum",
    "dependencies": [{ "name": "ferrum-loader", "version": "^0.1.0" }]
}
```

字段说明见 [manifest.md](./manifest.md)。

## 部署

```powershell
cargo build --release
```

把 `target\release\hello_ferrum.dll` 和 `manifest.json` 放到 `<bds>/mods/hello-ferrum/`：

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

- 启动 `bedrock_server_mod.exe`，用 `ll list` 确认模组已加载。
- 开发时覆盖 dll 后执行 `ll mod reload hello-ferrum`。宿主加载的是影子副本，原 dll 不会被锁定。

## 下一步

- [sdk.md](./sdk.md)：API 参考
- [events.md](./events.md)：玩家事件与自定义事件
- [testing.md](./testing.md)：mock 测试与调试
- [manifest.md](./manifest.md)：清单字段
