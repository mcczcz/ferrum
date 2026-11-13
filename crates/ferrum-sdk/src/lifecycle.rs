//! 模组生命周期 trait 与 `#[mod_entry]` 展开所用的跳板。

use std::sync::{Mutex, TryLockError};

use ferrum_abi::{
    AbiVersion, CORE_API_MIN_SIZE_V1_0, FERRUM_ABI_MAGIC, FERRUM_ABI_MAJOR,
    FERRUM_HOST_API_MIN_SIZE_V1_0, FerrumHostApi, FerrumModDescriptor, FerrumModFlags,
    FerrumStatus, LOG_API_MIN_SIZE_V1_0, ModContextHandle, ModEntryFnV1, OnDisableFn, OnEnableFn,
    OnLoadFn, OnUnloadFn, RawMainThreadToken,
};

use crate::context::ModContext;
use crate::error::{Error, Result};
use crate::firewall::{self, firewall};
use crate::host;
use crate::token::MainThreadToken;

/// Ferrum 模组的生命周期。用 `#[ferrum_sdk::mod_entry]` 标注实现块即可导出入口。
///
/// 模组实例可能在不同线程上被创建与使用（加载发生在进程主线程，启用/停用发生在服务端线程），
/// 因此要求 `Send`。只在主线程使用的非 `Send` 状态请放进回调闭包中。
pub trait FerrumMod: Sized + Send + 'static {
    /// 阶段 1：BDS 启动早期（或运行期 `ll mod load`）调用，此时 Level 尚未就绪。
    ///
    /// 适合读取配置与初始化内存状态。返回 `Err` 或 panic 则加载失败。
    fn on_load(ctx: &ModContext) -> Result<Self>;

    /// 阶段 2：服务端主线程与世界就绪。返回 `Err` 或 panic 则模组保持停用。
    fn on_enable(&mut self, ctx: &ModContext, token: &MainThreadToken<'_>) -> Result<()> {
        let _ = (ctx, token);
        Ok(())
    }

    /// 阶段 3：模组被停用或服务器正在关服。返回后宿主自动清理本模组注册的资源。
    fn on_disable(&mut self, ctx: &ModContext, token: &MainThreadToken<'_>) -> Result<()> {
        let _ = (ctx, token);
        Ok(())
    }

    /// 阶段 4：卸载前终结。自建线程或异步运行时必须在这里 join / shutdown。
    fn on_unload(self, ctx: &ModContext) -> Result<()> {
        let _ = ctx;
        Ok(())
    }
}

/// 由 `#[mod_entry]` 为模组类型实现，供 `ferrum-host-mock` 在测试中直接调用入口。
pub trait ModEntryPoint: FerrumMod {
    /// 该模组的 `ferrum_mod_entry_v1`。
    const ENTRY: ModEntryFnV1;
}

/// 每个模组类型一个的全局实例槽。
pub struct ModState<T> {
    instance: Mutex<Option<T>>,
}

impl<T> ModState<T> {
    pub const fn new() -> Self {
        Self {
            instance: Mutex::new(None),
        }
    }
}

impl<T> Default for ModState<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `#[mod_entry]` 生成的回调集合。
pub struct EntryCallbacks {
    pub flags: FerrumModFlags,
    pub min_abi: Option<(u16, u16)>,
    pub on_load: OnLoadFn,
    pub on_enable: OnEnableFn,
    pub on_disable: OnDisableFn,
    pub on_unload: OnUnloadFn,
}

/// 校验宿主根表（握手第 2 步）。
///
/// # Safety
/// `host_api` 为空或指向宿主提供的 `'static` 根表。
unsafe fn validate_host(host_api: *const FerrumHostApi) -> Option<&'static FerrumHostApi> {
    if host_api.is_null() {
        return None;
    }
    // SAFETY: 非空；先只读取 v1.0 起永远存在的前缀字段。
    let api: &'static FerrumHostApi = unsafe { &*host_api };
    if api.magic != FERRUM_ABI_MAGIC
        || api.struct_size < FERRUM_HOST_API_MIN_SIZE_V1_0
        || api.host_abi_version.major != FERRUM_ABI_MAJOR
        || api.core_v1.is_null()
        || api.log_v1.is_null()
    {
        return None;
    }
    // SAFETY: 两个子表指针已检查非空，宿主保证其为 'static。
    let (core, log) = unsafe { (&*api.core_v1, &*api.log_v1) };
    if core.struct_size < CORE_API_MIN_SIZE_V1_0 || log.struct_size < LOG_API_MIN_SIZE_V1_0 {
        return None;
    }
    Some(api)
}

