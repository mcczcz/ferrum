//! 模组镜像加载、双向握手与生命周期转发。
//!
//! M1 只实现“保留镜像”卸载模式：镜像永不 `FreeLibrary`（见 `docs/design/threading.md`）。

use std::fs;
use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ferrum_abi::{
    AbiVersion, FERRUM_MOD_DESCRIPTOR_MIN_SIZE_V1_0, FERRUM_MOD_ENTRY_SYMBOL, FerrumModDescriptor,
    FerrumModFlags, FerrumStatus, LogLevel, ModContextHandle, ModEntryFnV1, RawMainThreadToken,
};

use crate::last_error;
use crate::runtime::{self, ModEntry, ModInfo, ModState};
use crate::shim;
use crate::vtable;

const SHADOW_DIR: &str = ".ferrum-shadow";

fn fail(status: FerrumStatus, msg: impl Into<String>) -> FerrumStatus {
    last_error::fail(status, msg)
}

/// FNV-1a 64，用于给影子副本命名（只要求同内容同名，不要求密码学强度）。
fn content_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 把 `entry` 及同名 `.pdb` 复制到 `<mod_dir>/.ferrum-shadow/<name>-<hash>.dll`。
///
/// 顺带尝试删除旧影子文件：仍被映射的镜像删除会失败，直接忽略。
fn shadow_copy(info: &ModInfo) -> Result<PathBuf, String> {
    let entry = Path::new(&info.entry_path);
    let bytes = fs::read(entry).map_err(|e| format!("读取 {} 失败: {e}", entry.display()))?;
    let stem = format!("{}-{:016x}", info.name, content_hash(&bytes));
    let dir = Path::new(&info.mod_dir).join(SHADOW_DIR);
    fs::create_dir_all(&dir).map_err(|e| format!("创建 {} 失败: {e}", dir.display()))?;

    if let Ok(read_dir) = fs::read_dir(&dir) {
        for old in read_dir.flatten() {
            let path = old.path();
            if path.file_stem().and_then(|s| s.to_str()) != Some(stem.as_str()) {
                let _ = fs::remove_file(path);
            }
        }
    }

    let target = dir.join(format!("{stem}.dll"));
    if !target.exists() {
        fs::write(&target, &bytes).map_err(|e| format!("写入 {} 失败: {e}", target.display()))?;
    }
    let pdb = entry.with_extension("pdb");
    if pdb.exists() {
        let _ = fs::copy(&pdb, dir.join(format!("{stem}.pdb")));
    }
    Ok(target)
}

/// 加载镜像并解析入口。镜像被有意泄漏，入口在进程生命周期内有效。
fn load_image(info: &ModInfo) -> Result<ModEntryFnV1, FerrumStatus> {
    let path = shadow_copy(info).map_err(|e| fail(FerrumStatus::NOT_FOUND, e))?;

    // SAFETY: 加载任意 DLL 会执行其 DllMain 与 TLS 回调；这是加载器的本职，
    // 模组作者对其 DLL 初始化代码的正确性负责。
    let library = unsafe { libloading::Library::new(&path) }.map_err(|e| {
        fail(
            FerrumStatus::NOT_FOUND,
            format!("加载 {} 失败: {e}", path.display()),
        )
    })?;

    // SAFETY: 符号类型与 ferrum-abi 中 ModEntryFnV1 的定义一致；由 #[mod_entry] 生成。
    let entry: ModEntryFnV1 = unsafe {
        library.get::<ModEntryFnV1>(FERRUM_MOD_ENTRY_SYMBOL.as_bytes())
    }
    .map(|sym| *sym)
    .map_err(|_| {
        fail(
            FerrumStatus::ABI_INCOMPATIBLE,
            format!(
                "{} 未导出 {FERRUM_MOD_ENTRY_SYMBOL}，它不是 Ferrum 模组或使用了不兼容的 SDK",
                info.entry_path
            ),
        )
    })?;

    // 保留镜像：永不 FreeLibrary，函数指针在进程生命周期内始终有效。
    std::mem::forget(library);
    Ok(entry)
}

