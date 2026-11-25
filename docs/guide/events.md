# 事件

两类事件：

| 类型 | 入口 | 用途 |
| :--- | :--- | :--- |
| LL 官方事件 | `ctx.events()` | 玩家、实体、世界、命令、服务、服务器和控制台事件 |
| 自定义事件 | `ctx.custom_events()` | Rust 与 native C++ 模组互相发布、订阅 |

加入、断开和聊天事件从 ABI 1.1 起可用。其余官方事件及 `payload_snbt()` 已纳入当前 ABI 1.2 开发版。

## LL 官方事件

LeviLamina 26.60.7 服务端提供以下具体事件。类型均从 `ferrum_sdk::events` 导入。表中 **粗体** 表示可取消。

| 类别 | 事件 |
| --- | --- |
| 玩家 | **`PlayerJoinEvent`**、`PlayerDisconnectEvent`、**`PlayerChatEvent`**、**`PlayerAddExperienceEvent`**、**`PlayerAttackEvent`**、**`PlayerChangePermEvent`**、**`PlayerConnectEvent`**、**`PlayerDestroyBlockEvent`**、`PlayerDieEvent`、**`PlayerInteractBlockEvent`**、**`PlayerJumpEvent`**、**`PlayerPickUpItemEvent`**、**`PlayerPlacingBlockEvent`**、`PlayerPlacedBlockEvent`、`PlayerRespawnEvent`、**`PlayerSneakingEvent`**、**`PlayerSneakedEvent`**、`PlayerSprintingEvent`、`PlayerSprintedEvent`、`PlayerSwingEvent`、**`PlayerUseItemEvent`** |
| 实体 | **`ActorHurtEvent`**、`MobDieEvent` |
| 世界 | `BlockChangedEvent`、**`FireSpreadEvent`**、`ServerLevelTickEvent`、**`SpawningMobEvent`**、`SpawnedMobEvent` |
| 命令 | **`ExecutingCommandEvent`**、`ExecutedCommandEvent`、`ServerCommandRegisterEvent` |
| 服务器 | `ServerStartedEvent`、`ServerStoppingEvent` |
| 服务 | `ServiceRegisterEvent`、`ServiceUnregisterEvent` |
| 控制台 | **`ConsoleOutputtingEvent`**、`ConsoleOutputtedEvent` |

### 主线程事件

用 `subscribe::<E>()` 订阅玩家、实体、世界、命令和服务器事件。回调可以读取 `payload_snbt()?`，它返回 LL 在本监听器开始时序列化的字段快照，具体字段随 LL 版本变化。实体事件还可以用 `actor()?` 获取主体，生物和玩家事件分别提供 `mob()?` 和 `player()?`。聊天事件另有 `message()?`。订阅只接收注册并启用模组之后的事件。

```rust
use ferrum_sdk::events::{EventPriority, PlayerChatEvent, PlayerJoinEvent};

ctx.events().subscribe::<PlayerJoinEvent>(token, EventPriority::Normal, |event, _token| {
    let player = event.player()?;
    player.send_message(format!("欢迎，{}！", player.real_name()?))?;
    Ok(())
})?;

let listener = ctx.events().subscribe_light::<PlayerChatEvent>(token, EventPriority::High, |event, _token| {
    if event.message()?.contains("禁止内容") {
        event.cancel()?;
    }
    Ok(())
})?;

ctx.events().unsubscribe(token, listener)?; // 可选，提前移除
```

规则：

- 在 `on_enable` 或已启用模组的主线程回调中订阅、退订。
- 优先级 `Highest > High > Normal > Low > Lowest`。同优先级按监听器创建顺序。每个订阅对应一个 LL 监听器，与 native 模组的监听器一起排序。
- `cancel()` 立即作用于 LL 原事件，并跳过后续监听器。
- 取消加入事件只取消 LL 的初始化处理，不会拒绝登录或踢人。
- 回调返回 `Err`：记录日志，不取消，不回滚已发生的修改。
- 回调是 `FnMut + 'static`，可以捕获 `Rc`，不能借用 `&mut self`。`event`、`player`、`token` 不能带出回调。
- `ListenerHandle` 丢弃不退订。手动退订、启用失败、停用、卸载时清理。回调内可退订自身，闭包在调用结束后析构。
- 同一回调重入会被跳过并限频告警。回调 panic：监听器被摘除。

### 在 LL 触发线程同步处理

控制台和服务事件使用 `subscribe_sync::<E>()`。回调要求 `Fn + Send + Sync`，可由多个线程同时执行，只能通过事件上下文读取值数据。控制台事件可直接读取 `message()` 和 `is_cancelled()`。输出前调用 `cancel()` 会在 LL 继续派发前生效。服务事件的 SNBT 包含 `serviceName` 和 `serviceVersion`。

```rust
use ferrum_sdk::events::{ConsoleOutputtingEvent, EventPriority};

ctx.events().subscribe_sync::<ConsoleOutputtingEvent>(token, EventPriority::High, |event| {
    if event.message().starts_with("[private]") {
        event.cancel();
    }
    Ok(())
})?;
```

