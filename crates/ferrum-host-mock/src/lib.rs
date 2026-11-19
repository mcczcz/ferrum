//! 无需 BDS 的真实 ferrum-host + Rust 假 shim。
//! 同一进程同时只有一个 MockHost；世界状态位于创建它的线程。
mod domain;
mod events;
mod shim;
pub use domain::{MockActor, MockCommandOutput, MockCommandValue, MockPlayer, MockPlayerState};
use ferrum_abi::{FerrumStatus, LogLevel, ModContextHandle};
use ferrum_host::{exports, last_error, loader, runtime, runtime::ModInfo, shim::ShimModCookie};
use ferrum_sdk::{Error, Level, ModEntryPoint};
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, MutexGuard, Once, PoisonError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRecord {
    pub level: Level,
    pub message: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanicRecord {
    pub message: String,
    pub location: String,
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MockModState {
    Loaded,
    Enabling,
    Enabled,
    Disabling,
    Disabled,
    Unloaded,
}
struct ModRecord {
    ctx: Option<ModContextHandle>,
    logs: Vec<LogRecord>,
    panics: Vec<PanicRecord>,
}
struct World {
    next_cookie: usize,
    mods: BTreeMap<usize, ModRecord>,
    pending: Vec<usize>,
}
static HOST_LOCK: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();
static WORLD: Mutex<World> = Mutex::new(World {
    next_cookie: 0,
    mods: BTreeMap::new(),
    pending: Vec::new(),
});
fn world() -> MutexGuard<'static, World> {
    WORLD.lock().unwrap_or_else(PoisonError::into_inner)
}
fn level_from_abi(level: LogLevel) -> Option<Level> {
    Some(match level {
        LogLevel::FATAL => Level::Fatal,
        LogLevel::ERROR => Level::Error,
        LogLevel::WARN => Level::Warn,
        LogLevel::INFO => Level::Info,
        LogLevel::DEBUG => Level::Debug,
        LogLevel::TRACE => Level::Trace,
        _ => return None,
    })
}
fn take_error(status: FerrumStatus) -> Error {
    Error::new(
        status,
        last_error::take().unwrap_or_else(|| status.to_string()),
    )
}
fn check(status: FerrumStatus) -> Result<(), Error> {
    if status.is_ok() {
        Ok(())
    } else {
        Err(take_error(status))
    }
}
fn is_main_thread() -> ferrum_abi::FfiBool {
    ferrum_host::vtable::is_main_thread().into()
}
/// 已加载的模组；句柄的代数由真实宿主管理。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MockMod {
    ctx: ModContextHandle,
}
impl MockMod {
    pub fn context_handle(&self) -> ModContextHandle {
        self.ctx
    }
}
pub struct MockHost {
    _exclusive: MutexGuard<'static, ()>,
}
impl Default for MockHost {
    fn default() -> Self {
        Self::new()
    }
}
impl MockHost {
    pub fn new() -> Self {
        let guard = HOST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        INIT.call_once(|| {
            assert!(
                ferrum_host::shim::install(shim::api()),
                "宿主已由其他 shim 初始化"
            );
            ferrum_host::mock_support::install_panic_observer(|cookie, message, location| {
                if let Some(record) = world().mods.get_mut(&(cookie.0 as usize)) {
                    record.panics.push(PanicRecord {
                        message: message.into(),
                        location: location.into(),
                    });
                }
            });
        });
        // SAFETY: HOST_LOCK 排他保护所有 mock 会话，上一会话已在 Drop 中卸载并排空资源。
        unsafe { ferrum_host::mock_support::begin_session() };
        domain::reset();
        events::reset();
        Self { _exclusive: guard }
    }
    pub fn load<T: ModEntryPoint>(&self) -> Result<MockMod, Error> {
        let name = std::any::type_name::<T>()
            .rsplit("::")
            .next()
            .unwrap_or("mod");
        self.load_named::<T>(name, "0.1.0")
    }
    pub fn load_named<T: ModEntryPoint>(
        &self,
        name: &str,
        version: &str,
    ) -> Result<MockMod, Error> {
        let cookie = {
            let mut w = world();
            w.next_cookie += 1;
            let cookie = w.next_cookie;
            w.mods.insert(
                cookie,
                ModRecord {
                    ctx: None,
                    logs: Vec::new(),
                    panics: Vec::new(),
                },
            );
            cookie
        };
        let root = format!("mods/{name}");
        let info = ModInfo {
            cookie: ShimModCookie(cookie as *mut std::ffi::c_void),
            name: name.into(),
            version: version.into(),
            entry_path: String::new(),
            mod_dir: root.clone(),
            data_dir: format!("{root}/data"),
            config_dir: format!("{root}/config"),
            lang_dir: format!("{root}/lang"),
        };
        // SAFETY: ModEntryPoint 由 SDK 宏生成，入口与回调在整个测试进程内有效。
        let ctx = unsafe { loader::load_entry(info, T::ENTRY) }.map_err(take_error)?;
        world()
            .mods
            .get_mut(&cookie)
            .expect("本次加载的 mock 记录")
            .ctx = Some(ctx);
        self.flush_disables();
        Ok(MockMod { ctx })
    }
    /// 已启用时返回成功，不重复调用 on_enable。
    pub fn enable(&self, m: &MockMod) -> Result<(), Error> {
        let result = check(exports::ferrum_host_enable_mod(m.ctx.0.to_raw()));
        self.flush_disables();
        result
    }

