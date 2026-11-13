# Ferrum

Ferrum 是构建在 [LeviLamina](https://github.com/LiteLDev/LeviLamina) 之上的 Rust 二级模组加载器：
服主安装 `ferrum-loader`，模组作者只用 Cargo 就能编写 `type = "ferrum"` 的 Rust 模组，
无需 C++ 工具链、无需 BDS 头文件。

> 当前进度：M1（加载器最小闭环）。可用 API：生命周期、日志、模组元数据与目录、panic 防火墙。
> 事件、命令、玩家、调度等领域 API 按 [路线图](docs/00-overview/roadmap.md) 逐步加入。

## 写一个模组

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
ferrum-sdk = { git = "https://github.com/Yangyangzhong82/ferrum" }

[dev-dependencies]
ferrum-host-mock = { git = "https://github.com/Yangyangzhong82/ferrum" }
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

`manifest.json`（与 `.dll` 一起放进 `mods/<name>/`）：

```json
{
    "name": "my-mod",
    "entry": "my_mod.dll",
    "version": "0.1.0",
    "type": "ferrum",
    "dependencies": [{ "name": "ferrum-loader", "version": "^0.1.0" }]
}
```

完整示例见 [`examples/hello`](examples/hello)，开发指南见 [快速开始](docs/02-mod-developer-guide/quick-start.md)、[Rust SDK 开发手册](docs/02-mod-developer-guide/sdk-reference.md) 与 [测试与调试指南](docs/02-mod-developer-guide/testing-and-debugging.md)。

## 仓库结构

| 路径 | 内容 |
| :--- | :--- |
| `crates/ferrum-abi` | 纯 C ABI 契约（`#[repr(C)]` VTable） |
| `crates/ferrum-host` | 宿主运行时（Rust staticlib，链接进 `ferrum-loader.dll`） |
| `cpp-shim` | 对接 LeviLamina 的 C++ 垫片层，`FerrumModManager` |
| `crates/ferrum-sdk` / `ferrum-macros` | 模组开发 SDK 与 `#[mod_entry]` |
| `crates/ferrum-host-mock` | 无需 BDS 的假宿主，用于 `cargo test` |
| `examples/hello` | 最小示例模组与错误/panic 集成测试 |
| `xtask` | 生成 `ferrum_abi.h` 与 ABI 布局锁 |
| `docs` | 架构规范、开发者指南与维护手册 |

## 构建

```powershell
# 纯 Rust（无需 C++ 环境）
cargo build -p hello-ferrum
cargo test --workspace

# ferrum-loader.dll（需要 xmake + clang-cl）
xmake f -p windows -a x64 -m release -y --target_type=server
xmake
```

产物位于 `bin/ferrum-loader/`。详见 [源码联编指南](docs/03-maintainer-playbook/build-from-source.md)。

## License

CC0-1.0
