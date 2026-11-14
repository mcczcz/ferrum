use crate::{last_error, shim, vtable};
use ferrum_abi::*;

fn call(f: impl FnOnce(&MobApiV1) -> FerrumStatus) -> FerrumStatus {
    vtable::guard(|| {
        if !vtable::is_main_thread() {
            return last_error::fail(FerrumStatus::WRONG_THREAD, "生物 API 必须在主线程调用");
        }
        match shim::get() {
            Some(a) => f(&a.mob_v1),
            None => last_error::fail(FerrumStatus::SERVICE_UNAVAILABLE, "shim 未初始化"),
        }
    })
}
extern "C" fn health(id: ActorRuntimeId, out: *mut f32) -> FerrumStatus {
    call(|a| (a.health)(id, out))
}
extern "C" fn max_health(id: ActorRuntimeId, out: *mut f32) -> FerrumStatus {
    call(|a| (a.max_health)(id, out))
}
extern "C" fn set_health(id: ActorRuntimeId, value: f32) -> FerrumStatus {
    call(|a| (a.set_health)(id, value))
}
pub static MOB_V1: MobApiV1 = MobApiV1 {
    struct_size: size_of::<MobApiV1>() as u32,
    health,
    max_health,
    set_health,
};
