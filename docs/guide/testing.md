# 测试与调试

两步：先用 `ferrum-host-mock` 写纯 Rust 测试，再放进真实 BDS 联调。

## MockHost 单元测试

`ferrum-host-mock` 使用真实 `ferrum-host` 和 Rust 假 shim。握手、生命周期、接口查询、回调派发、资源撤销与熔断均复用真实宿主；假 shim 只提供世界数据、命令解析和事件路由。创建它的线程就是"主线程"。

`Cargo.toml`：模组必须同时是 `cdylib` 和 `rlib`，否则 `tests/*.rs` 链接不到模组类型。

```toml
[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
ferrum-sdk = "0.1"

[dev-dependencies]
ferrum-host-mock = "0.1"
```

### 生命周期与日志

```rust
use ferrum_host_mock::{MockHost, MockModState};
use ferrum_sdk::Level;

#[test]
fn lifecycle() {
    let host = MockHost::new();
    let m = host.load_named::<MyMod>("my-mod", "0.1.0").unwrap(); // 调 on_load
    host.enable(&m).unwrap();
    assert_eq!(host.state(&m), Some(MockModState::Enabled));
    host.disable(&m).unwrap();
    host.enable(&m).unwrap();
    host.unload(&m).unwrap(); // 仍启用则先停用
    assert_eq!(host.state(&m), Some(MockModState::Unloaded));

    assert!(host.logs(&m).iter().any(|r| r.level == Level::Info && r.message.contains("已启用")));
    assert!(host.panics(&m).is_empty());
}
```

- `host.load::<MyMod>()`：模组名取类型名末段，版本 `0.1.0`。
- 模组目录为 `mods/<name>`（`config_dir()` 等返回 `mods/<name>/config`，不会创建真实目录）。
- `MockHost` drop 时，按加载逆序卸载仍加载的模组。
- 状态：`Loaded`、`Enabling`、`Enabled`、`Disabling`、`Disabled`、`Unloaded`。
- 重复启用已启用模组直接成功；停用 `Loaded` / `Disabled` 模组直接成功，不重复调用生命周期钩子。停用、卸载钩子的错误由真实宿主记日志并继续清理。
- 模组上下文及回调句柄使用真实宿主的代数槽位，不会因创建下一次 `MockHost` 而重置宿主句柄池。

### Panic 防火墙

```rust
use ferrum_host_mock::{MockHost, MockModState};
use ferrum_sdk::prelude::*;

struct BrokenMod;

#[ferrum_sdk::mod_entry]
impl FerrumMod for BrokenMod {
    fn on_load(_ctx: &ModContext) -> Result<Self> { Ok(Self) }
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

    let panics = host.panics(&m);
    assert_eq!(panics[0].message, "simulated failure");
    assert!(panics[0].location.contains(".rs"));

    // 回调已毒化，再次启用不会进入用户代码
    assert_eq!(host.enable(&m).unwrap_err().kind(), ErrorKind::CallbackPoisoned);
    host.unload(&m).unwrap();
}
```

`on_load` 返回 `Err` 时，`load` 返回该错误（见 `examples/hello/tests/load_error.rs`）。

### 构造游戏世界

