// 由 `cargo xtask cbindgen` 从 crates/ferrum-abi 与 crates/ferrum-host/src/shim.rs 生成。
// 严禁手改；修改 Rust 定义后重新生成。

#ifndef FERRUM_ABI_H
#define FERRUM_ABI_H

#pragma once

#include <stdint.h>
#include <stddef.h>

/**
 * 跨 FFI 的统一状态码（`0` 为成功）。
 *
 * 取值一经发布永久冻结；解码方必须容忍未知值。
 */
typedef int32_t FerrumStatus;
#define FerrumStatus_OK 0
#define FerrumStatus_INVALID_ARGUMENT 1
#define FerrumStatus_NULL_POINTER 2
#define FerrumStatus_INVALID_UTF8 3
#define FerrumStatus_BUFFER_TOO_SMALL 4
#define FerrumStatus_OUT_OF_MEMORY 5
/**
 * 宿主不支持该函数/字段，或游戏特性已移除
 */
#define FerrumStatus_UNSUPPORTED 6
/**
 * 一般性失败（如模组生命周期钩子返回的自定义错误），详情见 last-error
 */
#define FerrumStatus_FAILED 7
#define FerrumStatus_ABI_INCOMPATIBLE 100
/**
 * 模组已停用、停用中或已卸载
 */
#define FerrumStatus_MOD_DISABLED 101
/**
 * 违反主线程约束
 */
#define FerrumStatus_WRONG_THREAD 102
/**
 * `FnMut` 回调重入被守卫拦截
 */
#define FerrumStatus_REENTRANT_CALL 103
/**
 * 代数不匹配或已销毁
 */
#define FerrumStatus_STALE_HANDLE 200
/**
 * 跨模组越权操作他人句柄
 */
#define FerrumStatus_PERMISSION_DENIED 201
/**
 * 实体/玩家/维度/服务不存在或已离线
 */
#define FerrumStatus_NOT_FOUND 202
/**
 * Level 或 CommandRegistry 尚未初始化
 */
#define FerrumStatus_SERVICE_UNAVAILABLE 203
/**
 * 重复注册同名命令/枚举/服务
 */
#define FerrumStatus_ALREADY_EXISTS 204
/**
 * C++ 侧捕获到异常或 Expected 错误
 */
#define FerrumStatus_CPP_EXCEPTION 500
/**
 * 模组侧发生 Rust Panic
 */
#define FerrumStatus_MOD_PANICKED 501
/**
 * 回调此前 panic 过，已被毒化
 */
#define FerrumStatus_CALLBACK_POISONED 502
#define FerrumStatus_INTERNAL_ERROR 599

/**
 * UTF-8 只读借用字符串切片（不保证 `'\0'` 结尾）。
 *
 * 仅在本次同步调用返回前有效。宿主 → 模组方向**不保证**是合法 UTF-8。
 */
typedef struct FfiStr {
  const uint8_t *ptr;
  size_t len;
} FfiStr;

/**
 * 64 位非零代数句柄：低 32 位为槽位下标，高 32 位为代数（恒非零）。
 */
typedef uint64_t GenerationalHandle;

typedef GenerationalHandle EventProviderHandle;

/**
 * 跨 FFI 布尔值：`0` 为 false，任何非 `0` 值为 true。
 */
typedef uint8_t FfiBool;
#define FfiBool_FALSE 0
#define FfiBool_TRUE 1

typedef uint32_t EventValueKind;
#define EventValueKind_I64 0
#define EventValueKind_U64 1
#define EventValueKind_F64 2
#define EventValueKind_BOOL 3
#define EventValueKind_STRING 4
#define EventValueKind_BYTES 5
#define EventValueKind_UUID 6

/**
 * 冻结的字段描述；name 和 kind 在事件版本内不变。
 */
typedef struct EventFieldV1 {
  struct FfiStr name;
  EventValueKind kind;
  uint32_t reserved;
} EventFieldV1;

/**
 * 连续元素的只读借用切片（`T` 必须为 `#[repr(C)]` 且无未初始化填充字节）。
 */
typedef struct FfiSlice_EventFieldV1 {
  const struct EventFieldV1 *ptr;
  size_t len;
} FfiSlice_EventFieldV1;

typedef struct CustomEventDefinitionV1 {
  uint32_t struct_size;
  /**
   * 完整名称必须以发布者模组名加点号开头，如 economy.balance_changing.v1。
   */
  struct FfiStr name;
  FfiBool cancellable;
  struct FfiSlice_EventFieldV1 fields;
} CustomEventDefinitionV1;

typedef GenerationalHandle CustomEventHandle;

/**
 * 连续元素的只读借用切片（`T` 必须为 `#[repr(C)]` 且无未初始化填充字节）。
 */
typedef struct FfiSlice_u8 {
  const uint8_t *ptr;
  size_t len;
} FfiSlice_u8;

/**
 * 冻结的发布字段。I64 为二进制补码，F64 为 IEEE 754 位模式，BOOL 为零/非零。
 * STRING/BYTES/UUID 使用 bytes（UUID 恰为 16 字节），其余使用 scalar。
 * 所有借用数据在 publish 返回前有效；宿主在调用任何监听器前复制载荷。
 */
typedef struct EventValueV1 {
  struct FfiStr name;
  EventValueKind kind;
  uint32_t reserved;
  uint64_t scalar;
  struct FfiSlice_u8 bytes;
} EventValueV1;

/**
 * 连续元素的只读借用切片（`T` 必须为 `#[repr(C)]` 且无未初始化填充字节）。
 */
