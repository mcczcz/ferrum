//! 玩家与命令的纯 Rust 假实现；命令参数由调用方传入已解析的值。
use super::{MockHost, MockModState, check, is_main_thread, with_mod};
use ferrum_abi::*;
use ferrum_sdk::Error;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct MockPlayer {
    pub uuid: PlayerUuid,
    pub name: String,
    pub operator: bool,
}
#[derive(Clone, Debug)]
pub struct MockActor {
    pub type_name: String,
    pub kind: ActorKind,
    pub position: Vec3,
    pub dimension: DimensionId,
    pub health: f32,
    pub max_health: f32,
}
impl MockActor {
    pub fn new(type_name: impl Into<String>, kind: ActorKind) -> Self {
        Self {
            type_name: type_name.into(),
            kind,
            position: Vec3::default(),
            dimension: DimensionId::OVERWORLD,
            health: 20.,
            max_health: 20.,
        }
    }
}
struct OtherActor {
    id: ActorRuntimeId,
    actor: MockActor,
}
/// 可读取、配置的 mock 玩家状态；生命值以点计，默认 20 点。
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct MockPlayerState {
    pub position: Vec3,
    pub dimension: DimensionId,
    pub health: f32,
    pub max_health: f32,
    pub game_mode: GameMode,
}
impl Default for MockPlayerState {
    fn default() -> Self {
        Self {
            position: Vec3::default(),
            dimension: DimensionId::OVERWORLD,
            health: 20.,
            max_health: 20.,
            game_mode: GameMode::SURVIVAL,
        }
    }
}
#[derive(Clone, Debug)]
pub enum MockCommandValue {
    Int(i32),
    Float(f32),
    Bool(bool),
    String(String),
    Players(Vec<PlayerUuid>),
}
impl MockCommandValue {
    fn kind(&self) -> CommandParamKind {
        match self {
            Self::Int(_) => CommandParamKind::INT,
            Self::Float(_) => CommandParamKind::FLOAT,
            Self::Bool(_) => CommandParamKind::BOOL,
            Self::String(_) => CommandParamKind::STRING,
            Self::Players(_) => CommandParamKind::PLAYER,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct MockCommandOutput {
    pub messages: Vec<(bool, String)>,
}

struct OnlinePlayer {
    id: ActorRuntimeId,
    player: MockPlayer,
    messages: Vec<String>,
    state: MockPlayerState,
}
struct Command {
    owner: ModContextHandle,
    name: String,
    permission: CommandPermission,
    params: Vec<(String, CommandParamKind, bool)>,
    callback: CommandCallbackV1,
}
impl Drop for Command {
    fn drop(&mut self) {
        (self.callback.drop_fn)(self.callback.env);
    }
}
struct Scope {
    owner: ModContextHandle,
    origin: Option<ActorRuntimeId>,
    args: BTreeMap<String, MockCommandValue>,
    params: Vec<(String, CommandParamKind, bool)>,
    output: MockCommandOutput,
}
#[derive(Default)]
struct Domain {
    players: Vec<OnlinePlayer>,
    actors: Vec<OtherActor>,
    blocks: BTreeMap<(i32, i32, i32, i32), String>,
    block_types: BTreeMap<String, BlockRuntimeId>,
    loaded_chunks: Vec<(i32, i32, i32)>,
    commands: BTreeMap<u64, Rc<Command>>,
    scopes: BTreeMap<u64, Scope>,
    serial: u32,
    next_player: u64,
    custom_dimensions: Vec<DimensionId>,
    active: BTreeMap<u64, usize>,
    pending: Vec<ModContextHandle>,
}
thread_local! { static DOMAIN: RefCell<Domain> = RefCell::new(Domain::default()); }

fn guard(f: impl FnOnce() -> FerrumStatus) -> FerrumStatus {
    if !is_main_thread().get() {
        return FerrumStatus::WRONG_THREAD;
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(s) => s,
        Err(p) => {
            std::mem::forget(p);
            FerrumStatus::INTERNAL_ERROR
        }
    }
}
fn text(s: FfiStr<'_>) -> String {
    // SAFETY: SDK 提供本次同步调用内有效的借用字节。
    String::from_utf8_lossy(unsafe { s.as_bytes() }).into_owned()
}
fn write(out: *mut FfiByteWriter, bytes: &[u8]) -> FerrumStatus {
    // SAFETY: 调用方保证 out 为空或为本次调用内的有效 writer。
    match unsafe { out.as_ref() } {
        Some(w) => w.write(bytes),
        None => FerrumStatus::NULL_POINTER,
    }
}
fn put<T>(out: *mut T, value: T) -> FerrumStatus {
    if out.is_null() {
        return FerrumStatus::NULL_POINTER;
    }
    // SAFETY: out 非空且调用方为匹配的 T 提供独占可写内存。
    unsafe { out.write(value) };
    FerrumStatus::OK
}
fn ids(out: *mut FfiByteWriter, values: impl Iterator<Item = ActorRuntimeId>) -> FerrumStatus {
    let bytes: Vec<u8> = values.flat_map(|id| id.0.to_le_bytes()).collect();
    write(out, &bytes)
}
fn player<T>(
    id: ActorRuntimeId,
    f: impl FnOnce(&mut OnlinePlayer) -> T,
) -> Result<T, FerrumStatus> {
    DOMAIN.with(|d| {
        d.borrow_mut()
            .players
            .iter_mut()
            .find(|p| p.id == id)
            .map(f)
            .ok_or(FerrumStatus::NOT_FOUND)
    })
}
extern "C" fn find_name(name: FfiStr<'_>, out: *mut ActorRuntimeId) -> FerrumStatus {
    guard(|| {
        let name = text(name);
        let found = DOMAIN.with(|d| {
            d.borrow()
                .players
                .iter()
                .find(|p| p.player.name == name)
                .map(|p| p.id)
        });
        found.map_or(FerrumStatus::NOT_FOUND, |id| put(out, id))
    })
}
extern "C" fn find_uuid(uuid: PlayerUuid, out: *mut ActorRuntimeId) -> FerrumStatus {
    guard(|| {
        let found = DOMAIN.with(|d| {
            d.borrow()
                .players
                .iter()
                .find(|p| p.player.uuid == uuid)
                .map(|p| p.id)
        });
        found.map_or(FerrumStatus::NOT_FOUND, |id| put(out, id))
    })
}
extern "C" fn online(out: *mut FfiByteWriter) -> FerrumStatus {
    guard(|| {
        let found = DOMAIN.with(|d| d.borrow().players.iter().map(|p| p.id).collect::<Vec<_>>());
        ids(out, found.into_iter())
    })
}
extern "C" fn real_name(id: ActorRuntimeId, out: *mut FfiByteWriter) -> FerrumStatus {
    guard(|| player(id, |p| p.player.name.clone()).map_or_else(|s| s, |s| write(out, s.as_bytes())))
}
extern "C" fn uuid(id: ActorRuntimeId, out: *mut PlayerUuid) -> FerrumStatus {
    guard(|| player(id, |p| p.player.uuid).map_or_else(|s| s, |v| put(out, v)))
}
extern "C" fn message(id: ActorRuntimeId, s: FfiStr<'_>) -> FerrumStatus {
    guard(|| player(id, |p| p.messages.push(text(s))).map_or_else(|s| s, |_| FerrumStatus::OK))
}
extern "C" fn operator(id: ActorRuntimeId, out: *mut FfiBool) -> FerrumStatus {
    guard(|| player(id, |p| FfiBool::from(p.player.operator)).map_or_else(|s| s, |v| put(out, v)))
}
fn valid_position(pos: Vec3) -> bool {
    pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite()
}
fn valid_mode(mode: GameMode) -> bool {
    matches!(
        mode,
        GameMode::SURVIVAL | GameMode::CREATIVE | GameMode::ADVENTURE | GameMode::SPECTATOR
    )
}
fn dimension_exists(dim: DimensionId) -> bool {
    (0..=2).contains(&dim.0) || DOMAIN.with(|d| d.borrow().custom_dimensions.contains(&dim))
}
extern "C" fn position(id: ActorRuntimeId, out: *mut Vec3) -> FerrumStatus {
    guard(|| player(id, |p| p.state.position).map_or_else(|s| s, |v| put(out, v)))
}
extern "C" fn dimension(id: ActorRuntimeId, out: *mut DimensionId) -> FerrumStatus {
    guard(|| player(id, |p| p.state.dimension).map_or_else(|s| s, |v| put(out, v)))
}
extern "C" fn teleport(id: ActorRuntimeId, pos: Vec3, dim: DimensionId) -> FerrumStatus {
    guard(|| {
        if !valid_position(pos) {
            return FerrumStatus::INVALID_ARGUMENT;
        }
        if !dimension_exists(dim) {
            return FerrumStatus::NOT_FOUND;
        }
        player(id, |p| {
            p.state.position = pos;
            p.state.dimension = dim;
        })
        .map_or_else(|s| s, |_| FerrumStatus::OK)
    })
}
extern "C" fn health(id: ActorRuntimeId, out: *mut f32) -> FerrumStatus {
    guard(|| player(id, |p| p.state.health).map_or_else(|s| s, |v| put(out, v)))
}
extern "C" fn max_health(id: ActorRuntimeId, out: *mut f32) -> FerrumStatus {
    guard(|| player(id, |p| p.state.max_health).map_or_else(|s| s, |v| put(out, v)))
}
extern "C" fn set_health(id: ActorRuntimeId, value: f32) -> FerrumStatus {
    guard(|| {
        if !value.is_finite() || value < 0. {
            return FerrumStatus::INVALID_ARGUMENT;
        }
        player(id, |p| {
            if value > p.state.max_health {
                return FerrumStatus::INVALID_ARGUMENT;
            }
            p.state.health = value;
            FerrumStatus::OK
        })
        .unwrap_or_else(|s| s)
    })
}
extern "C" fn game_mode(id: ActorRuntimeId, out: *mut GameMode) -> FerrumStatus {
    guard(|| player(id, |p| p.state.game_mode).map_or_else(|s| s, |v| put(out, v)))
}
extern "C" fn set_game_mode(id: ActorRuntimeId, mode: GameMode) -> FerrumStatus {
    guard(|| {
        if !valid_mode(mode) {
            return FerrumStatus::INVALID_ARGUMENT;
        }
        player(id, |p| p.state.game_mode = mode).map_or_else(|s| s, |_| FerrumStatus::OK)
    })
}
pub(super) static PLAYER: PlayerApiV1 = PlayerApiV1 {
    struct_size: size_of::<PlayerApiV1>() as u32,
    find_by_name: find_name,
    find_by_uuid: find_uuid,
    online_players: online,
    real_name,
    uuid,
    send_message: message,
    is_operator: operator,
    position,
    dimension,
    teleport,
    health,
    max_health,
    set_health,
    game_mode,
    set_game_mode,
};

fn serial(d: &mut Domain) -> GenerationalHandle {
    d.serial += 1;
    GenerationalHandle::new(d.serial, 1).expect("mock serial")
}

fn actor_snapshot(id: ActorRuntimeId) -> Result<MockActor, FerrumStatus> {
    DOMAIN.with(|d| {
        let d = d.borrow();
        if let Some(p) = d.players.iter().find(|p| p.id == id) {
            return Ok(MockActor {
                type_name: "minecraft:player".into(),
                kind: ActorKind::PLAYER,
                position: p.state.position,
                dimension: p.state.dimension,
                health: p.state.health,
                max_health: p.state.max_health,
            });
        }
        d.actors
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.actor.clone())
            .ok_or(FerrumStatus::NOT_FOUND)
    })
}
fn edit_actor(id: ActorRuntimeId, f: impl FnOnce(&mut MockActor)) -> FerrumStatus {
    DOMAIN.with(|d| {
        let mut d = d.borrow_mut();
        if let Some(p) = d.players.iter_mut().find(|p| p.id == id) {
            let mut a = MockActor {
                type_name: "minecraft:player".into(),
                kind: ActorKind::PLAYER,
                position: p.state.position,
                dimension: p.state.dimension,
                health: p.state.health,
                max_health: p.state.max_health,
            };
            f(&mut a);
            p.state.position = a.position;
            p.state.dimension = a.dimension;
            p.state.health = a.health;
            return FerrumStatus::OK;
        }
        match d.actors.iter_mut().find(|p| p.id == id) {
            Some(p) => {
                f(&mut p.actor);
                FerrumStatus::OK
            }
            None => FerrumStatus::NOT_FOUND,
        }
    })
}
extern "C" fn actor_find(id: ActorUniqueId, out: *mut ActorRuntimeId) -> FerrumStatus {
    guard(|| {
        let found = DOMAIN.with(|d| {
            let d = d.borrow();
            d.players
                .iter()
                .map(|p| p.id)
                .chain(d.actors.iter().map(|p| p.id))
                .find(|v| v.0 as i64 == id.0)
        });
        found.map_or(FerrumStatus::NOT_FOUND, |id| put(out, id))
    })
}
extern "C" fn actor_unique(id: ActorRuntimeId, out: *mut ActorUniqueId) -> FerrumStatus {
    guard(|| actor_snapshot(id).map_or_else(|s| s, |_| put(out, ActorUniqueId(id.0 as i64))))
}
extern "C" fn actor_type(id: ActorRuntimeId, out: *mut FfiByteWriter) -> FerrumStatus {
    guard(|| actor_snapshot(id).map_or_else(|s| s, |a| write(out, a.type_name.as_bytes())))
}
extern "C" fn actor_kind(id: ActorRuntimeId, out: *mut ActorKind) -> FerrumStatus {
    guard(|| actor_snapshot(id).map_or_else(|s| s, |a| put(out, a.kind)))
}
extern "C" fn actor_position(id: ActorRuntimeId, out: *mut Vec3) -> FerrumStatus {
    guard(|| actor_snapshot(id).map_or_else(|s| s, |a| put(out, a.position)))
}
extern "C" fn actor_dimension(id: ActorRuntimeId, out: *mut DimensionId) -> FerrumStatus {
    guard(|| actor_snapshot(id).map_or_else(|s| s, |a| put(out, a.dimension)))
}
extern "C" fn actor_teleport(id: ActorRuntimeId, pos: Vec3, dim: DimensionId) -> FerrumStatus {
    guard(|| {
        if !valid_position(pos) {
            return FerrumStatus::INVALID_ARGUMENT;
        }
        if !dimension_exists(dim) {
            return FerrumStatus::NOT_FOUND;
        }
        edit_actor(id, |a| {
            a.position = pos;
            a.dimension = dim;
        })
    })
}
fn mob_snapshot(id: ActorRuntimeId) -> Result<MockActor, FerrumStatus> {
    let actor = actor_snapshot(id)?;
    if !matches!(actor.kind, ActorKind::MOB | ActorKind::PLAYER) {
        return Err(FerrumStatus::INVALID_ARGUMENT);
    }
    Ok(actor)
}
extern "C" fn mob_health(id: ActorRuntimeId, out: *mut f32) -> FerrumStatus {
    guard(|| mob_snapshot(id).map_or_else(|s| s, |a| put(out, a.health)))
}
extern "C" fn mob_max_health(id: ActorRuntimeId, out: *mut f32) -> FerrumStatus {
    guard(|| mob_snapshot(id).map_or_else(|s| s, |a| put(out, a.max_health)))
}
extern "C" fn mob_set_health(id: ActorRuntimeId, value: f32) -> FerrumStatus {
    guard(|| {
        let actor = match mob_snapshot(id) {
            Ok(a) => a,
            Err(s) => return s,
        };
        if !value.is_finite() || value < 0. || value > actor.max_health {
            return FerrumStatus::INVALID_ARGUMENT;
        }
        edit_actor(id, |a| a.health = value)
    })
}
pub(super) static ACTOR: ActorApiV1 = ActorApiV1 {
    struct_size: size_of::<ActorApiV1>() as u32,
    find_by_unique_id: actor_find,
    unique_id: actor_unique,
    type_name: actor_type,
    kind: actor_kind,
    position: actor_position,
    dimension: actor_dimension,
    teleport: actor_teleport,
};
pub(super) static MOB: MobApiV1 = MobApiV1 {
    struct_size: size_of::<MobApiV1>() as u32,
    health: mob_health,
    max_health: mob_max_health,
    set_health: mob_set_health,
};

fn block_key(dim: DimensionId, pos: BlockPos) -> Result<(i32, i32, i32, i32), FerrumStatus> {
    if !dimension_exists(dim) {
        return Err(FerrumStatus::NOT_FOUND);
    }
    let (min, max) = match dim.0 {
        0 => (-64, 320),
        1 => (0, 128),
        _ => (0, 256),
    };
    if pos.y < min || pos.y >= max {
        return Err(FerrumStatus::INVALID_ARGUMENT);
    }
    if !DOMAIN.with(|d| {
        d.borrow()
            .loaded_chunks
            .contains(&(dim.0, pos.x.div_euclid(16), pos.z.div_euclid(16)))
    }) {
        return Err(FerrumStatus::NOT_FOUND);
    }
    Ok((dim.0, pos.x, pos.y, pos.z))
}
fn block_snapshot(
    dim: DimensionId,
    pos: BlockPos,
) -> Result<(String, BlockRuntimeId), FerrumStatus> {
    let key = block_key(dim, pos)?;
    DOMAIN.with(|d| {
        let d = d.borrow();
        let name = d
            .blocks
            .get(&key)
            .map(String::as_str)
            .unwrap_or("minecraft:air");
        let id = d
            .block_types
            .get(name)
            .copied()
            .ok_or(FerrumStatus::NOT_FOUND)?;
        Ok((name.to_owned(), id))
    })
}
extern "C" fn block_type(dim: DimensionId, pos: BlockPos, out: *mut FfiByteWriter) -> FerrumStatus {
    guard(|| block_snapshot(dim, pos).map_or_else(|s| s, |(name, _)| write(out, name.as_bytes())))
}
extern "C" fn block_runtime(
    dim: DimensionId,
    pos: BlockPos,
    out: *mut BlockRuntimeId,
) -> FerrumStatus {
    guard(|| block_snapshot(dim, pos).map_or_else(|s| s, |(_, id)| put(out, id)))
}
extern "C" fn block_air(dim: DimensionId, pos: BlockPos, out: *mut FfiBool) -> FerrumStatus {
    guard(|| {
        block_snapshot(dim, pos).map_or_else(
            |s| s,
            |(name, _)| put(out, (name == "minecraft:air").into()),
        )
    })
}
extern "C" fn block_set_type(dim: DimensionId, pos: BlockPos, name: FfiStr<'_>) -> FerrumStatus {
    guard(|| {
        let name = text(name);
        if name.is_empty() {
            return FerrumStatus::INVALID_ARGUMENT;
        }
        let key = match block_key(dim, pos) {
            Ok(k) => k,
            Err(s) => return s,
        };
        DOMAIN.with(|d| {
            let mut d = d.borrow_mut();
            if !d.block_types.contains_key(&name) {
                return FerrumStatus::NOT_FOUND;
            }
            d.blocks.insert(key, name);
            FerrumStatus::OK
        })
    })
}
pub(super) static BLOCK: BlockApiV1 = BlockApiV1 {
    struct_size: size_of::<BlockApiV1>() as u32,
    type_name: block_type,
    runtime_id: block_runtime,
    is_air: block_air,
    set_type: block_set_type,
};
fn enabled(ctx: ModContextHandle) -> Result<(), FerrumStatus> {
    with_mod(ctx, |m| {
        matches!(m.state, MockModState::Enabling | MockModState::Enabled)
    })
    .and_then(|ok| {
        if ok {
            Ok(())
        } else {
            Err(FerrumStatus::MOD_DISABLED)
        }
    })
}
extern "C" fn register(
    ctx: ModContextHandle,
    def: *const CommandDefinitionV1<'_>,
    callback: CommandCallbackV1,
    out: *mut CommandHandle,
) -> FerrumStatus {
    guard(|| {
        if let Err(s) = enabled(ctx) {
            return s;
        }
        if out.is_null() || def.is_null() {
            return FerrumStatus::NULL_POINTER;
        }
        // SAFETY: def 指向 ABI 调用方提供的同步借用描述头，先读取 struct_size。
        if unsafe { (*def).struct_size } < size_of::<CommandDefinitionV1<'_>>() as u32 {
            return FerrumStatus::ABI_INCOMPATIBLE;
        }
        // SAFETY: struct_size 覆盖完整描述，SDK 保证 params 和字符串在调用内有效。
        let def = unsafe { &*def };
        // SAFETY: def.params 由 SDK 的已初始化 CommandParamV1 数组构造，空切片被辅助方法处理。
        let params = unsafe { def.params.as_slice() }
            .iter()
            .map(|p| (text(p.name), p.kind, p.optional.get()))
            .collect::<Vec<_>>();
        let name = text(def.name);
        let mut optional = false;
        let mut names = std::collections::BTreeSet::new();
        if name.is_empty() || def.permission.0 > 4 {
            return FerrumStatus::INVALID_ARGUMENT;
        }
        for (n, k, o) in &params {
            if n.is_empty() || !names.insert(n) || k.0 > 4 || (!o && optional) {
                return FerrumStatus::INVALID_ARGUMENT;
            }
            optional |= *o;
        }
        DOMAIN.with(|d| {
            let mut d = d.borrow_mut();
            for c in d.commands.values().filter(|c| c.name == name) {
                if c.owner != ctx {
                    return FerrumStatus::PERMISSION_DENIED;
                }
                if c.params == params {
                    return FerrumStatus::ALREADY_EXISTS;
                }
                if c.permission != def.permission {
                    return FerrumStatus::INVALID_ARGUMENT;
                }
            }
            let handle = CommandHandle(serial(&mut d));
            d.commands.insert(
                handle.0.to_raw(),
                Rc::new(Command {
                    owner: ctx,
                    name,
                    permission: def.permission,
                    params,
                    callback,
                }),
            );
            put(out, handle)
        })
    })
}
extern "C" fn unregister(ctx: ModContextHandle, h: CommandHandle) -> FerrumStatus {
    guard(|| {
        let removed = DOMAIN.with(|d| {
            let mut d = d.borrow_mut();
            match d.commands.get(&h.0.to_raw()) {
                Some(c) if c.owner != ctx => Err(FerrumStatus::PERMISSION_DENIED),
                None => Err(FerrumStatus::STALE_HANDLE),
                _ => Ok(d.commands.remove(&h.0.to_raw())),
            }
        });
        match removed {
            Ok(c) => {
                drop(c);
                FerrumStatus::OK
            }
            Err(s) => s,
        }
    })
}
fn scope<T>(
    ctx: ModContextHandle,
    h: CommandScopeHandle,
    f: impl FnOnce(&mut Scope) -> T,
) -> Result<T, FerrumStatus> {
    DOMAIN.with(|d| {
        let mut d = d.borrow_mut();
        match d.scopes.get_mut(&h.0.to_raw()) {
            Some(s) if s.owner == ctx => Ok(f(s)),
            Some(_) => Err(FerrumStatus::PERMISSION_DENIED),
            None => Err(FerrumStatus::STALE_HANDLE),
        }
    })
}
fn arg(
    ctx: ModContextHandle,
    h: CommandScopeHandle,
    name: FfiStr<'_>,
    kind: CommandParamKind,
) -> Result<MockCommandValue, FerrumStatus> {
    let name = text(name);
    scope(ctx, h, |s| {
        if !s.params.iter().any(|(n, k, _)| *n == name && *k == kind) {
            return Err(FerrumStatus::INVALID_ARGUMENT);
        }
        s.args.get(&name).cloned().ok_or(FerrumStatus::NOT_FOUND)
    })?
}
extern "C" fn origin(
    ctx: ModContextHandle,
    h: CommandScopeHandle,
    out: *mut ActorRuntimeId,
) -> FerrumStatus {
    guard(|| {
        scope(ctx, h, |s| s.origin).map_or_else(
            |s| s,
            |p| p.map_or(FerrumStatus::NOT_FOUND, |id| put(out, id)),
        )
    })
}
extern "C" fn int(
    ctx: ModContextHandle,
    h: CommandScopeHandle,
    n: FfiStr<'_>,
    out: *mut i32,
) -> FerrumStatus {
    guard(|| match arg(ctx, h, n, CommandParamKind::INT) {
        Ok(MockCommandValue::Int(v)) => put(out, v),
        Err(s) => s,
        _ => FerrumStatus::INVALID_ARGUMENT,
    })
}
extern "C" fn float(
    ctx: ModContextHandle,
    h: CommandScopeHandle,
    n: FfiStr<'_>,
    out: *mut f32,
) -> FerrumStatus {
    guard(|| match arg(ctx, h, n, CommandParamKind::FLOAT) {
        Ok(MockCommandValue::Float(v)) => put(out, v),
        Err(s) => s,
        _ => FerrumStatus::INVALID_ARGUMENT,
    })
}
extern "C" fn bool_value(
    ctx: ModContextHandle,
    h: CommandScopeHandle,
    n: FfiStr<'_>,
    out: *mut FfiBool,
) -> FerrumStatus {
    guard(|| match arg(ctx, h, n, CommandParamKind::BOOL) {
        Ok(MockCommandValue::Bool(v)) => put(out, v.into()),
        Err(s) => s,
        _ => FerrumStatus::INVALID_ARGUMENT,
    })
}
extern "C" fn string(
    ctx: ModContextHandle,
    h: CommandScopeHandle,
    n: FfiStr<'_>,
    out: *mut FfiByteWriter,
) -> FerrumStatus {
    guard(|| match arg(ctx, h, n, CommandParamKind::STRING) {
        Ok(MockCommandValue::String(v)) => write(out, v.as_bytes()),
        Err(s) => s,
        _ => FerrumStatus::INVALID_ARGUMENT,
    })
}
extern "C" fn players(
    ctx: ModContextHandle,
    h: CommandScopeHandle,
    n: FfiStr<'_>,
    out: *mut FfiByteWriter,
) -> FerrumStatus {
    guard(|| match arg(ctx, h, n, CommandParamKind::PLAYER) {
        Ok(MockCommandValue::Players(uuids)) => {
            let found = DOMAIN.with(|d| {
                let d = d.borrow();
                uuids
                    .iter()
                    .filter_map(|uuid| {
                        d.players
                            .iter()
                            .find(|p| p.player.uuid == *uuid)
                            .map(|p| p.id)
                    })
                    .collect::<Vec<_>>()
            });
            ids(out, found.into_iter())
        }
        Err(s) => s,
        _ => FerrumStatus::INVALID_ARGUMENT,
    })
}
extern "C" fn output(
    ctx: ModContextHandle,
    h: CommandScopeHandle,
    error: FfiBool,
    msg: FfiStr<'_>,
) -> FerrumStatus {
    guard(|| {
        scope(ctx, h, |s| s.output.messages.push((error.get(), text(msg))))
            .map_or_else(|s| s, |_| FerrumStatus::OK)
    })
}
pub(super) static COMMAND: CommandApiV1 = CommandApiV1 {
    struct_size: size_of::<CommandApiV1>() as u32,
    register,
    unregister,
    origin_player: origin,
    get_int: int,
    get_float: float,
    get_bool: bool_value,
    get_string: string,
    get_players: players,
    output,
};

pub(super) fn revoke(ctx: ModContextHandle) {
    let removed = DOMAIN.with(|d| {
        let mut d = d.borrow_mut();
        let keys = d
            .commands
            .iter()
            .filter(|(_, c)| c.owner == ctx)
            .map(|(k, _)| *k)
            .collect::<Vec<_>>();
        keys.into_iter()
            .filter_map(|k| d.commands.remove(&k))
            .collect::<Vec<_>>()
    });
    drop(removed);
}
pub(super) fn defer_disable(ctx: ModContextHandle) -> bool {
    DOMAIN.with(|d| {
        let mut d = d.borrow_mut();
        if d.active.get(&ctx.0.to_raw()).copied().unwrap_or(0) > 0 {
            if !d.pending.contains(&ctx) {
                d.pending.push(ctx);
            }
            true
        } else {
            false
        }
    })
}
pub(super) fn active(ctx: ModContextHandle) -> bool {
    DOMAIN.with(|d| d.borrow().active.get(&ctx.0.to_raw()).copied().unwrap_or(0) > 0)
}
pub(super) fn reset() {
    let old = DOMAIN.with(|d| std::mem::take(&mut *d.borrow_mut()));
    drop(old);
    DOMAIN.with(|d| {
        let mut d = d.borrow_mut();
        d.block_types
            .insert("minecraft:air".into(), BlockRuntimeId(0));
        d.block_types
            .insert("minecraft:stone".into(), BlockRuntimeId(1));
    });
}

impl MockHost {
    /// mock 中逻辑 ID 为 ActorUniqueId(runtime_id.0 as i64)，只在此实例内有效。
    pub fn add_actor(&self, actor: MockActor) -> Result<ActorRuntimeId, Error> {
        if !matches!(actor.kind, ActorKind::ACTOR | ActorKind::MOB)
            || !valid_position(actor.position)
            || !dimension_exists(actor.dimension)
            || !actor.health.is_finite()
            || !actor.max_health.is_finite()
            || actor.health < 0.
            || actor.max_health < actor.health
        {
            return Err(Error::new(
                FerrumStatus::INVALID_ARGUMENT,
                "无效的 mock 实体",
            ));
        }
        DOMAIN.with(|d| {
            let mut d = d.borrow_mut();
            d.next_player += 1;
            let id = ActorRuntimeId(d.next_player);
            d.actors.push(OtherActor { id, actor });
            Ok(id)
        })
    }
    pub fn remove_actor(&self, id: ActorRuntimeId) {
        DOMAIN.with(|d| d.borrow_mut().actors.retain(|a| a.id != id));
    }
    pub fn actor_state(&self, id: ActorRuntimeId) -> Option<MockActor> {
        actor_snapshot(id).ok()
    }
    pub fn register_block_type(&self, name: &str, id: BlockRuntimeId) {
        DOMAIN.with(|d| {
            d.borrow_mut().block_types.insert(name.to_owned(), id);
        });
    }
    /// 该维度区块内未设置的位置视为空气；默认没有已加载区块。
    pub fn set_chunk_loaded(&self, dim: DimensionId, x: i32, z: i32, loaded: bool) {
        DOMAIN.with(|d| {
            let mut d = d.borrow_mut();
            let key = (dim.0, x, z);
            d.loaded_chunks.retain(|v| *v != key);
            if loaded {
                d.loaded_chunks.push(key);
            }
        });
    }
    pub fn set_block(&self, dim: DimensionId, pos: BlockPos, name: &str) -> Result<(), Error> {
        check(block_set_type(dim, pos, name.into()))
    }
    pub fn add_player(&self, player: MockPlayer) -> ActorRuntimeId {
        DOMAIN.with(|d| {
            let mut d = d.borrow_mut();
            d.players.retain(|p| p.player.uuid != player.uuid);
            d.next_player += 1;
            let id = ActorRuntimeId(d.next_player);
            d.players.push(OnlinePlayer {
                id,
                player,
                messages: Vec::new(),
                state: MockPlayerState::default(),
            });
            id
        })
    }
    pub fn remove_player(&self, uuid: PlayerUuid) {
        DOMAIN.with(|d| d.borrow_mut().players.retain(|p| p.player.uuid != uuid));
    }
    /// 让 mock 传送支持额外已存在的维度。
    pub fn add_dimension(&self, dimension: DimensionId) {
        DOMAIN.with(|d| {
            let mut d = d.borrow_mut();
            if !d.custom_dimensions.contains(&dimension) {
                d.custom_dimensions.push(dimension);
            }
        });
    }
    pub fn player_state(&self, uuid: PlayerUuid) -> Option<MockPlayerState> {
        DOMAIN.with(|d| {
            d.borrow()
                .players
                .iter()
                .find(|p| p.player.uuid == uuid)
                .map(|p| p.state)
        })
    }
    pub fn set_player_state(&self, uuid: PlayerUuid, state: MockPlayerState) -> Result<(), Error> {
        if !valid_position(state.position)
            || !state.health.is_finite()
            || !state.max_health.is_finite()
            || state.health < 0.
            || state.max_health < state.health
            || !valid_mode(state.game_mode)
        {
            return Err(Error::new(
                FerrumStatus::INVALID_ARGUMENT,
                "无效的 mock 玩家状态",
            ));
        }
        if !dimension_exists(state.dimension) {
            return Err(Error::new(FerrumStatus::NOT_FOUND, "mock 维度未注册"));
        }
        DOMAIN.with(|d| {
            let mut d = d.borrow_mut();
            let p = d
                .players
                .iter_mut()
                .find(|p| p.player.uuid == uuid)
                .ok_or_else(|| Error::new(FerrumStatus::NOT_FOUND, "mock 玩家不存在"))?;
            p.state = state;
            Ok(())
        })
    }
    pub fn player_messages(&self, uuid: PlayerUuid) -> Vec<String> {
        DOMAIN.with(|d| {
            d.borrow()
                .players
                .iter()
                .find(|p| p.player.uuid == uuid)
                .map(|p| p.messages.clone())
                .unwrap_or_default()
        })
    }
    /// 按名称与参数类型选择重载；控制台拥有最高权限，玩家使用其 operator 状态。
    pub fn execute_command(
        &self,
        name: &str,
        origin: Option<PlayerUuid>,
        args: &[(&str, MockCommandValue)],
    ) -> Result<MockCommandOutput, Error> {
        let args: BTreeMap<String, MockCommandValue> = args
            .iter()
            .map(|(n, v)| ((*n).to_owned(), v.clone()))
            .collect();
        let (command, origin) = DOMAIN
            .with(|d| {
                let d = d.borrow();
                let origin = origin
                    .map(|uuid| {
                        d.players
                            .iter()
                            .find(|p| p.player.uuid == uuid)
                            .map(|p| p.id)
                            .ok_or(FerrumStatus::NOT_FOUND)
                    })
                    .transpose()?;
                let command = d
                    .commands
                    .values()
                    .find(|c| {
                        c.name == name
                            && c.params
                                .iter()
                                .all(|(n, k, o)| args.get(n).map_or(*o, |v| v.kind() == *k))
                            && args.keys().all(|n| c.params.iter().any(|(p, _, _)| p == n))
                    })
                    .cloned()
                    .ok_or(FerrumStatus::NOT_FOUND)?;
                if let Some(id) = origin {
                    let op = d
                        .players
                        .iter()
                        .find(|p| p.id == id)
                        .is_some_and(|p| p.player.operator);
                    if command.permission.0 > if op { 1 } else { 0 } {
                        return Err(FerrumStatus::PERMISSION_DENIED);
                    }
                }
                Ok::<_, FerrumStatus>((command, origin))
            })
            .map_err(|s| Error::new(s, "mock 命令不可用或参数/权限不符"))?;
        enabled(command.owner).map_err(|s| Error::new(s, "模组未启用"))?;
        let h = DOMAIN.with(|d| {
            let mut d = d.borrow_mut();
            let h = CommandScopeHandle(serial(&mut d));
            d.scopes.insert(
                h.0.to_raw(),
                Scope {
                    owner: command.owner,
                    origin,
                    args,
                    params: command.params.clone(),
                    output: MockCommandOutput::default(),
                },
            );
            *d.active.entry(command.owner.0.to_raw()).or_default() += 1;
            h
        });
        let status =
            (command.callback.invoke_fn)(command.callback.env, h, RawMainThreadToken::ASSERTED);
        let (out, pending) = DOMAIN.with(|d| {
            let mut d = d.borrow_mut();
            let out = d
                .scopes
                .remove(&h.0.to_raw())
                .expect("active mock command scope")
                .output;
            let active = d.active.entry(command.owner.0.to_raw()).or_default();
            *active -= 1;
            let pending = *active == 0 && d.pending.contains(&command.owner);
            if pending {
                d.pending.retain(|c| *c != command.owner);
            }
            (out, pending)
        });
        let owner = command.owner;
        if status == FerrumStatus::MOD_PANICKED || status == FerrumStatus::CALLBACK_POISONED {
            let removed = DOMAIN.with(|d| {
                let mut d = d.borrow_mut();
                let key = d
                    .commands
                    .iter()
                    .find(|(_, c)| Rc::ptr_eq(c, &command))
                    .map(|(k, _)| *k);
                key.and_then(|k| d.commands.remove(&k))
            });
            drop(removed);
        }
        drop(command);
        if pending {
            self.finish_disable(&super::MockMod { ctx: owner })?;
        }
        check(status)?;
        Ok(out)
    }
}
