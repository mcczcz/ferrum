use crate::{FerrumStatus, FfiByteWriter};

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
}

/// 生物能力。非生物返回 INVALID_ARGUMENT；目标消失返回 NOT_FOUND。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct MobApiV1 {
    pub struct_size: u32,
    pub health: extern "C" fn(ActorRuntimeId, *mut f32) -> FerrumStatus,
    pub max_health: extern "C" fn(ActorRuntimeId, *mut f32) -> FerrumStatus,
    pub set_health: extern "C" fn(ActorRuntimeId, f32) -> FerrumStatus,
}