typedef struct FfiSlice_EventValueV1 {
  const struct EventValueV1 *ptr;
  size_t len;
} FfiSlice_EventValueV1;

/**
 * 写入器回调：在调用方（通常是模组）堆上追加 `bytes`。
 */
typedef FerrumStatus (*FfiWriteFn)(void *context, const uint8_t *bytes, size_t len);

/**
 * 由调用方在栈上构造、传给被调用方用于输出变长数据的写入器。
 *
 * - 被调用方可多次调用 `write_fn`（追加语义）；
 * - `write_fn` 返回非 `OK` 时，被调用方必须立即停止写入并原样返回该状态；
 * - 被调用方不得在调用返回后保留本结构或其 `context`。
 */
typedef struct FfiByteWriter {
  void *context;
  FfiWriteFn write_fn;
} FfiByteWriter;

/**
 * 原生发布端协议。注册及发布均要求主线程；函数表由 loader 导出入口取得。
 * 关闭发布者立即撤销其全部事件；再次启用需重新注册。宿主不保存原生函数指针。
 */
typedef struct NativeEventApiV1 {
  uint32_t struct_size;
  FerrumStatus (*open_provider)(struct FfiStr, EventProviderHandle*);
  FerrumStatus (*close_provider)(EventProviderHandle);
  FerrumStatus (*register_event)(EventProviderHandle,
                                 const struct CustomEventDefinitionV1*,
                                 CustomEventHandle*);
  FerrumStatus (*unregister_event)(EventProviderHandle, CustomEventHandle);
  /**
   * OK 时完整写出取消结果；回调错误被隔离。发布者被重入撤销时返回 MOD_DISABLED。
   */
  FerrumStatus (*publish)(EventProviderHandle,
                          CustomEventHandle,
                          struct FfiSlice_EventValueV1,
                          FfiBool*);
  FerrumStatus (*take_last_error)(struct FfiByteWriter*);
} NativeEventApiV1;

/**
 * 仅在当前监听器的同步调用期间有效。
 */
typedef GenerationalHandle EventScopeHandle;

/**
 * 标准 UUID 网络字节序，布局永久冻结。
 */
typedef struct PlayerUuid {
  uint8_t bytes[16];
} PlayerUuid;

/**
 * shim 与原生 LL 监听器的作用域访问表；不包含游戏对象指针。
 * 作用域只在同步 publish 期间有效，保存后再次访问返回 STALE_HANDLE。
 */
typedef struct CustomEventViewApiV1 {
  uint32_t struct_size;
  FerrumStatus (*name)(EventScopeHandle, struct FfiByteWriter*);
  FerrumStatus (*get_i64)(EventScopeHandle, struct FfiStr, int64_t*);
  FerrumStatus (*get_u64)(EventScopeHandle, struct FfiStr, uint64_t*);
  FerrumStatus (*get_f64)(EventScopeHandle, struct FfiStr, double*);
  FerrumStatus (*get_bool)(EventScopeHandle, struct FfiStr, FfiBool*);
  FerrumStatus (*get_string)(EventScopeHandle, struct FfiStr, struct FfiByteWriter*);
  FerrumStatus (*get_bytes)(EventScopeHandle, struct FfiStr, struct FfiByteWriter*);
  FerrumStatus (*get_uuid)(EventScopeHandle, struct FfiStr, struct PlayerUuid*);
  FerrumStatus (*is_cancelled)(EventScopeHandle, FfiBool*);
  FerrumStatus (*cancel)(EventScopeHandle);
  FerrumStatus (*is_active)(EventScopeHandle);
} CustomEventViewApiV1;

typedef GenerationalHandle ListenerHandle;

/**
 * `cpp-shim` 为每个 `FerrumMod` 提供的不透明 cookie（指向 C++ 侧对象）。
 *
 * 宿主只原样回传，绝不解引用；模组侧永远看不到它。
 */
typedef void *ShimModCookie;

/**
 * 日志级别（与 `ll::io::LogLevel` 的非 `Off` 取值一一对应）。
 */
typedef uint32_t LogLevel;
#define LogLevel_FATAL 0
#define LogLevel_ERROR 1
#define LogLevel_WARN 2
#define LogLevel_INFO 3
#define LogLevel_DEBUG 4
#define LogLevel_TRACE 5

/**
 * 当次会话内的实体标识；跨 Tick 保存请使用 ActorUniqueId 或 PlayerUuid。
 */
typedef uint64_t ActorRuntimeId;

typedef struct FerrumVec3 {
  float x;
  float y;
  float z;
} FerrumVec3;

typedef int32_t FerrumDimensionId;
#define FerrumDimensionId_OVERWORLD 0
#define FerrumDimensionId_NETHER 1
#define FerrumDimensionId_THE_END 2

/**
 * Ferrum 的稳定游戏模式码，未知值必须保留而非转为封闭枚举。
 */
typedef uint32_t FerrumGameMode;
#define FerrumGameMode_SURVIVAL 0
#define FerrumGameMode_CREATIVE 1
#define FerrumGameMode_ADVENTURE 2
#define FerrumGameMode_SPECTATOR 3
#define FerrumGameMode_UNKNOWN UINT32_MAX

/**
 * 一次解析玩家后读取的值，布局固定，可在回调结束后保留。
 */
typedef struct PlayerSnapshot {
  struct FerrumVec3 position;
  FerrumDimensionId dimension;
  float health;
} PlayerSnapshot;

