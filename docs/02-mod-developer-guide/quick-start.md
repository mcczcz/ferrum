# 快速开始与项目配置

本指南面向下游 Rust 模组作者，介绍如何从零搭建并构建一个运行在 Ferrum 上的二级模组。

---

## 1. 环境准备

1. **Rust 工具链**：安装最新稳定版 Rust（Windows x64 MSVC 目标：`x86_64-pc-windows-msvc`）：
   ```powershell
   rustup toolchain install stable-x86_64-pc-windows-msvc
   rustup default stable-x86_64-pc-windows-msvc
   ```
   > **注意**：编写下游 Ferrum 模组**不需要**安装 C++ 编译器（clang-cl/xmake）或下载几百 MB 的 BDS 符号/头文件库，纯 Cargo 即可秒级增量编译！
2. **测试服务端环境**：已安装 LeviLamina 的 BDS 目录，并在 `mods/ferrum-loader/` 中部署了 `ferrum-loader` 插件。
3. **脚手架工具（规划中）**：`crates/cargo-ferrum` 脚手架与打包 CLI 尚未实现，现阶段请参考 `examples/hello/` 手动创建 `Cargo.toml` 与 `manifest.json`。

---

## 2. `Cargo.toml` 标准配置

下游模组必须编译为动态链接库（`crate-type = ["cdylib"]`），且**严禁将 `panic` 策略配置为 `"abort"`**（`ferrum-sdk` 检测到 `panic = "abort"` 会直接编译失败）：

```toml
[package]
name = "hello-ferrum"
version = "0.1.0"
edition = "2024"

[lib]
name = "hello_ferrum"
crate-type = ["cdylib"]

[dependencies]
ferrum-sdk = "0.1"

[dev-dependencies]
ferrum-host-mock = "0.1" # 不启动 BDS 的单元测试假宿主

[profile.release]
lto = "thin"
codegen-units = 1
panic = "unwind" # 必须保持 unwind，确保宿主异常防火墙可拦截模组 Panic！
```

---

## 3. 最小模组代码结构（`src/lib.rs`）

通过 `#[ferrum_sdk::mod_entry]` 过程宏修饰模组结构体，SDK 会自动生成 `ferrum_mod_entry_v1` C 导出入口、ABI 版本握手与全边界 `catch_unwind` 保护：

```rust
use ferrum_sdk::prelude::*;

pub struct HelloFerrumMod {
    logger: Logger,
}

#[ferrum_sdk::mod_entry]
impl FerrumMod for HelloFerrumMod {
    /// 阶段 1：BDS 启动早期调用（Level 尚未创建）
    /// 适合读取配置文件、初始化模组内存状态与注册早期监听
    fn on_load(ctx: &ModContext) -> Result<Self> {
        let logger = ctx.logger();
        logger.info("HelloFerrum 正在加载配置...");
        Ok(Self { logger })
    }

    /// 阶段 2：服务端世界与主线程就绪（携带 MainThreadToken）
    /// （注：事件/命令/玩家等领域 API 将在 M2 引入，M1 可先使用日志与目录 API）
    fn on_enable(&mut self, ctx: &ModContext, token: &MainThreadToken<'_>) -> Result<()> {
        let _ = (ctx, token);
        self.logger.info("HelloFerrum 已启用！");
        Ok(())
    }

    /// 阶段 3：模组停用或服务器正在关服
    /// 返回后宿主会自动撤销本模组注册的监听器、命令、定时任务，无需手动清理
    fn on_disable(&mut self, _ctx: &ModContext, _token: &MainThreadToken<'_>) -> Result<()> {
        self.logger.info("HelloFerrum 已停用。");
        Ok(())
    }

    /// 阶段 4：卸载前终结（若自建了后台线程或 tokio Runtime，必须在此处等待退出）
    fn on_unload(self, _ctx: &ModContext) -> Result<()> {
        self.logger.info("HelloFerrum 已卸载。");
        Ok(())
    }
}
```

> 卸载默认**不会**释放 `.dll` 镜像（见《线程模型、并发与热卸载安全规范》§4.4）。只有当你能保证模组及其所有依赖 crate 在 `on_unload` 后不留任何线程、TLS 析构函数或系统回调时，才可写成 `#[ferrum_sdk::mod_entry(hot_unload_safe)]`。

### 单元测试（无需 BDS）

```rust
#[cfg(test)]
mod tests {
    use ferrum_host_mock::{MockHost, MockModState};

    #[test]
    fn full_lifecycle() {
        let host = MockHost::new();
        let m = host.load_named::<super::HelloFerrumMod>("hello-ferrum", "0.1.0").unwrap();
        host.enable(&m).unwrap();
        host.disable(&m).unwrap();
        host.unload(&m).unwrap();
        assert_eq!(host.state(&m), Some(MockModState::Unloaded));
        assert!(host.logs(&m).iter().any(|l| l.message.contains("已启用")));
    }
}
```

> 更多示例见 `examples/hello/src/lib.rs` 与 `examples/hello/tests/`（含错误回传与 panic 隔离测试）。

---

## 4. 部署运行目录布局

构建产物与清单文件按标准 LeviLamina 模组目录放置于 `<bds_root>/mods/hello-ferrum/`：

```text
mods/
├── ferrum-loader/
│   ├── manifest.json         # type = "native"
│   └── ferrum-loader.dll
└── hello-ferrum/
    ├── manifest.json         # type = "ferrum", entry = "hello_ferrum.dll"
    ├── hello_ferrum.dll
    ├── config/               # ctx.config_dir()
    ├── data/                 # ctx.data_dir()
    ├── lang/                 # ctx.lang_dir()
    ├── behavior_packs/       # 自动挂载行为包（可选）
    └── resource_packs/       # 自动挂载资源包（可选）
```

启动 `bedrock_server_mod.exe` 后，可在控制台使用原生的 `ll list` 验证加载。开发期热重载：直接覆盖 `mods/hello-ferrum/hello_ferrum.dll`，再执行 `ll mod reload hello-ferrum`。宿主通过影子副本加载新镜像，原文件不会被锁定。

---

## 下一步阅读

- 完整 API 与类型说明：[Rust SDK 开发手册（`ferrum-sdk` 参考）](./sdk-reference.md)
- 单元测试、错误/panic 断言与 PDB 调试：[模组测试与调试指南](./testing-and-debugging.md)
- 清单文件、依赖与打包：[清单文件与依赖管理](./manifest-and-deps.md)