| 方法 | 作用 |
| :--- | :--- |
| `add_player(MockPlayer { uuid, name, operator })` | 玩家上线，返回 `ActorRuntimeId`。同 UUID 会替换旧玩家 |
| `remove_player(uuid)` | 玩家下线 |
| `player_state(uuid)` / `set_player_state(uuid, MockPlayerState)` | 读写位置、维度、生命值（默认 20）、最大生命值、游戏模式。值非法返回 `InvalidArgument`，维度未注册返回 `NotFound` |
| `player_messages(uuid)` | 取玩家收到的 `send_message` 文本 |
| `add_dimension(dim)` | 注册额外维度，供传送使用 |
| `add_actor(MockActor)` / `remove_actor(id)` / `actor_state(id)` | 非玩家实体（`ActorKind::ACTOR` 或 `MOB`） |
| `register_block_type(name, id)` | 注册方块类型。默认只有 `minecraft:air`、`minecraft:stone` |
| `set_chunk_loaded(dim, x, z, loaded)` | 标记区块已加载。默认没有已加载区块 |
| `set_block(dim, pos, name)` | 预置方块。未设置的位置视为空气 |
| `execute_command(name, origin, &[(参数名, MockCommandValue)])` | 执行已注册命令，返回 `MockCommandOutput { messages: Vec<(bool, String)> }`，`bool` 为是否错误 |
| `emit_player_join(id)` / `emit_player_chat(id, msg)` / `emit_player_disconnect(id)` | 派发玩家事件，详见 [事件](./events.md) |
| `emit_official_event::<E>(subject, payload_snbt)` | 派发主线程官方事件快照 |
| `emit_sync_event::<E>(payload_snbt, message, was_cancelled)` | 在调用线程同步派发控制台或服务事件，返回取消状态 |
| `native_event_api()` | 以 native 发布者身份测试自定义事件 |

`execute_command` 的 `origin` 为 `None` 表示控制台，权限最高；为 `Some(uuid)` 时按该玩家是否 operator 检查权限。`MockCommandValue` 有 `Int`、`Float`、`Bool`、`String`、`Players`。按命令名和参数类型匹配重载，找不到返回 `NotFound`。

```rust
use ferrum_host_mock::{MockCommandValue, MockHost, MockPlayer};
use ferrum_sdk::PlayerUuid;
use hello_ferrum::HelloFerrum;

// 以 examples/hello 的 HelloFerrum 为例。
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

    // 返回值为是否有监听器取消了该事件；HelloFerrum 不取消聊天。
    let cancelled = host.emit_player_chat(id, "hi").unwrap();
    assert!(!cancelled);
}
```

### 限制

- 每个进程同时只有一个 `MockHost`。`MockHost::new()` 会等前一个被 drop，所以同进程内的测试自动串行，不用加 `--test-threads=1`。
- 世界状态与"主线程"绑定在创建 `MockHost` 的线程上，不要在其他线程调用 `host` 的方法。
- 每个模组 crate 只能有一个 `#[mod_entry]`（导出固定符号 `ferrum_mod_entry_v1`）。要测多个测试模组，每个放进独立的 `tests/*.rs`，Cargo 会把它们编译成独立的测试二进制。例：`examples/hello/tests/`。
- mock 不模拟 LL（EventBus、命令注册表、真实的模组身份）和 native C++ 监听器。涉及这些的行为要在真实 BDS 验证。

## 真实 BDS 联调

```powershell
cargo build --release
```

复制到 `<bds_root>/mods/my-mod/`：

```text
manifest.json   # "type": "ferrum", "entry": "my_mod.dll", 依赖 "ferrum-loader"
my_mod.dll      # target/release/my_mod.dll
my_mod.pdb      # 可选，调试符号
```

manifest 写法见 [manifest](./manifest.md)。

### 影子复制与重载

`ferrum-loader` 加载时，先把 `my_mod.dll` 和同名 `.pdb` 按内容哈希复制到 `mods/my-mod/.ferrum-shadow/my_mod-<hash>.dll`，再加载副本，所以不会锁定原 dll。

迭代流程：

1. BDS 保持运行。
2. `cargo build --release`，覆盖 `mods/my-mod/my_mod.dll`。
3. 在 BDS 控制台执行：

   ```text
   ll mod reload my-mod
   ```

旧镜像默认保留在内存里，不会 `FreeLibrary`，每次重载多占一份代码段。这是开发期成本，原因见 [线程与安全](../design/threading.md)。

### 调试

用 Visual Studio 或 VS Code（codelldb）附加到 BDS 进程。把 `.pdb` 放在 dll 同目录，加载器会一起复制到 `.ferrum-shadow/`，可直接在 Rust 源码里下断点。
