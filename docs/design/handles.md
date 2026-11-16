# 句柄与游戏对象标识

## 1. 原则：C++ 对象指针不出 `cpp-shim`

`Actor*`、`Player*` 等对象随时可能销毁，地址还会被新实体复用。即使在同一次回调里，模组调用 `kill()` 之类的 API 也能让对象立刻失效。仅靠 Rust 生命周期挡不住这种 UAF。

所以跨 ABI 的只有两种东西：值类型 ID，和宿主发放的代数句柄。`ferrum-abi` 不定义任何 `RawActorPtr` 类型。

## 2. 三类标识

| 类别 | 类型 | ABI 表示 | 可跨 Tick 保存 | 线程 |
| :--- | :--- | :--- | :--- | :--- |
| 瞬态引用 | SDK 的 `ActorRef<'tick>`、`PlayerRef<'tick>`、`MobRef<'tick>`、`BlockRef<'tick>` | `ActorRuntimeId`；方块用 `DimensionId` + `BlockPos` | 否，被 `'tick` 限制 | `!Send + !Sync`，仅主线程 |
| 代数句柄 | `ModContextHandle`、`ListenerHandle`、`EventScopeHandle`、`CommandHandle`、`CommandScopeHandle`、`CustomListenerHandle`、`EventProviderHandle`、`CustomEventHandle` | `GenerationalHandle`（`NonZeroU64`） | 视类型而定；`*ScopeHandle` 只在一次同步派发内有效 | 由宿主校验 |
| 值标识 | `ActorUniqueId(i64)`、`ActorRuntimeId(u64)`、`PlayerUuid`、`DimensionId(i32)`、`BlockPos` | 纯值 | 是（`ActorRuntimeId` 除外，见下） | `Copy + Send + Sync` |

- `ActorRuntimeId` 只在本次服务器会话内有效，可能被新实体复用，不能落盘。
- 跨 Tick 保存实体用 `ActorUniqueId`，保存玩家用 `PlayerUuid`。

BDS 对应关系（`cpp-shim/src/ferrum/DomainApi.cpp`）：

| Ferrum | BDS |
| :--- | :--- |
| `ActorUniqueId` | `ActorUniqueID::rawID` |
| `ActorRuntimeId` | `ActorRuntimeID::rawID` |
| `PlayerUuid` | `mce::UUID` |
| `DimensionId` | `DimensionType`（0 主世界、1 下界、2 末地） |

## 3. 瞬态引用：每次调用重新解析

- 实体类 VTable 函数以 `ActorRuntimeId` 为目标，例如 `type_name(ActorRuntimeId, *mut FfiByteWriter)`。
- `cpp-shim` 每次调用开头用 `level->getRuntimeEntity(id)` 取指针，只在本次函数体内使用。实体不存在返回 `NOT_FOUND`。
- SDK 的 `ActorRef<'tick>` 只保存 ID，不缓存任何解析结果。模组先 `kill()` 再读 `name()`，得到 `Err(NotFound)`，不会碰悬垂指针。
- `'tick` 的作用是限制线程、防止误存。内存安全靠“每次重新解析”。
- 取得 `ActorRef` 的途径只有两种：同步事件或命令回调的参数；持 `&MainThreadToken` 调用 `world::get_actor`、`get_player` 等按 ID 解析。
- 热点（如每 Tick 遍历全部实体）通过新增批量 API 解决，不通过放宽指针规则。

### 事件与命令作用域句柄

事件对象（如聊天事件）没有游戏 ID。`ferrum-host` 在派发前分配一个 `EventScopeHandle`，派发返回后立刻移除。命令的 `CommandScopeHandle` 同理。

模组即使用 `unsafe` 把句柄值带出回调，之后的调用只会得到 `STALE_HANDLE`。对事件或命令作用域的每次访问还会校验调用模组和主线程。

### 跨 Tick / 异步的做法

不能跨 `.await` 保存 `PlayerRef`（编译器拒绝）。做法：

1. 主线程回调中取值标识：`player.uuid()`。
2. 把它移到异步任务里做 IO。
3. 回到主线程。
4. 持 token 重新解析：`world::get_player(token, uuid)`，玩家已退服则得到 `None`。

回到主线程的调度 API（`on_main_thread` 等）尚未实现，规划中。

## 4. 代数句柄池

宿主托管的资源（监听器、命令、作用域、模组上下文等）使用 64 位非零句柄：

```rust
#[repr(transparent)]
pub struct GenerationalHandle(pub NonZeroU64);
```

- 低 32 位是 `slot_index`，高 32 位是 `generation`。
- `generation` 从 1 开始，槽位释放时加 1，回绕时跳过 0。因此句柄恒非零，`Option<Handle>` 与 `u64` 等宽。
- 句柄种类不编码在位里。每种资源一个独立的池（`ferrum-host/src/arena.rs` 的 `Arena<T>`），SDK 用不同 newtype 区分。
- 宿主每次查表：
  - `slot_index` 越界，或代数不匹配：`STALE_HANDLE`。
  - 句柄的 `owner` 不是调用方 `ModContextHandle`：`PERMISSION_DENIED`。
- 模组停用或卸载时，宿主按 `ModOwnership` 里记录的句柄逐个移除并析构，残留句柄全部失效。

规划中：服务句柄、NBT / 表单构建器句柄、定时任务句柄。
