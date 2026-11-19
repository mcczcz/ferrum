# ABI 规则

范围：`crates/ferrum-abi`。已发布模组直接按这些布局访问宿主内存，违反任何一条都会导致访问违例。

## 1. 允许与禁止的类型

禁止出现在 `ferrum-abi` 的公开字段、参数、返回值中：

- 默认 `repr(Rust)` 的结构体、元组、枚举。
- `String`、`&str`、`Vec`、`&[T]`、`Box`、`Arc`、`Result`，以及 `Option<extern "C" fn>` / `Option<NonZero*>` 之外的 `Option`。
- `dyn Trait`、未标 `extern "C"` 的函数指针。
- Rust 原生 `bool` 和封闭 `enum`。C++ 写入其他取值即 UB。
- C++/LL 类型：`std::string`、`std::function`、`shared_ptr`、`ll::Expected`、`mc/*` 对象。
- 游戏对象指针（`Actor*` 等）。见 [handles.md](handles.md)。

允许：

- 定宽整数、`f32`、`f64`、`usize`。
- `FfiBool(u8)`：0 为假，非 0 为真。
- 枚举值用 `#[repr(transparent)] struct FooKind(pub u32)` 加关联常量。SDK 解码未知值时映射为 `Unknown(u32)` / `Other(u32)`，不能 `transmute`。
- `*const T` / `*mut T`（`T` 满足本节），`*mut c_void`（仅作回调 `env` / `context`）。
- `extern "C" fn` 与 `Option<extern "C" fn>`。
- `NonZeroU64` 及其 `Option`（代数句柄）。
- `#[repr(C)]` 结构体，字段均满足本节。`PhantomData` 可用作生命周期标记。

## 2. 入口与握手

模组 DLL 不导入宿主符号，只导出一个函数 `ferrum_mod_entry_v1`。后缀 `v1` 是 ABI 主版本；破坏性变更必须换新符号。

```rust
pub type ModEntryFnV1 = unsafe extern "C" fn(
    host_api: *const FerrumHostApi,
    mod_ctx: ModContextHandle,
    out_descriptor: *mut FerrumModDescriptor,
) -> FerrumStatus;
```

数据结构（定义在 `host_api.rs`、`module.rs`）：

| 结构 | 内容 |
| :--- | :--- |
| `FerrumHostApi` | `struct_size`、`reserved_flags`、`magic`（`"FERRUMAB"`）、`host_abi_version`、`core_v1`、`log_v1`、`query_interface` |
| `FerrumModDescriptor` | `struct_size`、`flags`、`required_abi_version`、`sdk_version`、`on_load`、`on_enable`、`on_disable`、`on_unload` |

- `core_v1` / `log_v1` 恒非空。领域子表（actor、player、event 等）一律通过 `query_interface` 取得。
- `flags` 目前只有 `HOT_UNLOAD_SAFE`。
- `RawMainThreadToken` 是按值传递的占位结构，只表示“此调用在主线程”。SDK 把它转成 `&MainThreadToken<'tick>`。
- `ModContextHandle` 是宿主分配的代数句柄，宿主据此识别调用方。

握手步骤：

1. 宿主加载 DLL 后查找 `ferrum_mod_entry_v1`。找不到则以 `ABI_INCOMPATIBLE` 拒绝。
2. 宿主准备描述符缓冲区（见下“描述符容量协商”），调用入口。
3. 模组入口（`#[mod_entry]` 生成，外层 `catch_unwind`）校验宿主，任一项失败返回 `ABI_INCOMPATIBLE`：
   - `host_api` 非空且 `magic == FERRUM_ABI_MAGIC`；
   - `struct_size >= FERRUM_HOST_API_MIN_SIZE_V1_0`；
   - `host_abi_version.major == FERRUM_ABI_MAJOR`；
   - `core_v1`、`log_v1` 非空，`struct_size` 不小于 `CORE_API_MIN_SIZE_V1_0` / `LOG_API_MIN_SIZE_V1_0`。
4. 模组把 `host_api` 存入全局，写描述符。
5. 宿主校验描述符，任一项失败返回 `ABI_INCOMPATIBLE`：
   - `struct_size` 在 `[FERRUM_MOD_DESCRIPTOR_MIN_SIZE_V1_0, 缓冲区容量]` 内；
   - `required.major == host.major`；
   - `required.minor <= host.minor`，否则提示升级 `ferrum-loader`。

### 描述符容量协商

描述符以后会尾追加字段。新旧宿主与新旧模组靠容量协商互通：

