# 内存与所有权

## 1. 双堆隔离

- **宿主堆**：`ferrum-loader.dll` 的 C++ 部分启用 `LL_MEMORY_OPERATORS` 接入 LL 统一分配器；`ferrum-host` 使用其静态库分配器。
- **模组堆**：每个模组 `cdylib` 使用独立 Rust 全局分配器，不导出 `ll_memory_operator_overrided`。
- **铁律**：**谁分配谁释放**，严禁跨 DLL 边界 `free` / `dealloc` / `delete` 对方指针。

## 2. 输入：借用视图

```rust
#[repr(C)] pub struct FfiStr<'a>      { pub ptr: *const u8, pub len: usize, ... }
#[repr(C)] pub struct FfiSlice<'a, T> { pub ptr: *const T,  pub len: usize, ... }
```

- `len == 0` 时 `ptr` 允许为 null，还原切片必须使用 `FfiStr::as_bytes` / `to_str` 或 `FfiSlice::as_slice`（禁止直接对可能为空的 `ptr` 调用 `slice::from_raw_parts`）。
- 字符串不保证 `\0` 结尾，C++ 侧使用 `std::string_view(ptr, len)`。
- 宿主传入的字符串不保证合法 UTF-8，SDK 必须校验（展示用 `from_utf8_lossy`，严格字段返回 `InvalidUtf8`）。
- 借用视图仅在当次同步调用返回前有效，跨 Tick 须拷贝到己方堆。

## 3. 输出：`FfiByteWriter`

变长输出由调用方在栈上构造 `FfiByteWriter`（`context: *mut c_void` + `write_fn`）传入宿主。`write_fn` 在调用方 DLL 内以调用方分配器追加字节并包裹 `catch_unwind`：

- 支持多次追加调用；若 `write_fn` 返回非 `OK`，宿主必须立即中止写入并原样透传状态码。
- 宿主不得在调用返回后保留 writer 或 `context`。

## 4. 回调所有权

持久/一次性回调统一使用强类型结构体（`EventCallbackV1`、`ThreadEventCallbackV1`、`CommandCallbackV1`、`TaskCallbackV1`、`FormCallbackV1`），包含 `env: *mut c_void`、`invoke_fn` 与 `drop_fn`（不使用 `*const c_void` 强转函数指针）：

- **所有权转移**：仅当注册/发送接口返回 `OK` 时才将 `env` 移交宿主托管；失败时 `env` 仍由 SDK 在调用方收回并析构。
- **释放保证**：宿主通过 `Arc` 托管回调并在主线程执行 `drop_fn(env)`（`ThreadEventCallbackV1` 亦在无活跃派发后于主线程清理）；`invoke_fn` 与 `drop_fn` 调用时宿主绝不持有内部锁。
- 模组镜像卸载/停用时，宿主先排空并调用其名下所有 `drop_fn`。
