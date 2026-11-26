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
extern "C" fn is_sprinting(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_sprinting)(id, out))
}
extern "C" fn set_sprinting(id: ActorRuntimeId, value: FfiBool) -> FerrumStatus {
    call(|a| (a.set_sprinting)(id, value))
}
extern "C" fn speed(id: ActorRuntimeId, out: *mut f32) -> FerrumStatus {
    call(|a| (a.speed)(id, out))
}
extern "C" fn set_speed(id: ActorRuntimeId, value: f32) -> FerrumStatus {
    call(|a| (a.set_speed)(id, value))
}
extern "C" fn armor_value(id: ActorRuntimeId, out: *mut i32) -> FerrumStatus {
    call(|a| (a.armor_value)(id, out))
}
extern "C" fn death_time(id: ActorRuntimeId, out: *mut i32) -> FerrumStatus {
    call(|a| (a.death_time)(id, out))
}
extern "C" fn item_use_duration(id: ActorRuntimeId, out: *mut i32) -> FerrumStatus {
    call(|a| (a.item_use_duration)(id, out))
}
extern "C" fn item_use_startup_progress(id: ActorRuntimeId, out: *mut f32) -> FerrumStatus {
    call(|a| (a.item_use_startup_progress)(id, out))
}
extern "C" fn item_use_interval_progress(id: ActorRuntimeId, out: *mut f32) -> FerrumStatus {
    call(|a| (a.item_use_interval_progress)(id, out))
}
extern "C" fn jump_multiplier(id: ActorRuntimeId, out: *mut f32) -> FerrumStatus {
    call(|a| (a.jump_multiplier)(id, out))
}
extern "C" fn is_able_to_move(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_able_to_move)(id, out))
}
pub static MOB_V1: MobApiV1 = MobApiV1 {
    struct_size: size_of::<MobApiV1>() as u32,
    health,
    max_health,
    set_health,
    is_sprinting,
    set_sprinting,
    speed,
    set_speed,
    armor_value,
    death_time,
    item_use_duration,
    item_use_startup_progress,
    item_use_interval_progress,
    jump_multiplier,
    is_able_to_move,
};
