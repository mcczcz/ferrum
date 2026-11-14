//! 纯 Rust 假宿主，用于在不启动 BDS 的情况下测试 Ferrum 模组。
//!
//! ```ignore
//! use ferrum_host_mock::MockHost;
//!
//! let host = MockHost::new();
//! let m = host.load::<MyMod>().unwrap();
//! host.enable(&m).unwrap();
//! assert!(host.logs(&m).iter().any(|l| l.message.contains("已启用")));
//! ```
//!
//! SDK 把宿主根表保存在进程级全局变量中，且每个模组类型只有一个实例槽，
//! 因此同一进程内的 `MockHost` 互斥存在：[`MockHost::new`] 会等待前一个实例被 drop。

use std::ffi::c_void;
mod domain;
pub use domain::{MockActor, MockCommandOutput, MockCommandValue, MockPlayer, MockPlayerState};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::ThreadId;

use ferrum_abi::{
    AbiVersion, CoreApiV1, FERRUM_ABI_MAGIC, FerrumHostApi, FerrumModDescriptor, FerrumStatus,
    FfiBool, FfiByteWriter, FfiStr, GenerationalHandle, LogApiV1, LogLevel, ModContextHandle,
    ModDirKind, RawMainThreadToken,
};
use ferrum_sdk::{Error, Level, ModEntryPoint};

/// 捕获到的一条模组日志。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRecord {
    pub level: Level,
    pub message: String,
}

/// 捕获到的一次 panic 上报。
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
    name: String,
    version: String,
    root: String,
    descriptor: FerrumModDescriptor,
    state: MockModState,
    logs: Vec<LogRecord>,
    panics: Vec<PanicRecord>,
}

struct World {
    main_thread: Option<ThreadId>,
    mods: Vec<Option<ModRecord>>,
}

static HOST_LOCK: Mutex<()> = Mutex::new(());
static WORLD: Mutex<World> = Mutex::new(World {
    main_thread: None,
    mods: Vec::new(),
});

thread_local! {
    static LAST_ERROR: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn world() -> MutexGuard<'static, World> {
    WORLD.lock().unwrap_or_else(PoisonError::into_inner)
}

