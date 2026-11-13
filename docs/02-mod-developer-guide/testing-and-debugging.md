# 模组测试与调试指南（`ferrum-host-mock` 与 BDS 联调）

在开发 Ferrum Rust 模组时，不需要每次改动都启动完整的 BDS 服务端。推荐采用**“纯 Rust 单元测试（`ferrum-host-mock`）+ BDS 影子热重载”**的两段式工作流。

---

## 1. 使用 `ferrum-host-mock` 编写纯 Rust 单元测试

`ferrum-host-mock` 是一个纯 Rust 实现的假宿主，实现了与 `ferrum-loader` 一致的 `FerrumHostApi` VTable、双向版本握手、错误回传与 panic 捕获逻辑。

### 1.1 配置 `Cargo.toml`

将 crate 类型同时设为 `cdylib` 与 `rlib`（否则集成测试 `tests/*.rs` 无法链接你的模组类型）：

```toml
[lib]
name = "my_mod"
crate-type = ["cdylib", "rlib"]

[dependencies]
ferrum-sdk = "0.1"

[dev-dependencies]
ferrum-host-mock = "0.1"
```

### 1.2 基础生命周期与日志断言

```rust
#[cfg(test)]
mod tests {
    use ferrum_host_mock::{MockHost, MockModState};
    use ferrum_sdk::Level;

    use super::MyMod;

    #[test]
    fn lifecycle_and_logs() {
        // 创建 MockHost；当前测试线程即被视为“BDS 主线程”
        let host = MockHost::new();

        // 加载模组并触发 on_load（Mock 根目录为 mods/<name>/）
        let m = host.load_named::<MyMod>("my-mod", "0.1.0").unwrap();
        assert_eq!(host.state(&m), Some(MockModState::Loaded));

        // 启用 / 停用 / 重新启用 / 卸载
        host.enable(&m).unwrap();
        assert_eq!(host.state(&m), Some(MockModState::Enabled));

        host.disable(&m).unwrap();
        assert_eq!(host.state(&m), Some(MockModState::Disabled));

        host.enable(&m).unwrap();
        host.unload(&m).unwrap();
        assert_eq!(host.state(&m), Some(MockModState::Unloaded));

        // 检查模组输出的日志
        let logs = host.logs(&m);
        assert!(logs.iter().any(|r| r.level == Level::Info && r.message.contains("已启用")));
        assert!(host.panics(&m).is_empty());
    }
}
```

也可以直接用 `host.load::<MyMod>()`（默认以类型末段名作为模组名、版本 `"0.1.0"`）。`MockHost` 在 `Drop` 时会自动按加载逆序卸载尚未卸载的模组。

### 1.3 测试错误回传与 Panic 防火墙

验证 `on_load` 失败时错误信息是否正确传回宿主，以及 `on_enable` 触发 panic 后是否进入 `Disabled` 且回调被毒化：

```rust
use ferrum_host_mock::{MockHost, MockModState};
use ferrum_sdk::prelude::*;

struct BrokenMod;

#[ferrum_sdk::mod_entry]
impl FerrumMod for BrokenMod {
    fn on_load(_ctx: &ModContext) -> Result<Self> {
        Ok(Self)
    }

    fn on_enable(&mut self, _ctx: &ModContext, _token: &MainThreadToken<'_>) -> Result<()> {
        panic!("simulated failure");
    }
}

#[test]
fn panic_is_isolated() {
    let host = MockHost::new();
    let m = host.load::<BrokenMod>().unwrap();

    let err = host.enable(&m).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::ModPanicked);
    assert_eq!(host.state(&m), Some(MockModState::Disabled));

    // 验证 panic 消息和源码位置已上报
    let panics = host.panics(&m);
    assert_eq!(panics.len(), 1);
    assert_eq!(panics[0].message, "simulated failure");
    assert!(panics[0].location.contains(".rs"));

    // 毒化后再次启用直接返回 CallbackPoisoned，不会再次进入用户代码
    let err2 = host.enable(&m).unwrap_err();
    assert_eq!(err2.kind(), ErrorKind::CallbackPoisoned);

    host.unload(&m).unwrap();
}
```

### 1.4 编写多个测试模组的组织规范

由于每个使用了 `#[ferrum_sdk::mod_entry]` 的模块都会生成固定全局符号 `ferrum_mod_entry_v1` 且使用静态 `ModState`：
- **不同模组类型必须放在不同的集成测试文件**（如 `tests/load_error.rs`、`tests/panic_firewall.rs`），因为每个集成测试文件会被 Cargo 编译为独立的测试二进制。
- `ferrum-sdk` 的宿主全局根表保存在进程级 `OnceLock` 中，因此 `MockHost::new()` 内部带互斥锁，同一进程内的测试会串行持有 `MockHost`，无需手动加 `--test-threads=1`。

---

## 2. 真实 BDS 部署与开发期热重载

### 2.1 编译与部署

```powershell
cargo build --release
```

将产物与配置文件复制到 BDS 目录：

```text
<bds_root>/mods/my-mod/
├── manifest.json     # "type": "ferrum", "entry": "my_mod.dll", 依赖 "ferrum-loader"
├── my_mod.dll        # 来自 target/release/my_mod.dll
└── my_mod.pdb        # 可选，建议一并复制以便崩溃/调试符号解析
```

### 2.2 影子复制热重载流程

`ferrum-loader` 在加载模组时，会把 `my_mod.dll`（及同名 `.pdb`）按内容哈希复制到 `mods/my-mod/.ferrum-shadow/my_mod-<hash>.dll` 后再加载，**不会锁定原始 `my_mod.dll`**。

开发迭代时：
1. 保持 BDS 运行；
2. 本地执行 `cargo build`；
3. 直接覆盖 `<bds_root>/mods/my-mod/my_mod.dll`；
4. 在 BDS 控制台输入：
   ```text
   ll mod reload my-mod
   ```
5. 旧版本句柄失效并停用卸载，新版本 `.dll` 立即生效。

> 调试技巧：若使用 VS Code `codelldb` 或 Visual Studio 附加到 `bedrock_server_mod.exe`，只要把 `.pdb` 一起复制到模组目录（`ferrum-host` 会自动把它复制到 `.ferrum-shadow/`），就可以直接在 Rust 模组源码中下断点单步调试。