/**
 * ABI 1.1 玩家接口；所有操作仅允许在主线程调用。
 */
typedef struct PlayerApiV1 {
  uint32_t struct_size;
  FerrumStatus (*find_by_name)(struct FfiStr, ActorRuntimeId*);
  FerrumStatus (*find_by_uuid)(struct PlayerUuid, ActorRuntimeId*);
  /**
   * 输出逐个小端编码的 u64 runtime ID，不输出宿主容器。
   */
  FerrumStatus (*online_players)(struct FfiByteWriter*);
  FerrumStatus (*real_name)(ActorRuntimeId, struct FfiByteWriter*);
  FerrumStatus (*uuid)(ActorRuntimeId, struct PlayerUuid*);
  FerrumStatus (*send_message)(ActorRuntimeId, struct FfiStr);
  FerrumStatus (*is_operator)(ActorRuntimeId, FfiBool*);
  FerrumStatus (*position)(ActorRuntimeId, struct FerrumVec3*);
  FerrumStatus (*dimension)(ActorRuntimeId, FerrumDimensionId*);
  /**
   * 目标维度必须已存在，传送保留朝向，坐标必须有限。
   */
  FerrumStatus (*teleport)(ActorRuntimeId, struct FerrumVec3, FerrumDimensionId);
  FerrumStatus (*health)(ActorRuntimeId, float*);
  FerrumStatus (*max_health)(ActorRuntimeId, float*);
  /**
   * 直接设置属性，不模拟伤害/治疗事件；范围为 0..=max_health。
   */
  FerrumStatus (*set_health)(ActorRuntimeId, float);
  FerrumStatus (*game_mode)(ActorRuntimeId, FerrumGameMode*);
  FerrumStatus (*set_game_mode)(ActorRuntimeId, FerrumGameMode);
  FerrumStatus (*xp_earned_at_current_level)(ActorRuntimeId, int32_t*);
  FerrumStatus (*xp_needed_for_next_level)(ActorRuntimeId, int32_t*);
  FerrumStatus (*add_experience)(ActorRuntimeId, int32_t);
  FerrumStatus (*is_hungry)(ActorRuntimeId, FfiBool*);
  FerrumStatus (*snapshot)(ActorRuntimeId, struct PlayerSnapshot*);
} PlayerApiV1;

typedef GenerationalHandle CommandHandle;

typedef uint32_t CommandPermission;
#define CommandPermission_ANY 0
#define CommandPermission_GAME_DIRECTORS 1
#define CommandPermission_ADMIN 2
#define CommandPermission_HOST 3
#define CommandPermission_OWNER 4

typedef uint32_t CommandParamKind;
#define CommandParamKind_INT 0
#define CommandParamKind_FLOAT 1
#define CommandParamKind_BOOL 2
#define CommandParamKind_STRING 3
#define CommandParamKind_PLAYER 4

/**
 * 冻结的参数描述；显式保留字节使切片元素无未初始化填充。
 */
typedef struct CommandParamV1 {
  struct FfiStr name;
  CommandParamKind kind;
  FfiBool optional;
  uint8_t reserved[3];
} CommandParamV1;

/**
 * 连续元素的只读借用切片（`T` 必须为 `#[repr(C)]` 且无未初始化填充字节）。
 */
typedef struct FfiSlice_CommandParamV1 {
  const struct CommandParamV1 *ptr;
  size_t len;
} FfiSlice_CommandParamV1;

typedef struct CommandDefinitionV1 {
  uint32_t struct_size;
  struct FfiStr name;
  struct FfiStr description;
  CommandPermission permission;
  struct FfiSlice_CommandParamV1 params;
} CommandDefinitionV1;

/**
 * 方块整数坐标，按值传递且布局冻结；实际访问仍需主线程令牌。
 */
typedef struct FerrumBlockPos {
  int32_t x;
  int32_t y;
  int32_t z;
} FerrumBlockPos;

/**
 * 当前进程/注册表的状态 ID，不能落盘或跨版本保存。
 */
typedef uint32_t BlockRuntimeId;

/**
 * 主方块层；按维度/坐标实时解析，只访问已加载区块。
 */
typedef struct BlockApiV1 {
  uint32_t struct_size;
  FerrumStatus (*type_name)(FerrumDimensionId, struct FerrumBlockPos, struct FfiByteWriter*);
  FerrumStatus (*runtime_id)(FerrumDimensionId, struct FerrumBlockPos, BlockRuntimeId*);
  FerrumStatus (*is_air)(FerrumDimensionId, struct FerrumBlockPos, FfiBool*);
  /**
   * 按名称放置默认状态，更新邻居并同步客户端；不生成破坏掉落。
   */
  FerrumStatus (*set_type)(FerrumDimensionId, struct FerrumBlockPos, struct FfiStr);
} BlockApiV1;

typedef int64_t ActorUniqueId;

/**
 * Ferrum 能力类别，不暴露 MC 内部枚举；未知值不得转为封闭枚举。
 */
typedef uint32_t ActorKind;
#define ActorKind_ACTOR 0
#define ActorKind_MOB 1
#define ActorKind_PLAYER 2

