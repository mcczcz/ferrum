# ABI 稳定性与 VTable 演进铁律

本文档定义 `crates/ferrum-abi` 的二进制接口规范。**本规范中的每一条规则均为不可妥协的铁律**——任何破坏性修改都会导致已发布的下游 Rust 模组在新版 `ferrum-loader` 上直接发生内存访问违例（Access Violation）。

---

## 1. 跨 DLL 边界类型白名单与黑名单

### 1.1 严禁跨越边界的类型（Blacklisted Types）
以下类型内存布局不受稳定保证、取值域不受接收方控制，或隐含特定分配器契约，**严禁**出现在 `ferrum-abi` 的任何公开结构体字段或函数指针参数/返回值中：
- 任何默认 `repr(Rust)` 的结构体、元组或枚举。
- Rust 标准库与生态容器：`String`、`&str`、`Vec<T>`、`&[T]`、`Box<T>`、`Arc<T>`、`Result<T, E>`、除 `Option<extern "C" fn>` / `Option<NonZero*>` 以外的任何 `Option<T>`。
- 胖指针与动态分发对象：`&dyn Trait`、`*const dyn Trait`、`fn(...)`（未标注 `extern "C"` 的默认 Rust 调用约定函数指针）。
- Rust 原生 `bool` 与封闭 `enum`：C++ 侧写入的非 `0`/`1` 字节或未知判别值会在 Rust 侧立即构成 UB。
- 任何 C++ 标准库或 LeviLamina 类型：`std::string`、`std::string_view`、`std::function`、`std::shared_ptr`、`ll::Expected`、`mc/*` 类对象。
- 任何指向游戏对象的指针（`Actor*`、`Player*` 及其不透明包装）。游戏对象一律以 ID 或宿主代数句柄跨边界，见《句柄与游戏对象标识规范》。

### 1.2 允许跨越边界的原语（Whitelisted Primitives）
`ferrum-abi` 仅允许由以下确定性布局类型组合而成：
- 固定位宽数值：`u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, `usize`, `isize`, `f32`, `f64`。
- 布尔值统一使用 `FfiBool`：
  ```rust
  #[repr(transparent)]
  #[derive(Copy, Clone, PartialEq, Eq, Debug)]
  pub struct FfiBool(pub u8); // 0 = false，任何非 0 值 = true
  ```
- 枚举/种类值统一使用 `#[repr(transparent)] pub struct FooKind(pub u32);` 搭配关联常量。SDK 解码时**必须**把未知值映射为 `Unknown(u32)` / `Other(u32)`，不得 `transmute` 为封闭 Rust `enum`。
- 裸数据指针：`*const T` / `*mut T`（`T` 为本节允许的类型）、`*mut c_void`（仅用于回调上下文 `env`/`context`）。
- 显式声明调用约定的函数指针：`extern "C" fn(...) -> ...` 及 `Option<extern "C" fn(...) -> ...>`（空指针优化 NPO，保证与 C 函数指针等宽）。
- `NonZeroU64` 等 `NonZero*` 及其 `Option`（用于代数句柄）。
- 标注了 `#[repr(C)]` 的结构体，且字段均满足本节约束；`PhantomData` 零大小字段允许出现（用于 `FfiStr<'a>` 等生命周期标记）。

---

## 2. 单点注入入口与双向版本握手协议

下游模组 `.dll` **不导入任何宿主 DLL 符号**（零导入表耦合），唯一导出一个固定名称的 C 函数入口 `ferrum_mod_entry_v1`（后缀 `v1` 即 ABI 主版本号）：

