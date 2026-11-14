use crate::{
    arena::Arena,
    last_error,
    runtime::{self, ModState},
    shim, vtable,
};
use ferrum_abi::*;
use std::sync::{Arc, Mutex};

/// 当前阶段的资源归属账本：命令及执行中的回调。
#[derive(Default)]
pub struct ModOwnership {
    pub commands: Vec<CommandHandle>,
    pub active_invocations: usize,
    pub pending_disable: bool,
}

struct RegisteredCommand {
    owner: ModContextHandle,
    callback: CommandCallbackV1,
}

// SAFETY: env 只由主线程入口调用和析构；Mutex 仅为全局句柄池提供同步。
unsafe impl Send for RegisteredCommand {}
// SAFETY: 不在其他线程解引用 env；并发读写均由主线程检查排除。
unsafe impl Sync for RegisteredCommand {}

impl Drop for RegisteredCommand {
    fn drop(&mut self) {
        (self.callback.drop_fn)(self.callback.env);
    }
}

struct Scope {
    owner: ModContextHandle,
    shim_id: u64,
}
static COMMANDS: Mutex<Arena<Arc<RegisteredCommand>>> = Mutex::new(Arena::new());
static SCOPES: Mutex<Arena<Scope>> = Mutex::new(Arena::new());

fn main_thread() -> Result<(), FerrumStatus> {
    if vtable::is_main_thread() {
        Ok(())
    } else {
        Err(last_error::fail(
            FerrumStatus::WRONG_THREAD,
            "命令 API 必须在主线程调用",
        ))
    }
}

extern "C" fn register(
    ctx: ModContextHandle,
    definition: *const CommandDefinitionV1<'_>,
    callback: CommandCallbackV1,
    out: *mut CommandHandle,
) -> FerrumStatus {
    vtable::guard(|| {
        if let Err(s) = main_thread() {
            return s;
        }
        if out.is_null() || definition.is_null() {
            return FerrumStatus::NULL_POINTER;
        }
        let entry = match runtime::lookup(ctx) {
            Ok(e) => e,
            Err(s) => return s,
        };
        if !matches!(entry.state(), ModState::Enabling | ModState::Enabled) {
            return last_error::fail(FerrumStatus::MOD_DISABLED, "只能在启用阶段注册命令");
        }
        let Some(api) = shim::get() else {
            return FerrumStatus::SERVICE_UNAVAILABLE;
        };
        // SAFETY: ABI 调用方保证 definition 非空时指向有效描述头。
        if unsafe { (*definition).struct_size } < size_of::<CommandDefinitionV1<'_>>() as u32 {
            return FerrumStatus::ABI_INCOMPATIBLE;
        }
        let registered = Arc::new(RegisteredCommand {
            owner: ctx,
            callback,
        });
        let handle = CommandHandle(
            COMMANDS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(registered.clone()),
        );
        let status = (api.command_register)(entry.info.cookie, handle, definition);
        if status.is_ok() {
            entry
                .ownership
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .commands
                .push(handle);
            // SAFETY: out 非空且由调用方提供可写栈内存。
            unsafe { out.write(handle) };
        } else {
            let removed = COMMANDS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(handle.0);
            drop(removed);
            // 注册失败不接管 env；两个 Arc 现已只剩当前这一份。
            if let Ok(registered) = Arc::try_unwrap(registered) {
                std::mem::forget(registered);
            }
        }
        status
    })
}

extern "C" fn unregister(ctx: ModContextHandle, handle: CommandHandle) -> FerrumStatus {
    vtable::guard(|| {
        if let Err(s) = main_thread() {
            return s;
        }
        let entry = match runtime::lookup(ctx) {
            Ok(e) => e,
            Err(s) => return s,
        };
        let removed = {
            let mut commands = COMMANDS.lock().unwrap_or_else(|e| e.into_inner());
            match commands.get(handle.0) {
                None => return FerrumStatus::STALE_HANDLE,
                Some(c) if c.owner != ctx => return FerrumStatus::PERMISSION_DENIED,
                _ => {}
            }
            commands.remove(handle.0)
        };
        entry
            .ownership
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .commands
            .retain(|h| *h != handle);
        let status = shim::get().map_or(FerrumStatus::SERVICE_UNAVAILABLE, |a| {
            (a.command_unregister)(entry.info.cookie, handle)
        });
        drop(removed);
        status
    })
}

/// 先摘除路由，再于主线程且无宿主锁时析构回调。
pub fn revoke(ctx: ModContextHandle) {
    let Ok(entry) = runtime::lookup(ctx) else {
        return;
    };
    let handles = std::mem::take(
        &mut entry
            .ownership
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .commands,
    );
    for handle in handles {
        let removed = COMMANDS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(handle.0);
        if let Some(api) = shim::get() {
            (api.command_unregister)(entry.info.cookie, handle);
        }
        drop(removed);
    }
}