typedef struct ActorApiV1 {
  uint32_t struct_size;
  FerrumStatus (*find_by_unique_id)(ActorUniqueId, ActorRuntimeId*);
  FerrumStatus (*unique_id)(ActorRuntimeId, ActorUniqueId*);
  FerrumStatus (*type_name)(ActorRuntimeId, struct FfiByteWriter*);
  FerrumStatus (*kind)(ActorRuntimeId, ActorKind*);
  FerrumStatus (*position)(ActorRuntimeId, struct FerrumVec3*);
  FerrumStatus (*dimension)(ActorRuntimeId, FerrumDimensionId*);
  FerrumStatus (*teleport)(ActorRuntimeId, struct FerrumVec3, FerrumDimensionId);
  FerrumStatus (*name_tag)(ActorRuntimeId, struct FfiByteWriter*);
  FerrumStatus (*set_name_tag)(ActorRuntimeId, struct FfiStr);
  FerrumStatus (*is_on_ground)(ActorRuntimeId, FfiBool*);
  FerrumStatus (*is_in_water)(ActorRuntimeId, FfiBool*);
} ActorApiV1;

/**
 * 生物能力。非生物返回 INVALID_ARGUMENT；目标消失返回 NOT_FOUND。
 */
typedef struct MobApiV1 {
  uint32_t struct_size;
  FerrumStatus (*health)(ActorRuntimeId, float*);
  FerrumStatus (*max_health)(ActorRuntimeId, float*);
  FerrumStatus (*set_health)(ActorRuntimeId, float);
  FerrumStatus (*is_sprinting)(ActorRuntimeId, FfiBool*);
  FerrumStatus (*set_sprinting)(ActorRuntimeId, FfiBool);
} MobApiV1;

typedef uint32_t EventKind;
#define EventKind_PLAYER_JOIN 0
#define EventKind_PLAYER_DISCONNECT 1
#define EventKind_PLAYER_CHAT 2
#define EventKind_EXECUTING_COMMAND 3
#define EventKind_EXECUTED_COMMAND 4
#define EventKind_SERVER_COMMAND_REGISTER 5
#define EventKind_ACTOR_HURT 6
#define EventKind_MOB_DIE 7
#define EventKind_CONSOLE_OUTPUTTING 8
#define EventKind_CONSOLE_OUTPUTTED 9
#define EventKind_PLAYER_ADD_EXPERIENCE 10
#define EventKind_PLAYER_ATTACK 11
#define EventKind_PLAYER_CHANGE_PERM 12
#define EventKind_PLAYER_CONNECT 13
#define EventKind_PLAYER_DESTROY_BLOCK 14
#define EventKind_PLAYER_DIE 15
#define EventKind_PLAYER_INTERACT_BLOCK 16
#define EventKind_PLAYER_JUMP 17
#define EventKind_PLAYER_PICK_UP_ITEM 18
#define EventKind_PLAYER_PLACING_BLOCK 19
#define EventKind_PLAYER_PLACED_BLOCK 20
#define EventKind_PLAYER_RESPAWN 21
#define EventKind_PLAYER_SNEAKING 22
#define EventKind_PLAYER_SNEAKED 23
#define EventKind_PLAYER_SPRINTING 24
#define EventKind_PLAYER_SPRINTED 25
#define EventKind_PLAYER_SWING 26
#define EventKind_PLAYER_USE_ITEM 27
#define EventKind_SERVER_STARTED 28
#define EventKind_SERVER_STOPPING 29
#define EventKind_SERVICE_REGISTER 30
#define EventKind_SERVICE_UNREGISTER 31
#define EventKind_BLOCK_CHANGED 32
#define EventKind_FIRE_SPREAD 33
#define EventKind_SERVER_LEVEL_TICK 34
#define EventKind_SPAWNING_MOB 35
#define EventKind_SPAWNED_MOB 36

typedef uint32_t EventPriority;
#define EventPriority_HIGHEST 0
#define EventPriority_HIGH 100
#define EventPriority_NORMAL 200
#define EventPriority_LOW 300
#define EventPriority_LOWEST 400

typedef struct EventSubscriptionV1 {
  uint32_t struct_size;
  EventKind kind;
  EventPriority priority;
  /**
   * 完整订阅在监听器入口采集 NBT。触发线程事件必须设为真。
   */
  FfiBool include_payload;
} EventSubscriptionV1;

typedef GenerationalHandle CustomListenerHandle;

/**
 * LegacyMoney 经济接口。金额为有符号 64 位整数，账户使用 XUID 字符串。
 */
typedef struct LlMoneyApiV1 {
  uint32_t struct_size;
  FerrumStatus (*get)(struct FfiStr, int64_t*);
  FerrumStatus (*set)(struct FfiStr, int64_t);
  FerrumStatus (*add)(struct FfiStr, int64_t);
  FerrumStatus (*reduce)(struct FfiStr, int64_t);
  FerrumStatus (*transfer)(struct FfiStr, struct FfiStr, int64_t, struct FfiStr);
  FerrumStatus (*history)(struct FfiStr, int32_t, struct FfiByteWriter*);
  FerrumStatus (*clear_history)(int32_t);
  /**
   * 每行依次为小端 i64 余额、小端 u64 XUID 字节数、UTF-8 XUID。
   */
  FerrumStatus (*ranking)(uint16_t, struct FfiByteWriter*);
} LlMoneyApiV1;

/**
 * `cpp-shim` 在 `ferrum_host_init` 时注入的函数表。全部函数必须 `noexcept`。
 */
