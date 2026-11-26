use crate::{BlockPos, FerrumStatus, FfiBool, FfiByteWriter, FfiStr};

/// 当次会话内的实体标识；跨 Tick 保存请使用 ActorUniqueId 或 PlayerUuid。
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ActorRuntimeId(pub u64);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ActorUniqueId(pub i64);

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct DimensionId(pub i32);
impl DimensionId {
    pub const OVERWORLD: Self = Self(0);
    pub const NETHER: Self = Self(1);
    pub const THE_END: Self = Self(2);
}

/// Ferrum 能力类别，不暴露 MC 内部枚举；未知值不得转为封闭枚举。
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ActorKind(pub u32);
impl ActorKind {
    pub const ACTOR: Self = Self(0);
    pub const MOB: Self = Self(1);
    pub const PLAYER: Self = Self(2);
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ActorApiV1 {
    pub struct_size: u32,
    pub find_by_unique_id: extern "C" fn(ActorUniqueId, *mut ActorRuntimeId) -> FerrumStatus,
    pub unique_id: extern "C" fn(ActorRuntimeId, *mut ActorUniqueId) -> FerrumStatus,
    pub type_name: extern "C" fn(ActorRuntimeId, *mut FfiByteWriter) -> FerrumStatus,
    pub kind: extern "C" fn(ActorRuntimeId, *mut ActorKind) -> FerrumStatus,
    pub position: extern "C" fn(ActorRuntimeId, *mut Vec3) -> FerrumStatus,
    pub dimension: extern "C" fn(ActorRuntimeId, *mut DimensionId) -> FerrumStatus,
    pub teleport: extern "C" fn(ActorRuntimeId, Vec3, DimensionId) -> FerrumStatus,
    pub name_tag: extern "C" fn(ActorRuntimeId, *mut FfiByteWriter) -> FerrumStatus,
    pub set_name_tag: extern "C" fn(ActorRuntimeId, FfiStr<'_>) -> FerrumStatus,
    pub is_on_ground: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    pub is_in_water: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    /// 实体存档快照。输出未压缩、大端、带根名称的二进制 Compound。
    pub nbt: extern "C" fn(ActorRuntimeId, *mut FfiByteWriter) -> FerrumStatus,
    pub feet_position: extern "C" fn(ActorRuntimeId, *mut Vec3) -> FerrumStatus,
    pub head_position: extern "C" fn(ActorRuntimeId, *mut Vec3) -> FerrumStatus,
    pub velocity: extern "C" fn(ActorRuntimeId, *mut Vec3) -> FerrumStatus,
    pub position_delta: extern "C" fn(ActorRuntimeId, *mut Vec3) -> FerrumStatus,
    pub rotation: extern "C" fn(ActorRuntimeId, *mut Vec2) -> FerrumStatus,
    pub eye_position: extern "C" fn(ActorRuntimeId, *mut Vec3) -> FerrumStatus,
    pub fall_distance: extern "C" fn(ActorRuntimeId, *mut f32) -> FerrumStatus,
    pub aabb: extern "C" fn(ActorRuntimeId, *mut Aabb) -> FerrumStatus,
    pub block_pos_standing_on: extern "C" fn(ActorRuntimeId, *mut BlockPos) -> FerrumStatus,
    pub is_swimming: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    pub is_moving: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    pub is_sneaking: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    pub is_crawling: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    pub is_gliding: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    pub is_invisible: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    pub is_alive: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    pub is_dead: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    pub is_on_fire: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    pub is_sleeping: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    pub is_blocking: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    pub set_on_fire: extern "C" fn(ActorRuntimeId, i32, FfiBool) -> FerrumStatus,
    pub stop_fire: extern "C" fn(ActorRuntimeId) -> FerrumStatus,
    pub set_sneaking: extern "C" fn(ActorRuntimeId, FfiBool) -> FerrumStatus,
    pub teleport_with_rotation:
        extern "C" fn(ActorRuntimeId, Vec3, Vec2, crate::DimensionId) -> FerrumStatus,
}

/// 生物能力。非生物返回 INVALID_ARGUMENT；目标消失返回 NOT_FOUND。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct MobApiV1 {
    pub struct_size: u32,
    pub health: extern "C" fn(ActorRuntimeId, *mut f32) -> FerrumStatus,
    pub max_health: extern "C" fn(ActorRuntimeId, *mut f32) -> FerrumStatus,
    pub set_health: extern "C" fn(ActorRuntimeId, f32) -> FerrumStatus,
    pub is_sprinting: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
    pub set_sprinting: extern "C" fn(ActorRuntimeId, FfiBool) -> FerrumStatus,
    pub speed: extern "C" fn(ActorRuntimeId, *mut f32) -> FerrumStatus,
    pub set_speed: extern "C" fn(ActorRuntimeId, f32) -> FerrumStatus,
    pub armor_value: extern "C" fn(ActorRuntimeId, *mut i32) -> FerrumStatus,
    pub death_time: extern "C" fn(ActorRuntimeId, *mut i32) -> FerrumStatus,
    pub item_use_duration: extern "C" fn(ActorRuntimeId, *mut i32) -> FerrumStatus,
    pub item_use_startup_progress: extern "C" fn(ActorRuntimeId, *mut f32) -> FerrumStatus,
    pub item_use_interval_progress: extern "C" fn(ActorRuntimeId, *mut f32) -> FerrumStatus,
    pub jump_multiplier: extern "C" fn(ActorRuntimeId, *mut f32) -> FerrumStatus,
    pub is_able_to_move: extern "C" fn(ActorRuntimeId, *mut FfiBool) -> FerrumStatus,
}
