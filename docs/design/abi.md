# ABI 契约

作用域：`crates/ferrum-abi`。已发布模组按这些固定内存布局直接访问宿主，违反任一条均会引发 UB。

## 1. 跨边界类型规则

- **禁止**：`repr(Rust)` 结构体/枚举、`String`、`Vec`、`&str`、`&[T]`、`Box`、`Arc`、`Result`、除 `Option<extern "C" fn>` / `Option<NonZero*>` 外的 `Option`、`dyn Trait`、原生 `bool`、封闭 `enum`、C++/STL/MC 对象、任何游戏对象指针（见 [handles.md](handles.md)）。
- **允许**：定宽整数、`f32`/`f64`/`usize`、`FfiBool(u8)`（0 为假，非 0 为真）、`#[repr(transparent)] struct FooKind(pub u32)`（SDK 必须兼容未知值）、`*const T`/`*mut T`、`*mut c_void`（仅作回调 `env`/`context`）、`extern "C" fn`、`NonZeroU64`（句柄）、以及全字段满足本节的 `#[repr(C)]` 结构体。

## 2. 入口与握手

模组 DLL 仅导出 `ferrum_mod_entry_v1`（主版本升级须更换符号名）：

```rust
pub type ModEntryFnV1 = unsafe extern "C" fn(
    host_api: *const FerrumHostApi,
    mod_ctx: ModContextHandle,
    out_descriptor: *mut FerrumModDescriptor,
) -> FerrumStatus;
```

1. 宿主预清零描述符缓冲区，并将 `out_descriptor.struct_size` 预填为 `size_of::<FerrumModDescriptor>()`。
2. 模组入口校验 `host_api`（非空、`magic == "FERRUMAB"`、`struct_size >= 48`、`major == 1`、`core_v1`/`log_v1` 非空且满足 v1.0 最小尺寸），失败返回 `ABI_INCOMPATIBLE`。
3. **描述符容量协商**：模组读取预填容量（`0` 视为 `56`，小于 `56` 返回 `ABI_INCOMPATIBLE`），仅写入 `min(容量, 自身大小)` 字节并回填实际写入量到 `struct_size`。**描述符新增尾部字段必须以零值为合法默认**。
4. 宿主校验回填的 `struct_size` 合法，且 `required_abi_version.major == host.major && required_abi_version.minor <= host.minor`。
5. `required_abi_version` 默认为 `1.0`（按模组实际最低需要或 `#[mod_entry(min_abi = "...")]` 指定），不可直接设为编译期 SDK 版本。

## 3. 演进规则与当前接口

1. **首字段 `struct_size: u32`**：所有可扩展 VTable 与配置结构首字段必须是 `struct_size`。读取方访问字段前必须校验 `struct_size >= offset_of!(T, field) + size_of::<Field>()`（SDK 通过 `host.rs` 中的 `domain_api!` 宏用 `addr_of!` 按槽位读取，不构造整表引用）。
2. **只尾追加**：同一主版本内禁止删除、重排或修改既有字段签名/语义；新增字段只能追加到末尾并递增 `FERRUM_ABI_MINOR`（当前 **`5`**）。废弃槽位保留桩函数返回 `UNSUPPORTED`。
3. **按值结构冻结**：按值或 `FfiSlice<T>` 元素传递的结构（`BlockPos`、`Vec3`、`PlayerSnapshot`、各类 ID）发布后布局永久冻结。
4. **`query_interface` 扩展子表**：新领域通过接口 ID 查询，目前 `min_minor` 仅接受 `0`：

| 版本 | 根表 / 子表（接口 ID） | 能力 |
| :--- | :--- | :--- |
| 1.0 | `FerrumHostApi`、`CoreApiV1`、`LogApiV1` | 生命周期、last-error、panic 上报、元数据与目录、日志 |
| 1.1 | `ferrum.actor.v1`、`ferrum.mob.v1`、`ferrum.block.v1`、`ferrum.player.v1`、`ferrum.command.v1`、`ferrum.event.v1`、`ferrum.custom_event.v1`、`ferrum.custom_event_publisher.v1`（及 `NativeEventApiV1`） | 实体/生物/方块/玩家、命令、玩家基础事件、跨语言自定义事件 |
| 1.2 | `ferrum.scheduler.v1` + `Actor/Mob/Player/EventApiV1` 尾追加 | 主线程调度与异步等待、扩展实体/玩家属性、全部 LL 官方事件与 `subscribe_sync` |
| 1.3 | `ferrum.form.v1`、`ferrum.llmoney.v1` | 表单（JSON 借用与一次性回调）、LegacyMoney 经济接口 |
| 1.4 | `PlayerApiV1::snapshot`、`EventSubscriptionV1::include_payload` | 玩家原子快照、轻量事件订阅 |
| 1.5 | `ActorApiV1::nbt`、`BlockApiV1::{nbt, block_entity_nbt}` | 实体与方块二进制 NBT 快照 |

## 4. 布局锁与 C 头文件

- `crates/ferrum-abi/tests/layout_lock.rs` 由 `cargo xtask layout-lock` 追加生成，锁定已有类型对齐、精确大小/大小下限与字段偏移，禁止手改。新增类型或尾追加字段须先在 `xtask/src/layout_lock.rs` 登记。
- `cpp-shim/include/ferrum/ferrum_abi.h` 由 `cargo xtask cbindgen` 生成，禁止手写 C++ 镜像结构。CI 强制校验 `layout-lock --check` 与 `cbindgen --check`。