typedef struct FerrumShimApi {
  uint32_t struct_size;
  /**
   * 写入模组专属 `ll::io::Logger`（任意线程）。
   */
  void (*log)(ShimModCookie cookie, LogLevel level, struct FfiStr message);
  /**
   * 模组 Logger 是否会输出该级别。
   */
  FfiBool (*should_log)(ShimModCookie cookie, LogLevel level);
  /**
   * 写入 `ferrum-loader` 自身的 Logger（任意线程）。
   */
  void (*log_loader)(LogLevel level, struct FfiStr message);
  /**
   * 请求在下一个主线程切片停用该模组（熔断用，任意线程）。
   */
  void (*request_disable)(ShimModCookie cookie);
  /**
   * 宿主不再使用该 cookie（最后一个 `Arc<ModEntry>` 释放时调用，任意线程）。
   */
  void (*release_cookie)(ShimModCookie cookie);
  struct PlayerApiV1 player_v1;
  FerrumStatus (*command_register)(ShimModCookie, CommandHandle, const struct CommandDefinitionV1*);
  FerrumStatus (*command_unregister)(ShimModCookie, CommandHandle);
  FerrumStatus (*command_origin)(uint64_t, ActorRuntimeId*);
  FerrumStatus (*command_int)(uint64_t, struct FfiStr, int32_t*);
  FerrumStatus (*command_float)(uint64_t, struct FfiStr, float*);
  FerrumStatus (*command_bool)(uint64_t, struct FfiStr, FfiBool*);
  FerrumStatus (*command_string)(uint64_t, struct FfiStr, struct FfiByteWriter*);
  FerrumStatus (*command_players)(uint64_t, struct FfiStr, struct FfiByteWriter*);
  FerrumStatus (*command_output)(uint64_t, FfiBool, struct FfiStr);
  struct BlockApiV1 block_v1;
  struct ActorApiV1 actor_v1;
  struct MobApiV1 mob_v1;
  FerrumStatus (*event_subscribe)(ShimModCookie, ListenerHandle, const struct EventSubscriptionV1*);
  FerrumStatus (*event_unsubscribe)(ShimModCookie, ListenerHandle);
  FerrumStatus (*event_player)(uint64_t, ActorRuntimeId*);
  FerrumStatus (*event_is_cancelled)(uint64_t, FfiBool*);
  FerrumStatus (*event_cancel)(uint64_t);
  FerrumStatus (*event_chat_message)(uint64_t, struct FfiByteWriter*);
  FerrumStatus (*custom_event_publish)(EventScopeHandle, FfiBool*);
  FerrumStatus (*custom_event_subscribe)(ShimModCookie,
                                         CustomListenerHandle,
                                         struct FfiStr,
                                         EventPriority);
  FerrumStatus (*custom_event_unsubscribe)(CustomListenerHandle);
  FerrumStatus (*event_payload_snbt)(uint64_t, struct FfiByteWriter*);
  FerrumStatus (*event_actor)(uint64_t, ActorRuntimeId*);
  /**
   * 返回 shim 内进程期有效的静态适配表。
   */
  const struct LlMoneyApiV1 *(*llmoney_v1)(void);
  FerrumStatus (*form_send)(uint64_t, ActorRuntimeId, struct FfiStr);
} FerrumShimApi;

typedef struct AbiVersion {
  uint16_t major;
  uint16_t minor;
  uint32_t patch;
} AbiVersion;

/**
 * 0..=255 保留给基岩版的取消原因。
 */
typedef uint32_t FormResponseKind;
#define FormResponseKind_USER_CLOSED 0
#define FormResponseKind_USER_BUSY 1
#define FormResponseKind_CANCELLED 256
#define FormResponseKind_SUBMITTED 257
#define FormResponseKind_DISCONNECTED 258

/**
 * `cpp-shim` 调用 `ferrum_host_load_mod` 时提供的模组信息（所有字符串为 UTF-8，仅本次调用内有效）。
 */
typedef struct ShimModInfo {
  uint32_t struct_size;
  ShimModCookie cookie;
  struct FfiStr name;
  struct FfiStr version;
  /**
   * `entry` 指向的 `.dll` 绝对路径
   */
  struct FfiStr entry_path;
  struct FfiStr mod_dir;
  struct FfiStr data_dir;
  struct FfiStr config_dir;
  struct FfiStr lang_dir;
} ShimModInfo;

/**
 * 宿主为每个已加载模组分配的上下文句柄，宿主据此识别调用方并校验归属。
 */
typedef GenerationalHandle ModContextHandle;

/**
 * 模组目录种类（对应 `ll::mod::Mod::get*Dir`）。
 */
typedef uint32_t ModDirKind;
/**
 * `mods/<name>/`
 */
#define ModDirKind_MOD 0
/**
 * `mods/<name>/data/`
 */
#define ModDirKind_DATA 1
/**
 * `mods/<name>/config/`
 */
#define ModDirKind_CONFIG 2
/**
 * `mods/<name>/lang/`
 */
#define ModDirKind_LANG 3

/**
 * 核心子表：last-error、panic 上报、模组元数据与目录、线程判定。
 */
