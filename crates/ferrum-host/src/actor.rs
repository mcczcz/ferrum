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
pub static ACTOR_V1: ActorApiV1 = ActorApiV1 {
    struct_size: size_of::<ActorApiV1>() as u32,
    find_by_unique_id,
    unique_id,
    type_name,
    kind,
    position,
    dimension,
    teleport,
};
