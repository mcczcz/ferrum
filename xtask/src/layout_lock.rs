//! 生成 `crates/ferrum-abi/tests/layout_lock.rs`：只追加、从不改写既有行。
//!
//! 每条断言按当前 `ferrum-abi` 的实际布局计算。既有行若与新布局冲突，
//! 测试会直接编译失败——这正是布局锁的目的：已发布的偏移量永不改变。

use std::mem::{align_of, offset_of, size_of};

use ferrum_abi::*;

use crate::{emit, workspace_root};

const PREAMBLE: &str = "\
//! ABI 布局锁：由 `cargo xtask layout-lock` 追加生成，既有行严禁修改或删除。
//! 见 docs/01-specs/abi-stability-spec.md §4。

#![allow(unused_imports)]

use core::mem::{align_of, offset_of, size_of};
use ferrum_abi::*;
";

macro_rules! lock {
    ($out:ident, $ty:ident $(, $field:ident)*) => {{
        $out.push(format!(
            "const _: () = assert!(size_of::<{}>() == {});",
            stringify!($ty),
            size_of::<$ty>()
        ));
        $out.push(format!(
            "const _: () = assert!(align_of::<{}>() == {});",
            stringify!($ty),
            align_of::<$ty>()
        ));
        $(
            $out.push(format!(
                "const _: () = assert!(offset_of!({}, {}) == {});",
                stringify!($ty),
                stringify!($field),
                offset_of!($ty, $field)
            ));
        )*
    }};
}

/// 已发布类型的布局断言。新增类型或尾追加字段时在此登记。
fn assertions() -> Vec<String> {
    let mut out = Vec::new();
    lock!(out, AbiVersion, major, minor, patch);
    lock!(out, FerrumStatus);
    lock!(out, FfiBool);
    lock!(out, LogLevel);
    lock!(out, ModDirKind);
    lock!(out, FerrumModFlags);
    lock!(out, GenerationalHandle);
    lock!(out, ModContextHandle);
    lock!(out, ActorRuntimeId);
    lock!(out, ActorUniqueId);
    lock!(out, ActorKind);
    lock!(out, BlockPos, x, y, z);
    lock!(out, BlockRuntimeId);
    lock!(
        out,
        ActorApiV1,
        struct_size,
        find_by_unique_id,
        unique_id,
        type_name,
        kind,
        position,
        dimension,
        teleport
    );
    lock!(out, MobApiV1, struct_size, health, max_health, set_health);
    lock!(
        out,
        BlockApiV1,
        struct_size,
        type_name,
        runtime_id,
        is_air,
        set_type
    );
    lock!(out, PlayerUuid, bytes);
    lock!(out, Vec3, x, y, z);
    lock!(out, DimensionId);
    lock!(out, GameMode);
    lock!(
        out,
        PlayerApiV1,
        struct_size,
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
        set_game_mode
    );
    lock!(out, CommandHandle);
    lock!(out, CommandScopeHandle);
    lock!(out, CommandPermission);
    lock!(out, CommandParamKind);
    lock!(out, CommandCallbackV1, env, invoke_fn, drop_fn);
    lock!(
        out,
        CommandApiV1,
        struct_size,
        register,
        unregister,
        origin_player,
        get_int,
        get_float,
        get_bool,
        get_string,
        get_players,
        output
    );
    lock!(out, CommandParamV1, name, kind, optional, reserved);
    lock!(
        out,
        CommandDefinitionV1,
        struct_size,
        name,
        description,
        permission,
        params
    );
    lock!(out, RawMainThreadToken);
    lock!(out, FfiByteWriter, context, write_fn);
    lock!(
        out,
        CoreApiV1,
        struct_size,
        take_last_error,
        set_last_error,
        report_panic,
        get_mod_name,
        get_mod_version,
        get_mod_dir,
        is_main_thread
    );
    lock!(out, LogApiV1, struct_size, log, should_log);
    lock!(
        out,
        FerrumHostApi,
        struct_size,
        reserved_flags,
        magic,
        host_abi_version,
        core_v1,
        log_v1,
        query_interface
    );
    lock!(
        out,
        FerrumModDescriptor,
        struct_size,
        flags,
        required_abi_version,
        sdk_version,
        on_load,
        on_enable,
        on_disable,
        on_unload
    );
    // FfiStr / FfiSlice 带生命周期参数，单独处理。
    out.push(format!(
        "const _: () = assert!(size_of::<FfiStr<'static>>() == {});",
        size_of::<FfiStr<'static>>()
    ));
    out.push(format!(
        "const _: () = assert!(offset_of!(FfiStr<'static>, len) == {});",
        offset_of!(FfiStr<'static>, len)
    ));
    out.push(format!(
        "const _: () = assert!(size_of::<FfiSlice<'static, u8>>() == {});",
        size_of::<FfiSlice<'static, u8>>()
    ));
    // v1.0 最小尺寸常量与真实布局保持一致。
    out.push(format!(
        "const _: () = assert!(FERRUM_HOST_API_MIN_SIZE_V1_0 == {});",
        FERRUM_HOST_API_MIN_SIZE_V1_0
    ));
    out.push(format!(
        "const _: () = assert!(CORE_API_MIN_SIZE_V1_0 == {});",
        CORE_API_MIN_SIZE_V1_0
    ));
    out.push(format!(
        "const _: () = assert!(LOG_API_MIN_SIZE_V1_0 == {});",
        LOG_API_MIN_SIZE_V1_0
    ));
    out.push(format!(
        "const _: () = assert!(FERRUM_MOD_DESCRIPTOR_MIN_SIZE_V1_0 == {});",
        FERRUM_MOD_DESCRIPTOR_MIN_SIZE_V1_0
    ));
    out
}

pub fn run(check: bool) -> Result<bool, String> {
    let path = workspace_root().join("crates/ferrum-abi/tests/layout_lock.rs");
    let existing = std::fs::read_to_string(&path)
        .map(|s| s.replace("\r\n", "\n"))
        .unwrap_or_else(|_| PREAMBLE.to_owned());

    let mut text = existing.clone();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let known: std::collections::HashSet<&str> = existing.lines().map(str::trim).collect();
    let fresh: Vec<String> = assertions()
        .into_iter()
        .filter(|l| !known.contains(l.as_str()))
        .collect();
    if !fresh.is_empty() {
        text.push('\n');
        for line in &fresh {
            text.push_str(line);
            text.push('\n');
        }
    }
    let expected = if fresh.is_empty() { existing } else { text };
    emit(&path, &expected, check)
}
