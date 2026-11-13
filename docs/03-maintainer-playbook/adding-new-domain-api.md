# 新增领域 API 标准流程

当社区需要暴露新的游戏能力（例如新增“计分板读写”、“Boss 血条控制”或“容器物品槽位操作”）时，任何 PR **必须自底向上完整走完以下五步清单**，并逐项通过代码审查红线。

> 每新增一个 API，都意味着今后每次 MC / LeviLamina 升级都要多维护一个 shim 函数。只有在“社区确有需求、语义跨版本稳定”时才新增。

---

## 步骤 1：领域建模与安全边界审查（Design Check）

在动手写代码前，先回答以下四个问题：
1. **线程归属**：底层 BDS 函数是否必须在主线程调用？
   - 若是（99% 的世界/实体/方块/容器操作均是），SDK 暴露必须绑定 `MainThreadToken<'_>` 或瞬态借用引用（如 `PlayerRef<'_>`）。
2. **生命周期与句柄选型**：目标对象以什么标识跨边界？返回的对象是瞬态引用还是需要跨 Tick 托管？
   - 游戏对象一律以 ID（如 `ActorRuntimeId`）或宿主代数句柄跨边界，**严禁**任何形式的 C++ 对象指针出现在 `ferrum-abi` 中。
   - 若该操作可能销毁目标对象或同步触发其他事件，确认 SDK 侧之后的调用会自然得到 `NotFound`。
3. **跨版本抗脆弱性**：参数设计是否泄漏了当前某个 MC 小版本的内部实现细节（如直接暴露内部 `TypedStorage` 偏移或不稳定位掩码）？
   - 必须抽象为领域语义稳定的参数与 `#[repr(transparent)]` 状态/类型码。
4. **所有权归属**：若该 API 会注册持久回调或后台资源，是否已接入 `ModOwnership` 以便在 `disable`/`unload` 时自动清理？

---

## 步骤 2：在 `crates/ferrum-abi` 定义 C 契约

1. **决策挂载位置**：
   - 若属于现有核心表（如给 `PlayerApiV1` 增加 `get_ping`），将新函数指针**追加在 `PlayerApiV1` 结构体的最末尾**。
   - 若属于全新的独立子系统（如 `ScoreboardApiV1`），新建独立 `#[repr(C)]` 结构体（首字段 `pub struct_size: u32`），通过 `FerrumHostApi::query_interface("ferrum.scoreboard.v1", 0)` 暴露（或在根表尾部追加并递增 `AbiVersion.minor`）。
2. **签名自检**：
   - 所有函数指针显式标注 `extern "C" fn(...) -> FerrumStatus`；
   - 字符串输入用 `FfiStr<'_>`，变长输出用 `*mut FfiByteWriter`；
   - 布尔值用 `FfiBool`，封闭 `enum` 一律改为 `#[repr(transparent)] pub struct XxxKind(pub u32);`；
   - 回调使用该类别专属的强类型回调结构体，不使用 `*const c_void` 存放函数指针。
3. **更新布局锁与 C 头文件**：
   - 在 `crates/ferrum-abi/src/version.rs` 中递增 `FERRUM_ABI_MINOR`；
   - 若新增了类型或尾追加了字段，在 `xtask/src/layout_lock.rs` 登记后运行 `cargo xtask layout-lock` 追加断言，确认旧字段偏移一字未改；
   - 运行 `cargo xtask cbindgen` 重新生成 `cpp-shim/include/ferrum/ferrum_abi.h`，与 ABI 改动一并提交。

---

## 步骤 3：在 `cpp-shim/` 实现薄 C++20 垫片

在 `cpp-shim/src/<domain>_shim.cpp` 中编写 `extern "C"` 导出给 `ferrum-host` 的内部桥接函数（类型来自 cbindgen 生成的 `ferrum/ferrum_abi.h`）：

