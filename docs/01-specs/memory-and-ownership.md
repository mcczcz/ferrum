# 跨边界内存与所有权规范

## 1. 双堆物理隔离架构（Dual-Heap Isolation）

在 Windows x64 + BDS 环境下，进程内同时存在两套完全独立的内存分配体系：

1. **宿主堆（Host / Bedrock Unified Heap）**：
   - LeviLamina 要求所有一级 `native` 模组链接统一内存分配器操作符并导出 `ll_memory_operator_overrided`（对接 `ll::memory::getDefaultAllocator()` / `Bedrock::Memory::IMemoryAllocator`），以便 C++ STL 容器在 `bedrock_server.exe`、`LeviLamina.dll` 与 `ferrum-loader.dll`（`cpp-shim`）之间安全传递。
   - `ferrum-loader.dll` 的 C++ 翻译单元启用该机制；`ferrum-host`（Rust 静态库）使用自身分配器，且绝不把 Rust `Box`/`Vec` 裸指针直接交给 C++ `delete`，反之亦然。
2. **模组独立堆（Per-Mod Rust Heap）**：
   - 每个下游 `type: "ferrum"` 的 Rust `.dll` 都是独立的 `cdylib`，使用各自的 Rust 全局分配器（默认系统 HeapAlloc 或模组自行配置的 `mimalloc`）。
   - 下游模组**不链接** LeviLamina 分配器，也**不导出** `ll_memory_operator_overrided`。

> **内存第一铁律：谁分配，谁释放（Allocator Affinity）。严禁在边界一侧调用 `dealloc`/`free`/`delete` 释放另一侧分配的任何指针！**

---

## 2. 输入方向：零拷贝只读借用视图（Borrowed Views）

当模组向宿主传递字符串或字节数组（如发送消息、注册命令名、查询玩家），或宿主在同步回调中向模组推送临时数据时，一律采用**带编译期生命周期标记的 `#[repr(C)]` 借用切片**，实现零堆分配传递：

```rust
/// UTF-8 编码的只读借用字符串切片（不保证末尾 '\0' 结尾）
#[repr(C)]
#[derive(Copy, Clone)]
pub struct FfiStr<'a> {
    pub ptr: *const u8,
    pub len: usize,
    pub _marker: core::marker::PhantomData<&'a str>,
}

/// 连续内存元素的只读借用切片（T 必须满足 #[repr(C)] 且无填充未初始化字节）
#[repr(C)]
#[derive(Copy, Clone)]
pub struct FfiSlice<'a, T> {
    pub ptr: *const T,
    pub len: usize,
    pub _marker: core::marker::PhantomData<&'a [T]>,
}
```

### 指针合法性硬约束
- **空切片非空对齐指针规则**：当 `len == 0` 时，由 Rust `slice.as_ptr()` 生成的 `ptr` 是非空悬垂对齐指针（如 `NonNull::dangling()`）；而在 C/C++ 侧空视图 `ptr` 可能是 `nullptr`。
  - **转换规范**：从 `FfiStr` / `FfiSlice` 还原为 Rust `&str` / `&[T]` 时，**严禁直接对 `len == 0 && ptr.is_null()` 调用 `core::slice::from_raw_parts`**（在 Rust 中即使 `len == 0`，传入空指针也是立即 UB！）。必须统一通过安全辅助函数：
    ```rust
    pub unsafe fn as_bytes(&self) -> &'a [u8] {
        if self.len == 0 || self.ptr.is_null() {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
        }
    }
    ```
  - C++ Shim 侧转换：`std::string_view(reinterpret_cast<char const*>(s.ptr), s.len)` 在 `s.len == 0` 时天然合法。
- **UTF-8 契约（不对称）**：
  - 模组 → 宿主：`FfiStr` 由 Rust `&str` 构造，静态保证合法 UTF-8。
  - 宿主 → 模组：来自 BDS C++ `std::string` 的数据（玩家名、聊天内容、NBT 字符串等）**不保证**是合法 UTF-8。SDK 必须使用 `core::str::from_utf8` 校验；对展示类文本以 `String::from_utf8_lossy` 兜底，对需要精确比较的场景返回 `Err(InvalidUtf8)`。**严禁**无条件调用 `from_utf8_unchecked`。
- **同步借用窗口限制**：传入 FFI 函数或同步事件回调的 `FfiStr<'a>` / `FfiSlice<'a, T>` **仅在该次同步函数调用栈返回前有效**。任何一方需要跨 Tick 或跨线程留存数据，必须在返回前拷贝到己方堆中。

---

## 3. 输出方向：Writer 回调写入模式（首选标准）

当宿主需要向模组返回变长数据（如读取玩家名称、序列化 NBT 字节流、获取在线玩家列表、格式化错误消息）时，如何避免双堆冲突与二次查询开销？

Ferrum 统一采用**调用方提供写入器回调（Caller-Provided Writer Callback）**作为标准模式：

```rust
/// 模组栈上构造并传入宿主的字节流/字符串写入器
#[repr(C)]
pub struct FfiByteWriter {
    pub context: *mut core::ffi::c_void,
    /// 在模组侧堆上预留并追加写入 bytes；返回 FerrumStatus::OK 或 OUT_OF_MEMORY
    pub write_fn: extern "C" fn(
        context: *mut core::ffi::c_void,
        bytes: *const u8,
        len: usize,
    ) -> FerrumStatus,
}
```

