//! 模组镜像加载、双向握手与生命周期转发。
//!
//! M1 只实现“保留镜像”卸载模式：镜像永不 `FreeLibrary`（见线程规范 §4.4–4.6）。

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

/// 加载镜像并执行握手；成功后返回已校验的描述符。镜像被有意泄漏（保留镜像模式）。
fn load_image(ctx: ModContextHandle, info: &ModInfo) -> Result<FerrumModDescriptor, FerrumStatus> {
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

    let mut descriptor = MaybeUninit::<FerrumModDescriptor>::zeroed();
    // SAFETY: host_api 为 'static；descriptor 指向可写的、足够大的栈内存。
    let status = unsafe { entry(vtable::host_api(), ctx, descriptor.as_mut_ptr()) };
    if !status.is_ok() {
        let detail = last_error::take().unwrap_or_default();
        return Err(fail(status, format!("模组入口返回 {status}: {detail}")));
    }

    // SAFETY: 入口返回 OK 时 SDK 保证已完整写入描述符；前 4 字节 struct_size 必定已写入。
    let struct_size = unsafe { (*descriptor.as_ptr()).struct_size };
    if struct_size < FERRUM_MOD_DESCRIPTOR_MIN_SIZE_V1_0 {
        return Err(fail(
            FerrumStatus::ABI_INCOMPATIBLE,
            format!("模组描述符过小（struct_size = {struct_size}）"),
        ));
    }
    // SAFETY: struct_size 覆盖 v1.0 全部字段，描述符已完整初始化。
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

    let descriptor = match load_image(ctx, &entry.info) {
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
            return lifecycle_failed(&entry, "on_enable", status);
        }
    }
    entry.set_state(ModState::Enabled);
    FerrumStatus::OK
}

/// `FerrumModManager::disable` 的实现，必须在主线程调用。
///
/// 自身命令回调内停用时，通知与排空推迟到最外层回调返回。
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
    if !vtable::is_main_thread()
        && !entry
            .ownership
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .commands
            .is_empty()
    {
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
            "不能在执行中的命令回调里同步卸载自身",
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

    #[test]
    fn fnv_is_stable() {
        assert_eq!(content_hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(content_hash(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
