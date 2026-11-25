# 新增领域 API

每新增一个 API 都会增加后续 MC / LeviLamina 升级的 `cpp-shim` 维护成本，仅在需求明确且语义跨版本稳定时添加。

## 设计前核对

- 是否必须在主线程访问？若是，SDK 签名须要求 `&MainThreadToken<'_>` 或瞬态引用。
- 跨边界如何标识对象？使用值 ID 或 `Arena` 代数句柄，禁止暴露游戏对象指针。
- 参数与返回是否屏蔽 MC 内部细节？使用语义稳定的参数及 `#[repr(transparent)]` 码。
- 是否注册持久资源或回调？是则必须接入 `ModOwnership` 并在停用/卸载时撤销。

## 改动清单

1. **`ferrum-abi`**
   - 在现有子表末尾追加槽位，或新建带 `struct_size: u32` 首字段的子表并通过 `query_interface` 暴露。
   - 签名统一返回 `FerrumStatus`；输入用 `FfiStr` / `FfiSlice`，变长输出用 `*mut FfiByteWriter`，布尔用 `FfiBool`，回调用强类型回调结构体。
   - 递增 `crates/ferrum-abi/src/version.rs` 的 `FERRUM_ABI_MINOR`。
2. **`layout_lock`**
   - 在 `xtask/src/layout_lock.rs` 登记新类型与新增字段，运行 `cargo xtask layout-lock`（仅追加，不改旧断言）。
3. **`cbindgen`**
   - 如新增导出类型，加入 `xtask/src/cbindgen_gen.rs` 的 `export.include`，运行 `cargo xtask cbindgen` 更新 `ferrum_abi.h`。
4. **`cpp-shim`**
   - 在 `cpp-shim/src/ferrum/DomainApi.cpp`（或同目录新文件）实现并在 `installDomainApi` 挂接。
   - 标注 `noexcept` 并包裹 `guarded(...)`；按 ID 实时解析对象指针；调用带 `weak_ptr<mod::Mod>` 默认参数的 LL 接口时显式传入目标 `FerrumMod` 的 `weak_ptr`。
5. **`ferrum-host`**
   - 在 `shim.rs` 的 `FerrumShimApi` 末尾追加接口，在对应领域模块用 `vtable::guard` + 主线程检查实现，必要时在 `vtable.rs` 的 `query_interface` 注册。
   - 持久资源记入 `ModOwnership` 并在 revoke 路径清理；宿主调模组回调须用 `Invocation::begin`（或 `Active`）守卫，且回调 `Arc` 须晚于守卫声明；调模组前释放宿主锁。
6. **`ferrum-sdk`**
   - 在 `host.rs` 用 `domain_api!` 声明槽位访问器；对外提供安全包装与文档注释（注明最低 ABI）。
   - 回调使用 HRTB、`in_call` 重入守卫、`poisoned` 标记与 `firewall`。
7. **`ferrum-host-mock`**
   - 在假 shim 与 `MockHost` 中实现对应测试接口。
8. **文档与验证**
   - 同步更新 `docs/design/abi.md`、`docs/guide/sdk.md` 与 `docs/roadmap.md`。
   - 为每处 `unsafe` 添加 `// SAFETY:` 注释，并通过 [build.md](./build.md) 的 5 条纯 Rust 检查。
