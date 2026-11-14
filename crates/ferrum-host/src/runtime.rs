//! 全局模组注册表与模组记录。
//!
//! 锁只用于短暂地查表并克隆 `Arc<ModEntry>`，调用任何模组函数指针前必须已经释放。

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use ferrum_abi::{
    FerrumModDescriptor, FerrumStatus, GenerationalHandle, ModContextHandle, ModDirKind,
};

use crate::arena::Arena;
use crate::breaker::CircuitBreaker;
use crate::shim::ShimModCookie;

/// 模组状态机，见 `docs/01-specs/threading-and-safety.md` §4.2。
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ModState {
    Loading = 0,
    Loaded = 1,
    Enabling = 2,
    Enabled = 3,
    Disabling = 4,
    Disabled = 5,
    Unloading = 6,
}

impl ModState {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Loading,
            1 => Self::Loaded,
            2 => Self::Enabling,
            3 => Self::Enabled,
            4 => Self::Disabling,
            5 => Self::Disabled,
            _ => Self::Unloading,
        }
    }
}

/// 加载时由 shim 提供、此后不再变化的模组元数据。
#[derive(Clone, Debug)]
pub struct ModInfo {
    pub cookie: ShimModCookie,
    pub name: String,
    pub version: String,
    pub entry_path: String,
    pub mod_dir: String,
    pub data_dir: String,
    pub config_dir: String,
    pub lang_dir: String,
}

impl ModInfo {
    pub fn dir(&self, kind: ModDirKind) -> Option<&str> {
        Some(match kind {
            ModDirKind::MOD => &self.mod_dir,
            ModDirKind::DATA => &self.data_dir,
            ModDirKind::CONFIG => &self.config_dir,
            ModDirKind::LANG => &self.lang_dir,
            _ => return None,
        })
    }
}

pub struct ModEntry {
    pub info: ModInfo,
    state: AtomicU8,
    descriptor: OnceLock<FerrumModDescriptor>,
    pub breaker: Mutex<CircuitBreaker>,
    pub ownership: Mutex<crate::command::ModOwnership>,
}

impl ModEntry {
    pub fn new(info: ModInfo) -> Self {
        Self {
            info,
            state: AtomicU8::new(ModState::Loading as u8),
            descriptor: OnceLock::new(),
            breaker: Mutex::new(CircuitBreaker::default()),
            ownership: Mutex::new(crate::command::ModOwnership::default()),
        }
    }

    pub fn state(&self) -> ModState {
        ModState::from_u8(self.state.load(Ordering::Acquire))
    }

    pub fn set_state(&self, state: ModState) {
        self.state.store(state as u8, Ordering::Release);
    }

    pub fn descriptor(&self) -> Option<&FerrumModDescriptor> {
        self.descriptor.get()
    }

    pub fn set_descriptor(&self, descriptor: FerrumModDescriptor) {
        let _ = self.descriptor.set(descriptor);
    }
}

impl Drop for ModEntry {
    fn drop(&mut self) {
        // 最后一个 Arc 释放后才通知 shim，保证没有任何线程仍在用 cookie 写日志。
        if let Some(shim) = crate::shim::get() {
            (shim.release_cookie)(self.info.cookie);
        }
    }
}

static REGISTRY: Mutex<Arena<Arc<ModEntry>>> = Mutex::new(Arena::new());

fn registry() -> MutexGuard<'static, Arena<Arc<ModEntry>>> {
    // 宿主代码在持锁期间不会 panic，也不调用模组代码；即使中毒也继续使用其数据。
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn insert(entry: ModEntry) -> (ModContextHandle, Arc<ModEntry>) {
    let entry = Arc::new(entry);
    let handle = registry().insert(entry.clone());
    (ModContextHandle(handle), entry)
}

/// 查表并克隆记录；返回时锁已释放。
pub fn lookup(ctx: ModContextHandle) -> Result<Arc<ModEntry>, FerrumStatus> {
    registry()
        .get(ctx.0)
        .cloned()
        .ok_or(FerrumStatus::STALE_HANDLE)
}

pub fn lookup_raw(raw: u64) -> Result<(ModContextHandle, Arc<ModEntry>), FerrumStatus> {
    let handle = GenerationalHandle::from_raw(raw).ok_or(FerrumStatus::STALE_HANDLE)?;
    let ctx = ModContextHandle(handle);
    Ok((ctx, lookup(ctx)?))
}

/// 移除记录并递增代数，残留的 `ModContextHandle` 随即失效。
pub fn remove(ctx: ModContextHandle) -> Option<Arc<ModEntry>> {
    registry().remove(ctx.0)
}

pub fn find_by_name(name: &str) -> Option<(ModContextHandle, Arc<ModEntry>)> {
    registry()
        .iter()
        .find(|(_, e)| e.info.name == name)
        .map(|(h, e)| (ModContextHandle(h), e.clone()))
}