fn scope_call(
    ctx: ModContextHandle,
    scope: CommandScopeHandle,
    f: impl FnOnce(&shim::FerrumShimApi, u64) -> FerrumStatus,
) -> FerrumStatus {
    vtable::guard(|| {
        if let Err(s) = main_thread() {
            return s;
        }
        let id = {
            let scopes = SCOPES.lock().unwrap_or_else(|e| e.into_inner());
            match scopes.get(scope.0) {
                Some(s) if s.owner == ctx => s.shim_id,
                Some(_) => return FerrumStatus::PERMISSION_DENIED,
                None => return FerrumStatus::STALE_HANDLE,
            }
        };
        match shim::get() {
            Some(a) => f(a, id),
            None => FerrumStatus::SERVICE_UNAVAILABLE,
        }
    })
}

extern "C" fn origin_player(
    c: ModContextHandle,
    s: CommandScopeHandle,
    out: *mut ActorRuntimeId,
) -> FerrumStatus {
    scope_call(c, s, |a, id| (a.command_origin)(id, out))
}
extern "C" fn get_int(
    c: ModContextHandle,
    s: CommandScopeHandle,
    n: FfiStr<'_>,
    out: *mut i32,
) -> FerrumStatus {
    scope_call(c, s, |a, id| (a.command_int)(id, n, out))
}
extern "C" fn get_float(
    c: ModContextHandle,
    s: CommandScopeHandle,
    n: FfiStr<'_>,
    out: *mut f32,
) -> FerrumStatus {
    scope_call(c, s, |a, id| (a.command_float)(id, n, out))
}
extern "C" fn get_bool(
    c: ModContextHandle,
    s: CommandScopeHandle,
    n: FfiStr<'_>,
    out: *mut FfiBool,
) -> FerrumStatus {
    scope_call(c, s, |a, id| (a.command_bool)(id, n, out))
}
extern "C" fn get_string(
    c: ModContextHandle,
    s: CommandScopeHandle,
    n: FfiStr<'_>,
    out: *mut FfiByteWriter,
) -> FerrumStatus {
    scope_call(c, s, |a, id| (a.command_string)(id, n, out))
}
extern "C" fn get_players(
    c: ModContextHandle,
    s: CommandScopeHandle,
    n: FfiStr<'_>,
    out: *mut FfiByteWriter,
) -> FerrumStatus {
    scope_call(c, s, |a, id| (a.command_players)(id, n, out))
}
extern "C" fn output(
    c: ModContextHandle,
    s: CommandScopeHandle,
    error: FfiBool,
    text: FfiStr<'_>,
) -> FerrumStatus {
    scope_call(c, s, |a, id| (a.command_output)(id, error, text))
}

/// cpp-shim 只传入自身栈帧的数字标识，游戏指针留在 C++ 内。
#[unsafe(no_mangle)]
pub extern "C" fn ferrum_host_dispatch_command(raw: u64, shim_scope: u64) -> FerrumStatus {
    vtable::guard(|| {
        if let Err(s) = main_thread() {
            return s;
        }
        let Some(handle) = GenerationalHandle::from_raw(raw) else {
            return FerrumStatus::STALE_HANDLE;
        };
        let registered = COMMANDS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(handle)
            .cloned();
        let Some(registered) = registered else {
            return FerrumStatus::STALE_HANDLE;
        };
        let ctx = registered.owner;
        let entry = match runtime::lookup(ctx) {
            Ok(e) => e,
            Err(s) => return s,
        };
        if entry.state() != ModState::Enabled {
            return FerrumStatus::MOD_DISABLED;
        }
        entry
            .ownership
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active_invocations += 1;
        let scope = CommandScopeHandle(SCOPES.lock().unwrap_or_else(|e| e.into_inner()).insert(
            Scope {
                owner: ctx,
                shim_id: shim_scope,
            },
        ));
        let status = (registered.callback.invoke_fn)(
            registered.callback.env,
            scope,
            RawMainThreadToken::ASSERTED,
        );
        SCOPES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(scope.0);
        if status == FerrumStatus::MOD_PANICKED || status == FerrumStatus::CALLBACK_POISONED {
            let detail = last_error::take().unwrap_or_default();
            if let Some(api) = shim::get() {
                (api.log)(
                    entry.info.cookie,
                    LogLevel::ERROR,
                    FfiStr::new(&format!("命令回调 {status}: {detail}")),
                );
            }
            unregister(ctx, CommandHandle(handle));
        }
        drop(registered);
        let pending = {
            let mut own = entry.ownership.lock().unwrap_or_else(|e| e.into_inner());
            own.active_invocations -= 1;
            own.pending_disable && own.active_invocations == 0
        };
        if pending {
            crate::loader::finish_disable(ctx, &entry);
        }
        status
    })
}

pub static COMMAND_V1: CommandApiV1 = CommandApiV1 {
    struct_size: size_of::<CommandApiV1>() as u32,
    register,
    unregister,
    origin_player,
    get_int,
    get_float,
    get_bool,
    get_string,
    get_players,
    output,
};
