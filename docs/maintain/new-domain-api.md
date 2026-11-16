# 新增领域 API

每加一个 API，以后每次 MC / LeviLamina 升级都要多维护一个 shim 函数。只在需求明确、语义跨版本稳定时才加。

## 设计前先问

- 底层是否必须在主线程？世界、实体、方块、容器几乎都是。SDK 方法要收 `&MainThreadToken<'_>` 或瞬态引用（如 `PlayerRef<'_>`）。
- 对象用什么 ID 跨边界？用 `ActorRuntimeId` 之类的 ID 或宿主代数句柄。`ferrum-abi` 里不能出现游戏对象指针。
- 参数有没有泄漏某个 MC 版本的内部细节？有就换成领域语义稳定的参数和 `#[repr(transparent)]` 码。
- 会注册持久回调或后台资源吗？会就接入 `ModOwnership`，让 `disable` / `unload` 自动清理。

## 清单

### 1. ferrum-abi

- [ ] 在已有子表（如 `PlayerApiV1`）末尾追加函数指针。全新子系统则新建 `#[repr(C)]` 结构，首字段 `struct_size: u32`，并用新接口标识（如 `ferrum.scoreboard.v1`）经 `query_interface` 暴露。
- [ ] 签名：`extern "C" fn(...) -> FerrumStatus`。字符串输入用 `FfiStr<'_>`，变长输出用 `*mut FfiByteWriter`，布尔用 `FfiBool`，枚举用 `#[repr(transparent)] struct XxxKind(pub u32)`。回调用强类型回调结构，不用 `*const c_void`。
- [ ] 任何已发布子表尾追加字段，都递增 `crates/ferrum-abi/src/version.rs` 的 `FERRUM_ABI_MINOR`。

### 2. layout_lock

- [ ] 在 `xtask/src/layout_lock.rs` 里为新类型和新字段加 `lock!(...)`。
- [ ] 运行 `cargo xtask layout-lock`。只追加到 `crates/ferrum-abi/tests/layout_lock.rs`，确认旧行没有被改。

### 3. cbindgen

- [ ] 新子表要出现在 C 头里，就把类型名加到 `xtask/src/cbindgen_gen.rs` 的 `export.include`。
- [ ] 运行 `cargo xtask cbindgen`，把生成的 `cpp-shim/include/ferrum/ferrum_abi.h` 一起提交。不要手写 C++ 镜像结构。

### 4. cpp-shim

在 `cpp-shim/src/ferrum/DomainApi.cpp`（或同目录新文件）里实现，并在 `installDomainApi` 里填入 `FerrumShimApi`。

- [ ] 函数标 `noexcept`，外层包 `guarded(...)`（`try / catch (...)`，返回 `CPP_EXCEPTION` 并写 last-error）。
- [ ] 对象指针每次按 ID 重新解析，只活在本函数体内。解析不到返回 `NOT_FOUND`。不要缓存，不要返回给 `ferrum-host`。
- [ ] 调用带 `std::weak_ptr<mod::Mod> = NativeMod::current()` 默认参数的 LeviLamina 接口时，显式传入目标模组的 `weak_ptr`。
- [ ] 只有这个目录可以 `#include` `ll/api/*` 和 `mc/*`。

### 5. ferrum-host

- [ ] 在 `crates/ferrum-host/src/shim.rs` 的 `FerrumShimApi` 末尾追加字段或子表。
- [ ] 在 `crates/ferrum-host/src/` 新增或扩展领域文件（参照 `player.rs`）：用 `vtable::guard(|| ...)` 包裹，检查 `vtable::is_main_thread()`，经 `shim::get()` 调 shim，错误用 `last_error::fail` 写详情。
- [ ] 新接口标识要在 `vtable.rs` 的 `query_interface` 里挂上。
- [ ] 注册持久资源（命令、监听器等）：记入 `ModOwnership`，并在 `command::revoke` / `event::revoke` / `custom_event::revoke` 同级的 revoke 路径里撤销。
- [ ] 新的"宿主调模组回调"路径：用 `loader::Invocation::begin(ctx, entry)` 守卫计数，不要手写 `active_invocations += 1` / `-= 1`。守卫保证宿主 panic 时计数也配平，推迟的停用才能完成。回调对象（`Arc`/`Rc`）必须在守卫之后声明或重新绑定（`let cb = cb;`），panic 展开时才会先释放回调、再结束计数。自定义事件那条链用 `custom_event_core.rs` 的 `Active`。
- [ ] 调模组回调前不持有宿主锁。

### 6. ferrum-sdk

- [ ] 在 `crates/ferrum-sdk/src/host.rs` 用 `domain_api!` 登记新槽位名和函数指针类型。访问器会检查槽位偏移，不构造整表引用。槽位不存在时方法返回 `UNSUPPORTED`，旧 API 不受影响。
- [ ] 在对应模块里封装安全方法：不暴露裸指针、`FfiByteWriter`、状态码，失败用 `host::check` 或 `error_from_status` 转成 `Error`。
- [ ] 回调类 API：用 HRTB 签名，`FnMut` 跳板带 `in_call` 重入守卫和 `poisoned`，过 `firewall`。参照 `events.rs`。
- [ ] 文档注释写明最低 ABI 版本。

### 7. mock

- [ ] 在 `crates/ferrum-host-mock/src/` 实现假 shim 函数，并在 `shim.rs` 的 `FerrumShimApi` 注入表中挂上。接口查询、生命周期和回调所有权复用 `ferrum-host`，不另写一套。
- [ ] 给 `MockHost` 加测试用的 setter 和 getter。

### 8. 文档与验证

- [ ] 更新 [../design/abi.md](../design/abi.md) 的接口列表、[../guide/sdk.md](../guide/sdk.md) 和 [../roadmap.md](../roadmap.md)。
- [ ] 跑 [build.md](build.md) 里的 5 条纯 Rust 检查。
- [ ] 每个 `unsafe` 块上方写 `// SAFETY:`。workspace lint 对缺失会报错。
