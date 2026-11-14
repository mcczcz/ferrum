//! Ferrum 模组开发 SDK。
//!
//! ```ignore
//! use ferrum_sdk::prelude::*;
//!
//! pub struct MyMod {
//!     logger: Logger,
//! }
//!
//! #[ferrum_sdk::mod_entry]
//! impl FerrumMod for MyMod {
//!     fn on_load(ctx: &ModContext) -> Result<Self> {
//!         let logger = ctx.logger();
//!         logger.info("加载中");
//!         Ok(Self { logger })
//!     }
//! }
//! ```
//!
//! 模组必须编译为 `cdylib` 且保持 `panic = "unwind"`。

#[cfg(panic = "abort")]
compile_error!(
    "Ferrum mods must be built with panic = \"unwind\"; see docs/01-specs/error-handling-spec.md"
);

pub mod actor;
pub mod block;
pub mod command;
mod context;
mod error;
mod firewall;
mod host;
mod lifecycle;
mod logger;
pub mod mob;
pub mod player;
mod token;
pub mod world;

pub(crate) mod sealed {
    pub trait Sealed {}
}

pub use actor::{ActorKind, ActorOps, ActorRef, ActorUniqueId};
pub use block::{BlockPos, BlockRef, BlockRuntimeId};
pub use command::{
    CommandBuilder, CommandHandle, CommandInvocation, CommandParamKind, CommandPermission, Commands,
};
pub use context::{ModContext, ModDir};
pub use error::{Error, ErrorKind, Result};
pub use ferrum_macros::mod_entry;
pub use lifecycle::{FerrumMod, ModEntryPoint};
pub use logger::{Level, Logger};
pub use mob::{MobOps, MobRef};
pub use player::{ActorRuntimeId, DimensionId, GameMode, PlayerRef, PlayerUuid, Vec3};
pub use token::{MainThreadToken, is_main_thread};

/// 本 SDK 的版本（写入模组描述符，仅用于诊断）。
pub const SDK_VERSION: ferrum_abi::AbiVersion = ferrum_abi::AbiVersion::new(
    parse_u16(env!("CARGO_PKG_VERSION_MAJOR")),
    parse_u16(env!("CARGO_PKG_VERSION_MINOR")),
    parse_u16(env!("CARGO_PKG_VERSION_PATCH")) as u32,
);

const fn parse_u16(s: &str) -> u16 {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut value: u16 = 0;
    while i < bytes.len() {
        value = value * 10 + (bytes[i] - b'0') as u16;
        i += 1;
    }
    value
}

pub mod prelude {
    pub use crate::{
        ActorKind, ActorOps, ActorRef, ActorRuntimeId, ActorUniqueId, BlockPos, BlockRef,
        BlockRuntimeId, CommandInvocation, CommandParamKind, CommandPermission, DimensionId, Error,
        ErrorKind, FerrumMod, GameMode, Level, Logger, MainThreadToken, MobOps, MobRef, ModContext,
        ModDir, PlayerRef, PlayerUuid, Result, Vec3, world,
    };
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;

    #[test]
    fn actor_ops_accepts_all_entity_refs() {
        fn actor_id<'tick, T: ActorOps<'tick>>(entity: T) -> ActorRuntimeId {
            let _: fn(&T) -> Result<ActorUniqueId> = T::unique_id;
            let _: fn(&T) -> Result<String> = T::type_name;
            let _: fn(&T) -> Result<ActorKind> = T::kind;
            let _: fn(&T) -> Result<Vec3> = T::position;
            let _: fn(&T) -> Result<DimensionId> = T::dimension;
            let _: fn(&T, Vec3, DimensionId) -> Result<()> = T::teleport;
            assert_eq!(entity.runtime_id(), entity.as_actor().runtime_id());
            entity.runtime_id()
        }

        // 仅构造引用并检查纯 ID 操作，不调用需要真实主线程的宿主 API。
        let token = MainThreadToken::assume_main_thread();
        let id = ActorRuntimeId(42);
        let actor = ActorRef::new(id, &token);
        let mob = MobRef::from_actor(actor);
        let player = PlayerRef::new(id, &token);

        assert_eq!(actor_id(actor), id);
        assert_eq!(actor_id(mob), id);
        assert_eq!(actor_id(player), id);
        assert_eq!(player.as_mob().as_actor().runtime_id(), id);
    }

    #[test]
    fn mob_ops_accepts_mob_and_player_refs() {
        fn mob_id<'tick, T: MobOps<'tick>>(entity: T) -> ActorRuntimeId {
            let _: fn(&T) -> Result<f32> = T::health;
            let _: fn(&T) -> Result<f32> = T::max_health;
            let _: fn(&T, f32) -> Result<()> = T::set_health;
            let _: fn(&MobRef<'tick>) -> Result<Option<PlayerRef<'tick>>> = MobRef::try_as_player;
            assert_eq!(entity.runtime_id(), entity.as_mob().runtime_id());
            entity.as_mob().runtime_id()
        }

        let token = MainThreadToken::assume_main_thread();
        let id = ActorRuntimeId(7);
        let mob = MobRef::from_actor(ActorRef::new(id, &token));
        let player = PlayerRef::from_mob(mob);

        assert_eq!(mob_id(mob), id);
        assert_eq!(mob_id(player), id);
    }

    #[test]
    fn entity_refs_are_not_send_or_sync() {
        // 若类型实现目标 trait，两个候选实现会使类型推导产生歧义并编译失败。
        macro_rules! assert_not_impl {
            ($ty:ty, $bound:path) => {{
                trait AmbiguousIfImpl<A> {
                    fn check() {}
                }
                impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
                impl<T: ?Sized + $bound> AmbiguousIfImpl<u8> for T {}
                let _ = <$ty as AmbiguousIfImpl<_>>::check;
            }};
        }

        assert_not_impl!(ActorRef<'static>, Send);
        assert_not_impl!(ActorRef<'static>, Sync);
        assert_not_impl!(MobRef<'static>, Send);
        assert_not_impl!(MobRef<'static>, Sync);
        assert_not_impl!(PlayerRef<'static>, Send);
        assert_not_impl!(PlayerRef<'static>, Sync);
    }
}

/// 宏展开与 `ferrum-host-mock` 使用的内部接口，不属于稳定 API。
#[doc(hidden)]
pub mod __private {
    pub use crate::lifecycle::{
        EntryCallbacks, ModState, entry, on_disable, on_enable, on_load, on_unload,
    };
    pub use ferrum_abi as abi;
}