- 宿主把缓冲区清零，并把 `struct_size` 预填为缓冲区字节数（`size_of::<FerrumModDescriptor>()`）。
- 模组读预填值：`0` 视为 v1.0 大小（兼容不预填的旧宿主）；小于 v1.0 大小则返回 `ABI_INCOMPATIBLE`。
- 模组写 `min(预填值, 自身描述符大小)` 字节，并把实际写入量回填到 `struct_size`。
- 模组没写到的尾部字段保持为零。**新增描述符字段必须以 0 作为合法默认值。**

### `required_abi_version`

- 取模组实际需要的最低版本，默认 `1.0`。不要取编译时 SDK 的 ABI 版本，否则只用 v1.0 功能的模组会被旧宿主拒绝。
- 模组用到更高 minor 的 API 时，SDK 在调用时检查子表 `struct_size`。宿主过旧则该调用返回 `UNSUPPORTED`，模组可降级。
- 想在加载期就失败：`#[ferrum_sdk::mod_entry(min_abi = "1.3")]`。

## 3. 演进规则

适用于每个 `#[repr(C)]` 子表（`PlayerApiV1` 等）。

1. **首字段 `struct_size: u32`。** 通过指针传递、可扩展的结构（VTable、选项、载荷）首字段必须是 `struct_size`，值为 `size_of::<Self>()`。
   - 读取方访问字段前必须检查 `struct_size >= offset_of!(T, field) + size_of::<Field>()`。
   - SDK 用 `domain_api!` 生成的槽位访问器（`require_field`）：先检查边界，再用 `addr_of!` 读字段。不构造整表引用，也不因整表比 SDK 新而拒绝仍有可用槽位的旧宿主。
   - SDK 对每个接口用 `OnceLock` 缓存子表指针和 `struct_size`（包括查询失败的结果）。缺少尾部字段时，只有对应方法返回 `UNSUPPORTED`。
2. **只尾追加。** 同一主版本内，已发布字段不得删除、重排、改签名或改语义。新增函数指针只能放在结构末尾，并递增 `FERRUM_ABI_MINOR`（当前 3）。
3. **按值传递的结构冻结。** 按值或作为 `FfiSlice<T>` 元素传递的结构（`BlockPos`、`Vec3`、各种 ID）大小与对齐发布后不可变，不需要 `struct_size`。需要扩展的数据改为指针加 `struct_size`，或句柄加 getter。
4. **废弃不删除。** 槽位无法再实现时，不能删除或置空。宿主保留桩函数，返回 `UNSUPPORTED` 或安全默认值。
5. **`query_interface` 取扩展接口。** 新领域用接口标识（如 `ferrum.event.v1`）查询，避免根表膨胀。标识中的 `vN` 是该子表主版本；破坏性变更用新标识并保留旧实现。子表同样遵守第 1、2、4 条。

当前宿主提供 `ferrum.actor.v1`、`ferrum.mob.v1`、`ferrum.block.v1`、`ferrum.player.v1`、`ferrum.command.v1`、`ferrum.event.v1`、`ferrum.custom_event.v1`、`ferrum.custom_event_publisher.v1` 和 ABI 1.2 新增的 `ferrum.scheduler.v1`。宿主目前只在 `min_minor == 0` 时返回子表，其他值返回空指针。

ABI 1.3 新增 `ferrum.llmoney.v1`。SDK 的 `ferrum_sdk::llmoney` 模块通过主线程令牌访问余额、转账、交易记录和排行榜。LegacyMoney 为可选依赖，缺失或停用时调用返回 `SERVICE_UNAVAILABLE`，缺少对应导出函数时返回 `UNSUPPORTED`。

## 4. 布局锁

`crates/ferrum-abi/tests/layout_lock.rs` 锁定已登记类型的对齐和字段偏移。按值传递的类型锁定精确大小。以 `struct_size` 开头的可扩展表和配置结构以各版已发布的大小作为下限。

- 改动既有偏移会让该测试编译失败。
- 尾部追加字段时，生成器追加新字段的偏移断言。结构变大时也会追加新的大小下限，保留各版前缀的记录。
- 文件由 `cargo xtask layout-lock` 生成，只追加不改写。新类型要在 `xtask/src/layout_lock.rs` 的 `assertions()` 登记。
- CI 运行 `cargo xtask layout-lock --check`。

## 5. C 头文件

`cpp-shim/include/ferrum/ferrum_abi.h` 由 `cargo xtask cbindgen` 从 `ferrum-abi` 和 `ferrum-host/src/shim.rs` 生成。

- 不要在 C++ 侧手写镜像结构。
- 头文件纳入版本控制，CI 运行 `cargo xtask cbindgen --check` 确认未手改、未过期。
- 新增需要导出到头文件的类型，要加进 `xtask/src/cbindgen_gen.rs` 的 `export.include`。
