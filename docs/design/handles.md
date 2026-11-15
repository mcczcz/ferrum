# 句柄与游戏对象标识规范

## 1. 为什么严禁向模组直接暴露 C++ 对象裸指针？

在 Minecraft Bedrock 引擎中，`Actor*`、`Player*`、`BlockActor*`、`ItemStack*`、`Level*` 等底层 C++ 对象的生命周期完全由游戏引擎与区块加载系统控制：
1. **随时销毁与地址复用（ABA 悬垂风险）**：实体死亡、被清除、所在区块卸载或玩家断线重连后，原 `Actor*` 内存会立即释放，随后新生成的实体极可能分配到完全相同的内存地址。若 Rust 模组跨 Tick 持有了裸指针，解引用将造成灾难性的释放后使用（Use-After-Free）或操作错误实体。
2. **同一回调内也会失效**：即使引用被生命周期锁死在单次同步回调内，模组在回调中调用的 API（如 `kill()`、`disconnect()`、`execute_command()`）可能同步销毁该实体或触发其他事件链间接销毁它。仅靠 Rust 生命周期 `'a` **无法**防止这种回调内部的 UAF。
3. **布局随版本漂移**：裸指针一旦配合偏移访问暴露，即破坏跨版本 ABI 隔离。

因此，Ferrum 遵循**“裸指针永不出 `cpp-shim`”**原则：C++ 对象指针只在 `cpp-shim` 单次垫片函数调用内部存在，跨越 `ferrum-abi` 边界的一律是 ID 或宿主代数句柄。游戏对象引用划分为**三类语义截然不同的标识体系**，在类型层杜绝误用。

---

## 2. 三级对象引用分类体系

| 分类 | 代表类型 | 底层表示（跨 ABI） | 跨 Tick 持久化 | 线程安全性 | 适用场景 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **A. 瞬态作用域引用 (Scoped Ref)** | `ActorRef<'t>`, `PlayerRef<'t>`, `BlockSourceRef<'t>`, `EventCtx<'t>` | 实体类：`ActorRuntimeId`；事件类：事件作用域代数句柄 | **严禁**（受 `'t` 生命周期静态锁死） | `!Send + !Sync`（仅主线程） | 事件处理函数、命令执行器、单 Tick 主线程闭包内的读写 |
| **B. 托管代数句柄 (Generational Handle)** | `ListenerHandle`, `TaskHandle`, `ModContextHandle`, `OwnedNbtHandle`, `FormBuilderHandle`, `ServiceHandle` | 64 位代数槽位索引 `(generation: u32, slot_index: u32)` | 允许（直到显式销毁或模组停用） | `Send + Sync`（按类型定义） | 由 `ferrum-host` 分配并掌控生命周期的运行时资源、堆对象 |
| **C. 游戏逻辑标识 (Logical Identity)** | `ActorUniqueId(i64)`, `ActorRuntimeId(u64)`, `PlayerUuid([u8; 16])`, `DimensionId(i32)`, `BlockPos` | 纯值类型 ID（Plain Old Data） | **允许**（跨 Tick、跨异步任务、可落盘保存） | `Send + Sync + Copy + Hash + Eq` | 在 `tokio` 异步任务、定时器、全局状态表或数据库中记录目标实体/玩家 |

---

## 3. 详细设计与安全契约

### 3.1 瞬态作用域引用：每次调用按 ID 重新解析（Resolve-per-Call）

- `ferrum-abi` 层**不定义**任何 `RawActorPtr` / `RawPlayerPtr` 类型。实体类 VTable 函数统一以 `ActorRuntimeId` 作为目标参数：
  ```rust
  pub get_name: extern "C" fn(target: ActorRuntimeId, out: *mut FfiByteWriter) -> FerrumStatus,
  ```
- `cpp-shim` 在**每一次**垫片调用开头通过 `Level::getRuntimeEntity(runtimeId)`（哈希表 O(1) 查找）取得 `Actor*`，仅在本次函数体内使用，返回前丢弃。实体已不存在时返回 `FerrumStatus::NOT_FOUND`。
- `ferrum-sdk` 层将 ID 封装为绑定主线程作用域 `'t` 的零成本 Newtype：
  ```rust
  #[derive(Copy, Clone)]
  pub struct ActorRef<'t> {
      id: ActorRuntimeId,
      /// 锁定在主线程令牌作用域内，且 !Send + !Sync
      _scope: core::marker::PhantomData<(&'t (), *mut ())>,
  }
  ```
- **安全不变式（Safety Invariant）**：
  - `ActorRef<'t>` 只能通过两类途径取得：(1) 同步事件/命令回调参数（`'t` 由 HRTB 限定为当次回调，见《线程模型》§1.2）；(2) 凭 `&MainThreadToken<'t>` 从 `world` 按 `ActorUniqueId` / `PlayerUuid` 实时解析（返回 `Option<ActorRef<'t>>`）。
  - `'t` 的作用是**线程约束与防止误存**，**不是**内存安全的唯一依据。内存安全由“每次调用重新解析”保证：即使模组在回调中先 `actor.kill()` 再 `actor.name()`，后者也只会得到 `Err(NotFound)`，绝不触碰悬垂指针。
  - 实体已销毁后的方法调用返回 `Err(ferrum_sdk::Error::NotFound)`；SDK 对只读便捷方法可提供 `Option` 形式，但**严禁**在 SDK 内部缓存解析结果跨越另一次 FFI 调用。