```rust
pub const FERRUM_ABI_MAGIC: u64 = 0x4645_5252_554D_4142; // "FERRUMAB"

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AbiVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u32,
}

/// 宿主在加载模组时传入的全局根表（进程级静态生命周期 'static）
#[repr(C)]
pub struct FerrumHostApi {
    pub struct_size: u32,
    pub reserved_flags: u32,
    pub magic: u64,
    pub host_abi_version: AbiVersion,

    // v1.0 核心基础子表（指针恒非空）
    pub core_v1: *const CoreApiV1,           // last-error、panic 上报、模组元数据与目录、线程判定
    pub log_v1: *const LogApiV1,

    /// 按接口标识与最小次版本动态查询扩展子表（见 §3 第 5 条）；未找到返回空指针
    pub query_interface: extern "C" fn(
        interface_id_utf8: FfiStr<'_>,
        min_minor: u16,
    ) -> *const core::ffi::c_void,
    // 后续 minor 版本可在末尾追加领域子表（或统一通过 query_interface 暴露）
}

#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct FerrumModFlags(pub u32);

impl FerrumModFlags {
    pub const NONE: Self = Self(0);
    /// 模组声明自身可被 FreeLibrary（见《线程模型、并发与热卸载安全规范》§4.4）
    pub const HOT_UNLOAD_SAFE: Self = Self(1 << 0);
}

/// 模组向宿主回填的生命周期回调描述符
#[repr(C)]
pub struct FerrumModDescriptor {
    pub struct_size: u32,
    pub flags: FerrumModFlags,
    /// 模组运行所需的最低宿主 ABI 版本（见“握手校验流程”第 3 步）
    pub required_abi_version: AbiVersion,
    /// 编译该模组所用的 ferrum-sdk 版本，仅用于诊断日志
    pub sdk_version: AbiVersion,
    pub on_load: extern "C" fn(mod_ctx: ModContextHandle) -> FerrumStatus,
    pub on_enable: Option<extern "C" fn(mod_ctx: ModContextHandle, token: RawMainThreadToken) -> FerrumStatus>,
    pub on_disable: Option<extern "C" fn(mod_ctx: ModContextHandle, token: RawMainThreadToken) -> FerrumStatus>,
    pub on_unload: Option<extern "C" fn(mod_ctx: ModContextHandle) -> FerrumStatus>,
}

pub type ModEntryFnV1 = unsafe extern "C" fn(
    host_api: *const FerrumHostApi,
    mod_ctx: ModContextHandle,
    out_descriptor: *mut FerrumModDescriptor,
) -> FerrumStatus;
```

- `ModContextHandle` 是宿主代数句柄，宿主据此识别调用方模组，并在每次调用时校验其归属。
- `RawMainThreadToken` 为 ABI 层零大小占位（`#[repr(C)] struct RawMainThreadToken { _reserved: u8 }` 按值传递），仅作为“此调用发生在主线程”的显式证明；SDK 在跳板中将其转换为 `&MainThreadToken<'tick>`。

### 握手校验流程（Handshake Rules）
1. `ferrum-host` 加载模组 `.dll` 后查找符号 `ferrum_mod_entry_v1`；若不存在则拒绝加载并给出清晰诊断。
2. 模组入口（由 `#[ferrum_sdk::mod_entry]` 自动生成，整体包裹 `catch_unwind`）验证宿主：
   - `!host_api.is_null()` 且 `(*host_api).magic == FERRUM_ABI_MAGIC`；
   - `(*host_api).struct_size >= FERRUM_HOST_API_MIN_SIZE_V1_0`；
   - `host.major == FERRUM_ABI_MAJOR`；
   - `core_v1` / `log_v1` 非空且各自 `struct_size` 满足 `CORE_API_MIN_SIZE_V1_0` / `LOG_API_MIN_SIZE_V1_0`。
   校验失败返回 `FerrumStatus::ABI_INCOMPATIBLE`。
3. 宿主验证模组回填的描述符：
   - `descriptor.struct_size >= FERRUM_MOD_DESCRIPTOR_MIN_SIZE_V1_0`；
   - `required.major == host.major`，否则拒绝加载（不兼容代际）；
   - `required.minor <= host.minor`，否则拒绝加载并提示“该模组需要 ferrum-loader ABI ≥ 1.x，请升级 ferrum-loader”。
4. 校验通过后，SDK 将 `host_api` 存入模组内部的 `OnceLock<&'static FerrumHostApi>`。

### `required_abi_version` 的取值规则（按需而非按 SDK 版本）
- **严禁**把 `required_abi_version` 直接设为编译时 SDK 所对应的 ABI 版本。否则用新 SDK 编译、实际只用到 `v1.0` 函数的模组，会被旧宿主无谓拒绝。
- 默认值为 `ferrum-sdk` 核心功能（生命周期、日志、错误上报）所需的最低 minor，通常为 `1.0`。
- 模组使用了更高 minor 才引入的 API 时，SDK 在**调用时**检查对应子表 `struct_size`，宿主过旧则返回 `Err(Unsupported)`，模组可自行降级。
- 若模组希望在加载阶段就快速失败，可显式声明：`#[ferrum_sdk::mod_entry(min_abi = "1.3")]`。

---

## 3. VTable 演进五大铁律（Evolution Laws）

每一个领域子表（如 `PlayerApiV1`、`WorldApiV1`）都必须严格遵守以下演进规范：

