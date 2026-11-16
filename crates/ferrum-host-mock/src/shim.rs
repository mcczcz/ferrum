//! 真实 ferrum-host 的 Rust shim 注入表；不接管任何模组回调。
use super::{LogRecord, domain, events, level_from_abi, world};
use ferrum_abi::*;
use ferrum_host::shim::{FerrumShimApi, ShimModCookie};
extern "C" fn log(cookie: ShimModCookie, level: LogLevel, message: FfiStr<'_>) {
    let Some(level) = level_from_abi(level) else {
        return;
    };
    let message = domain::text(message);
    if let Some(record) = world().mods.get_mut(&(cookie.0 as usize)) {
        record.logs.push(LogRecord { level, message });
    }
}
extern "C" fn should_log(_: ShimModCookie, _: LogLevel) -> FfiBool {
    FfiBool::TRUE
}
extern "C" fn log_loader(_: LogLevel, _: FfiStr<'_>) {}
extern "C" fn request_disable(cookie: ShimModCookie) {
    let mut w = world();
    let cookie = cookie.0 as usize;
    if !w.pending.contains(&cookie) {
        w.pending.push(cookie);
    }
}
extern "C" fn release_cookie(_: ShimModCookie) {}
pub(super) fn api() -> FerrumShimApi {
    FerrumShimApi {
        struct_size: size_of::<FerrumShimApi>() as u32,
        log,
        should_log,
        log_loader,
        request_disable,
        release_cookie,
        player_v1: domain::PLAYER,
        command_register: domain::register,
        command_unregister: domain::unregister,
        command_origin: domain::origin,
        command_int: domain::int,
        command_float: domain::float,
        command_bool: domain::bool_value,
        command_string: domain::string,
        command_players: domain::players,
        command_output: domain::output,
        block_v1: domain::BLOCK,
        actor_v1: domain::ACTOR,
        mob_v1: domain::MOB,
        event_subscribe: events::subscribe,
        event_unsubscribe: events::unsubscribe,
        event_player: events::player,
        event_is_cancelled: events::cancelled,
        event_cancel: events::cancel,
        event_chat_message: events::message,
        custom_event_publish: events::publish_custom,
        custom_event_subscribe: events::subscribe_custom,
        custom_event_unsubscribe: events::unsubscribe_custom,
    }
}
