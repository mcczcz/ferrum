# 测试与调试

先用 `ferrum-host-mock` 在本地运行纯 Rust 单元测试，再部署到真实 BDS 联调。`Cargo.toml` 配置见 [快速开始](./quick-start.md)。

## MockHost 单元测试

`MockHost` 模拟了生命周期、主线程检查、世界状态、命令、事件、调度器与 Panic 防火墙。创建 `MockHost` 的线程即测试期间的"主线程"。

### 生命周期与日志

```rust
use ferrum_host_mock::{MockHost, MockModState};
use ferrum_sdk::Level;

#[test]
fn lifecycle() {
    let host = MockHost::new();
    let m = host.load_named::<MyMod>("my-mod", "0.1.0").unwrap();
    host.enable(&m).unwrap();
    assert_eq!(host.state(&m), Some(MockModState::Enabled));

    host.disable(&m).unwrap();
    host.enable(&m).unwrap();
    host.unload(&m).unwrap();
    assert_eq!(host.state(&m), Some(MockModState::Unloaded));

    assert!(host.logs(&m).iter().any(|r| r.level == Level::Info));
    assert!(host.panics(&m).is_empty());
}
```

- `host.load::<MyMod>()` 默认使用类型名末段和版本 `"0.1.0"`。
- `MockHost` 析构时会自动按逆序卸载剩余模组。

### 模拟游戏世界与交互

| 方法 | 作用 |
| :--- | :--- |
| `add_player(MockPlayer { uuid, name, operator })` / `remove_player(uuid)` | 玩家上下线，返回 `ActorRuntimeId` |
| `player_state(uuid)` / `set_player_state(uuid, MockPlayerState)` | 读写玩家位置、维度、生命值、游戏模式 |
| `player_messages(uuid)` | 读取玩家收到的聊天消息 |
| `add_dimension(dim)` | 注册额外维度 |
| `add_actor(MockActor)` / `remove_actor(id)` / `actor_state(id)` | 非玩家实体 |
| `register_block_type(name, id)` / `set_chunk_loaded(dim, x, z, loaded)` / `set_block(dim, pos, name)` | 配置方块类型、已加载区块与方块 |
| `execute_command(name, origin, &[(参数名, MockCommandValue)])` | 执行已注册命令（`origin = None` 表示控制台） |
| `emit_player_join(id)` / `emit_player_chat(id, msg)` / `emit_player_disconnect(id)` | 派发玩家事件，返回是否被取消 |
| `emit_official_event::<E>(subject, payload_snbt)` / `emit_sync_event::<E>(...)` | 派发 LL 官方事件或同步事件 |
| `tick()` | 手动推进一个服务端 Tick，触发到期调度任务 |
| `native_event_api()` | 以 C++ Native 发布者身份测试自定义事件 |

```rust
use ferrum_host_mock::{MockCommandValue, MockHost, MockPlayer};
use ferrum_sdk::PlayerUuid;
use hello_ferrum::HelloFerrum;

#[test]
fn hello_command_and_chat() {
    let host = MockHost::new();
    let m = host.load::<HelloFerrum>().unwrap();
    host.enable(&m).unwrap();

    let uuid = PlayerUuid { bytes: [1; 16] };
    let id = host.add_player(MockPlayer { uuid, name: "Steve".into(), operator: false });

    let out = host
        .execute_command("ferrum-hello", Some(uuid), &[("target", MockCommandValue::Players(vec![uuid]))])
        .unwrap();
    assert!(out.messages.iter().any(|(is_error, _)| !is_error));
    assert_eq!(host.player_messages(uuid), ["你好，Steve！"]);
    assert!(!host.emit_player_chat(id, "hi").unwrap());
}
```

### 限制

- 同进程内同时只能存在一个活跃 `MockHost`（`MockHost::new()` 会自动等待上一个释放，无需加 `--test-threads=1`）。
- 不要在创建 `MockHost` 之外的线程调用 `host` 方法。
- 每个测试 crate 只能有一个 `#[mod_entry]`。如需测试多个入口（如错误/panic 场景），请像 [`examples/hello/tests/`](../../examples/hello/tests/) 一样拆分成独立的集成测试文件。
- `MockHost` 不模拟真实 C++ Native 模组监听器与 LeviLamina 注册表，跨模组联调仍需在真实 BDS 验证。

## 真实 BDS 调试

1. 编译：`cargo build --release`，将 `<name>.dll`（及可选 `<name>.pdb`）与 `manifest.json` 放入 `<bds>/mods/<name>/`。
2. 热重载：覆盖 `<name>.dll` 后在控制台执行 `ll mod reload <name>`。加载器通过影子副本加载，原 DLL 不会被文件锁占用。
3. 断点调试：使用 Visual Studio 或 VS Code（`codelldb`）附加到 `bedrock_server_mod.exe`。将 `.pdb` 放在 DLL 同目录，加载器会一并复制，即可直接在 Rust 源码中下断点。
