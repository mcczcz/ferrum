# Rust 与原生模组通用事件协议

开发版 ABI 继续使用 1.1。Rust 和原生模组都可以注册字段并发布事件。真实宿主统一通过 LL 的 `FerrumCustomEvent` 派发，Rust 通过 `ctx.custom_events()` 订阅，native 通过 LL EventBus 订阅公开 C++ 类型。已有其他 C++ 自定义事件不会自动转换，需要发布者或适配插件接入。

## Rust 订阅

在 `on_enable` 中注册，允许早于原生事件注册；返回值为独立的 `CustomListenerHandle`，只能用 `custom_events().unsubscribe()` 注销：

```rust
use ferrum_sdk::events::EventPriority;

let listener = ctx.custom_events().subscribe(
    token,
    "economy.balance_changing.v1",
    EventPriority::High,
    |event, _token| {
        let next = event.i64("new_balance")?;
        if next < 0 {
            event.cancel()?;
        }
        Ok(())
    },
)?;
// 提前移除时：ctx.custom_events().unsubscribe(token, listener)?;
```

字段访问：`i64`、`u64`、`f64`、`bool`、`string`、`bytes`、`uuid`，均接收字段名并返回 `Result<T>`。未知字段返回 `NotFound`，类型不符返回 `InvalidArgument`。`name()` 返回事件名，`is_cancelled()` 返回取消状态。变长输出属于 Rust 模组自己的堆。

监听顺序为 Highest → High → Normal → Low → Lowest。每个 Rust 订阅对应一个 LL 监听器，因此与 native 监听按相同优先级交错执行；同优先级按 LL 监听器创建顺序。LL 派发使用监听快照，新增监听从下次派发生效；已失效的 Rust 监听句柄跳过。取消停止后续两端监听，但不回滚之前的副作用。

回调 HRTB、主线程令牌、非 Send 闭包、自注销延迟析构、panic 摘除及自停用延迟排空，与内建事件相同。回调返回 Err 记录日志，不自动取消，不让 publish 失败。不可取消事件调用 cancel 返回 Unsupported。

## Rust 注册与发布

`CustomEvents::register` 在 on_enable 或已启用模组的主线程回调中调用。事件全名必须以当前 manifest 的模组名加点号开头，以下假设模组名为 `economy`：

```rust
use ferrum_sdk::custom_events::{EventFieldType, EventValue, RegisteredEvent};

// 在 on_enable 中注册，可以把 RegisteredEvent 保存到模组字段中。
let balance_changing: RegisteredEvent = ctx.custom_events().register(
    token,
    "economy.balance_changing.v1",
    true,
    &[
        ("old_balance", EventFieldType::I64),
        ("new_balance", EventFieldType::I64),
    ],
)?;

// 在后续命令/事件回调中发布，此时模组必须已 Enabled。
// 可将 Copy 的句柄 move 进其他回调，不要在 on_enable 返回前立即发布。
let cancelled = balance_changing.publish(
    token,
    &[
        ("old_balance", EventValue::I64(100)),
        ("new_balance", EventValue::I64(-10)),
    ],
)?;
if !cancelled {
    // 提交实际余额修改。
}

// 若需提前撤销：balance_changing.unregister(token)?;
```

EventFieldType 和 EventValue 均支持 I64/U64/F64/Bool/String/Bytes/Uuid。RegisteredEvent 丢弃不撤销；启用失败、停用或卸载会自动撤销定义，旧句柄失效。重新启用要重新注册。模组不能使用别人的已注册事件句柄发布。Rust 发布端在独立的 `ferrum.custom_event_publisher.v1` 子表中，不改变已有订阅表布局。

## native 使用 LL EventBus 监听

包含公开的 `ferrum/CustomEvent.h`，直接使用 LL 监听器，无需链接 loader 导入库。所有业务事件共用 `ferrum::FerrumCustomEvent` 类型，通过 name 区分。示例放在 native 模组的 onEnable 中；目标 Rust/native 发布者都适用：

```cpp
#include "ferrum/CustomEvent.h"
#include "ll/api/event/EventBus.h"
#include "ll/api/mod/NativeMod.h"

auto owner = ll::mod::NativeMod::current();
auto listener = ll::event::EventBus::getInstance()
    .emplaceListener<ferrum::FerrumCustomEvent>(
        [](ferrum::FerrumCustomEvent& event) {
            if (!event.active()) return;
            std::string name;
            if (event.name(name) != FerrumStatus_OK
                || name != "economy.balance_changing.v1") return;
            int64_t next{};
            if (event.getI64("new_balance", next) == FerrumStatus_OK && next < 0) {
                (void)event.cancel();
            }
        },
        ll::event::EventPriority::High,
        owner
    );
// 保存 ListenerPtr 以便在 onDisable 中提前 removeListener(listener)。
// LL 同时按显式 owner 管理监听归属。
```

`name`、`getString`、`getBytes` 将输出写入当前 native 模组自己的容器，getI64/getU64/getF64/getBool/getUuid 通过引用写出，均返回 FerrumStatus。cancel 返回 Unsupported 表示事件不可取消；请通过该方法取消，不要强转父类绕过协议检查。事件引用不能保存到回调之外，字段副本可以保存。

native 模组需依赖 ferrum-loader，确保通用 LL emitter 已注册；无监听时 LL 仍接受后续订阅。不要自行构造 FerrumCustomEvent 并 publish，业务发布必须走协议以验证描述、生命周期和字段。

## C++ 原生发布端

原生模组在 manifest 声明对 `ferrum-loader` 的依赖，将 `cpp-shim/include/ferrum/` 的生成 ABI 头及 `NativeEvents.h` 加入 include 路径。无需链接 Rust 或 loader 导入库。公开入口只有 `ferrum_native_event_api_v1`，通过 `GetProcAddress` 获取：

