//! 所有游戏对象均通过主线程令牌构造瞬态引用。
use crate::{
    ActorRef, ActorRuntimeId, ActorUniqueId, MainThreadToken, PlayerRef, PlayerUuid, Result, host,
    player::decode_players,
};
use ferrum_abi::{DimensionId, FerrumStatus, FfiStr};

pub fn get_actor_by_runtime_id<'tick>(
    token: &'tick MainThreadToken<'tick>,
    id: ActorRuntimeId,
) -> Result<Option<ActorRef<'tick>>> {
    let mut kind = ferrum_abi::ActorKind::ACTOR;
    let status = (host::actor_api()?.kind)(id, &mut kind);
    if status == FerrumStatus::NOT_FOUND {
        return Ok(None);
    }
    host::check(status)?;
    Ok(Some(ActorRef::new(id, token)))
}
/// 跨 Tick 保存 ActorUniqueId 后，在主线程重新解析。
pub fn get_actor<'tick>(
    token: &'tick MainThreadToken<'tick>,
    id: ActorUniqueId,
) -> Result<Option<ActorRef<'tick>>> {
    let mut runtime = ActorRuntimeId::default();
    let status = (host::actor_api()?.find_by_unique_id)(id, &mut runtime);
    if status == FerrumStatus::NOT_FOUND {
        return Ok(None);
    }
    host::check(status)?;
    Ok(Some(ActorRef::new(runtime, token)))
}
pub fn get_player<'tick>(
    token: &'tick MainThreadToken<'tick>,
    uuid: PlayerUuid,
) -> Result<Option<PlayerRef<'tick>>> {
    let mut id = ActorRuntimeId::default();
    let status = (host::player_api()?.find_by_uuid)(uuid, &mut id);
    if status == FerrumStatus::NOT_FOUND {
        return Ok(None);
    }
    host::check(status)?;
    Ok(Some(PlayerRef::new(id, token)))
}
pub fn get_player_by_name<'tick>(
    token: &'tick MainThreadToken<'tick>,
    name: &str,
) -> Result<Option<PlayerRef<'tick>>> {
    let mut id = ActorRuntimeId::default();
    let status = (host::player_api()?.find_by_name)(FfiStr::new(name), &mut id);
    if status == FerrumStatus::NOT_FOUND {
        return Ok(None);
    }
    host::check(status)?;
    Ok(Some(PlayerRef::new(id, token)))
}
pub fn online_players<'tick>(
    token: &'tick MainThreadToken<'tick>,
) -> Result<Vec<PlayerRef<'tick>>> {
    let api = host::player_api()?;
    decode_players(
        host::read_bytes(|w| (api.online_players)(w)).map_err(host::error_from_status)?,
        token,
    )
}
/// 只读写已加载区块中的主方块层，不触发区块加载。
pub fn get_block<'tick>(
    token: &'tick MainThreadToken<'tick>,
    dimension: DimensionId,
    position: crate::BlockPos,
) -> Result<crate::BlockRef<'tick>> {
    let block = crate::BlockRef::new(token, dimension, position);
    block.runtime_id()?;
    Ok(block)
}