fn handshake(
    ctx: ModContextHandle,
    entry: ModEntryFnV1,
) -> Result<FerrumModDescriptor, FerrumStatus> {
    // 全零缓冲区 + 预填容量：模组只写它与宿主都认识的前缀，未写的尾部字段保持零值。
    let capacity = size_of::<FerrumModDescriptor>() as u32;
    let mut descriptor = MaybeUninit::<FerrumModDescriptor>::zeroed();
    // SAFETY: descriptor 为本栈帧内对齐、可写的内存，首字段为 u32。
    unsafe { descriptor.as_mut_ptr().cast::<u32>().write(capacity) };
    // SAFETY: host_api 为 'static；descriptor 指向 capacity 字节的可写栈内存。
    let status = unsafe { entry(vtable::host_api(), ctx, descriptor.as_mut_ptr()) };
    if !status.is_ok() {
        let detail = last_error::take().unwrap_or_default();
        return Err(fail(status, format!("模组入口返回 {status}: {detail}")));
    }

    // SAFETY: 首字段 struct_size 在调用前已初始化，模组只会覆盖为实际写入量。
    let struct_size = unsafe { descriptor.as_ptr().cast::<u32>().read() };
    if !(FERRUM_MOD_DESCRIPTOR_MIN_SIZE_V1_0..=capacity).contains(&struct_size) {
        return Err(fail(
            FerrumStatus::ABI_INCOMPATIBLE,
            format!("模组描述符大小非法（struct_size = {struct_size}，宿主容量 {capacity}）"),
        ));
    }
    // SAFETY: v1.0 字段已由模组写入；其余尾部字段为全零，ABI 规定追加字段必须以零值为合法默认。
    let descriptor = unsafe { descriptor.assume_init() };

    let host = AbiVersion::CURRENT;
    let required = descriptor.required_abi_version;
    if required.major != host.major {
        return Err(fail(
            FerrumStatus::ABI_INCOMPATIBLE,
            format!("模组需要 ABI {required}，与 ferrum-loader 的 ABI {host} 主版本不兼容"),
        ));
    }
    if required.minor > host.minor {
        return Err(fail(
            FerrumStatus::ABI_INCOMPATIBLE,
            format!(
                "该模组需要 ferrum-loader ABI ≥ {required}（当前 {host}），请升级 ferrum-loader"
            ),
        ));
    }
    Ok(descriptor)
}

fn log(entry: &ModEntry, level: LogLevel, message: &str) {
    match shim::get() {
        Some(shim) => (shim.log)(entry.info.cookie, level, message.into()),
        None => eprintln!("[{}][{}] {message}", entry.info.name, level.0),
    }
}

/// 模组生命周期回调返回非 OK 时，补全 last-error 详情。
fn lifecycle_failed(entry: &ModEntry, phase: &str, status: FerrumStatus) -> FerrumStatus {
    let detail = last_error::take().unwrap_or_default();
    let msg = if detail.is_empty() {
        format!("{phase} 返回 {status}")
    } else {
        format!("{phase} 返回 {status}: {detail}")
    };
    log(entry, LogLevel::ERROR, &msg);
    fail(status, msg)
}

/// `FerrumModManager::load` 的实现。成功时返回模组上下文句柄。
///
/// 无论成败，`info.cookie` 的所有权都已转交宿主：最终由 `release_cookie` 归还。
pub fn load(info: ModInfo) -> Result<ModContextHandle, FerrumStatus> {
    load_with(info, load_image)
}

/// 使用进程内入口复用真实宿主的握手与生命周期，供 Rust 假 shim 使用。
///
/// # Safety
/// entry 必须遵守 ModEntryFnV1 契约，所有回调在宿主使用期间保持有效。
#[cfg(feature = "mock")]
pub unsafe fn load_entry(
    info: ModInfo,
    entry: ModEntryFnV1,
) -> Result<ModContextHandle, FerrumStatus> {
    load_with(info, |_| Ok(entry))
}

fn load_with(
    info: ModInfo,
    resolve: impl FnOnce(&ModInfo) -> Result<ModEntryFnV1, FerrumStatus>,
) -> Result<ModContextHandle, FerrumStatus> {
    let existing = runtime::find_by_name(&info.name).map(|(_, e)| e.state());
    // 先构造记录，使任何失败路径都经由 ModEntry::drop 归还 cookie。
    let (ctx, entry) = runtime::insert(ModEntry::new(info));
    if let Some(state) = existing {
        runtime::remove(ctx);
        return Err(fail(
            FerrumStatus::ALREADY_EXISTS,
            format!("模组 {} 已加载（状态 {state:?}）", entry.info.name),
        ));
    }

    let descriptor = match resolve(&entry.info).and_then(|f| handshake(ctx, f)) {
        Ok(d) => d,
        Err(status) => {
            runtime::remove(ctx);
            return Err(status);
        }
    };
    entry.set_descriptor(descriptor);

    let sdk = descriptor.sdk_version;
    log(
        &entry,
        LogLevel::DEBUG,
        &format!(
            "握手成功：ferrum-sdk {sdk}，需要 ABI ≥ {}",
            descriptor.required_abi_version
        ),
    );

    // 调用模组代码前不持有任何宿主锁。
    let status = (descriptor.on_load)(ctx);
    if !status.is_ok() {
        let status = lifecycle_failed(&entry, "on_load", status);
        runtime::remove(ctx);
        return Err(status);
    }
    entry.set_state(ModState::Loaded);
    Ok(ctx)
}