fn with_mod<R>(
    ctx: ModContextHandle,
    f: impl FnOnce(&mut ModRecord) -> R,
) -> Result<R, FerrumStatus> {
    let mut w = world();
    let index = ctx.0.slot_index() as usize;
    // mock 中每个模组独占一个槽位，代数恒为 1。
    match w.mods.get_mut(index).and_then(Option::as_mut) {
        Some(m) if ctx.0.generation() == 1 && m.state != MockModState::Unloaded => Ok(f(m)),
        _ => Err(FerrumStatus::STALE_HANDLE),
    }
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

fn lossy(s: FfiStr<'_>) -> String {
    // SAFETY: SDK 按 ABI 契约传入本次调用内有效的借用视图。
    String::from_utf8_lossy(unsafe { s.as_bytes() }).into_owned()
}

fn write_out(out: *mut FfiByteWriter, text: &str) -> FerrumStatus {
    // SAFETY: SDK 传入栈上有效的写入器或空指针。
    match unsafe { out.as_ref() } {
        Some(w) if !text.is_empty() => w.write(text.as_bytes()),
        Some(_) => FerrumStatus::OK,
        None => FerrumStatus::NULL_POINTER,
    }
}

fn status_of<T>(r: Result<T, FerrumStatus>, f: impl FnOnce(T) -> FerrumStatus) -> FerrumStatus {
    match r {
        Ok(v) => f(v),
        Err(s) => s,
    }
}

// ---------------------------------------------------------------------------
// VTable 实现
// ---------------------------------------------------------------------------

extern "C" fn take_last_error(out: *mut FfiByteWriter) -> FerrumStatus {
    let text = LAST_ERROR
        .with(|s| s.borrow_mut().take())
        .unwrap_or_default();
    write_out(out, &text)
}

extern "C" fn set_last_error(message: FfiStr<'_>) -> FerrumStatus {
    let text = lossy(message);
    LAST_ERROR.with(|s| *s.borrow_mut() = Some(text));
    FerrumStatus::OK
}

extern "C" fn report_panic(
    ctx: ModContextHandle,
    message: FfiStr<'_>,
    location: FfiStr<'_>,
) -> FerrumStatus {
    let record = PanicRecord {
        message: lossy(message),
        location: lossy(location),
    };
    status_of(with_mod(ctx, |m| m.panics.push(record)), |_| {
        FerrumStatus::OK
    })
}

extern "C" fn get_mod_name(ctx: ModContextHandle, out: *mut FfiByteWriter) -> FerrumStatus {
    status_of(with_mod(ctx, |m| m.name.clone()), |s| write_out(out, &s))
}

extern "C" fn get_mod_version(ctx: ModContextHandle, out: *mut FfiByteWriter) -> FerrumStatus {
    status_of(with_mod(ctx, |m| m.version.clone()), |s| write_out(out, &s))
}

extern "C" fn get_mod_dir(
    ctx: ModContextHandle,
    kind: ModDirKind,
    out: *mut FfiByteWriter,
) -> FerrumStatus {
    let sub = match kind {
        ModDirKind::MOD => "",
        ModDirKind::DATA => "/data",
        ModDirKind::CONFIG => "/config",
        ModDirKind::LANG => "/lang",
        _ => return FerrumStatus::INVALID_ARGUMENT,
    };
    status_of(with_mod(ctx, |m| format!("{}{sub}", m.root)), |s| {
        write_out(out, &s)
    })
}

extern "C" fn is_main_thread() -> FfiBool {
    (world().main_thread == Some(std::thread::current().id())).into()
}

extern "C" fn log(ctx: ModContextHandle, level: LogLevel, message: FfiStr<'_>) -> FerrumStatus {
    let Some(level) = level_from_abi(level) else {
        return FerrumStatus::INVALID_ARGUMENT;
    };
    let record = LogRecord {
        level,
        message: lossy(message),
    };
    status_of(with_mod(ctx, |m| m.logs.push(record)), |_| FerrumStatus::OK)
}

extern "C" fn should_log(_ctx: ModContextHandle, _level: LogLevel) -> FfiBool {
    FfiBool::TRUE
}

extern "C" fn query_interface(id: FfiStr<'_>, min_minor: u16) -> *const c_void {
    match lossy(id).as_str() {
        "ferrum.block.v1" if min_minor == 0 => {
            (&domain::BLOCK as *const ferrum_abi::BlockApiV1).cast()
        }
        "ferrum.actor.v1" if min_minor == 0 => {
            (&domain::ACTOR as *const ferrum_abi::ActorApiV1).cast()
        }
        "ferrum.mob.v1" if min_minor == 0 => (&domain::MOB as *const ferrum_abi::MobApiV1).cast(),
        "ferrum.player.v1" if min_minor == 0 => {
            (&domain::PLAYER as *const ferrum_abi::PlayerApiV1).cast()
        }
        "ferrum.command.v1" if min_minor == 0 => {
            (&domain::COMMAND as *const ferrum_abi::CommandApiV1).cast()
        }
        _ => std::ptr::null(),
    }
}

static CORE_V1: CoreApiV1 = CoreApiV1 {
    struct_size: size_of::<CoreApiV1>() as u32,
    take_last_error,
    set_last_error,
    report_panic,
    get_mod_name,
    get_mod_version,
    get_mod_dir,
    is_main_thread,
};

static LOG_V1: LogApiV1 = LogApiV1 {
    struct_size: size_of::<LogApiV1>() as u32,
    log,
    should_log,
};

struct HostApiCell(FerrumHostApi);

// SAFETY: 根表与子表为不可变 'static 数据。
unsafe impl Sync for HostApiCell {}

static HOST_API: HostApiCell = HostApiCell(FerrumHostApi {
    struct_size: size_of::<FerrumHostApi>() as u32,
    reserved_flags: 0,
    magic: FERRUM_ABI_MAGIC,
    host_abi_version: AbiVersion::CURRENT,
    core_v1: &CORE_V1,
    log_v1: &LOG_V1,
    query_interface,
});

// ---------------------------------------------------------------------------
// 公开 API
// ---------------------------------------------------------------------------

/// 已加载到 [`MockHost`] 中的模组。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MockMod {
    ctx: ModContextHandle,
}

