use crate::{last_error, shim, vtable};
use ferrum_abi::*;

fn call(f: impl FnOnce(&ActorApiV1) -> FerrumStatus) -> FerrumStatus {
    vtable::guard(|| {
        if !vtable::is_main_thread() {
            return last_error::fail(FerrumStatus::WRONG_THREAD, "实体 API 必须在主线程调用");
        }
        match shim::get() {
            Some(a) => f(&a.actor_v1),
            None => last_error::fail(FerrumStatus::SERVICE_UNAVAILABLE, "shim 未初始化"),
        }
    })
}
extern "C" fn find_by_unique_id(id: ActorUniqueId, out: *mut ActorRuntimeId) -> FerrumStatus {
    call(|a| (a.find_by_unique_id)(id, out))
}
extern "C" fn unique_id(id: ActorRuntimeId, out: *mut ActorUniqueId) -> FerrumStatus {
    call(|a| (a.unique_id)(id, out))
}
extern "C" fn type_name(id: ActorRuntimeId, out: *mut FfiByteWriter) -> FerrumStatus {
    call(|a| (a.type_name)(id, out))
}
extern "C" fn kind(id: ActorRuntimeId, out: *mut ActorKind) -> FerrumStatus {
    call(|a| (a.kind)(id, out))
}
extern "C" fn position(id: ActorRuntimeId, out: *mut Vec3) -> FerrumStatus {
    call(|a| (a.position)(id, out))
}
extern "C" fn dimension(id: ActorRuntimeId, out: *mut DimensionId) -> FerrumStatus {
    call(|a| (a.dimension)(id, out))
}
extern "C" fn teleport(id: ActorRuntimeId, pos: Vec3, dim: DimensionId) -> FerrumStatus {
    call(|a| (a.teleport)(id, pos, dim))
}
extern "C" fn name_tag(id: ActorRuntimeId, out: *mut FfiByteWriter) -> FerrumStatus {
    call(|a| (a.name_tag)(id, out))
}
extern "C" fn set_name_tag(id: ActorRuntimeId, value: FfiStr<'_>) -> FerrumStatus {
    call(|a| (a.set_name_tag)(id, value))
}
extern "C" fn is_on_ground(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_on_ground)(id, out))
}
extern "C" fn is_in_water(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_in_water)(id, out))
}
extern "C" fn nbt(id: ActorRuntimeId, out: *mut FfiByteWriter) -> FerrumStatus {
    call(|a| (a.nbt)(id, out))
}
extern "C" fn feet_position(id: ActorRuntimeId, out: *mut Vec3) -> FerrumStatus {
    call(|a| (a.feet_position)(id, out))
}
extern "C" fn head_position(id: ActorRuntimeId, out: *mut Vec3) -> FerrumStatus {
    call(|a| (a.head_position)(id, out))
}
extern "C" fn velocity(id: ActorRuntimeId, out: *mut Vec3) -> FerrumStatus {
    call(|a| (a.velocity)(id, out))
}
extern "C" fn position_delta(id: ActorRuntimeId, out: *mut Vec3) -> FerrumStatus {
    call(|a| (a.position_delta)(id, out))
}
extern "C" fn rotation(id: ActorRuntimeId, out: *mut Vec2) -> FerrumStatus {
    call(|a| (a.rotation)(id, out))
}
extern "C" fn eye_position(id: ActorRuntimeId, out: *mut Vec3) -> FerrumStatus {
    call(|a| (a.eye_position)(id, out))
}
extern "C" fn fall_distance(id: ActorRuntimeId, out: *mut f32) -> FerrumStatus {
    call(|a| (a.fall_distance)(id, out))
}
extern "C" fn aabb(id: ActorRuntimeId, out: *mut Aabb) -> FerrumStatus {
    call(|a| (a.aabb)(id, out))
}
extern "C" fn block_pos_standing_on(id: ActorRuntimeId, out: *mut BlockPos) -> FerrumStatus {
    call(|a| (a.block_pos_standing_on)(id, out))
}
extern "C" fn is_swimming(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_swimming)(id, out))
}
extern "C" fn is_moving(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_moving)(id, out))
}
extern "C" fn is_sneaking(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_sneaking)(id, out))
}
extern "C" fn is_crawling(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_crawling)(id, out))
}
extern "C" fn is_gliding(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_gliding)(id, out))
}
extern "C" fn is_invisible(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_invisible)(id, out))
}
extern "C" fn is_alive(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_alive)(id, out))
}
extern "C" fn is_dead(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_dead)(id, out))
}
extern "C" fn is_on_fire(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_on_fire)(id, out))
}
extern "C" fn is_sleeping(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_sleeping)(id, out))
}
extern "C" fn is_blocking(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_blocking)(id, out))
}
extern "C" fn set_on_fire(id: ActorRuntimeId, seconds: i32, effect: FfiBool) -> FerrumStatus {
    call(|a| (a.set_on_fire)(id, seconds, effect))
}
extern "C" fn stop_fire(id: ActorRuntimeId) -> FerrumStatus {
    call(|a| (a.stop_fire)(id))
}
extern "C" fn set_sneaking(id: ActorRuntimeId, value: FfiBool) -> FerrumStatus {
    call(|a| (a.set_sneaking)(id, value))
}
extern "C" fn teleport_with_rotation(
    id: ActorRuntimeId,
    pos: Vec3,
    rot: Vec2,
    dim: DimensionId,
) -> FerrumStatus {
    call(|a| (a.teleport_with_rotation)(id, pos, rot, dim))
}
pub static ACTOR_V1: ActorApiV1 = ActorApiV1 {
    struct_size: size_of::<ActorApiV1>() as u32,
    find_by_unique_id,
    unique_id,
    type_name,
    kind,
    position,
    dimension,
    teleport,
    name_tag,
    set_name_tag,
    is_on_ground,
    is_in_water,
    nbt,
    feet_position,
    head_position,
    velocity,
    position_delta,
    rotation,
    eye_position,
    fall_distance,
    aabb,
    block_pos_standing_on,
    is_swimming,
    is_moving,
    is_sneaking,
    is_crawling,
    is_gliding,
    is_invisible,
    is_alive,
    is_dead,
    is_on_fire,
    is_sleeping,
    is_blocking,
    set_on_fire,
    stop_fire,
    set_sneaking,
    teleport_with_rotation,
};