```cpp
static FerrumStatus shimPlayerGetPing(
    FerrumActorRuntimeId target,
    int32_t*             outPingMs
) noexcept {
    if (!outPingMs) {
        return FerrumStatus_NULL_POINTER;
    }
    try {
        // 每次调用按 ID 重新解析，指针只在本函数体内存活
        auto* player = ferrum::shim::resolvePlayer(target);
        if (!player) {
            return FerrumStatus_NOT_FOUND;
        }
        // 调用 LeviLamina / BDS 稳定接口
        auto netId = player->getNetworkIdentifier();
        if (auto peer = ll::service::getRakPeer(); peer) {
            *outPingMs = peer->GetAveragePing(netId.mGuid);
            return FerrumStatus_OK;
        }
        return FerrumStatus_SERVICE_UNAVAILABLE;
    } catch (...) {
        ferrum::shim::setLastErrorFromCurrentException();
        return FerrumStatus_CPP_EXCEPTION;
    }
}
```

**Shim 审查红线**：
- 函数必须标记 `noexcept` 并由 `try / catch (...)` 兜底；
- 游戏对象指针只能在单次函数体内由 ID 解析得到，严禁缓存到静态变量或返回给 `ferrum-host`；
- 若调用了任何带 `std::weak_ptr<ll::mod::Mod> = NativeMod::current()` 默认参数的 LeviLamina 接口，**必须显式传入目标二级模组的 `weak_ptr`**！

---

## 步骤 4：在 `crates/ferrum-host` 装配 VTable 与校验

1. 在 `crates/ferrum-host/src/shim.rs` 的 `FerrumShimApi` 末尾追加对应函数指针字段（并在 `cpp-shim/src/ferrum/FerrumModManager.cpp` 的 `shimApi` 中注入实现，同步更新 `cargo xtask cbindgen`）。同时在 `crates/ferrum-host-mock` 中补充该函数的假实现，供下游模组测试使用。
2. 在 `crates/ferrum-host/src/vtable.rs` 的 VTable 实现函数中以 `guard(|| ...)` 包裹并完成：
   - 主线程断言（`vtable::is_main_thread()`）；
   - 代数句柄查表与归属校验（若涉及句柄）；
   - 通过 `shim::get()` 回调 C++ 垫片，并将错误详情存入当前线程 `LastErrorSlot`（`last_error::fail`）。
3. 将该函数挂载到静态全局子表（如 `CORE_V1` 或对应领域子表）的新槽位上。

---

### 4. 安全注释规范（`unsafe` 块）
- `ferrum-sdk` 与 `ferrum-host` 中每一处 `unsafe { ... }` 块上方**必须**附带 `// SAFETY: ...` 注释（workspace lint 已启用 `undocumented_unsafe_blocks = "deny"` 与 `unsafe_op_in_unsafe_fn = "deny"`），逐条论证指针非空性、对齐、生命周期边界、线程归属与别名独占性不变式。

---

## 步骤 5：在 `crates/ferrum-sdk` 封装安全 Rust 接口

在 `ferrum-sdk` 中隐藏所有裸指针、`FfiByteWriter` 和状态码转换细节，暴露零 `unsafe` 的地道 Rust 方法，并标注最低所需 ABI 次版本说明：

```rust
impl<'a> PlayerRef<'a> {
    /// 获取玩家当前网络平均延迟（毫秒）。
    pub fn ping_ms(&self) -> Result<i32> {
        let api = host_api().player_v1()?;
        // 若允许在更低版本宿主上优雅降级，先检查 struct_size 边界：
        api.require_field(core::mem::offset_of!(PlayerApiV1, get_ping))?;

        let mut out_ping: i32 = 0;
        // SAFETY: get_ping 槽位已由 require_field 确认存在；out_ping 位于当前栈帧且在调用期间独占。
        let status = unsafe { (api.get_ping)(self.id, &mut out_ping) };
        status.into_result_with_last_error().map(|()| out_ping)
    }
}
```

> **安全注释铁律**：`ferrum-sdk` 和 `ferrum-host` 中出现的每一处 `unsafe { ... }` 块，上方**必须**附带 `// SAFETY: ...` 注释，逐条论证指针非空性、对齐、生命周期边界、线程归属与别名独占性不变式。