impl MockMod {
    pub fn context_handle(&self) -> ModContextHandle {
        self.ctx
    }
}

/// 假宿主。创建它的线程被视为“服务端主线程”。drop 时卸载全部仍加载的模组。
pub struct MockHost {
    _exclusive: MutexGuard<'static, ()>,
}

impl Default for MockHost {
    fn default() -> Self {
        Self::new()
    }
}

fn take_error(status: FerrumStatus) -> Error {
    let message = LAST_ERROR
        .with(|s| s.borrow_mut().take())
        .unwrap_or_default();
    Error::new(status, message)
}

fn check(status: FerrumStatus) -> Result<(), Error> {
    if status.is_ok() {
        Ok(())
    } else {
        Err(take_error(status))
    }
}

impl MockHost {
    pub fn new() -> Self {
        let guard = HOST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let mut w = world();
        w.main_thread = Some(std::thread::current().id());
        w.mods.clear();
        drop(w);
        domain::reset();
        Self { _exclusive: guard }
    }

    /// 以默认名称（类型名的末段，转为小写短横线形式）加载模组。
    pub fn load<T: ModEntryPoint>(&self) -> Result<MockMod, Error> {
        let name = std::any::type_name::<T>()
            .rsplit("::")
            .next()
            .unwrap_or("mod")
            .to_owned();
        self.load_named::<T>(&name, "0.1.0")
    }

    /// 加载模组：握手并调用 `on_load`。
    pub fn load_named<T: ModEntryPoint>(
        &self,
        name: &str,
        version: &str,
    ) -> Result<MockMod, Error> {
        let index = {
            let w = world();
            u32::try_from(w.mods.len()).expect("too many mock mods")
        };
        let ctx =
            ModContextHandle(GenerationalHandle::new(index, 1).expect("generation 1 is non-zero"));

        let mut descriptor = std::mem::MaybeUninit::<FerrumModDescriptor>::zeroed();
        // SAFETY: HOST_API 为 'static，descriptor 指向可写栈内存。
        let status = unsafe { T::ENTRY(&HOST_API.0, ctx, descriptor.as_mut_ptr()) };
        check(status)?;
        // SAFETY: 入口返回 OK 即已完整写入描述符。
        let descriptor = unsafe { descriptor.assume_init() };
        if !AbiVersion::CURRENT.satisfies(descriptor.required_abi_version) {
            return Err(Error::new(
                FerrumStatus::ABI_INCOMPATIBLE,
                format!("模组需要 ABI {}", descriptor.required_abi_version),
            ));
        }

        world().mods.push(Some(ModRecord {
            name: name.to_owned(),
            version: version.to_owned(),
            root: format!("mods/{name}"),
            descriptor,
            state: MockModState::Loaded,
            logs: Vec::new(),
            panics: Vec::new(),
        }));

        let status = (descriptor.on_load)(ctx);
        if !status.is_ok() {
            with_mod(ctx, |m| m.state = MockModState::Unloaded).ok();
            return Err(take_error(status));
        }
        Ok(MockMod { ctx })
    }

    fn descriptor(&self, m: &MockMod) -> Result<FerrumModDescriptor, Error> {
        with_mod(m.ctx, |m| m.descriptor).map_err(|s| Error::new(s, "模组未加载"))
    }