fn lookup(raw: u64) -> Result<(ModContextHandle, Arc<ModEntry>), FerrumStatus> {
    runtime::lookup_raw(raw).map_err(|s| fail(s, "无效或已失效的模组句柄"))
}

/// `FerrumModManager::enable` 的实现，必须在主线程调用。
pub fn enable(raw: u64) -> FerrumStatus {
    let (ctx, entry) = match lookup(raw) {
        Ok(v) => v,
        Err(s) => return s,
    };
    if !vtable::is_main_thread() {
        return fail(FerrumStatus::WRONG_THREAD, "enable 必须在服务端主线程调用");
    }
    match entry.state() {
        ModState::Enabled => return FerrumStatus::OK,
        ModState::Loaded | ModState::Disabled => {}
        s => {
            return fail(
                FerrumStatus::MOD_DISABLED,
                format!("模组当前状态 {s:?} 不能启用"),
            );
        }
    }
    let Some(descriptor) = entry.descriptor().copied() else {
        return fail(FerrumStatus::INTERNAL_ERROR, "模组缺少描述符");
    };

    entry.set_state(ModState::Enabling);
    entry
        .breaker
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .reset();
    if let Some(on_enable) = descriptor.on_enable {
        let status = on_enable(ctx, RawMainThreadToken::ASSERTED);
        if !status.is_ok() {
            // on_enable 失败（含 panic）：模组进入 Disabled。
            entry.set_state(ModState::Disabled);
            crate::command::revoke(ctx);
            crate::event::revoke(ctx);
            crate::custom_event::revoke(ctx);
            return lifecycle_failed(&entry, "on_enable", status);
        }
    }
    entry.set_state(ModState::Enabled);
    FerrumStatus::OK
}

/// `FerrumModManager::disable` 的实现，必须在主线程调用。
///
/// 自身回调内停用时，通知与排空推迟到最外层回调返回。
pub fn disable(raw: u64) -> FerrumStatus {
    let (ctx, entry) = match lookup(raw) {
        Ok(v) => v,
        Err(s) => return s,
    };
    if !vtable::is_main_thread() {
        return fail(FerrumStatus::WRONG_THREAD, "disable 必须在服务端主线程调用");
    }
    match entry.state() {
        ModState::Enabled => {}
        ModState::Loaded | ModState::Disabled => return FerrumStatus::OK,
        s => {
            return fail(
                FerrumStatus::MOD_DISABLED,
                format!("模组当前状态 {s:?} 不能停用"),
            );
        }
    }
    // 1. 关闸
    entry.set_state(ModState::Disabling);
    {
        let mut own = entry.ownership.lock().unwrap_or_else(|e| e.into_inner());
        if own.active_invocations > 0 {
            own.pending_disable = true;
            return FerrumStatus::OK;
        }
    }
    finish_disable(ctx, &entry);
    FerrumStatus::OK
}

/// 计入一次执行中的模组回调；模组未启用时返回 `MOD_DISABLED`。
///
/// 每次成功的 `enter` 必须配一次 [`leave`]；优先用 [`Invocation`] 守卫。
pub(crate) fn enter(entry: &ModEntry) -> Result<(), FerrumStatus> {
    if entry.state() != ModState::Enabled {
        return Err(FerrumStatus::MOD_DISABLED);
    }
    entry
        .ownership
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .active_invocations += 1;
    Ok(())
}

/// 结束一次回调；最外层回调结束且停用被推迟时，在此完成停用。
pub(crate) fn leave(ctx: ModContextHandle, entry: &ModEntry) {
    let pending = {
        let mut own = entry.ownership.lock().unwrap_or_else(|e| e.into_inner());
        own.active_invocations -= 1;
        own.pending_disable && own.active_invocations == 0
    };
    if pending {
        finish_disable(ctx, entry);
    }
}

/// [`enter`] / [`leave`] 的 RAII 守卫：宿主代码 panic 展开时计数同样会配平，
/// 避免模组永久卡在 `Disabling`。
pub(crate) struct Invocation {
    ctx: ModContextHandle,
    entry: Arc<ModEntry>,
}

impl Invocation {
    pub(crate) fn begin(ctx: ModContextHandle, entry: Arc<ModEntry>) -> Result<Self, FerrumStatus> {
        enter(&entry)?;
        Ok(Self { ctx, entry })
    }
}

impl Drop for Invocation {
    fn drop(&mut self) {
        leave(self.ctx, &self.entry);
    }
}

pub(crate) fn finish_disable(ctx: ModContextHandle, entry: &ModEntry) {
    let Some(descriptor) = entry.descriptor().copied() else {
        return;
    };
    // 2. 通知；无论成功与否，模组最终都进入 Disabled。
    let status = match descriptor.on_disable {
        Some(on_disable) => on_disable(ctx, RawMainThreadToken::ASSERTED),
        None => FerrumStatus::OK,
    };
    // 3–4. 撤销路由，排空回调；析构不在 ownership 锁内执行。
    crate::command::revoke(ctx);
    crate::event::revoke(ctx);
    crate::custom_event::revoke(ctx);
    entry
        .ownership
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .pending_disable = false;
    entry.set_state(ModState::Disabled);
    if !status.is_ok() {
        lifecycle_failed(entry, "on_disable", status);
    }
}

