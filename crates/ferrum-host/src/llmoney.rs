use crate::{last_error, shim, vtable};
use ferrum_abi::{FerrumStatus, FfiByteWriter, FfiStr, LlMoneyApiV1};

fn call(f: impl FnOnce(&LlMoneyApiV1) -> FerrumStatus) -> FerrumStatus {
    vtable::guard(|| {
        if !vtable::is_main_thread() {
            return FerrumStatus::WRONG_THREAD;
        }
        let Some(table) = shim::get().and_then(|shim| shim.llmoney_v1) else {
            return last_error::fail(FerrumStatus::SERVICE_UNAVAILABLE, "LegacyMoney 服务不可用");
        };
        // SAFETY: 内部 shim 安装的访问器返回进程期有效且不可变的完整适配表。
        f(unsafe { &*table() })
    })
}

extern "C" fn get(xuid: FfiStr<'_>, out: *mut i64) -> FerrumStatus {
    call(|api| (api.get)(xuid, out))
}

extern "C" fn set(xuid: FfiStr<'_>, amount: i64) -> FerrumStatus {
    call(|api| (api.set)(xuid, amount))
}

extern "C" fn add(xuid: FfiStr<'_>, amount: i64) -> FerrumStatus {
    call(|api| (api.add)(xuid, amount))
}

extern "C" fn reduce(xuid: FfiStr<'_>, amount: i64) -> FerrumStatus {
    call(|api| (api.reduce)(xuid, amount))
}

extern "C" fn transfer(
    from: FfiStr<'_>,
    to: FfiStr<'_>,
    amount: i64,
    note: FfiStr<'_>,
) -> FerrumStatus {
    call(|api| (api.transfer)(from, to, amount, note))
}

extern "C" fn history(xuid: FfiStr<'_>, seconds: i32, out: *mut FfiByteWriter) -> FerrumStatus {
    call(|api| (api.history)(xuid, seconds, out))
}

extern "C" fn clear_history(seconds: i32) -> FerrumStatus {
    call(|api| (api.clear_history)(seconds))
}

extern "C" fn ranking(limit: u16, out: *mut FfiByteWriter) -> FerrumStatus {
    call(|api| (api.ranking)(limit, out))
}

pub(crate) static API: LlMoneyApiV1 = LlMoneyApiV1 {
    struct_size: size_of::<LlMoneyApiV1>() as u32,
    get,
    set,
    add,
    reduce,
    transfer,
    history,
    clear_history,
    ranking,
};
