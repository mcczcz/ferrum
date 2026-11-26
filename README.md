# Ferrum

Ferrum 是构建在 [LeviLamina](https://github.com/LiteLDev/LeviLamina) 之上的 Rust 二级模组加载器。服主在服务端安装 `ferrum-loader`，模组作者只需使用 Cargo 编写 `type = "ferrum"` 的 Rust 模组，无需 C++ 工具链或 BDS 头文件。

- 当前宿主 ABI：**`1.6`**
- 已支持能力：生命周期、日志与目录、panic 隔离、玩家 / 实体 / 方块 / NBT、命令、LL 官方事件（主线程与触发线程）、自定义事件（Rust / C++ 互通）、表单、LegacyMoney、主线程调度与异步等待。

## 写一个模组

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
ferrum-sdk = { git = "https://github.com/Yangyangzhong82/ferrum" }

[dev-dependencies]
ferrum-host-mock = { git = "https://github.com/Yangyangzhong82/ferrum" }

[profile.release]
panic = "unwind"
```

```rust
use ferrum_sdk::prelude::*;

pub struct MyMod {
    logger: Logger,
}

#[ferrum_sdk::mod_entry]
impl FerrumMod for MyMod {
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

将编译出的 `.dll` 与 `manifest.json` 放入 `mods/<name>/`：

```json
{
    "name": "my-mod",
    "entry": "my_mod.dll",
    "version": "0.1.0",
    "type": "ferrum",
    "dependencies": [{ "name": "ferrum-loader", "version": "^0.1.0" }]
}
```

完整示例见 [`examples/hello`](examples/hello)。

## 文档导航

- **模组开发者**：从 [快速开始](docs/guide/quick-start.md) 和 [文档索引（模组开发）](docs/README.md#模组开发者文档) 开始。
- **Loader 维护者与贡献者**：见 [文档索引（项目设计与维护）](docs/README.md#项目设计与维护文档) 与 [构建指南](docs/maintain/build.md)。

## License

CC0-1.0
