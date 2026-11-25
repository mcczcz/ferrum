# 路线图

当前宿主 ABI 版本：**`1.5`**。

| 里程碑 | 内容摘要 | 状态 |
| :--- | :--- | :--- |
| M0 | 仓库骨架、`cpp-shim`、`cbindgen` 与 `layout-lock` 门禁 | 完成 |
| M1 | 核心 ABI 1.0、影子加载、双向握手、Panic 防火墙与熔断 | 已实现，待真实 BDS 验收 |
| M2 | ABI 1.1：回调基础设施、玩家/实体/生物/方块、命令、玩家基础事件、自定义事件、`MockHost` | 已实现，待真实 BDS 验收 |
| M3 | ABI 1.2：`SchedulerApiV1`、`on_main_thread().await`、全部 LL 官方事件与同步订阅 | 已实现，待 BDS 重载压测 |
| M4 | ABI 1.3–1.5：表单、LegacyMoney、`PlayerSnapshot`、`subscribe_light`、实体/方块 NBT | 表单/经济/NBT 已完成；跨模组服务与派生宏待实现 |
| M5 | `HOT_UNLOAD_SAFE` 真正热卸载（`FreeLibrary`，可选） | 未开始 |

## 待办事项

### M1–M3 实机验收
- [ ] 真实 BDS：`examples/hello` 完整走完 load / enable / disable / reload / 关服，以及 `on_enable` panic 不崩服
- [ ] 真实 BDS：回调内注销自身、回调内停用自身、玩家离线后访问句柄均安全返回预期错误
- [ ] 发布首个二进制兼容基准模组（`compat_test_mod_v1_0.dll`）
- [ ] 异步运行时（如 Tokio）示例模组连续 `ll mod reload` 100 次压测无崩溃与悬挂 future

### M4 后续扩展
- [ ] 跨模组服务：`ServiceHandle` 与 `ServiceGuard`（提供方停用时撤销并等待释放）
- [ ] 命令派生宏 `#[derive(CommandArgs)]` 与更多参数类型（`BlockPos`、`Vec3`、`Actor`、`Enum`）
- [ ] `cargo-ferrum` 脚手架与打包 CLI
- [ ] 按需扩展领域子表（计分板、容器、实体属性等，流程见 [新增领域 API](./maintain/new-domain-api.md)）

### M5（可选）
- [ ] `HOT_UNLOAD_SAFE` 排空验证后执行 `FreeLibrary`