同一监听器在同一线程递归触发时跳过内层调用。Rust 回调 panic 后被毒化，宿主在主线程清理监听器。

## 自定义事件

事件按名称标识，字段为键值。真实宿主用 LL 的 `FerrumCustomEvent` 派发：Rust 通过 SDK 订阅，native 通过 `EventBus` 监听。其他已有的 C++ 事件不会自动转换。

### 命名与字段

- 事件名以发布者模组名加点开头，例如 `economy.balance_changing.v1`。改字段类型、删字段、改含义时升版本号。
- 事件名和字段名只允许 ASCII 字母、数字和 `. _ - :`。
- 字段类型：`I64`、`U64`、`F64`、`Bool`、`String`（UTF-8）、`Bytes`、`Uuid`（16 字节）。
- 字段只读。唯一的修改能力是取消。
- 发布时必须给出全部已声明字段：不能缺、多、重复、类型错。顺序不限。

### Rust 订阅

可以早于发布者注册。返回的 `CustomListenerHandle` 只能用 `custom_events().unsubscribe()` 注销。

```rust
use ferrum_sdk::events::EventPriority;

let listener = ctx.custom_events().subscribe(
    token,
    "economy.balance_changing.v1",
    EventPriority::High,
    |event, _token| {
        if event.i64("new_balance")? < 0 {
            event.cancel()?;
        }
        Ok(())
    },
)?;
```

`CustomEventCtx` 方法：

| 方法 | 返回 |
| :--- | :--- |
| `i64` `u64` `f64` `bool` `string` `bytes` `uuid`（字段名） | `Result<T>`。字段不存在 `NotFound`，类型不符 `InvalidArgument` |
| `name()` | 事件名 |
| `is_cancelled()` | 是否已取消 |
| `cancel()` | 不可取消的事件返回 `Unsupported` |

变长字段返回模组自己堆上的副本。

派发顺序：Rust 和 native 监听器按优先级交错执行，同优先级按创建顺序。任一端 `cancel` 都会终止后续两端的监听器，已发生的副作用不回滚。回调语义（Err、panic、重入、自退订）同玩家事件。

### Rust 注册与发布

在 `on_enable` 或已启用模组的主线程回调中注册。

```rust
use ferrum_sdk::custom_events::{EventFieldType, EventValue, RegisteredEvent};

// on_enable 中注册，句柄是 Copy，可存进模组字段
let balance_changing: RegisteredEvent = ctx.custom_events().register(
    token,
    "economy.balance_changing.v1",
    true, // cancellable
    &[("old_balance", EventFieldType::I64), ("new_balance", EventFieldType::I64)],
)?;

// 之后在命令或事件回调中发布（模组必须已 Enabled，不要在 on_enable 返回前发布）
let cancelled = balance_changing.publish(
    token,
    &[("old_balance", EventValue::I64(100)), ("new_balance", EventValue::I64(-10))],
)?;
if !cancelled {
    // 执行实际修改
}
```

- `register` 的 `name` 前缀必须是当前 manifest 的模组名。
- 丢弃 `RegisteredEvent` 不撤销。`unregister(token)` 手动撤销。启用失败、停用、卸载自动撤销，旧句柄失效，重新启用要重新注册。
- 只能用自己注册的句柄发布。

### native C++ 监听

包含 `ferrum/CustomEvent.h`，直接用 LL `EventBus`，不需要链接 loader。所有自定义事件共用 `ferrum::FerrumCustomEvent` 类型，用 `name()` 区分。模组需依赖 `ferrum-loader`。

```cpp
#include "ferrum/CustomEvent.h"
#include "ll/api/event/EventBus.h"
#include "ll/api/mod/NativeMod.h"

// onEnable 中
auto owner = ll::mod::NativeMod::current();
auto listener = ll::event::EventBus::getInstance().emplaceListener<ferrum::FerrumCustomEvent>(
    [](ferrum::FerrumCustomEvent& event) {
        if (!event.active()) return;
        std::string name;
        if (event.name(name) != FerrumStatus_OK || name != "economy.balance_changing.v1") return;
        int64_t next{};
        if (event.getI64("new_balance", next) == FerrumStatus_OK && next < 0) {
            (void)event.cancel();
        }
    },
    ll::event::EventPriority::High,
    owner);
// 保存 listener，可在 onDisable 中 removeListener
```

- 取值方法：`getI64` `getU64` `getF64` `getBool` `getUuid` 写入引用；`name`、`getString`、`getBytes` 写入调用方容器。均返回 `FerrumStatus`。
- `cancel()` 返回 `Unsupported` 表示事件不可取消。不要强转基类绕过。
- 事件引用不能保存到回调之外，取出的字段副本可以保存。
- 回调中先查 `event.active()`：发布者在派发中被撤销时，LL 已取得的监听快照仍可能进入回调。
- 不要自己构造 `FerrumCustomEvent` 并 `publish`。发布必须走下面的协议。

### native C++ 发布