    /// 推进一个服务端 Tick，并执行到期任务。
    pub fn tick(&self) -> Result<(), Error> {
        let result = check(exports::ferrum_host_scheduler_tick());
        self.flush_disables();
        result
    }
    /// Loaded / Disabled 状态直接成功，与真实宿主保持一致。
    pub fn disable(&self, m: &MockMod) -> Result<(), Error> {
        let result = check(exports::ferrum_host_disable_mod(m.ctx.0.to_raw()));
        self.flush_disables();
        result
    }
    pub fn unload(&self, m: &MockMod) -> Result<(), Error> {
        let result = check(exports::ferrum_host_unload_mod(m.ctx.0.to_raw()));
        self.flush_disables();
        result
    }
    pub fn state(&self, m: &MockMod) -> Option<MockModState> {
        if !world().mods.values().any(|r| r.ctx == Some(m.ctx)) {
            return None;
        }
        Some(match runtime::lookup(m.ctx).map(|e| e.state()) {
            Ok(runtime::ModState::Loaded | runtime::ModState::Loading) => MockModState::Loaded,
            Ok(runtime::ModState::Enabling) => MockModState::Enabling,
            Ok(runtime::ModState::Enabled) => MockModState::Enabled,
            Ok(runtime::ModState::Disabling) => MockModState::Disabling,
            Ok(runtime::ModState::Disabled) => MockModState::Disabled,
            Ok(runtime::ModState::Unloading) | Err(_) => MockModState::Unloaded,
        })
    }
    pub fn logs(&self, m: &MockMod) -> Vec<LogRecord> {
        world()
            .mods
            .values()
            .find(|r| r.ctx == Some(m.ctx))
            .map(|r| r.logs.clone())
            .unwrap_or_default()
    }
    pub fn panics(&self, m: &MockMod) -> Vec<PanicRecord> {
        world()
            .mods
            .values()
            .find(|r| r.ctx == Some(m.ctx))
            .map(|r| r.panics.clone())
            .unwrap_or_default()
    }
    pub(crate) fn flush_disables(&self) {
        // 调度清理可能写 last-error，不能覆盖调用方尚未取走的失败详情。
        let previous = last_error::take();
        let requests = {
            let mut w = world();
            std::mem::take(&mut w.pending)
                .into_iter()
                .filter_map(|cookie| w.mods.get(&cookie).and_then(|r| r.ctx))
                .collect::<Vec<_>>()
        };
        for ctx in requests {
            let _ = exports::ferrum_host_disable_mod(ctx.0.to_raw());
        }
        last_error::take();
        if let Some(previous) = previous {
            last_error::set(previous);
        }
    }
    pub fn native_event_api(&self) -> &'static ferrum_abi::NativeEventApiV1 {
        let ptr = ferrum_host::custom_event::ferrum_host_native_event_api();
        // SAFETY: 宿主返回进程生命周期内有效的不可变原生事件表。
        unsafe { &*ptr }
    }
}
impl Drop for MockHost {
    fn drop(&mut self) {
        let loaded = world()
            .mods
            .values()
            .filter_map(|r| r.ctx)
            .collect::<Vec<_>>();
        for ctx in loaded.into_iter().rev() {
            if runtime::lookup(ctx).is_ok() {
                let _ = catch_unwind(AssertUnwindSafe(|| {
                    exports::ferrum_host_unload_mod(ctx.0.to_raw())
                }));
            }
        }
        // SAFETY: Drop 在持有 HOST_LOCK 的会话主线程执行，所有模组回调已经结束。
        unsafe { ferrum_host::mock_support::end_session() };
        domain::reset();
        events::reset();
        let mut w = world();
        w.mods.clear();
        w.pending.clear();
    }
}