    fn set_state(&self, m: &MockMod, state: MockModState) {
        let _ = with_mod(m.ctx, |m| m.state = state);
    }

    /// 调用 `on_enable`；失败时模组保持 `Disabled`。
    pub fn enable(&self, m: &MockMod) -> Result<(), Error> {
        let descriptor = self.descriptor(m)?;
        self.set_state(m, MockModState::Enabling);
        if let Some(f) = descriptor.on_enable {
            let status = f(m.ctx, RawMainThreadToken::ASSERTED);
            if !status.is_ok() {
                self.set_state(m, MockModState::Disabled);
                domain::revoke(m.ctx);
                return Err(take_error(status));
            }
        }
        self.set_state(m, MockModState::Enabled);
        Ok(())
    }

    /// 调用 `on_disable`；无论结果如何模组都进入 `Disabled`。
    pub fn disable(&self, m: &MockMod) -> Result<(), Error> {
        self.set_state(m, MockModState::Disabling);
        if domain::defer_disable(m.ctx) {
            return Ok(());
        }
        self.finish_disable(m)
    }

    fn finish_disable(&self, m: &MockMod) -> Result<(), Error> {
        let descriptor = self.descriptor(m)?;
        let status = descriptor
            .on_disable
            .map_or(FerrumStatus::OK, |f| f(m.ctx, RawMainThreadToken::ASSERTED));
        self.set_state(m, MockModState::Disabled);
        domain::revoke(m.ctx);
        check(status)
    }

    /// 若已启用先停用，再调用 `on_unload`；之后句柄失效。
    pub fn unload(&self, m: &MockMod) -> Result<(), Error> {
        if domain::active(m.ctx) {
            return Err(Error::new(
                FerrumStatus::REENTRANT_CALL,
                "不能在命令回调内同步卸载自身",
            ));
        }
        if self.state(m) == Some(MockModState::Enabled) {
            self.disable(m)?;
        }
        let descriptor = self.descriptor(m)?;
        domain::revoke(m.ctx);
        let status = descriptor.on_unload.map_or(FerrumStatus::OK, |f| f(m.ctx));
        self.set_state(m, MockModState::Unloaded);
        check(status)
    }

    pub fn state(&self, m: &MockMod) -> Option<MockModState> {
        let w = world();
        w.mods
            .get(m.ctx.0.slot_index() as usize)?
            .as_ref()
            .map(|m| m.state)
    }

    pub fn logs(&self, m: &MockMod) -> Vec<LogRecord> {
        let w = world();
        w.mods
            .get(m.ctx.0.slot_index() as usize)
            .and_then(Option::as_ref)
            .map(|m| m.logs.clone())
            .unwrap_or_default()
    }

    pub fn panics(&self, m: &MockMod) -> Vec<PanicRecord> {
        let w = world();
        w.mods
            .get(m.ctx.0.slot_index() as usize)
            .and_then(Option::as_ref)
            .map(|m| m.panics.clone())
            .unwrap_or_default()
    }
}

impl Drop for MockHost {
    fn drop(&mut self) {
        let loaded: Vec<MockMod> = {
            let w = world();
            w.mods
                .iter()
                .enumerate()
                .filter(|(_, m)| {
                    m.as_ref()
                        .is_some_and(|m| m.state != MockModState::Unloaded)
                })
                .filter_map(|(i, _)| GenerationalHandle::new(i as u32, 1))
                .map(|h| MockMod {
                    ctx: ModContextHandle(h),
                })
                .collect()
        };
        for m in loaded.iter().rev() {
            // drop 中不允许 panic 外泄；卸载失败直接忽略。
            let _ = catch_unwind(AssertUnwindSafe(|| self.unload(m)));
        }
        let mut w = world();
        w.mods.clear();
        w.main_thread = None;
    }
}
