use crate::{MainThreadToken, MobRef, PlayerRef, Result, host, sealed};
pub use ferrum_abi::{ActorKind, ActorRuntimeId, ActorUniqueId, DimensionId, Vec3};
use std::marker::PhantomData;

/// 任意实体的主线程瞬态引用；仅保存 ID，每次调用重新解析。
#[derive(Copy, Clone, Debug)]
pub struct ActorRef<'tick> {
    id: ActorRuntimeId,
    _scope: PhantomData<(&'tick (), *mut ())>,
}
impl<'tick> ActorRef<'tick> {
    pub(crate) fn new(id: ActorRuntimeId, _token: &'tick MainThreadToken<'tick>) -> Self {
        Self {
            id,
            _scope: PhantomData,
        }
    }
    /// 向下转换须实时校验能力；普通实体不能伪装成生物。
    pub fn try_as_mob(&self) -> Result<Option<MobRef<'tick>>> {
        Ok(matches!(self.kind()?, ActorKind::MOB | ActorKind::PLAYER)
            .then(|| MobRef::from_actor(*self)))
    }
    pub fn try_as_player(&self) -> Result<Option<PlayerRef<'tick>>> {
        Ok((self.kind()? == ActorKind::PLAYER)
            .then(|| PlayerRef::from_mob(MobRef::from_actor(*self))))
    }
}

impl sealed::Sealed for ActorRef<'_> {}

impl<'tick> ActorOps<'tick> for ActorRef<'tick> {
    fn as_actor(&self) -> ActorRef<'tick> {
        *self
    }
}

/// 所有实体引用共有的能力；仅 SDK 提供的引用类型可实现。
///
/// 由 `ferrum_sdk::prelude::*` 导入，支持 `impl ActorOps<'tick>` 泛型约束。
///
/// ```compile_fail
/// use ferrum_sdk::{ActorOps, ActorRef};
///
/// #[derive(Copy, Clone)]
/// struct ForgedActor;
///
/// impl<'tick> ActorOps<'tick> for ForgedActor {
///     fn as_actor(&self) -> ActorRef<'tick> {
///         unimplemented!()
///     }
/// }
/// ```
pub trait ActorOps<'tick>: sealed::Sealed + Copy {
    fn as_actor(&self) -> ActorRef<'tick>;

    /// 仅当前会话有效；跨 Tick 保存实体身份请用 unique_id()，玩家身份请用 uuid()。
    fn runtime_id(&self) -> ActorRuntimeId {
        self.as_actor().id
    }
    fn unique_id(&self) -> Result<ActorUniqueId> {
        let mut out = ActorUniqueId::default();
        host::check((host::actor_api()?.unique_id)(self.as_actor().id, &mut out))?;
        Ok(out)
    }
    fn type_name(&self) -> Result<String> {
        let a = host::actor_api()?;
        host::read_string(|w| (a.type_name)(self.as_actor().id, w))
    }
    fn kind(&self) -> Result<ActorKind> {
        let mut out = ActorKind::ACTOR;
        host::check((host::actor_api()?.kind)(self.as_actor().id, &mut out))?;
        Ok(out)
    }
    fn position(&self) -> Result<Vec3> {
        let mut out = Vec3::default();
        host::check((host::actor_api()?.position)(self.as_actor().id, &mut out))?;
        Ok(out)
    }
    fn dimension(&self) -> Result<DimensionId> {
        let mut out = DimensionId::default();
        host::check((host::actor_api()?.dimension)(self.as_actor().id, &mut out))?;
        Ok(out)
    }
    fn teleport(&self, position: Vec3, dimension: DimensionId) -> Result<()> {
        host::check((host::actor_api()?.teleport)(
            self.as_actor().id,
            position,
            dimension,
        ))
    }
}
