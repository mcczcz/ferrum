use std::cell::RefCell;

use ferrum_abi::*;

use crate::arena::Arena;
use crate::loader::Invocation;
use crate::runtime::{self, ModState};
use crate::{last_error, shim, vtable};

struct Callback(FormCallbackV1);

impl Drop for Callback {
    fn drop(&mut self) {
        (self.0.drop_fn)(self.0.env);
    }
}

struct PendingForm {
    owner: ModContextHandle,
    player: PlayerUuid,
    callback: Option<Callback>,
}

thread_local! {
    static PENDING: RefCell<Arena<PendingForm>> = const { RefCell::new(Arena::new()) };
}

extern "C" fn send(
    ctx: ModContextHandle,
    player: ActorRuntimeId,
    json: FfiStr<'_>,
    callback: FormCallbackV1,
) -> FerrumStatus {
    vtable::guard(|| {
        if !vtable::is_main_thread() {
            return FerrumStatus::WRONG_THREAD;
        }
        let entry = match runtime::lookup(ctx) {
            Ok(entry) => entry,
            Err(status) => return status,
        };
        if !matches!(entry.state(), ModState::Enabling | ModState::Enabled) {
            return FerrumStatus::MOD_DISABLED;
        }
        let Some(api) = shim::get() else {
            return FerrumStatus::SERVICE_UNAVAILABLE;
        };
        let Some(send) = api.form_send else {
            return FerrumStatus::UNSUPPORTED;
        };
        let mut uuid = PlayerUuid::default();
        let status = (api.player_v1.uuid)(player, &mut uuid);
        if !status.is_ok() {
            return status;
        }
        let id = PENDING.with_borrow_mut(|forms| {
            forms.insert(PendingForm {
                owner: ctx,
                player: uuid,
                callback: None,
            })
        });
        // LeviLamina 发送后在响应包的处理路径中回调。发送成功后才接管 env。
        let status = send(id.to_raw(), player, json);
        if status.is_ok() {
            let mut callback = Some(Callback(callback));
            PENDING.with_borrow_mut(|forms| {
                if let Some(form) = forms.get_mut(id) {
                    form.callback = callback.take();
                }
            });
        } else {
            PENDING.with_borrow_mut(|forms| forms.remove(id));
        }
        status
    })
}

fn complete(
    id: GenerationalHandle,
    player: ActorRuntimeId,
    kind: FormResponseKind,
    json: FfiStr<'_>,
) -> FerrumStatus {
    // 回调在 Invocation 前析构，包含 panic 展开的路径。
    let invocation;
    let Some(form) = PENDING.with_borrow_mut(|forms| forms.remove(id)) else {
        return FerrumStatus::OK;
    };
    let Ok(entry) = runtime::lookup(form.owner) else {
        return FerrumStatus::OK;
    };
    invocation = Invocation::begin(form.owner, entry.clone());
    let (Ok(_), Some(callback)) = (&invocation, form.callback) else {
        return FerrumStatus::OK;
    };
    let status = (callback.0.invoke_fn)(
        callback.0.env,
        RawMainThreadToken::ASSERTED,
        player,
        kind,
        json,
    );
    if !matches!(status, FerrumStatus::OK | FerrumStatus::MOD_PANICKED) {
        let detail = last_error::take().unwrap_or_default();
        if let Some(api) = shim::get() {
            (api.log)(
                entry.info.cookie,
                LogLevel::ERROR,
                FfiStr::new(&format!("表单回调返回 {status}: {detail}")),
            );
        }
    }
    FerrumStatus::OK
}

pub(crate) fn respond(
    id: u64,
    player: ActorRuntimeId,
    uuid: PlayerUuid,
    kind: FormResponseKind,
    json: FfiStr<'_>,
) -> FerrumStatus {
    if !vtable::is_main_thread() {
        return FerrumStatus::WRONG_THREAD;
    }
    let Some(id) = GenerationalHandle::from_raw(id) else {
        return FerrumStatus::STALE_HANDLE;
    };
    if PENDING.with_borrow(|forms| forms.get(id).is_some_and(|form| form.player == uuid)) {
        complete(id, player, kind, json)
    } else {
        FerrumStatus::OK
    }
}

pub(crate) fn disconnect(player: PlayerUuid) -> FerrumStatus {
    if !vtable::is_main_thread() {
        return FerrumStatus::WRONG_THREAD;
    }
    let ids = PENDING.with_borrow(|forms| {
        forms
            .iter()
            .filter_map(|(id, form)| (form.player == player).then_some(id))
            .collect::<Vec<_>>()
    });
    for id in ids {
        complete(
            id,
            ActorRuntimeId::default(),
            FormResponseKind::DISCONNECTED,
            FfiStr::new(""),
        );
    }
    FerrumStatus::OK
}

pub(crate) fn revoke(owner: ModContextHandle) {
    let removed = PENDING.with_borrow_mut(|forms| {
        let ids: Vec<_> = forms
            .iter()
            .filter_map(|(id, form)| (form.owner == owner).then_some(id))
            .collect();
        ids.into_iter()
            .filter_map(|id| forms.remove(id))
            .collect::<Vec<_>>()
    });
    drop(removed);
}

pub(crate) static API: FormApiV1 = FormApiV1 {
    struct_size: size_of::<FormApiV1>() as u32,
    send,
};