typedef struct CoreApiV1 {
  uint32_t struct_size;
  /**
   * 取回并清空当前线程的 `LastErrorSlot`；无错误时写入 0 字节并返回 `OK`。
   */
  FerrumStatus (*take_last_error)(struct FfiByteWriter *out);
  /**
   * 模组从回调返回非 `OK` 前写入当前线程的 `LastErrorSlot`，供宿主输出详情。
   */
  FerrumStatus (*set_last_error)(struct FfiStr message);
  /**
   * 模组在回调中捕获 panic 后上报（`location` 可为空）。
   */
  FerrumStatus (*report_panic)(ModContextHandle ctx, struct FfiStr message, struct FfiStr location);
  /**
   * 模组在 `manifest.json` 中声明的 `name`。
   */
  FerrumStatus (*get_mod_name)(ModContextHandle ctx, struct FfiByteWriter *out);
  /**
   * 模组版本（`manifest.json` 中的 `version`，缺省为 `0.0.0`）。
   */
  FerrumStatus (*get_mod_version)(ModContextHandle ctx, struct FfiByteWriter *out);
  /**
   * 以 UTF-8 写出模组目录的绝对路径。
   */
  FerrumStatus (*get_mod_dir)(ModContextHandle ctx, ModDirKind kind, struct FfiByteWriter *out);
  /**
   * 当前线程是否为 BDS 主线程（服务端逻辑线程）。
   */
  FfiBool (*is_main_thread)(void);
} CoreApiV1;

/**
 * 日志子表：写入该模组专属的 `ll::io::Logger`（任意线程安全）。
 */
typedef struct LogApiV1 {
  uint32_t struct_size;
  FerrumStatus (*log)(ModContextHandle ctx, LogLevel level, struct FfiStr message);
  FfiBool (*should_log)(ModContextHandle ctx, LogLevel level);
} LogApiV1;

/**
 * 按接口标识与最小次版本查询扩展子表；未找到返回空指针。
 */
typedef const void *(*QueryInterfaceFn)(struct FfiStr interface_id_utf8, uint16_t min_minor);

/**
 * 宿主在加载模组时传入的全局根表（进程级 `'static`）。
 *
 * 后续 minor 版本只允许在末尾追加字段；领域子表（事件、命令、玩家……）
 * 将按里程碑逐个追加，或通过 `query_interface` 暴露。
 */
typedef struct FerrumHostApi {
  uint32_t struct_size;
  uint32_t reserved_flags;
  uint64_t magic;
  struct AbiVersion host_abi_version;
  const struct CoreApiV1 *core_v1;
  const struct LogApiV1 *log_v1;
  QueryInterfaceFn query_interface;
} FerrumHostApi;

typedef uint32_t FerrumModFlags;
#define FerrumModFlags_NONE 0
/**
 * 模组声明自身可被 `FreeLibrary`（见 `docs/design/threading.md`）。
 */
#define FerrumModFlags_HOT_UNLOAD_SAFE (1 << 0)

/**
 * “此调用发生在主线程”的显式证明（ABI 层占位，按值传递）。
 *
 * 仅宿主在确认当前线程为主线程后构造；SDK 在跳板中把它转换为 `&MainThreadToken<'tick>`。
 */
typedef struct RawMainThreadToken {
  uint8_t _reserved;
} RawMainThreadToken;
/**
 * 只能由宿主在主线程上调用。
 */
#define RawMainThreadToken_ASSERTED (RawMainThreadToken){ ._reserved = 0 }

/**
 * 模组向宿主回填的生命周期回调描述符。
 */
typedef struct FerrumModDescriptor {
  uint32_t struct_size;
  FerrumModFlags flags;
  /**
   * 模组运行所需的最低宿主 ABI 版本（按需取值，而非编译所用 SDK 的版本）。
   */
  struct AbiVersion required_abi_version;
  /**
   * 编译该模组所用的 ferrum-sdk 版本，仅用于诊断日志。
   */
  struct AbiVersion sdk_version;
  FerrumStatus (*on_load)(ModContextHandle mod_ctx);
  FerrumStatus (*on_enable)(ModContextHandle mod_ctx, struct RawMainThreadToken token);
  FerrumStatus (*on_disable)(ModContextHandle mod_ctx, struct RawMainThreadToken token);
  FerrumStatus (*on_unload)(ModContextHandle mod_ctx);
} FerrumModDescriptor;

/**
 * `ferrum_mod_entry_v1` 的签名。
 *
 * 模组校验 `host_api` 后把描述符写入 `out_descriptor`。
 *
 * 容量协商：宿主调用前把 `out_descriptor.struct_size` 预填为缓冲区字节数；
 * 模组最多写入 `min(预填值, 自身描述符大小)` 字节，并把实际写入量回填到 `struct_size`。
 * 预填 `0` 视为 [`FERRUM_MOD_DESCRIPTOR_MIN_SIZE_V1_0`]（兼容不预填的旧宿主）。
 */
typedef FerrumStatus (*ModEntryFnV1)(const struct FerrumHostApi *host_api,
                                     ModContextHandle mod_ctx,
                                     struct FerrumModDescriptor *out_descriptor);

/**
 * 只在一次同步命令执行期间有效。
 */
typedef GenerationalHandle CommandScopeHandle;

/**
 * 多次调用；注册返回 OK 才转移 env 所有权，失败时仍由调用方释放。
 * invoke_fn 和 drop_fn 只会在主线程、宿主未持锁时执行。
 */
typedef struct CommandCallbackV1 {
  void *env;
  FerrumStatus (*invoke_fn)(void*, CommandScopeHandle, struct RawMainThreadToken);
  void (*drop_fn)(void*);
} CommandCallbackV1;

/**
 * ABI 1.1 命令接口；scope 的每次访问同时校验调用模组与主线程。
 */