1. **首字段 `struct_size: u32` 强制原则**
   - 所有可扩展的公开 `#[repr(C)]` 结构体（VTable、通过指针传递的配置项与事件载荷）第一个字段**必须**是 `pub struct_size: u32`（值为 `size_of::<Self>() as u32`）。
   - 读取方在访问 `v1.x` 新增的尾部字段前，必须检查 `table.struct_size as usize >= offset_of!(Table, new_field) + size_of_val(&table.new_field)`。SDK 统一封装为 `api.require_field(offset_of!(..))`。
   - 按值传递的冻结结构体（见第 3 条）不需要 `struct_size`。
2. **只增不改、只尾追加（Append-Only）**
   - 在同一个主版本（如 `major = 1`）生命周期内，已发布子表中的任何既有字段：**严禁删除、严禁重排顺序、严禁修改函数签名或参数语义**。
   - 新增 API 函数指针只能追加在对应 VTable 结构体的**最末尾**，同时递增 `AbiVersion.minor`。
3. **结构体按值传参冻结原则**
   - 若某个 `#[repr(C)]` 数据结构体以**按值（by-value）**或**数组切片元素（`FfiSlice<Item>`）**传递（如 `BlockPos`、`Vec3`、各类 ID），其总大小与对齐在首次发布后**永久冻结**（不可尾追加字段，否则数组步长改变会破坏指针算术）。
   - 需要未来扩展字段的参数/事件数据，**必须通过指针传递（`*const OptionsV1`）配合首字段 `struct_size`**，或通过句柄 + Getter 函数暴露。
4. **安全废弃机制（Deprecation without Removal）**
   - 若某个旧 VTable 槽位因游戏大改无法再实现，**严禁删除该指针槽位或将其置为空指针**（旧模组调用会导致空指针解引用崩溃）。
   - 宿主必须保留一个合法的桩函数（Stub Function），在被调用时记录一次限频警告日志并返回 `FerrumStatus::UNSUPPORTED` 或安全默认值。
5. **扩展接口动态查询（`query_interface`）**
   - 对于实验性功能、平台特定扩展或全新的独立领域模块（如计分板 `ferrum.scoreboard.v1`、粒子系统 `ferrum.particle.v1`），优先通过 `FerrumHostApi::query_interface` 按 ASCII 接口标识查询，避免根表无节制膨胀。
   - 返回的子表同样遵守本节第 1、2、4 条；接口标识中的 `vN` 即该子表的主版本，破坏性变更必须使用新标识（如 `ferrum.scoreboard.v2`）并保留旧标识的实现。

---

## 4. 编译期静态断言与 CI 门禁

在 `crates/ferrum-abi/tests/layout_lock.rs` 中，必须对所有已发布的 VTable 和固定大小结构体建立**编译期大小与字段偏移锁**：

```rust
const _: () = {
    assert!(core::mem::size_of::<AbiVersion>() == 8);
    assert!(core::mem::align_of::<AbiVersion>() == 4);
    assert!(core::mem::offset_of!(FerrumHostApi, magic) == 8);
};
```

- 任何 PR 若修改了既有字段偏移量，静态断言直接编译失败。
- 每次递增 `minor` 版本时，仅允许增加总 `size_of` 并追加新的 `offset_of!` 断言，绝不允许修改已有行的断言数值。
- 断言文件**应当**由 `cargo xtask layout-lock` 根据 `ferrum-abi` 自动追加生成，生成器只追加、从不改写既有行；CI 校验工作区中的文件与生成结果一致。

---

## 5. C/C++ 头文件单一事实来源（cbindgen）

`cpp-shim` 需要与 Rust 共享 `FerrumStatus`、`FfiStr`、`FfiByteWriter`、`FfiCallback`、各类 ID 与 `ferrum-host`↔`cpp-shim` 内部函数签名。**严禁在 C++ 侧手写镜像结构体**：

- 由 `cargo xtask cbindgen` 从 `crates/ferrum-abi` 与 `crates/ferrum-host/src/shim.rs` 生成 `cpp-shim/include/ferrum/ferrum_abi.h`；
- 生成的头文件纳入版本控制，通过 `cargo xtask cbindgen --check`（以及 `cargo xtask layout-lock --check`）校验未被手改、未过期；
- `cpp-shim` 中可额外使用 `static_assert(sizeof(...) == N)` 交叉校验关键结构体大小，数值与 `layout_lock.rs` 保持一致。
