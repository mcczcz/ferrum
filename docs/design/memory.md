# 内存与所有权

## 1. 两个堆

- 宿主堆：`ferrum-loader.dll` 的 C++ 部分使用 LL 统一分配器（`MemoryOperators.cpp` 定义 `LL_MEMORY_OPERATORS`）。`ferrum-host` 是 Rust 静态库，用自己的分配器。
- 模组堆：每个模组是独立 `cdylib`，用自己的 Rust 全局分配器。模组不链接 LL 分配器，也不导出 `ll_memory_operator_overrided`。

**谁分配谁释放。** 任何一侧都不能对另一侧分配的指针调用 `free` / `dealloc` / `delete`。

## 2. 输入：借用视图

模组向宿主传字符串或数组，或宿主在同步回调里给模组传临时数据，一律用借用切片：

```rust
#[repr(C)] pub struct FfiStr<'a>      { ptr: *const u8, len: usize, _marker: PhantomData<&'a str> }
#[repr(C)] pub struct FfiSlice<'a, T> { ptr: *const T,  len: usize, _marker: PhantomData<&'a [T]> }
```

规则：

- `len == 0` 时 `ptr` 可能为空。还原为切片必须走 `FfiStr::as_bytes` / `to_str`、`FfiSlice::as_slice`，它们处理了空指针。不能对空指针直接调用 `slice::from_raw_parts`。
- 字符串不保证末尾 `\0`。C++ 侧用 `std::string_view(ptr, len)`。
- 模组到宿主的 `FfiStr` 来自 Rust `&str`，一定是合法 UTF-8。
- 宿主到模组的字符串（玩家名、聊天内容等）可能不是合法 UTF-8。SDK 必须校验：展示文本用 `from_utf8_lossy`，需要精确比较的用 `Err(InvalidUtf8)`。不能无条件 `from_utf8_unchecked`。
- 借用视图只在本次同步调用返回前有效。要跨 Tick 或跨线程保存，先拷贝到自己的堆。

## 3. 输出：Writer 回调

宿主返回变长数据（名称、NBT、列表、错误消息）时，调用方提供写入器：

```rust
#[repr(C)]
pub struct FfiByteWriter {
    pub context: *mut c_void,
    pub write_fn: extern "C" fn(*mut c_void, *const u8, usize) -> FerrumStatus,
}
```

流程：

1. SDK 在栈上放一个 `Vec<u8>`，把它包成 `FfiByteWriter` 传给宿主函数。
2. `cpp-shim` 取得数据后调用 `writer.write_fn`。
3. `write_fn` 在模组 DLL 内执行，用模组自己的分配器追加数据。它本身包着 `catch_unwind`。

只拷贝一次，没有跨堆释放，也不需要先查长度。

约束：

- 宿主可多次调用 `write_fn`，模组按追加处理。
- `write_fn` 返回非 `OK` 时，宿主立即停止写入，并原样返回该状态。
- 宿主不能在函数返回后保留 writer 或 `context`。

## 4. 回调对象（`env` + `invoke_fn` + `drop_fn`）

模组把带捕获的闭包交给宿主保管时，用强类型回调结构。目前有 `EventCallbackV1` 和 `CommandCallbackV1`：

```rust
#[repr(C)]
pub struct EventCallbackV1 {
    pub env: *mut c_void,      // 模组堆上的 Box<CallbackEnv<F>>
    pub invoke_fn: extern "C" fn(*mut c_void, EventScopeHandle, RawMainThreadToken) -> FerrumStatus,
    pub drop_fn: extern "C" fn(*mut c_void),
}
```

- `invoke_fn` 与 `drop_fn` 是模组 DLL 内的单态化跳板，内部 `catch_unwind`。
- 不能用 `*const c_void` 存函数指针再让对方强转。每类回调有自己的结构。

所有权规则：

- 注册成功才转移 `env` 所有权。注册失败时 `env` 仍归模组，由模组自己释放。
- 宿主用 `Arc` 托管回调。监听器被移除或模组停用时，等没有活跃调用后，调用且仅调用一次 `drop_fn(env)`。
- `invoke_fn` 与 `drop_fn` 只在主线程执行，且调用时宿主不持有锁。
- `FnMut` 的重入守卫和 panic 毒化由 SDK 跳板实现，见 [errors.md](errors.md) 和 [threading.md](threading.md)。

`TaskCallbackV1` 支持一次性和周期任务。注册成功后，宿主负责在主线程调用 `drop_fn`；注册失败时，SDK 在提交线程收回闭包。一次性任务只消费一次 `FnOnce`。