### 执行流程与零多余开销优势
1. **模组侧（`ferrum-sdk`）**：在栈上准备一个空的 `Vec<u8>`（或 `String`），将其可变引用封装进 `FfiByteWriter` 传入宿主 VTable 函数（例如 `(player_v1.get_real_name)(runtime_id, &mut writer)`）。
2. **宿主侧（`cpp-shim`）**：按 ID 解析出 `Player*`，拿到 `Player::getRealName()` 的 `std::string_view`，直接调用一次 `writer.write_fn(writer.context, sv.data(), sv.size())`。
3. **`write_fn` 在模组 DLL 代码段内执行**：使用**模组自己的分配器**执行 `vec.extend_from_slice(...)`。`write_fn` 是模组侧跳板，同样以 `catch_unwind` 包裹。
4. **全程仅 1 次内存拷贝、0 次跨堆释放风险、无需“先查长度再分配缓冲”的二次调用**。

**约束**：
- 宿主可对同一 writer 多次调用 `write_fn`（流式输出），模组侧按追加语义处理。
- `write_fn` 返回非 `OK` 时，宿主必须立即停止写入并把该状态码原样返回。
- 宿主不得在调用返回后保留 `FfiByteWriter` 或其 `context`。

### 备选模式：宿主托管缓冲（`HostOwnedBuffer`）
仅在极少数无法流式回调的大块零拷贝只读缓冲场景下，允许返回携带显式释放钩子的自描述块：
```rust
#[repr(C)]
pub struct HostOwnedBuffer {
    pub ptr: *const u8,
    pub len: usize,
    pub token: *mut core::ffi::c_void,
    pub drop_fn: extern "C" fn(token: *mut core::ffi::c_void),
}
```
SDK 将其立即包装在 RAII 类型中，在 `Drop` 时回调宿主的 `drop_fn` 归还给宿主堆。

---

## 4. 跨边界闭包与回调对象的三态胖结构（`FfiCallback`）

当模组向宿主注册事件监听器、命令执行器、表单按钮回调或异步任务时，需要将带捕获环境的 Rust 闭包（`Fn` / `FnMut` / `FnOnce`）交给宿主保管。规范布局如下：

每种回调类别在 `ferrum-abi` 中定义**强类型**的回调结构体，`invoke_fn` 签名固定，**严禁**使用 `*const c_void` 存放函数指针再由接收方强转：

```rust
/// 示例：主线程事件回调
#[repr(C)]
pub struct EventCallbackV1 {
    /// 指向模组自身堆分配的 Box<CallbackEnv<F>>
    pub env: *mut core::ffi::c_void,
    /// 业务触发函数（模组 DLL 代码段内的单态化跳板，内部包裹 catch_unwind）
    pub invoke_fn: extern "C" fn(
        env: *mut core::ffi::c_void,
        event: EventScopeHandle,
        token: RawMainThreadToken,
    ) -> FerrumStatus,
    /// 析构函数（在模组 DLL 内将 env 还原为 Box<CallbackEnv<F>> 并执行 Drop，内部包裹 catch_unwind）
    pub drop_fn: extern "C" fn(env: *mut core::ffi::c_void),
}

/// 示例：一次性投递闭包（FnOnce）
#[repr(C)]
pub struct OnceCallbackV1 {
    pub env: *mut core::ffi::c_void,
    /// 调用后 env 所有权已被跳板取回，宿主不得再调用 drop_fn
    pub call_once_fn: extern "C" fn(env: *mut core::ffi::c_void, token: RawMainThreadToken) -> FerrumStatus,
    pub drop_fn: extern "C" fn(env: *mut core::ffi::c_void),
}
```

下文以 `FfiCallback` 统称这类“`env` + 调用函数 + `drop_fn`”三元结构。

### 闭包生命周期铁律
1. **单次消费回调（`FnOnce`，如 `after_ticks` 一次性定时器、`on_main_thread` 投递、表单提交回调）**：
   - 若正常触发，跳板在内部通过 `Box::from_raw(env as *mut F)` 取回所有权并执行，宿主标记已消费，**不再**调用 `drop_fn`（即使跳板返回 `MOD_PANICKED`）。
   - 若在触发前服务器关闭、玩家掉线或模组停用（取消未触发回调），宿主调用 `drop_fn(env)` 释放捕获变量，绝不泄漏。
2. **多次调用回调（`Fn` / `FnMut`，如事件监听、命令回调、周期任务）**：
   - 宿主以 `Arc<RegisteredCallback>` 托管，每次触发调用 `invoke_fn(env, ...)`；
   - 当监听器被手动移除或模组停用时，宿主在确保无活跃调用后调用**且仅调用一次** `drop_fn(env)`；
   - `FnMut` 的重入守卫与 panic 毒化由 SDK 跳板实现，见《线程模型、并发与热卸载安全规范》§3.2 与《状态码、异常与 Panic 隔离规范》§1.3。
3. **线程约束**：`invoke_fn` 与 `drop_fn` 的执行线程遵循《线程模型、并发与热卸载安全规范》§1.4 的表格。非 `Send` 闭包的 `drop_fn` 只能在主线程执行。
4. **代码段存活前置条件（Image Liveness Invariant）**：
   - `invoke_fn` 与 `drop_fn` 的机器码位于**下游模组 `.dll` 的 `.text` 代码段**中。
   - 因此宿主**必须**在模组镜像被释放之前完成该模组名下所有 `FfiCallback` 的注销与 `drop_fn` 执行，包括暂存在 `ServerThreadExecutor` 队列中的待执行闭包。
   - 默认情况下 Ferrum 不释放模组镜像（见《线程模型、并发与热卸载安全规范》§4.4），该不变式由停用协议的“排空”步骤保证；对声明了 `HOT_UNLOAD_SAFE` 的模组，`FreeLibrary` 必须排在排空之后。
