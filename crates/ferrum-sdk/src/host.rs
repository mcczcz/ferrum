//! 宿主根表的保存与底层调用辅助。

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;

use ferrum_abi::{
    ActorKind, ActorRuntimeId, ActorUniqueId, BlockPos, BlockRuntimeId, CommandCallbackV1,
    CommandDefinitionV1, CommandHandle, CommandScopeHandle, CoreApiV1, DimensionId, FerrumHostApi,
    FerrumStatus, FfiBool, FfiByteWriter, FfiStr, GameMode, LogApiV1, ModContextHandle, PlayerUuid,
    Vec3,
};

use crate::error::Error;

/// 宿主根表的 `'static` 引用（握手成功后设置一次）。
#[derive(Copy, Clone)]
pub(crate) struct HostRef(pub(crate) &'static FerrumHostApi);

// SAFETY: 宿主保证根表及其子表在进程生命周期内不可变且可从任意线程读取，
// 其中的函数指针按 ABI 契约线程安全（主线程专属函数由令牌在类型层约束）。
unsafe impl Send for HostRef {}
// SAFETY: 同上。
unsafe impl Sync for HostRef {}

static HOST: OnceLock<HostRef> = OnceLock::new();

/// 记录宿主根表；同一镜像被再次加载时宿主指针不变，重复设置被忽略。
pub(crate) fn install(api: &'static FerrumHostApi) {
    let _ = HOST.set(HostRef(api));
}

pub(crate) fn try_api() -> Option<&'static FerrumHostApi> {
    HOST.get().map(|h| h.0)
}

/// 握手之后才可能被调用，因此根表必定存在。
pub(crate) fn api() -> &'static FerrumHostApi {
    try_api().expect("ferrum-sdk: host api used before ferrum_mod_entry_v1 handshake")
}

pub(crate) fn core() -> &'static CoreApiV1 {
    // SAFETY: 握手时已校验 core_v1 非空且 struct_size 覆盖 v1.0；子表为 'static。
    unsafe { &*api().core_v1 }
}

pub(crate) fn log() -> &'static LogApiV1 {
    // SAFETY: 握手时已校验 log_v1 非空且 struct_size 覆盖 v1.0；子表为 'static。
    unsafe { &*api().log_v1 }
}

pub(crate) fn check(status: FerrumStatus) -> Result<(), Error> {
    if status.is_ok() {
        Ok(())
    } else {
        Err(error_from_status(status))
    }
}

/// 宿主不可变子表的原始视图；只保证宿主声明的前缀存在，不构造完整表引用。
struct ApiTable {
    ptr: *const c_void,
    struct_size: u32,
}

// SAFETY: 指针只来自宿主的不可变静态子表，不拥有资源，可以跨线程保存；
// 具体函数的调用线程仍由 SDK 的主线程令牌与瞬态引用约束。
unsafe impl Send for ApiTable {}
// SAFETY: 子表和缓存的大小在进程生命周期内不变，所有访问均为只读。
unsafe impl Sync for ApiTable {}

impl ApiTable {
    fn query(
        interface_id: &'static str,
        missing: &'static str,
        incomplete: &'static str,
    ) -> Result<Self, &'static str> {
        let ptr = (api().query_interface)(interface_id.into(), 0);
        if ptr.is_null() {
            return Err(missing);
        }
        // SAFETY: query_interface 返回对齐的静态子表，冻结的 u32 首字段始终可读。
        let struct_size = unsafe { ptr.cast::<u32>().read() };
        if struct_size < size_of::<u32>() as u32 {
            return Err(incomplete);
        }
        Ok(Self { ptr, struct_size })
    }

    fn require_field(
        &self,
        offset: usize,
        size: usize,
        missing: &'static str,
    ) -> Result<(), Error> {
        if ferrum_abi::has_field(self.struct_size, offset, size) {
            Ok(())
        } else {
            Err(Error::new(FerrumStatus::UNSUPPORTED, missing))
        }
    }
}