模组声明依赖 `ferrum-loader`，把 `cpp-shim/include/ferrum/` 加入 include 路径（`ferrum_abi.h` 与 `NativeEvents.h`）。不需要链接 Rust 或 loader 导入库。入口只有 `ferrum_native_event_api_v1`，用 `GetProcAddress` 获取。

```cpp
#include <Windows.h>
#include <string_view>
#include "ferrum/NativeEvents.h"

static FfiStr ffi(std::string_view s) {
    return {reinterpret_cast<uint8_t const*>(s.data()), s.size()};
}

NativeEventApiV1 const* api = nullptr;
EventProviderHandle     provider{};
CustomEventHandle       balanceChanging{};

// onEnable 内、主线程调用。"economy" 必须是本模组 manifest 的 name。
bool enableEvents() {
    auto dll = GetModuleHandleW(L"ferrum-loader.dll");
    if (!dll) return false;
    auto getApi = reinterpret_cast<FerrumGetNativeEventApiV1>(
        GetProcAddress(dll, "ferrum_native_event_api_v1"));
    if (!getApi) return false;
    api = getApi();
    if (!api || api->struct_size < sizeof(NativeEventApiV1)) return false;
    if (api->open_provider(ffi("economy"), &provider) != FerrumStatus_OK) return false;

    EventFieldV1 fields[]{
        {ffi("old_balance"), EventValueKind_I64, 0},
        {ffi("new_balance"), EventValueKind_I64, 0},
    };
    CustomEventDefinitionV1 def{};
    def.struct_size = sizeof(def);
    def.name        = ffi("economy.balance_changing.v1");
    def.cancellable = FfiBool_TRUE;
    def.fields      = {fields, 2};
    if (api->register_event(provider, &def, &balanceChanging) != FerrumStatus_OK) {
        api->close_provider(provider);
        provider = {};
        return false;
    }
    return true;
}

// 修改余额前调用。返回 false 时不得修改。
bool allowBalanceChange(int64_t oldValue, int64_t newValue) {
    EventValueV1 values[]{
        {ffi("old_balance"), EventValueKind_I64, 0, static_cast<uint64_t>(oldValue), {}},
        {ffi("new_balance"), EventValueKind_I64, 0, static_cast<uint64_t>(newValue), {}},
    };
    FfiBool cancelled = FfiBool_FALSE;
    auto status = api->publish(provider, balanceChanging, {values, 2}, &cancelled);
    return status == FerrumStatus_OK && cancelled == FfiBool_FALSE;
}

// onDisable 内调用。LL 的停用、卸载通知也会兜底撤销。
void disableEvents() {
    if (provider) api->close_provider(provider);
    provider = {};
    balanceChanging = {};
}
```

`NativeEventApiV1` 函数：`open_provider`、`close_provider`、`register_event`、`unregister_event`、`publish`、`take_last_error`。`take_last_error` 用调用方的 `FfiByteWriter` 取回详情；写入器不得抛异常，也不得在返回后使用其缓冲。

### C ABI 生命周期约定

- 注册、发布都必须在主线程，否则返回 `WrongThread`。
- 只有 LL 已标记 Enabled 的 native 模组才能发布。`onEnable` 返回前不要发布。
- 发布者名必须是已加载的 native 模组名，同名只能打开一个发布者（`AlreadyExists`）。事件名前缀不符返回 `PermissionDenied`。名称归属用于协作与清理，不防恶意的进程内代码。
- 句柄就是能力：只有持有发布者句柄的代码能操作其事件。发布者、事件、监听器、作用域各用独立的代数池。关闭后旧句柄返回 `StaleHandle`，不要复用。
- 宿主同步复制事件描述和发布载荷，不保存发布者的指针。输入借用只在调用返回前有效。没有跨 DLL 释放。
- `F64` 的 `scalar` 用 `std::bit_cast<uint64_t>(value)`。`reserved` 填 0。
- native 发布者停用或卸载时撤销其事件。Rust 的按名订阅保留，发布者重新启用并重新注册同名事件后继续接收。
- Rust 模组停用、卸载或启用失败时撤销其订阅。
- 发布过程中事件或发布者被撤销：后续派发停止，`publish` 返回 `ModDisabled`。只在 `OK` 时读取取消结果。
- Rust 发布者计入宿主的活跃调用，停用延迟到排空后，栈内同步自卸载被拒绝。native 发布者在监听器里间接触发自身停用或卸载时，调用栈安全由该模组自己负责。

## Mock

- `MockHost::emit_player_join(id)`、`emit_player_disconnect(id)`、`emit_player_chat(id, msg)` 同步派发玩家事件。玩家需先 `add_player`。加入和聊天返回是否被取消。不改变在线状态，不模拟广播。
- Rust 的 `register` / `publish` / `subscribe` 在 mock 中可用，字段校验、取消、重入与真实宿主共用同一份核心。
- `MockHost::native_event_api()` 返回与 C++ 发布端相同的 `NativeEventApiV1`，用 `close_provider` 模拟 native 停用。
- mock 不模拟 native C++ 监听器，不检查真实的 LL 模组身份与启用状态。

见 [测试与调试](./testing.md)。
