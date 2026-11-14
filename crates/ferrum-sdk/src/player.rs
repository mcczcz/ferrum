use crate::{ActorOps, ActorRef, Error, MainThreadToken, MobOps, MobRef, Result, host, sealed};
pub use ferrum_abi::{ActorRuntimeId, DimensionId, GameMode, PlayerUuid, Vec3};
use ferrum_abi::{FerrumStatus, FfiBool, FfiStr};

/// 玩家能力层：组合 MobRef，通过 ActorOps / MobOps 复用父层操作。
#[derive(Copy, Clone, Debug)]
pub struct PlayerRef<'tick> {
    mob: MobRef<'tick>,
}
impl<'tick> PlayerRef<'tick> {
    pub(crate) fn new(id: ActorRuntimeId, token: &'tick MainThreadToken<'tick>) -> Self {
        Self::from_mob(MobRef::from_actor(ActorRef::new(id, token)))
    }
    pub(crate) fn from_mob(mob: MobRef<'tick>) -> Self {
        Self { mob }
    }
    pub fn real_name(&self) -> Result<String> {
        let a = host::player_api()?;
        host::read_string(|w| (a.real_name)(self.runtime_id(), w))
    }
    pub fn uuid(&self) -> Result<PlayerUuid> {
        let mut out = PlayerUuid::default();
        host::check((host::player_api()?.uuid)(self.runtime_id(), &mut out))?;
        Ok(out)
    }
    pub fn send_message(&self, text: impl AsRef<str>) -> Result<()> {
        host::check((host::player_api()?.send_message)(
            self.runtime_id(),
            FfiStr::new(text.as_ref()),
        ))
    }
    pub fn is_operator(&self) -> Result<bool> {
        let mut out = FfiBool::FALSE;
        host::check((host::player_api()?.is_operator)(
            self.runtime_id(),
            &mut out,
        ))?;
        Ok(out.get())
    }
    pub fn game_mode(&self) -> Result<GameMode> {
        let mut out = GameMode::UNKNOWN;
        host::check((host::player_api()?.game_mode)(self.runtime_id(), &mut out))?;
        Ok(out)
    }
    pub fn set_game_mode(&self, mode: GameMode) -> Result<()> {
        host::check((host::player_api()?.set_game_mode)(self.runtime_id(), mode))
    }
}

impl sealed::Sealed for PlayerRef<'_> {}

impl<'tick> ActorOps<'tick> for PlayerRef<'tick> {
    fn as_actor(&self) -> ActorRef<'tick> {
        self.mob.as_actor()
    }
}

impl<'tick> MobOps<'tick> for PlayerRef<'tick> {
    fn as_mob(&self) -> MobRef<'tick> {
        self.mob
    }
}

pub(crate) fn decode_players<'tick>(
    bytes: Vec<u8>,
    token: &'tick MainThreadToken<'tick>,
) -> Result<Vec<PlayerRef<'tick>>> {
    if bytes.len() % 8 != 0 {
        return Err(Error::new(
            FerrumStatus::INTERNAL_ERROR,
            "玩家 ID 列表长度错误",
        ));
    }
    Ok(bytes
        .chunks_exact(8)
        .map(|b| {
            let mut id = [0; 8];
            id.copy_from_slice(b);
            PlayerRef::new(ActorRuntimeId(u64::from_le_bytes(id)), token)
        })
        .collect())
}