/// `FerrumModManager::unload` 的实现。成功后句柄失效，镜像保留。
pub fn unload(raw: u64) -> FerrumStatus {
    let (ctx, entry) = match lookup(raw) {
        Ok(v) => v,
        Err(s) => return s,
    };
    let has_resources = {
        let own = entry.ownership.lock().unwrap_or_else(|e| e.into_inner());
        !own.commands.is_empty()
            || !own.listeners.is_empty()
            || own.custom_listeners != 0
            || own.custom_providers != 0
    };
    if !vtable::is_main_thread() && has_resources {
        return fail(FerrumStatus::WRONG_THREAD, "unload 必须在服务端主线程调用");
    }
    if entry
        .ownership
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .active_invocations
        > 0
    {
        return fail(
            FerrumStatus::REENTRANT_CALL,
            "不能在执行中的回调里同步卸载自身",
        );
    }
    if entry.state() == ModState::Enabled {
        let status = disable(raw);
        if !status.is_ok() {
            return status;
        }
    }
    let Some(descriptor) = entry.descriptor().copied() else {
        return fail(FerrumStatus::INTERNAL_ERROR, "模组缺少描述符");
    };

    crate::command::revoke(ctx);
    crate::event::revoke(ctx);
    crate::custom_event::revoke(ctx);
    entry.set_state(ModState::Unloading);
    let status = match descriptor.on_unload {
        Some(on_unload) => on_unload(ctx),
        None => FerrumStatus::OK,
    };
    // 句柄失效：之后模组残留线程的任何调用都会得到 STALE_HANDLE。
    runtime::remove(ctx);
    if !status.is_ok() {
        lifecycle_failed(&entry, "on_unload", status);
    }
    if descriptor.flags.contains(FerrumModFlags::HOT_UNLOAD_SAFE) {
        log(
            &entry,
            LogLevel::DEBUG,
            "模组声明了 hot_unload_safe，但当前版本仍保留镜像（真正的热卸载在 M5 实现）",
        );
    }
    FerrumStatus::OK
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn noop_load(_: ModContextHandle) -> FerrumStatus {
        FerrumStatus::OK
    }

    fn enabled_entry() -> Arc<ModEntry> {
        let entry = Arc::new(ModEntry::new(ModInfo {
            cookie: crate::shim::ShimModCookie(std::ptr::null_mut()),
            name: "t".into(),
            version: "0.0.0".into(),
            entry_path: String::new(),
            mod_dir: String::new(),
            data_dir: String::new(),
            config_dir: String::new(),
            lang_dir: String::new(),
        }));
        entry.set_descriptor(FerrumModDescriptor {
            struct_size: size_of::<FerrumModDescriptor>() as u32,
            flags: FerrumModFlags::NONE,
            required_abi_version: AbiVersion::CURRENT,
            sdk_version: AbiVersion::CURRENT,
            on_load: noop_load,
            on_enable: None,
            on_disable: None,
            on_unload: None,
        });
        entry.set_state(ModState::Enabled);
        entry
    }

    fn active(entry: &ModEntry) -> usize {
        entry.ownership.lock().unwrap().active_invocations
    }

    #[test]
    fn invocation_balances_on_panic_and_finishes_deferred_disable() {
        let ctx = ModContextHandle(ferrum_abi::GenerationalHandle::new(0, 1).unwrap());
        let entry = enabled_entry();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = Invocation::begin(ctx, entry.clone()).unwrap();
            assert_eq!(active(&entry), 1);
            // 回调中途请求停用：被推迟到最外层回调结束。
            entry.set_state(ModState::Disabling);
            entry.ownership.lock().unwrap().pending_disable = true;
            panic!("host bug");
        }));
        assert!(result.is_err());
        assert_eq!(active(&entry), 0);
        assert!(!entry.ownership.lock().unwrap().pending_disable);
        assert_eq!(entry.state(), ModState::Disabled);
    }

    #[test]
    fn invocation_rejects_disabled_mod() {
        let ctx = ModContextHandle(ferrum_abi::GenerationalHandle::new(0, 1).unwrap());
        let entry = enabled_entry();
        entry.set_state(ModState::Disabled);
        assert_eq!(
            Invocation::begin(ctx, entry.clone()).err(),
            Some(FerrumStatus::MOD_DISABLED)
        );
        assert_eq!(active(&entry), 0);
    }

    #[test]
    fn fnv_is_stable() {
        assert_eq!(content_hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(content_hash(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