```cpp
#include <Windows.h>
#include <string_view>
#include "ferrum/NativeEvents.h"

static FfiStr ffi(std::string_view s) {
    return {reinterpret_cast<uint8_t const*>(s.data()), s.size()};
}

// 作为原生模组实例成员持有；禁止在第一次启用后仍复用被撤销的旧句柄。
NativeEventApiV1 const* api = nullptr;
EventProviderHandle provider{};
CustomEventHandle balanceChanging{};

// 在 onEnable 内、主线程调用。economy 必须是该原生模组真实的 manifest name。
bool enableEvents() {
    auto dll = GetModuleHandleW(L"ferrum-loader.dll");
    if (!dll) return false;
    auto getApi = reinterpret_cast<FerrumGetNativeEventApiV1>(
        GetProcAddress(dll, "ferrum_native_event_api_v1")
    );
    if (!getApi) return false;
    api = getApi();
    if (!api || api->struct_size < sizeof(NativeEventApiV1)) return false;
    if (api->open_provider(ffi("economy"), &provider) != FerrumStatus_OK) return false;

    EventFieldV1 fields[]{
        {ffi("old_balance"), EventValueKind_I64, 0},
        {ffi("new_balance"), EventValueKind_I64, 0},
    };
    CustomEventDefinitionV1 definition{};
    definition.struct_size = sizeof(definition);
    definition.name = ffi("economy.balance_changing.v1");
    definition.cancellable = FfiBool_TRUE;
    definition.fields = {fields, 2};
    if (api->register_event(provider, &definition, &balanceChanging) != FerrumStatus_OK) {
        api->close_provider(provider);
        provider = {};
        return false;
    }
    return true;
}

// 在真正改变余额前调用；返回 false 时调用方不得修改余额。
bool allowBalanceChange(int64_t oldValue, int64_t newValue) {
    EventValueV1 values[]{
        {ffi("old_balance"), EventValueKind_I64, 0, static_cast<uint64_t>(oldValue), {}},
        {ffi("new_balance"), EventValueKind_I64, 0, static_cast<uint64_t>(newValue), {}},
    };
    FfiBool cancelled = FfiBool_FALSE;
    auto status = api->publish(provider, balanceChanging, {values, 2}, &cancelled);
    return status == FerrumStatus_OK && cancelled == FfiBool_FALSE;
}

// 在 onDisable 内调用；LL 的停用/卸载通知也会兜底撤销。
void disableEvents() {
    if (provider) api->close_provider(provider);
    provider = {};
    balanceChanging = {};
}
```

金额单位由 economy 契约约定；更改字段类型、删除字段或改变含义时使用新的事件名版本。该示例不进行实际扣款。发布时必须提交全部声明字段，不能缺少、多出、重复或类型不匹配；字段顺序不限。首版字段只读，唯一修改能力是取消。

## C ABI 与生命周期

- 注册及发布都要求主线程，错误线程返回 WrongThread。注册可以在 onEnable 内进行；只有 LL 已标记 Enabled 的原生模组可以发布，因此不要在 onEnable 尚未返回时立即发布。
- 发布者名称必须是已加载的 native 模组名称；每个名称只能打开一个发布者。事件必须使用 `<模组名>.` 前缀。名称归属用于协作和清理，不是针对恶意进程内代码的权限隔离。
- 句柄是能力：只有持有发布者句柄的代码才能操作对应事件。事件/监听器/作用域分别使用独立代数池；不要混用，关闭后不能复用旧句柄。
- 宿主同步复制事件描述和发布载荷，不保存发布者 C++ 指针或函数指针。输入借用在调用返回前有效；没有跨 DLL 释放。STRING 为 UTF-8，BYTES 为任意字节，UUID 恰为 16 字节；F64 的 scalar 使用 `std::bit_cast<uint64_t>(value)`。所有 reserved 填 0。
- 发布者停用/卸载时撤销其事件；Rust 按名称的订阅继续保留，发布者重新启用后用新句柄重新注册同名事件即可继续接收。Rust 模组停用、卸载或启用失败则撤销其订阅。
- 发布期间撤销发布者或事件会停止后续派发并返回 ModDisabled；发布端必须检查 status，仅在 OK 后读取取消结果及继续业务操作。
- native 发布者不向协议注册函数指针；native 的订阅回调由 LL 管理。若监听间接触发 native 发布者自停用/卸载，调用方自己的 native 生命周期和调用栈安全仍由该模组负责。Rust 发布者会计入宿主 active_invocations，停用延迟排空，栈内同步自卸载被拒绝。
- 真实宿主通过同一个 LL EventBus 流派发两端监听，任意一端 cancel 都会同步终止后续两端监听。将旧 LL 事件转入协议时，适配器仍负责决定桥接时机和向原事件回写取消。
- 发布过程中撤销事件或停用 Rust 发布者后，Rust 监听跳过，publish 返回 ModDisabled。LL 已取得的 native 监听快照仍可能进入回调；需用 event.active() 检查是否仍有效。
- `NativeEventApiV1::take_last_error` 用调用方 Writer 取回详情；Writer 必须不抛异常，并且不得在返回后使用其借用缓冲。

## Mock

`MockHost::native_event_api()` 返回同一协议的纯 Rust 原生发布端，Rust 的 register/publish 也可在 mock 使用。字段验证、作用域、取消、重入与回调核心和真实宿主共用；无 LL 环境时直接按优先级派发 Rust 监听，不模拟 native C++ 的 LL 监听器。mock 不查询真实 LL 模组身份/状态，调用方通过 close_provider 模拟原生停用。
