# 事件

Ferrum 提供两类事件：

| 类型 | 入口 | 用途 |
| :--- | :--- | :--- |
| LL 官方事件 | `ctx.events()` | 订阅 LeviLamina 内置的玩家、实体、世界、命令、服务器、服务与控制台事件 |
| 自定义事件 | `ctx.custom_events()` | Rust 模组与 C++ Native 模组之间互相发布、订阅事件 |

## LL 官方事件

以下事件均可从 `ferrum_sdk::events` 导入。**粗体**表示支持 `cancel()` / `is_cancelled()`。

| 类别 | 事件 |
| :--- | :--- |
| 玩家 | **`PlayerJoinEvent`**、`PlayerDisconnectEvent`、**`PlayerChatEvent`**、**`PlayerAddExperienceEvent`**、**`PlayerAttackEvent`**、**`PlayerChangePermEvent`**、**`PlayerConnectEvent`**、**`PlayerDestroyBlockEvent`**、`PlayerDieEvent`、**`PlayerInteractBlockEvent`**、**`PlayerJumpEvent`**、**`PlayerPickUpItemEvent`**、**`PlayerPlacingBlockEvent`**、`PlayerPlacedBlockEvent`、`PlayerRespawnEvent`、**`PlayerSneakingEvent`**、**`PlayerSneakedEvent`**、`PlayerSprintingEvent`、`PlayerSprintedEvent`、`PlayerSwingEvent`、**`PlayerUseItemEvent`** |
| 实体 | **`ActorHurtEvent`**、`MobDieEvent` |
| 世界 | `BlockChangedEvent`、**`FireSpreadEvent`**、`ServerLevelTickEvent`、**`SpawningMobEvent`**、`SpawnedMobEvent` |
| 命令 | **`ExecutingCommandEvent`**、`ExecutedCommandEvent`、`ServerCommandRegisterEvent` |
| 服务器 | `ServerStartedEvent`、`ServerStoppingEvent` |
| 服务（触发线程） | `ServiceRegisterEvent`、`ServiceUnregisterEvent` |
| 控制台（触发线程） | **`ConsoleOutputtingEvent`**、`ConsoleOutputtedEvent` |

### 主线程事件

玩家、实体、世界、命令和服务器事件在主线程派发，使用 `subscribe::<E>()`（附带 `payload_snbt()`）或 `subscribe_light::<E>()`（跳过 SNBT 序列化，开销更低）：

```rust
use ferrum_sdk::events::{EventPriority, PlayerChatEvent, PlayerJoinEvent};

ctx.events().subscribe::<PlayerJoinEvent>(token, EventPriority::Normal, |event, _token| {
    let player = event.player()?;
    player.send_message(format!("欢迎，{}！", player.real_name()?))?;
    Ok(())
})?;

let listener = ctx.events().subscribe_light::<PlayerChatEvent>(
    token,
    EventPriority::High,
    |event, _token| {
        if event.message()?.contains("禁止内容") {
            event.cancel()?;
        }
        Ok(())
    },
)?;

ctx.events().unsubscribe(token, listener)?; // 可选，提前退订
```

- 优先级顺序为 `Highest > High > Normal > Low > Lowest`，与同服 Native 模组监听器一起排序。
- `cancel()` 立即生效并跳过后续监听器，已发生的副作用不回滚；取消 `PlayerJoinEvent` 仅中止 LL 初始化处理，不会踢出玩家。
- 回调支持捕获 `Rc` 等非 `Send` 状态；丢弃 `ListenerHandle` 不会自动退订，停用与卸载时宿主会自动清理。
- 回调返回 `Err` 只记日志，不隐式取消事件；回调 panic 会使该监听器被摘除。

### 触发线程同步事件

控制台与服务事件可能从任意线程触发，必须使用 `subscribe_sync::<E>()`（要求 `Fn + Send + Sync`）：

```rust
use ferrum_sdk::events::{ConsoleOutputtingEvent, EventPriority};

ctx.events().subscribe_sync::<ConsoleOutputtingEvent>(token, EventPriority::High, |event| {
    if event.message().starts_with("[private]") {
        event.cancel();
    }
    Ok(())
})?;
```

同一监听器在同一线程递归触发时自动跳过内层调用；回调可通过 `payload_snbt()` 读取事件快照（控制台事件额外提供 `message()` 与 `is_cancelled()`）。

## 自定义事件（Rust / C++ 互通）

自定义事件按名称标识，携带只读键值字段，并支持取消。Rust 端与 C++ Native 端（使用公开头文件）的监听器按优先级交错执行。

### 命名与字段规范

- 事件名必须以发布者模组名加 `.` 开头（如 `economy.balance_changing.v1`）；名称和字段名仅允许 ASCII 字母、数字及 `._-:`。
- 字段类型：`I64`、`U64`、`F64`、`Bool`、`String`（UTF-8）、`Bytes`、`Uuid`（16 字节）。
- 发布时必须提供全部已声明字段且类型匹配，顺序不限。

### Rust 订阅与发布

订阅可以早于发布者注册，发布者重载并重新注册同名事件后会自动恢复接收：

```rust
use ferrum_sdk::custom_events::{EventFieldType, EventValue, RegisteredEvent};
use ferrum_sdk::events::EventPriority;

// 订阅
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

// 注册与发布（on_enable 中注册，模组 Enabled 后发布）
let balance_changing: RegisteredEvent = ctx.custom_events().register(
    token,
    "economy.balance_changing.v1",
    true, // cancellable
    &[("old_balance", EventFieldType::I64), ("new_balance", EventFieldType::I64)],
)?;

let cancelled = balance_changing.publish(
    token,
    &[("old_balance", EventValue::I64(100)), ("new_balance", EventValue::I64(-10))],
)?;
if !cancelled {
    // 执行修改
}
```

### C++ Native 模组订阅

Native 模组在 `manifest.json` 中依赖 `ferrum-loader`，包含 `ferrum/CustomEvent.h`（位于 `cpp-shim/include/`）并通过 LeviLamina `EventBus` 监听 `ferrum::FerrumCustomEvent`，无需链接导入库：

```cpp
#include "ferrum/CustomEvent.h"
#include "ll/api/event/EventBus.h"
#include "ll/api/mod/NativeMod.h"

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
```

### C++ Native 模组发布

包含 `ferrum/NativeEvents.h`，通过 `GetProcAddress(GetModuleHandleW(L"ferrum-loader.dll"), "ferrum_native_event_api_v1")` 获取发布协议表：

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

bool allowBalanceChange(int64_t oldValue, int64_t newValue) {
    EventValueV1 values[]{
        {ffi("old_balance"), EventValueKind_I64, 0, static_cast<uint64_t>(oldValue), {}},
        {ffi("new_balance"), EventValueKind_I64, 0, static_cast<uint64_t>(newValue), {}},
    };
    FfiBool cancelled = FfiBool_FALSE;
    auto status = api->publish(provider, balanceChanging, {values, 2}, &cancelled);
    return status == FerrumStatus_OK && cancelled == FfiBool_FALSE;
}

void disableEvents() {
    if (provider) api->close_provider(provider);
    provider = {};
    balanceChanging = {};
}
```

Native 端须在主线程注册和发布，且仅在模组 `onEnable` 完成后发布；停用时调用 `close_provider` 撤销其注册的事件。`F64` 字段请用 `std::bit_cast<uint64_t>(value)` 填入 `scalar`。
