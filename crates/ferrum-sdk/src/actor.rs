use crate::{MainThreadToken, MobRef, PlayerRef, Result, host, sealed};
pub use ferrum_abi::{Aabb, ActorKind, ActorRuntimeId, ActorUniqueId, DimensionId, Vec2, Vec3};
use ferrum_abi::{FfiBool, FfiStr};
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
        host::check((host::actor_api()?.unique_id()?)(
            self.as_actor().id,
            &mut out,
        ))?;
        Ok(out)
    }
    fn type_name(&self) -> Result<String> {
        let type_name = host::actor_api()?.type_name()?;
        host::read_string(|w| type_name(self.as_actor().id, w))
    }
    /// 读取此刻的实体存档 NBT；快照可在当前 Tick 结束后继续使用。
    fn nbt(&self) -> Result<crate::nbt::NbtCompound> {
        let read = host::actor_api()?.nbt()?;
        let bytes = host::read_bytes(|out| read(self.runtime_id(), out))
            .map_err(host::error_from_status)?;
        crate::nbt::decode(&bytes)
    }
    fn kind(&self) -> Result<ActorKind> {
        let mut out = ActorKind::ACTOR;
        host::check((host::actor_api()?.kind()?)(self.as_actor().id, &mut out))?;
        Ok(out)
    }
    fn position(&self) -> Result<Vec3> {
        let mut out = Vec3::default();
        host::check((host::actor_api()?.position()?)(
            self.as_actor().id,
            &mut out,
        ))?;
        Ok(out)
    }
    fn dimension(&self) -> Result<DimensionId> {
        let mut out = DimensionId::default();
        host::check((host::actor_api()?.dimension()?)(
            self.as_actor().id,
            &mut out,
        ))?;
        Ok(out)
    }
    fn teleport(&self, position: Vec3, dimension: DimensionId) -> Result<()> {
        host::check((host::actor_api()?.teleport()?)(
            self.as_actor().id,
            position,
            dimension,
        ))
    }
    fn name_tag(&self) -> Result<String> {
        let read = host::actor_api()?.name_tag()?;
        host::read_string(|out| read(self.runtime_id(), out))
    }
    fn set_name_tag(&self, name: &str) -> Result<()> {
        host::check((host::actor_api()?.set_name_tag()?)(
            self.runtime_id(),
            FfiStr::new(name),
        ))
    }
    fn is_on_ground(&self) -> Result<bool> {
        let mut out = FfiBool::FALSE;
        host::check((host::actor_api()?.is_on_ground()?)(
            self.runtime_id(),
            &mut out,
        ))?;
        Ok(out.get())
    }
    fn is_in_water(&self) -> Result<bool> {
        let mut out = FfiBool::FALSE;
        host::check((host::actor_api()?.is_in_water()?)(
            self.runtime_id(),
            &mut out,
        ))?;
        Ok(out.get())
    }
    fn feet_position(&self) -> Result<Vec3> {
        let mut out = Vec3::default();
        host::check((host::actor_api()?.feet_position()?)(
            self.runtime_id(),
            &mut out,
        ))?;
        Ok(out)
    }
    fn head_position(&self) -> Result<Vec3> {
        let mut out = Vec3::default();
        host::check((host::actor_api()?.head_position()?)(
            self.runtime_id(),
            &mut out,
        ))?;
        Ok(out)
    }
    fn velocity(&self) -> Result<Vec3> {
        let mut out = Vec3::default();
        host::check((host::actor_api()?.velocity()?)(
            self.runtime_id(),
            &mut out,
        ))?;
        Ok(out)
    }
    fn position_delta(&self) -> Result<Vec3> {
        let mut out = Vec3::default();
        host::check((host::actor_api()?.position_delta()?)(
            self.runtime_id(),
            &mut out,
        ))?;
        Ok(out)
    }
    fn rotation(&self) -> Result<Vec2> {
        let mut out = Vec2::default();
        host::check((host::actor_api()?.rotation()?)(
            self.runtime_id(),
            &mut out,
        ))?;
        Ok(out)
    }
    fn eye_position(&self) -> Result<Vec3> {
        let mut out = Vec3::default();
        host::check((host::actor_api()?.eye_position()?)(
            self.runtime_id(),
            &mut out,
        ))?;
        Ok(out)
    }
    fn fall_distance(&self) -> Result<f32> {
        let mut out = 0.0;
        host::check((host::actor_api()?.fall_distance()?)(
            self.runtime_id(),
            &mut out,
        ))?;
        Ok(out)
    }
    fn aabb(&self) -> Result<Aabb> {
        let mut out = Aabb::default();
        host::check((host::actor_api()?.aabb()?)(self.runtime_id(), &mut out))?;
        Ok(out)
    }
    fn block_pos_standing_on(&self) -> Result<crate::BlockPos> {
        let mut out = crate::BlockPos::default();
        host::check((host::actor_api()?.block_pos_standing_on()?)(
            self.runtime_id(),
            &mut out,
        ))?;
        Ok(out)
    }
    fn is_swimming(&self) -> Result<bool> {
        actor_flag(self.runtime_id(), |api| api.is_swimming())
    }
    fn is_moving(&self) -> Result<bool> {
        actor_flag(self.runtime_id(), |api| api.is_moving())
    }
    fn is_sneaking(&self) -> Result<bool> {
        actor_flag(self.runtime_id(), |api| api.is_sneaking())
    }
    fn is_crawling(&self) -> Result<bool> {
        actor_flag(self.runtime_id(), |api| api.is_crawling())
    }
    fn is_gliding(&self) -> Result<bool> {
        actor_flag(self.runtime_id(), |api| api.is_gliding())
    }
    fn is_invisible(&self) -> Result<bool> {
        actor_flag(self.runtime_id(), |api| api.is_invisible())
    }
    fn is_alive(&self) -> Result<bool> {
        actor_flag(self.runtime_id(), |api| api.is_alive())
    }
    fn is_dead(&self) -> Result<bool> {
        actor_flag(self.runtime_id(), |api| api.is_dead())
    }
    fn is_on_fire(&self) -> Result<bool> {
        actor_flag(self.runtime_id(), |api| api.is_on_fire())
    }
    fn is_sleeping(&self) -> Result<bool> {
        actor_flag(self.runtime_id(), |api| api.is_sleeping())
    }
    fn is_blocking(&self) -> Result<bool> {
        actor_flag(self.runtime_id(), |api| api.is_blocking())
    }
    fn set_on_fire(&self, seconds: i32, show_effect: bool) -> Result<()> {
        host::check((host::actor_api()?.set_on_fire()?)(
            self.runtime_id(),
            seconds,
            show_effect.into(),
        ))
    }
    fn stop_fire(&self) -> Result<()> {
        host::check((host::actor_api()?.stop_fire()?)(self.runtime_id()))
    }
    fn set_sneaking(&self, sneaking: bool) -> Result<()> {
        host::check((host::actor_api()?.set_sneaking()?)(
            self.runtime_id(),
            sneaking.into(),
        ))
    }
    fn teleport_with_rotation(
        &self,
        position: Vec3,
        rotation: Vec2,
        dimension: DimensionId,
    ) -> Result<()> {
        host::check((host::actor_api()?.teleport_with_rotation()?)(
            self.runtime_id(),
            position,
            rotation,
            dimension,
        ))
    }
}

fn actor_flag(
    id: ActorRuntimeId,
    field: impl FnOnce(
        &host::ActorApi,
    ) -> Result<
        extern "C" fn(ActorRuntimeId, *mut FfiBool) -> ferrum_abi::FerrumStatus,
    >,
) -> Result<bool> {
    let mut out = FfiBool::FALSE;
    host::check(field(host::actor_api()?)?(id, &mut out))?;
    Ok(out.get())
}