// 生成类型化槽位访问器：先检查边界，再以原始指针读取字段，绝不借用整表。
// 返回类型与 ABI 字段类型由编译器核对；每个子表独立缓存成功或失败的查询结果。
macro_rules! domain_api {
    ($view:ident, $accessor:ident, $abi:ident, $id:literal, $missing:literal, $incomplete:literal,
        { $($field:ident: $ty:ty;)* }) => {
        pub(crate) struct $view(ApiTable);

        impl $view {
            $(
                pub(crate) fn $field(&self) -> Result<$ty, Error> {
                    self.0.require_field(
                        std::mem::offset_of!(ferrum_abi::$abi, $field),
                        size_of::<$ty>(),
                        concat!("宿主未提供 API 槽位：", $id, ".", stringify!($field)),
                    )?;
                    let ptr = self.0.ptr.cast::<ferrum_abi::$abi>();
                    // SAFETY: 指针来自该接口的对齐、不可变静态子表；已确认字段完整存在，
                    // ABI 保证该槽位为有效函数指针。addr_of! 只投影字段，不构造完整表引用。
                    Ok(unsafe { std::ptr::addr_of!((*ptr).$field).read() })
                }
            )*
        }

        pub(crate) fn $accessor() -> Result<&'static $view, Error> {
            static TABLE: OnceLock<Result<$view, &'static str>> = OnceLock::new();
            TABLE
                .get_or_init(|| ApiTable::query($id, $missing, $incomplete).map($view))
                .as_ref()
                .map_err(|message| Error::new(FerrumStatus::UNSUPPORTED, *message))
        }
    };
}

domain_api!(SchedulerApi, scheduler_api, SchedulerApiV1, "ferrum.scheduler.v1",
    "宿主未提供调度 API（需要 ABI 1.2）", "调度 API 子表不完整", {
    post: extern "C" fn(ModContextHandle, ferrum_abi::TaskCallbackV1, *mut ferrum_abi::TaskHandle) -> FerrumStatus;
    after_ticks: extern "C" fn(ModContextHandle, u64, ferrum_abi::TaskCallbackV1, *mut ferrum_abi::TaskHandle) -> FerrumStatus;
    every_ticks: extern "C" fn(ModContextHandle, u64, ferrum_abi::TaskCallbackV1, *mut ferrum_abi::TaskHandle) -> FerrumStatus;
    cancel: extern "C" fn(ModContextHandle, ferrum_abi::TaskHandle) -> FerrumStatus;
});