typedef struct CommandApiV1 {
  uint32_t struct_size;
  FerrumStatus (*register_)(ModContextHandle,
                            const struct CommandDefinitionV1*,
                            struct CommandCallbackV1,
                            CommandHandle*);
  FerrumStatus (*unregister)(ModContextHandle, CommandHandle);
  FerrumStatus (*origin_player)(ModContextHandle, CommandScopeHandle, ActorRuntimeId*);
  /**
   * 未提供的可选参数返回 NOT_FOUND；类型不符返回 INVALID_ARGUMENT。
   */
  FerrumStatus (*get_int)(ModContextHandle, CommandScopeHandle, struct FfiStr, int32_t*);
  FerrumStatus (*get_float)(ModContextHandle, CommandScopeHandle, struct FfiStr, float*);
  FerrumStatus (*get_bool)(ModContextHandle, CommandScopeHandle, struct FfiStr, FfiBool*);
  FerrumStatus (*get_string)(ModContextHandle,
                             CommandScopeHandle,
                             struct FfiStr,
                             struct FfiByteWriter*);
  /**
   * 选择器在每次读取时重新解析，输出与 online_players 相同。
   */
  FerrumStatus (*get_players)(ModContextHandle,
                              CommandScopeHandle,
                              struct FfiStr,
                              struct FfiByteWriter*);
  FerrumStatus (*output)(ModContextHandle, CommandScopeHandle, FfiBool, struct FfiStr);
} CommandApiV1;

/**
 * 注册成功后宿主接管 env。invoke_fn 和 drop_fn 仅在服务端主线程串行调用。
 */
typedef struct TaskCallbackV1 {
  void *env;
  FerrumStatus (*invoke_fn)(void*, struct RawMainThreadToken);
  void (*drop_fn)(void*);
} TaskCallbackV1;

typedef GenerationalHandle TaskHandle;

typedef struct SchedulerApiV1 {
  uint32_t struct_size;
  /**
   * 任意线程投递，下一 Tick 执行一次。
   */
  FerrumStatus (*post)(ModContextHandle, struct TaskCallbackV1, TaskHandle*);
  /**
   * 主线程注册，零延迟表示下一 Tick。
   */
  FerrumStatus (*after_ticks)(ModContextHandle, uint64_t, struct TaskCallbackV1, TaskHandle*);
  /**
   * 主线程注册，间隔必须大于零。
   */
  FerrumStatus (*every_ticks)(ModContextHandle, uint64_t, struct TaskCallbackV1, TaskHandle*);
  /**
   * 主线程取消本模组的任务。正在执行的回调可以取消自身。
   */
  FerrumStatus (*cancel)(ModContextHandle, TaskHandle);
} SchedulerApiV1;

/**
 * 发送成功后由宿主接管 env。回调最多调用一次，响应视图仅在回调期间有效。
 */
typedef struct FormCallbackV1 {
  void *env;
  FerrumStatus (*invoke_fn)(void*,
                            struct RawMainThreadToken,
                            ActorRuntimeId,
                            FormResponseKind,
                            struct FfiStr);
  void (*drop_fn)(void*);
} FormCallbackV1;

typedef struct FormApiV1 {
  uint32_t struct_size;
  /**
   * 主线程发送一张表单，json 在本次调用内借用。
   */
  FerrumStatus (*send)(ModContextHandle, ActorRuntimeId, struct FfiStr, struct FormCallbackV1);
} FormApiV1;

/**
 * 注册成功才接管 env；调用与析构都在主线程且不持有宿主锁。
 */
typedef struct EventCallbackV1 {
  void *env;
  FerrumStatus (*invoke_fn)(void*, EventScopeHandle, struct RawMainThreadToken);
  void (*drop_fn)(void*);
} EventCallbackV1;

/**
 * 触发线程同步调用。两个字符串分别为 SNBT 和控制台消息。取消请求由调用者初始化为假。
 */
typedef struct ThreadEventCallbackV1 {
  void *env;
  FerrumStatus (*invoke_fn)(void*, struct FfiStr, struct FfiStr, FfiBool, FfiBool*);
  void (*drop_fn)(void*);
} ThreadEventCallbackV1;

/**
 * 主线程事件接口，通过 ferrum.event.v1 查询。取消会停止 LL 的后续监听。
 */
typedef struct EventApiV1 {
  uint32_t struct_size;
  FerrumStatus (*subscribe)(ModContextHandle,
                            const struct EventSubscriptionV1*,
                            struct EventCallbackV1,
                            ListenerHandle*);
  FerrumStatus (*unsubscribe)(ModContextHandle, ListenerHandle);
  FerrumStatus (*player)(ModContextHandle, EventScopeHandle, ActorRuntimeId*);
  FerrumStatus (*is_cancelled)(ModContextHandle, EventScopeHandle, FfiBool*);
  FerrumStatus (*cancel)(ModContextHandle, EventScopeHandle);
  FerrumStatus (*chat_message)(ModContextHandle, EventScopeHandle, struct FfiByteWriter*);
  FerrumStatus (*payload_snbt)(ModContextHandle, EventScopeHandle, struct FfiByteWriter*);
  FerrumStatus (*actor)(ModContextHandle, EventScopeHandle, ActorRuntimeId*);
  FerrumStatus (*subscribe_sync)(ModContextHandle,
                                 const struct EventSubscriptionV1*,
                                 struct ThreadEventCallbackV1,
                                 ListenerHandle*);
} EventApiV1;

/**
 * Rust 订阅端；按名称订阅允许早于原生发布者注册，发布者重载后继续接收同名事件。
 */