/// `ferrum_mod_entry_v1` 的实现。
///
/// # Safety
/// 由宿主按 [`ModEntryFnV1`] 契约调用：`host_api` 为空或有效，`out_descriptor` 为空或可写。
pub unsafe fn entry(
    host_api: *const FerrumHostApi,
    _ctx: ModContextHandle,
    out_descriptor: *mut FerrumModDescriptor,
    callbacks: EntryCallbacks,
) -> FerrumStatus {
    let result = std::panic::catch_unwind(move || {
        // SAFETY: 透传宿主保证。
        let Some(api) = (unsafe { validate_host(host_api) }) else {
            return FerrumStatus::ABI_INCOMPATIBLE;
        };
        if out_descriptor.is_null() {
            return FerrumStatus::NULL_POINTER;
        }
        host::install(api);
        firewall::install_hook();

        let required = match callbacks.min_abi {
            Some((major, minor)) => AbiVersion::new(major, minor, 0),
            None => AbiVersion::V1_0,
        };
        let descriptor = FerrumModDescriptor {
            struct_size: size_of::<FerrumModDescriptor>() as u32,
            flags: callbacks.flags,
            required_abi_version: required,
            sdk_version: crate::SDK_VERSION,
            on_load: callbacks.on_load,
            on_enable: Some(callbacks.on_enable),
            on_disable: Some(callbacks.on_disable),
            on_unload: Some(callbacks.on_unload),
        };
        // SAFETY: 已检查非空，宿主保证其指向可写的描述符内存。
        unsafe { out_descriptor.write(descriptor) };
        FerrumStatus::OK
    });
    result.unwrap_or_else(|payload| {
        std::mem::forget(payload);
        FerrumStatus::MOD_PANICKED
    })
}

fn reentrant() -> Error {
    Error::new(FerrumStatus::REENTRANT_CALL, "生命周期回调发生重入")
}

pub fn on_load<T: FerrumMod>(state: &'static ModState<T>, ctx: ModContextHandle) -> FerrumStatus {
    firewall(ctx, || {
        let slot = match state.instance.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
            Err(TryLockError::WouldBlock) => return Err(reentrant()),
        };
        if slot.is_some() {
            return Err(Error::new(
                FerrumStatus::ALREADY_EXISTS,
                "同一模组镜像中已有一个已加载的实例",
            ));
        }
        // 用户代码不在锁内执行：on_load 期间若重入只会看到 slot 为空。
        drop(slot);
        let instance = T::on_load(&ModContext::new(ctx))?;
        *state.instance.lock().unwrap_or_else(|p| p.into_inner()) = Some(instance);
        Ok(())
    })
}

fn with_instance<T: FerrumMod>(
    state: &'static ModState<T>,
    ctx: ModContextHandle,
    f: impl FnOnce(&mut T, &ModContext, &MainThreadToken<'_>) -> Result<()>,
) -> FerrumStatus {
    firewall(ctx, || {
        let mut slot = match state.instance.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::Poisoned(_)) => {
                return Err(Error::new(
                    FerrumStatus::CALLBACK_POISONED,
                    "模组此前在生命周期回调中 panic，实例状态可能不一致",
                ));
            }
            Err(TryLockError::WouldBlock) => return Err(reentrant()),
        };
        let Some(instance) = slot.as_mut() else {
            return Err(Error::new(FerrumStatus::STALE_HANDLE, "模组实例不存在"));
        };
        let token = MainThreadToken::assume_main_thread();
        f(instance, &ModContext::new(ctx), &token)
    })
}

pub fn on_enable<T: FerrumMod>(
    state: &'static ModState<T>,
    ctx: ModContextHandle,
    _token: RawMainThreadToken,
) -> FerrumStatus {
    with_instance(state, ctx, |m, ctx, token| m.on_enable(ctx, token))
}

pub fn on_disable<T: FerrumMod>(
    state: &'static ModState<T>,
    ctx: ModContextHandle,
    _token: RawMainThreadToken,
) -> FerrumStatus {
    with_instance(state, ctx, |m, ctx, token| m.on_disable(ctx, token))
}

pub fn on_unload<T: FerrumMod>(state: &'static ModState<T>, ctx: ModContextHandle) -> FerrumStatus {
    firewall(ctx, || {
        let instance = match state.instance.try_lock() {
            Ok(mut slot) => slot.take(),
            // 即使此前 panic 过，也要把实例取出并析构。
            Err(TryLockError::Poisoned(p)) => p.into_inner().take(),
            Err(TryLockError::WouldBlock) => return Err(reentrant()),
        };
        match instance {
            Some(instance) => instance.on_unload(&ModContext::new(ctx)),
            None => Ok(()),
        }
    })
}
