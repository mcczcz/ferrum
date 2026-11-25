# 句柄与游戏对象标识

## 1. 铁律：裸指针永不出 `cpp-shim`

`Actor*`、`Player*`、`Block*` 等游戏对象指针随时可能失效或被地址复用（例如回调内调用 `kill()`）。`ferrum-abi` 中严禁定义任何游戏指针类型；跨边界只传递**值标识**与**代数句柄**。

## 2. 三类标识与按需重解析

| 类别 | 类型示例 | ABI 表示 | 跨 Tick 保存 | 线程约束 |
| :--- | :--- | :--- | :--- | :--- |
| 瞬态引用 | SDK `ActorRef<'t>`、`MobRef<'t>`、`PlayerRef<'t>`、`BlockRef<'t>` | `ActorRuntimeId` / `DimensionId + BlockPos` | 否（受 `'t` 限制） | `!Send + !Sync`，仅主线程 |
| 代数句柄 | `ModContextHandle`、`ListenerHandle`、`CommandHandle`、`TaskHandle`、`*ScopeHandle`、`Custom*Handle`、`EventProviderHandle` | `GenerationalHandle(NonZeroU64)` | 视类型（`*ScopeHandle` 仅单次同步派发有效） | 宿主校验归属与代数 |
| 值标识 | `ActorUniqueId(i64)`、`ActorRuntimeId(u64)`、`PlayerUuid`、`DimensionId(i32)`、`BlockPos` | 纯值 | 是（`ActorRuntimeId` 限本次会话） | `Copy + Send + Sync` |

BDS 对应：`ActorUniqueId -> ActorUniqueID::rawID`，`ActorRuntimeId -> ActorRuntimeID::rawID`，`PlayerUuid -> mce::UUID`，`DimensionId -> DimensionType`（0 主世界 / 1 下界 / 2 末地）。

**重解析模式**：
- SDK 的实体/方块引用只保存 ID，不缓存游戏指针。`cpp-shim` 每次调用通过 `level->getRuntimeEntity(id)` 或区块坐标实时解析，缺失返回 `NOT_FOUND`。
- `'tick` 防止误将引用跨 Tick 保存；真正的内存安全由“每次重解析”保证。
- 异步/跨 Tick 访问模式：主线程取 `PlayerUuid` 或 `ActorUniqueId` -> 移入后台任务 -> 回到主线程后用 `world::get_player` / `world::get_actor` 重新解析。
- **作用域句柄**：`EventScopeHandle` / `CommandScopeHandle` 在单次派发前分配、返回后立即移除，跨出回调访问返回 `STALE_HANDLE`。

## 3. 代数句柄池（`Arena<T>`）

`crates/ferrum-host/src/arena.rs` 为每类资源维护独立池，句柄结构：

```rust
#[repr(transparent)]
pub struct GenerationalHandle(pub NonZeroU64); // 低 32 位 slot_index，高 32 位 generation (>= 1，回绕跳过 0)
```

- `Option<Handle>` 与 `u64` 等宽。
- 每次查表校验：槽位越界或代数不匹配返回 `STALE_HANDLE`；`owner` 与调用方 `ModContextHandle` 不一致返回 `PERMISSION_DENIED`。
- 模组停用/卸载时，宿主按 `ModOwnership` 逐一移除其句柄并触发 `drop_fn`，旧句柄全部失效。
