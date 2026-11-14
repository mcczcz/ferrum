use crate::{last_error, shim, vtable};
use ferrum_abi::*;

fn call(f: impl FnOnce(&PlayerApiV1) -> FerrumStatus) -> FerrumStatus {
    vtable::guard(|| {
        if !vtable::is_main_thread() {
            return last_error::fail(FerrumStatus::WRONG_THREAD, "玩家 API 必须在主线程调用");
        }
        match shim::get() {
            Some(api) => f(&api.player_v1),
            None => last_error::fail(FerrumStatus::SERVICE_UNAVAILABLE, "shim 未初始化"),
        }
    })
}

extern "C" fn find_by_name(name: FfiStr<'_>, out: *mut ActorRuntimeId) -> FerrumStatus {
    call(|a| (a.find_by_name)(name, out))
}
extern "C" fn find_by_uuid(uuid: PlayerUuid, out: *mut ActorRuntimeId) -> FerrumStatus {
    call(|a| (a.find_by_uuid)(uuid, out))
}
extern "C" fn online_players(out: *mut FfiByteWriter) -> FerrumStatus {
    call(|a| (a.online_players)(out))
}
extern "C" fn real_name(id: ActorRuntimeId, out: *mut FfiByteWriter) -> FerrumStatus {
    call(|a| (a.real_name)(id, out))
}
extern "C" fn uuid(id: ActorRuntimeId, out: *mut PlayerUuid) -> FerrumStatus {
    call(|a| (a.uuid)(id, out))
}
extern "C" fn send_message(id: ActorRuntimeId, text: FfiStr<'_>) -> FerrumStatus {
    call(|a| (a.send_message)(id, text))
}
extern "C" fn is_operator(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_operator)(id, out))
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
extern "C" fn health(id: ActorRuntimeId, out: *mut f32) -> FerrumStatus {
    call(|a| (a.health)(id, out))
}
extern "C" fn max_health(id: ActorRuntimeId, out: *mut f32) -> FerrumStatus {
    call(|a| (a.max_health)(id, out))
}
extern "C" fn set_health(id: ActorRuntimeId, value: f32) -> FerrumStatus {
    call(|a| (a.set_health)(id, value))
}
extern "C" fn game_mode(id: ActorRuntimeId, out: *mut GameMode) -> FerrumStatus {
    call(|a| (a.game_mode)(id, out))
}
extern "C" fn set_game_mode(id: ActorRuntimeId, value: GameMode) -> FerrumStatus {
    call(|a| (a.set_game_mode)(id, value))
}

pub static PLAYER_V1: PlayerApiV1 = PlayerApiV1 {
    struct_size: size_of::<PlayerApiV1>() as u32,
    find_by_name,
    find_by_uuid,
    online_players,
    real_name,
    uuid,
    send_message,
    is_operator,
    position,
    dimension,
    teleport,
    health,
    max_health,
    set_health,
    game_mode,
    set_game_mode,
};