typedef struct CustomEventApiV1 {
  uint32_t struct_size;
  FerrumStatus (*subscribe)(ModContextHandle,
                            struct FfiStr,
                            EventPriority,
                            struct EventCallbackV1,
                            CustomListenerHandle*);
  FerrumStatus (*unsubscribe)(ModContextHandle, CustomListenerHandle);
  FerrumStatus (*name)(ModContextHandle, EventScopeHandle, struct FfiByteWriter*);
  FerrumStatus (*get_i64)(ModContextHandle, EventScopeHandle, struct FfiStr, int64_t*);
  FerrumStatus (*get_u64)(ModContextHandle, EventScopeHandle, struct FfiStr, uint64_t*);
  FerrumStatus (*get_f64)(ModContextHandle, EventScopeHandle, struct FfiStr, double*);
  FerrumStatus (*get_bool)(ModContextHandle, EventScopeHandle, struct FfiStr, FfiBool*);
  FerrumStatus (*get_string)(ModContextHandle,
                             EventScopeHandle,
                             struct FfiStr,
                             struct FfiByteWriter*);
  FerrumStatus (*get_bytes)(ModContextHandle, EventScopeHandle, struct FfiStr, struct FfiByteWriter*);
  FerrumStatus (*get_uuid)(ModContextHandle, EventScopeHandle, struct FfiStr, struct PlayerUuid*);
  FerrumStatus (*is_cancelled)(ModContextHandle, EventScopeHandle, FfiBool*);
  FerrumStatus (*cancel)(ModContextHandle, EventScopeHandle);
} CustomEventApiV1;

/**
 * Rust 自定义事件发布端，独立查询 ferrum.custom_event_publisher.v1。
 */
typedef struct CustomEventPublisherApiV1 {
  uint32_t struct_size;
  /**
   * 注册到当前 Rust 模组名下；启用失败、停用与卸载时自动撤销。
   */
  FerrumStatus (*register_event)(ModContextHandle,
                                 const struct CustomEventDefinitionV1*,
                                 CustomEventHandle*);
  FerrumStatus (*unregister_event)(ModContextHandle, CustomEventHandle);
  /**
   * 同步派发，OK 时写出取消结果。仅已启用模组可以发布。
   */
  FerrumStatus (*publish)(ModContextHandle,
                          CustomEventHandle,
                          struct FfiSlice_EventValueV1,
                          FfiBool*);
} CustomEventPublisherApiV1;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * cpp-shim 只传入自身栈帧的数字标识，游戏指针留在 C++ 内。
 */
FerrumStatus ferrum_host_dispatch_command(uint64_t raw, uint64_t shim_scope);

/**
 * 仅供 shim 获取原生发布端的内部表；公开入口由 C++ shim 做原生模组身份与状态检查。
 */
const struct NativeEventApiV1 *ferrum_host_native_event_api(void);

const struct CustomEventViewApiV1 *ferrum_host_custom_event_view_api(void);

FerrumStatus ferrum_host_dispatch_custom_listener(uint64_t raw, EventScopeHandle scope);

/**
 * shim 仅传递栈上事件的数字索引，C++ 对象指针不会进入 Rust。
 */
FerrumStatus ferrum_host_dispatch_event(uint64_t raw, uint64_t shim_scope);

/**
 * 两个文本及取消输出只在同步派发期间借用。该路径永远不发放主线程令牌。
 */
FerrumStatus ferrum_host_dispatch_thread_event(ListenerHandle handle,
                                               struct FfiStr payload,
                                               struct FfiStr message,
                                               FfiBool cancelled,
                                               FfiBool *out);

/**
 * 安装 shim 函数表。必须在其他任何 `ferrum_host_*` 调用之前、且只调用一次。
 *
 * # Safety
 * `api` 必须指向有效的 [`FerrumShimApi`]，其中函数在进程生命周期内保持有效。
 */
FerrumStatus ferrum_host_init(const struct FerrumShimApi *api);

/**
 * 宿主实现的 ABI 版本。
 */
struct AbiVersion ferrum_host_abi_version(void);

/**
 * shim 将 C++ 异常详情写入同一线程的错误槽。
 */
FerrumStatus ferrum_host_set_last_error(struct FfiStr message);

/**
 * 由 shim 在服务端主线程上调用，记录主线程 ID。
 */
void ferrum_host_mark_main_thread(void);

FerrumStatus ferrum_host_scheduler_tick(void);

FerrumStatus ferrum_host_form_response(uint64_t id,
                                       ActorRuntimeId player,
                                       struct PlayerUuid uuid,
                                       FormResponseKind kind,
                                       struct FfiStr json);

FerrumStatus ferrum_host_form_disconnect(struct PlayerUuid player);

/**
 * shim 在派发前确认当前线程，不会修改已记录的主线程。
 */
FfiBool ferrum_host_is_main_thread(void);

/**
 * 加载一个 `type = "ferrum"` 模组并执行其 `on_load`。成功时写出上下文句柄。
 *
 * # Safety
 * `info` 指向有效的 [`ShimModInfo`]（字符串在本次调用内有效）；`out_ctx` 指向可写的 `u64`。
 */
FerrumStatus ferrum_host_load_mod(const struct ShimModInfo *info,
                                  uint64_t *out_ctx);

FerrumStatus ferrum_host_enable_mod(uint64_t ctx);

FerrumStatus ferrum_host_disable_mod(uint64_t ctx);

FerrumStatus ferrum_host_unload_mod(uint64_t ctx);

/**
 * 取回并清空当前线程的错误详情。
 *
 * # Safety
 * `out` 为空或指向本次调用内有效的写入器。
 */
FerrumStatus ferrum_host_take_last_error(struct FfiByteWriter *out);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* FERRUM_ABI_H */
