use crate::{ActorOps, ActorRef, PlayerRef, Result, host, sealed};

/// 生物能力层，通过组合复用 ActorRef；不持有 C++ Mob 指针。
#[derive(Copy, Clone, Debug)]
pub struct MobRef<'tick> {
    actor: ActorRef<'tick>,
}
impl<'tick> MobRef<'tick> {
    pub(crate) fn from_actor(actor: ActorRef<'tick>) -> Self {
        Self { actor }
    }
    pub fn try_as_player(&self) -> Result<Option<PlayerRef<'tick>>> {
        self.actor.try_as_player()
    }
}

impl sealed::Sealed for MobRef<'_> {}

impl<'tick> ActorOps<'tick> for MobRef<'tick> {
    fn as_actor(&self) -> ActorRef<'tick> {
        self.actor
    }
}

impl<'tick> MobOps<'tick> for MobRef<'tick> {
    fn as_mob(&self) -> MobRef<'tick> {
        *self
    }
}

/// 生物引用共有的能力，包含 ActorOps 提供的公共实体操作。
///
/// 由 `ferrum_sdk::prelude::*` 导入，支持 `impl MobOps<'tick>` 泛型约束。
pub trait MobOps<'tick>: ActorOps<'tick> {
    fn as_mob(&self) -> MobRef<'tick>;

    /// 当前生命值，2 点为 1 颗心，不截断小数。
    fn health(&self) -> Result<f32> {
        let mut out = 0.;
        host::check((host::mob_api()?.health)(self.runtime_id(), &mut out))?;
        Ok(out)
    }
    fn max_health(&self) -> Result<f32> {
        let mut out = 0.;
        host::check((host::mob_api()?.max_health)(self.runtime_id(), &mut out))?;
        Ok(out)
    }
    /// 直接设置有限的 0..=max_health 属性值，不模拟受伤/治疗。
    fn set_health(&self, value: f32) -> Result<()> {
        host::check((host::mob_api()?.set_health)(self.runtime_id(), value))
    }
}