- **性能说明**：单次 `getRuntimeEntity` 哈希查找远小于一次 FFI 往返与 C++ 业务逻辑本身的开销。若后续确有热点（如每 Tick 遍历全部实体），通过新增**批量 API**（一次 FFI 处理 N 个 ID）优化，而不是放宽裸指针规则。

#### 事件作用域句柄（`EventCtx<'t>`）
- 事件对象本身（如 `PlayerChatEvent&`）没有游戏 ID。`ferrum-host` 在派发事件前于事件专用代数槽位池中分配一个**事件作用域句柄**，派发返回后立即递增该槽位代数。
- 模组即使通过 `unsafe` 把 `EventCtx` 的原始句柄值带出回调，之后的调用也只会得到 `STALE_HANDLE`。
- 事件中涉及的实体（如 `event.player()`）仍以 `ActorRuntimeId` 形式提供，遵循上述按 ID 解析规则。对于尚未注册进 `Level` 的实体（如生成前事件），由事件句柄提供专用访问器，访问器同样在派发窗口结束后失效。

### 3.2 跨 Tick 与异步交互的标准范式（Resolve-on-Use）
当模组需要“查询数据库 50ms 后给玩家发放奖励”时，**严禁**尝试跨 `.await` 保留 `PlayerRef`（编译器会直接报错拦截）。标准范式为：

1. 在主线程事件回调中提取值类型标识：`let uuid: PlayerUuid = player.uuid()?;`（或 `let uid: ActorUniqueId = actor.unique_id()?;`）。
2. 将 `uuid` 移入异步线程完成耗时 IO。
3. 通过 `scheduler::on_main_thread(move |token| { ... })` 切回主线程。
4. 在主线程闭包内凭 `token` 重新解析：`let Some(player) = world::get_player(token, uuid) else { return; };`——若这 50ms 内玩家已退服，安全返回 `None`。

> `ActorRuntimeId` 仅在当次会话内有效，且可能被新实体复用。跨 Tick 持久保存实体请使用 `ActorUniqueId`；保存玩家请使用 `PlayerUuid`。

### 3.3 宿主代数槽位池（Generational Arena）
对于由 `ferrum-host` 托管的资源（监听器、定时任务、事件作用域句柄、服务句柄、托管复合对象如 `NbtCompound` 构建器等），统一采用 **64 位非零代数句柄**：

```rust
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GenerationalHandle(pub core::num::NonZeroU64);
```

位域编码规范：
- **低 32 位（Bits `0..32`）**：槽位数组下标 `slot_index: u32`（O(1) 连续内存随机访问，无指针追逐）。
- **高 32 位（Bits `32..64`）**：代数 `generation: u32`。初始为 `1`，每次槽位释放时 `+1`，回绕时跳过 `0`。由于 `generation` 恒非零，整个句柄恒为 `NonZeroU64`，`Option<Handle>` 享受空指针优化（NPO）。
- 句柄种类不编码进位域：每种资源使用**独立的槽位池**，SDK 层用不同 Newtype（`ListenerHandle`、`TaskHandle` 等）在编译期区分。

**校验规则（每次宿主查表）**：
1. `slot_index` 越界或 `slot.generation != handle.generation()` → `FerrumStatus::STALE_HANDLE`；
2. `slot.owner_mod_id != caller_mod_id` → `FerrumStatus::PERMISSION_DENIED`（禁止跨模组操作他人资源）。

**安全性收益**：
- 模组因逻辑 Bug 拷贝或伪造了已释放资源的旧句柄（Use-After-Close）时，宿主 O(1) 判定失效，不会访问已释放对象。
- 理论上同一槽位需被复用 2³² 次才可能发生代数碰撞；在服务器生命周期内可视为不可达。
- 当模组被停用或卸载时，宿主按 `ModOwnership` 索引将所有归属该 `ModId` 的槽位代数递增并析构底层对象，残留句柄全部失效。

---

## 4. BDS 核心标识映射细则

在 `cpp-shim` 中对接 BDS 标识的精确对应关系：
- **`ActorUniqueId(i64)`** <-> `::ActorUniqueID::rawID`（来自 `mc/legacy/ActorUniqueID.h`）。用于存档级唯一实体标识，注意 `INVALID_ID` 判空转换。
- **`ActorRuntimeId(u64)`** <-> `::ActorRuntimeID::rawID`（来自 `mc/legacy/ActorRuntimeID.h`）。当次会话内的运行时标识，也是瞬态引用的底层表示；不可落盘。
- **`PlayerUuid([u8; 16])`** <-> `::mce::UUID`（128 位网络字节序标准布局）。用于在线/离线玩家跨重连唯一追踪，可配合 `ll::service::PlayerInfo` 离线反查 XUID 与名称。
- **`DimensionId(i32)`** <-> `::DimensionType`（`0` 主世界 Overworld、`1` 下界 Nether、`2` 末地 TheEnd，以及自定义维度正整数 ID）。
