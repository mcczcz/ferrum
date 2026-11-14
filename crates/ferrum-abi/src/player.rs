use crate::{ActorRuntimeId, DimensionId, FerrumStatus, FfiBool, FfiByteWriter, FfiStr, Vec3};

/// 标准 UUID 网络字节序，布局永久冻结。
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PlayerUuid {
    pub bytes: [u8; 16],
}

/// Ferrum 的稳定游戏模式码，未知值必须保留而非转为封闭枚举。
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct GameMode(pub u32);

impl GameMode {
    pub const SURVIVAL: Self = Self(0);
    pub const CREATIVE: Self = Self(1);
    pub const ADVENTURE: Self = Self(2);
    pub const SPECTATOR: Self = Self(3);
    pub const UNKNOWN: Self = Self(u32::MAX);
}

/// ABI 1.1 玩家接口；所有操作仅允许在主线程调用。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct PlayerApiV1 {
    pub struct_size: u32,
    pub find_by_name: extern "C" fn(FfiStr<'_>, *mut ActorRuntimeId) -> FerrumStatus,
    pub find_by_uuid: extern "C" fn(PlayerUuid, *mut ActorRuntimeId) -> FerrumStatus,
    /// 输出逐个小端编码的 u64 runtime ID，不输出宿主容器。
    pub online_players: extern "C" fn(*mut FfiByteWriter) -> FerrumStatus,
    pub real_name: extern "C" fn(ActorRuntimeId, *mut FfiByteWriter) -> FerrumStatus,
    pub uuid: extern "C" fn(ActorRuntimeId, *mut PlayerUuid) -> FerrumStatus,
    pub send_message: extern "C" fn(ActorRuntimeId, FfiStr<'_>) -> FerrumStatus,
    pub is_operator: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    pub position: extern "C" fn(ActorRuntimeId, *mut Vec3) -> FerrumStatus,
    pub dimension: extern "C" fn(ActorRuntimeId, *mut DimensionId) -> FerrumStatus,
    /// 目标维度必须已存在，传送保留朝向，坐标必须有限。
    pub teleport: extern "C" fn(ActorRuntimeId, Vec3, DimensionId) -> FerrumStatus,
    pub health: extern "C" fn(ActorRuntimeId, *mut f32) -> FerrumStatus,
    pub max_health: extern "C" fn(ActorRuntimeId, *mut f32) -> FerrumStatus,
    /// 直接设置属性，不模拟伤害/治疗事件；范围为 0..=max_health。
    pub set_health: extern "C" fn(ActorRuntimeId, f32) -> FerrumStatus,
    pub game_mode: extern "C" fn(ActorRuntimeId, *mut GameMode) -> FerrumStatus,
    pub set_game_mode: extern "C" fn(ActorRuntimeId, GameMode) -> FerrumStatus,
}
