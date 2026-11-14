//! 端到端：通过 ferrum-host 真实加载 `hello_ferrum.dll`（影子复制 → 握手 → 生命周期）。
//!
//! 需要先构建示例：`cargo build -p hello-ferrum`。DLL 不存在时测试跳过。

use ferrum_abi::{
    ActorApiV1, ActorKind, ActorUniqueId, BlockApiV1, BlockPos, BlockRuntimeId, MobApiV1,
};
use std::path::PathBuf;
use std::sync::Mutex;

use ferrum_abi::{
    ActorRuntimeId, CommandDefinitionV1, CommandHandle, DimensionId, FerrumStatus, FfiBool,
    FfiByteWriter, FfiStr, GameMode, LogLevel, PlayerApiV1, PlayerUuid, Vec3,
};
use ferrum_host::exports::{
    ferrum_host_disable_mod, ferrum_host_enable_mod, ferrum_host_init, ferrum_host_load_mod,
    ferrum_host_mark_main_thread, ferrum_host_unload_mod,
};
use ferrum_host::shim::{FerrumShimApi, ShimModCookie, ShimModInfo};

static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

extern "C" fn log(_c: ShimModCookie, _l: LogLevel, m: FfiStr<'_>) {
    // SAFETY: 宿主传入本次调用内有效的视图。
    let s = String::from_utf8_lossy(unsafe { m.as_bytes() }).into_owned();
    LOGS.lock().unwrap().push(s);
}
extern "C" fn should_log(_c: ShimModCookie, _l: LogLevel) -> FfiBool {
    FfiBool::TRUE
}
extern "C" fn log_loader(_l: LogLevel, _m: FfiStr<'_>) {}
extern "C" fn request_disable(_c: ShimModCookie) {}
extern "C" fn release_cookie(_c: ShimModCookie) {}

// 此既有集成检查只覆盖 DLL 生命周期，补齐内部 shim 新槽位。
extern "C" fn command_register(
    _: ShimModCookie,
    _: CommandHandle,
    _: *const CommandDefinitionV1<'_>,
) -> FerrumStatus {
    FerrumStatus::OK
}
extern "C" fn command_unregister(_: ShimModCookie, _: CommandHandle) -> FerrumStatus {
    FerrumStatus::OK
}
macro_rules! unavailable {
    ($name:ident($($arg:ty),*)) => {
        extern "C" fn $name($(_: $arg),*) -> FerrumStatus { FerrumStatus::SERVICE_UNAVAILABLE }
    };
}
unavailable!(player_name(FfiStr<'_>, *mut ActorRuntimeId));
unavailable!(player_uuid_find(PlayerUuid, *mut ActorRuntimeId));
unavailable!(player_list(*mut FfiByteWriter));
unavailable!(player_real_name(ActorRuntimeId, *mut FfiByteWriter));
unavailable!(player_uuid(ActorRuntimeId, *mut PlayerUuid));
unavailable!(player_message(ActorRuntimeId, FfiStr<'_>));
unavailable!(player_operator(ActorRuntimeId, *mut FfiBool));
unavailable!(player_position(ActorRuntimeId, *mut Vec3));
unavailable!(player_dimension(ActorRuntimeId, *mut DimensionId));
unavailable!(player_teleport(ActorRuntimeId, Vec3, DimensionId));
unavailable!(player_health(ActorRuntimeId, *mut f32));
unavailable!(player_set_health(ActorRuntimeId, f32));
unavailable!(player_game_mode(ActorRuntimeId, *mut GameMode));
unavailable!(player_set_game_mode(ActorRuntimeId, GameMode));
unavailable!(command_origin(u64, *mut ActorRuntimeId));
unavailable!(command_int(u64, FfiStr<'_>, *mut i32));
unavailable!(command_float(u64, FfiStr<'_>, *mut f32));
unavailable!(command_bool(u64, FfiStr<'_>, *mut FfiBool));
unavailable!(command_string(u64, FfiStr<'_>, *mut FfiByteWriter));
unavailable!(command_output(u64, FfiBool, FfiStr<'_>));
unavailable!(actor_find(ActorUniqueId, *mut ActorRuntimeId));
unavailable!(actor_unique(ActorRuntimeId, *mut ActorUniqueId));
unavailable!(actor_kind(ActorRuntimeId, *mut ActorKind));
unavailable!(block_type(DimensionId, BlockPos, *mut FfiByteWriter));
unavailable!(block_runtime(DimensionId, BlockPos, *mut BlockRuntimeId));
unavailable!(block_air(DimensionId, BlockPos, *mut FfiBool));
unavailable!(block_set_type(DimensionId, BlockPos, FfiStr<'_>));

fn example_dll() -> Option<PathBuf> {
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target");
    ["debug", "release"]
        .iter()
        .map(|p| target.join(p).join("hello_ferrum.dll"))
        .find(|p| p.exists())
}

#[test]
fn load_enable_disable_unload_reload() {
    let Some(dll) = example_dll() else {
        eprintln!("跳过：未找到 hello_ferrum.dll，请先 cargo build -p hello-ferrum");
        return;
    };

    let shim = FerrumShimApi {
        struct_size: size_of::<FerrumShimApi>() as u32,
        log,
        should_log,
        log_loader,
        request_disable,
        release_cookie,
        command_register,
        command_unregister,
        command_origin,
        command_int,
        command_float,
        command_bool,
        command_string,
        command_players: command_string,
        command_output,
        actor_v1: ActorApiV1 {
            struct_size: size_of::<ActorApiV1>() as u32,
            find_by_unique_id: actor_find,
            unique_id: actor_unique,
            type_name: player_real_name,
            kind: actor_kind,
            position: player_position,
            dimension: player_dimension,
            teleport: player_teleport,
        },
        mob_v1: MobApiV1 {
            struct_size: size_of::<MobApiV1>() as u32,
            health: player_health,
            max_health: player_health,
            set_health: player_set_health,
        },
        block_v1: BlockApiV1 {
            struct_size: size_of::<BlockApiV1>() as u32,
            type_name: block_type,
            runtime_id: block_runtime,
            is_air: block_air,
            set_type: block_set_type,
        },
        player_v1: PlayerApiV1 {
            struct_size: size_of::<PlayerApiV1>() as u32,
            find_by_name: player_name,
            find_by_uuid: player_uuid_find,
            online_players: player_list,
            real_name: player_real_name,
            uuid: player_uuid,
            send_message: player_message,
            is_operator: player_operator,
            position: player_position,
            dimension: player_dimension,
            teleport: player_teleport,
            health: player_health,
            max_health: player_health,
            set_health: player_set_health,
            game_mode: player_game_mode,
            set_game_mode: player_set_game_mode,
        },
    };
    // SAFETY: shim 函数为 'static。
    assert_eq!(unsafe { ferrum_host_init(&shim) }, FerrumStatus::OK);
    ferrum_host_mark_main_thread();

    let mod_dir = std::env::temp_dir().join(format!("ferrum-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&mod_dir).unwrap();
    let entry = mod_dir.join("hello_ferrum.dll");
    std::fs::copy(&dll, &entry).unwrap();

    let s = |p: &PathBuf| p.to_string_lossy().into_owned();
    let (entry_s, dir_s) = (s(&entry), s(&mod_dir));
    let config_s = s(&mod_dir.join("config"));
    let info = ShimModInfo {
        struct_size: size_of::<ShimModInfo<'_>>() as u32,
        cookie: ShimModCookie(std::ptr::null_mut()),
        name: FfiStr::new("hello-ferrum"),
        version: FfiStr::new("0.1.0"),
        entry_path: FfiStr::new(&entry_s),
        mod_dir: FfiStr::new(&dir_s),
        data_dir: FfiStr::new(&dir_s),
        config_dir: FfiStr::new(&config_s),
        lang_dir: FfiStr::new(&dir_s),
    };

    for round in 1..=2 {
        let mut ctx = 0u64;
        // SAFETY: info 与 ctx 在调用期间有效。
        let status = unsafe { ferrum_host_load_mod(&info, &mut ctx) };
        assert_eq!(status, FerrumStatus::OK, "round {round}");
        assert_eq!(ferrum_host_enable_mod(ctx), FerrumStatus::OK);
        assert_eq!(ferrum_host_disable_mod(ctx), FerrumStatus::OK);
        assert_eq!(ferrum_host_unload_mod(ctx), FerrumStatus::OK);
        // 卸载后旧句柄失效。
        assert_eq!(ferrum_host_enable_mod(ctx), FerrumStatus::STALE_HANDLE);
    }

    assert!(mod_dir.join(".ferrum-shadow").is_dir());
    let logs = LOGS.lock().unwrap().clone();
    assert!(logs.iter().any(|l| l.contains("正在加载")), "{logs:?}");
    assert!(
        logs.iter().any(|l| l.contains("已启用（第 1 次）")),
        "{logs:?}"
    );
    assert_eq!(
        logs.iter().filter(|l| l.contains("已卸载")).count(),
        2,
        "{logs:?}"
    );

    // 镜像被有意保留，影子 DLL 删不掉；只尽力清理。
    let _ = std::fs::remove_dir_all(&mod_dir);
}