domain_api!(PlayerApi, player_api, PlayerApiV1, "ferrum.player.v1",
    "宿主未提供玩家 API（需要 ABI 1.1）", "玩家 API 子表不完整", {
    find_by_name: extern "C" fn(FfiStr<'_>, *mut ActorRuntimeId) -> FerrumStatus;
    find_by_uuid: extern "C" fn(PlayerUuid, *mut ActorRuntimeId) -> FerrumStatus;
    online_players: extern "C" fn(*mut FfiByteWriter) -> FerrumStatus;
    real_name: extern "C" fn(ActorRuntimeId, *mut FfiByteWriter) -> FerrumStatus;
    uuid: extern "C" fn(ActorRuntimeId, *mut PlayerUuid) -> FerrumStatus;
    send_message: extern "C" fn(ActorRuntimeId, FfiStr<'_>) -> FerrumStatus;
    is_operator: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus;
    game_mode: extern "C" fn(ActorRuntimeId, *mut GameMode) -> FerrumStatus;
    set_game_mode: extern "C" fn(ActorRuntimeId, GameMode) -> FerrumStatus;
});

domain_api!(CustomEventPublisherApi, custom_event_publisher_api, CustomEventPublisherApiV1, "ferrum.custom_event_publisher.v1",
    "宿主未提供通用事件发布端", "通用事件发布端子表不完整", {
    register_event: extern "C" fn(ModContextHandle, *const ferrum_abi::CustomEventDefinitionV1<'_>, *mut ferrum_abi::CustomEventHandle) -> FerrumStatus;
    unregister_event: extern "C" fn(ModContextHandle, ferrum_abi::CustomEventHandle) -> FerrumStatus;
    publish: extern "C" fn(ModContextHandle, ferrum_abi::CustomEventHandle, ferrum_abi::FfiSlice<'_, ferrum_abi::EventValueV1<'_>>, *mut FfiBool) -> FerrumStatus;
});

domain_api!(CustomEventApi, custom_event_api, CustomEventApiV1, "ferrum.custom_event.v1",
    "宿主未提供通用事件协议", "通用事件子表不完整", {
    subscribe: extern "C" fn(ModContextHandle, FfiStr<'_>, ferrum_abi::EventPriority, ferrum_abi::EventCallbackV1, *mut ferrum_abi::CustomListenerHandle) -> FerrumStatus;
    unsubscribe: extern "C" fn(ModContextHandle, ferrum_abi::CustomListenerHandle) -> FerrumStatus;
    name: extern "C" fn(ModContextHandle, ferrum_abi::EventScopeHandle, *mut FfiByteWriter) -> FerrumStatus;
    get_i64: extern "C" fn(ModContextHandle, ferrum_abi::EventScopeHandle, FfiStr<'_>, *mut i64) -> FerrumStatus;
    get_u64: extern "C" fn(ModContextHandle, ferrum_abi::EventScopeHandle, FfiStr<'_>, *mut u64) -> FerrumStatus;
    get_f64: extern "C" fn(ModContextHandle, ferrum_abi::EventScopeHandle, FfiStr<'_>, *mut f64) -> FerrumStatus;
    get_bool: extern "C" fn(ModContextHandle, ferrum_abi::EventScopeHandle, FfiStr<'_>, *mut FfiBool) -> FerrumStatus;
    get_string: extern "C" fn(ModContextHandle, ferrum_abi::EventScopeHandle, FfiStr<'_>, *mut FfiByteWriter) -> FerrumStatus;
    get_bytes: extern "C" fn(ModContextHandle, ferrum_abi::EventScopeHandle, FfiStr<'_>, *mut FfiByteWriter) -> FerrumStatus;
    get_uuid: extern "C" fn(ModContextHandle, ferrum_abi::EventScopeHandle, FfiStr<'_>, *mut PlayerUuid) -> FerrumStatus;
    is_cancelled: extern "C" fn(ModContextHandle, ferrum_abi::EventScopeHandle, *mut FfiBool) -> FerrumStatus;
    cancel: extern "C" fn(ModContextHandle, ferrum_abi::EventScopeHandle) -> FerrumStatus;
});

domain_api!(EventApi, event_api, EventApiV1, "ferrum.event.v1",
    "宿主未提供事件 API", "事件 API 子表不完整", {
    subscribe: extern "C" fn(ModContextHandle, *const ferrum_abi::EventSubscriptionV1, ferrum_abi::EventCallbackV1, *mut ferrum_abi::ListenerHandle) -> FerrumStatus;
    unsubscribe: extern "C" fn(ModContextHandle, ferrum_abi::ListenerHandle) -> FerrumStatus;
    player: extern "C" fn(ModContextHandle, ferrum_abi::EventScopeHandle, *mut ActorRuntimeId) -> FerrumStatus;
    is_cancelled: extern "C" fn(ModContextHandle, ferrum_abi::EventScopeHandle, *mut FfiBool) -> FerrumStatus;
    cancel: extern "C" fn(ModContextHandle, ferrum_abi::EventScopeHandle) -> FerrumStatus;
    chat_message: extern "C" fn(ModContextHandle, ferrum_abi::EventScopeHandle, *mut FfiByteWriter) -> FerrumStatus;
});

domain_api!(CommandApi, command_api, CommandApiV1, "ferrum.command.v1",
    "宿主未提供命令 API（需要 ABI 1.1）", "命令 API 子表不完整", {
    register: extern "C" fn(ModContextHandle, *const CommandDefinitionV1<'_>, CommandCallbackV1, *mut CommandHandle) -> FerrumStatus;
    unregister: extern "C" fn(ModContextHandle, CommandHandle) -> FerrumStatus;
    origin_player: extern "C" fn(ModContextHandle, CommandScopeHandle, *mut ActorRuntimeId) -> FerrumStatus;
    get_int: extern "C" fn(ModContextHandle, CommandScopeHandle, FfiStr<'_>, *mut i32) -> FerrumStatus;
    get_float: extern "C" fn(ModContextHandle, CommandScopeHandle, FfiStr<'_>, *mut f32) -> FerrumStatus;
    get_bool: extern "C" fn(ModContextHandle, CommandScopeHandle, FfiStr<'_>, *mut FfiBool) -> FerrumStatus;
    get_string: extern "C" fn(ModContextHandle, CommandScopeHandle, FfiStr<'_>, *mut FfiByteWriter) -> FerrumStatus;
    get_players: extern "C" fn(ModContextHandle, CommandScopeHandle, FfiStr<'_>, *mut FfiByteWriter) -> FerrumStatus;
    output: extern "C" fn(ModContextHandle, CommandScopeHandle, FfiBool, FfiStr<'_>) -> FerrumStatus;
});

domain_api!(BlockApi, block_api, BlockApiV1, "ferrum.block.v1",
    "宿主未提供方块 API", "方块 API 子表不完整", {
    type_name: extern "C" fn(DimensionId, BlockPos, *mut FfiByteWriter) -> FerrumStatus;
    runtime_id: extern "C" fn(DimensionId, BlockPos, *mut BlockRuntimeId) -> FerrumStatus;
    is_air: extern "C" fn(DimensionId, BlockPos, *mut FfiBool) -> FerrumStatus;
    set_type: extern "C" fn(DimensionId, BlockPos, FfiStr<'_>) -> FerrumStatus;
});

domain_api!(ActorApi, actor_api, ActorApiV1, "ferrum.actor.v1",
    "宿主未提供实体 API", "实体 API 子表不完整", {
    find_by_unique_id: extern "C" fn(ActorUniqueId, *mut ActorRuntimeId) -> FerrumStatus;
    unique_id: extern "C" fn(ActorRuntimeId, *mut ActorUniqueId) -> FerrumStatus;
    type_name: extern "C" fn(ActorRuntimeId, *mut FfiByteWriter) -> FerrumStatus;
    kind: extern "C" fn(ActorRuntimeId, *mut ActorKind) -> FerrumStatus;
    position: extern "C" fn(ActorRuntimeId, *mut Vec3) -> FerrumStatus;
    dimension: extern "C" fn(ActorRuntimeId, *mut DimensionId) -> FerrumStatus;
    teleport: extern "C" fn(ActorRuntimeId, Vec3, DimensionId) -> FerrumStatus;
});

domain_api!(MobApi, mob_api, MobApiV1, "ferrum.mob.v1",
    "宿主未提供生物 API", "生物 API 子表不完整", {
    health: extern "C" fn(ActorRuntimeId, *mut f32) -> FerrumStatus;
    max_health: extern "C" fn(ActorRuntimeId, *mut f32) -> FerrumStatus;
    set_health: extern "C" fn(ActorRuntimeId, f32) -> FerrumStatus;
});

/// 宿主调用失败后立即取回 last-error，组装为 [`Error`]。
pub(crate) fn error_from_status(status: FerrumStatus) -> Error {
    let message = read_bytes(|w| (core().take_last_error)(w))
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    Error::new(status, message)
}

extern "C" fn vec_write(context: *mut c_void, bytes: *const u8, len: usize) -> FerrumStatus {
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: context 是 read_bytes 在栈上传入的 &mut Vec<u8>，仅在本次同步调用内使用。
        let buf = unsafe { &mut *(context as *mut Vec<u8>) };
        if len == 0 || bytes.is_null() {
            return FerrumStatus::OK;
        }
        if buf.try_reserve(len).is_err() {
            return FerrumStatus::OUT_OF_MEMORY;
        }
        // SAFETY: 宿主保证 bytes 指向 len 个本次调用内有效的字节，且已排除空指针。
        buf.extend_from_slice(unsafe { std::slice::from_raw_parts(bytes, len) });
        FerrumStatus::OK
    }));
    result.unwrap_or(FerrumStatus::MOD_PANICKED)
}

/// 用一个写入模组自身堆的 [`FfiByteWriter`] 调用宿主，收集输出字节（不检查 UTF-8）。
///
/// 返回非 `OK` 时不会读取 last-error（避免递归）；需要富错误时用 [`read_string`]。
pub(crate) fn read_bytes(
    f: impl FnOnce(*mut FfiByteWriter) -> FerrumStatus,
) -> Result<Vec<u8>, FerrumStatus> {
    let mut buf = Vec::<u8>::new();
    let mut writer = FfiByteWriter {
        context: &mut buf as *mut Vec<u8> as *mut c_void,
        write_fn: vec_write,
    };
    let status = f(&mut writer);
    if status.is_ok() { Ok(buf) } else { Err(status) }
}

/// 调用宿主并把输出解码为 UTF-8 字符串。
pub(crate) fn read_string(
    f: impl FnOnce(*mut FfiByteWriter) -> FerrumStatus,
) -> Result<String, Error> {
    let bytes = read_bytes(f).map_err(error_from_status)?;
    Ok(String::from_utf8(bytes)?)
}
