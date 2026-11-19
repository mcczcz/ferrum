//! 向基岩版玩家发送表单。发送和响应回调在服务端主线程执行。

mod custom;

use std::cell::Cell;
use std::ffi::c_void;
use std::future::Future;

use ferrum_abi::{
    ActorRuntimeId, FerrumStatus, FfiStr, FormCallbackV1, FormResponseKind, ModContextHandle,
    RawMainThreadToken,
};
use futures_channel::oneshot;
use serde_json::{Value, json};

use crate::firewall::firewall;
use crate::{ActorOps, Error, MainThreadToken, ModContext, PlayerRef, Result, host};

pub use custom::{CustomForm, CustomFormResponse};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FormCancelReason {
    UserClosed,
    UserBusy,
    Unspecified,
    Unknown(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FormOutcome<T> {
    Submitted(T),
    Cancelled(FormCancelReason),
}

/// 按钮图片，使用资源包路径或 URL。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormImage {
    Path(String),
    Url(String),
}

/// 按钮菜单，提交结果是从 0 开始的按钮索引。
pub struct SimpleForm {
    title: String,
    content: String,
    buttons: Vec<Value>,
}

impl SimpleForm {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            content: String::new(),
            buttons: Vec::new(),
        }
    }

    pub fn content(mut self, text: impl Into<String>) -> Self {
        self.content = text.into();
        self
    }

    pub fn button(mut self, text: impl Into<String>) -> Self {
        self.buttons
            .push(json!({"type": "button", "text": text.into()}));
        self
    }

    pub fn button_with_image(mut self, text: impl Into<String>, image: FormImage) -> Self {
        let (kind, data) = match image {
            FormImage::Path(path) => ("path", path),
            FormImage::Url(url) => ("url", url),
        };
        self.buttons.push(json!({
            "type": "button", "text": text.into(), "image": {"type": kind, "data": data}
        }));
        self
    }

    pub fn send(
        self,
        ctx: &ModContext,
        player: PlayerRef<'_>,
        callback: impl for<'tick> FnOnce(
            PlayerRef<'tick>,
            FormOutcome<usize>,
            &'tick MainThreadToken<'tick>,
        ) -> Result<()>
        + 'static,
    ) -> Result<()> {
        let (form, decode) = self.prepare()?;
        send(ctx, player, form, decode, callback)
    }

    /// 立即发送，返回可跨线程等待的响应。玩家断线或模组停用时返回错误。
    pub fn send_async(
        self,
        ctx: &ModContext,
        player: PlayerRef<'_>,
    ) -> Result<impl Future<Output = Result<FormOutcome<usize>>> + Send + 'static + use<>> {
        let (form, decode) = self.prepare()?;
        send_async(ctx, player, form, decode)
    }

    fn prepare(self) -> Result<(Value, impl FnOnce(Value) -> Result<usize>)> {
        let count = self.buttons.len();
        if count == 0 {
            return Err(invalid("按钮表单至少需要一个按钮"));
        }
        let form = json!({
            "type": "form", "title": self.title, "content": self.content, "elements": self.buttons
        });
        Ok((form, move |value| {
            index(&value)
                .filter(|&selected| selected < count)
                .ok_or_else(|| invalid("表单返回的按钮索引无效"))
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalChoice {
    First,
    Second,
}

/// 两按钮确认框。
pub struct ModalForm {
    title: String,
    content: String,
    first: String,
    second: String,
}

impl ModalForm {
    pub fn new(
        title: impl Into<String>,
        content: impl Into<String>,
        first: impl Into<String>,
        second: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            content: content.into(),
            first: first.into(),
            second: second.into(),
        }
    }

    pub fn send(
        self,
        ctx: &ModContext,
        player: PlayerRef<'_>,
        callback: impl for<'tick> FnOnce(
            PlayerRef<'tick>,
            FormOutcome<ModalChoice>,
            &'tick MainThreadToken<'tick>,
        ) -> Result<()>
        + 'static,
    ) -> Result<()> {
        send(ctx, player, self.into_json(), modal_choice, callback)
    }

    /// 立即发送，返回可跨线程等待的响应。玩家断线或模组停用时返回错误。
    pub fn send_async(
        self,
        ctx: &ModContext,
        player: PlayerRef<'_>,
    ) -> Result<impl Future<Output = Result<FormOutcome<ModalChoice>>> + Send + 'static + use<>>
    {
        send_async(ctx, player, self.into_json(), modal_choice)
    }

    fn into_json(self) -> Value {
        json!({
            "type": "modal", "title": self.title, "content": self.content,
            "button1": self.first, "button2": self.second
        })
    }
}

fn modal_choice(value: Value) -> Result<ModalChoice> {
    match value.as_bool() {
        Some(true) => Ok(ModalChoice::First),
        Some(false) => Ok(ModalChoice::Second),
        None => Err(invalid("确认表单响应必须是布尔值")),
    }
}

fn invalid(message: impl Into<std::borrow::Cow<'static, str>>) -> Error {
    Error::new(FerrumStatus::INVALID_ARGUMENT, message)
}

fn index(value: &Value) -> Option<usize> {
    usize::try_from(value.as_u64()?).ok()
}

fn decode<T>(
    response: Result<FormOutcome<Value>>,
    submitted: impl FnOnce(Value) -> Result<T>,
) -> Result<FormOutcome<T>> {
    match response? {
        FormOutcome::Submitted(value) => submitted(value).map(FormOutcome::Submitted),
        FormOutcome::Cancelled(reason) => Ok(FormOutcome::Cancelled(reason)),
    }
}

fn send<T>(
    ctx: &ModContext,
    player: PlayerRef<'_>,
    form: Value,
    submitted: impl FnOnce(Value) -> Result<T> + 'static,
    callback: impl for<'tick> FnOnce(
        PlayerRef<'tick>,
        FormOutcome<T>,
        &'tick MainThreadToken<'tick>,
    ) -> Result<()>
    + 'static,
) -> Result<()> {
    register(ctx, player, form, move |id, response, token| {
        if matches!(&response, Err(error) if error.status() == FerrumStatus::NOT_FOUND) {
            return Ok(());
        }
        callback(
            PlayerRef::new(id, token),
            decode(response, submitted)?,
            token,
        )
    })
}

fn send_async<T, D>(
    ctx: &ModContext,
    player: PlayerRef<'_>,
    form: Value,
    submitted: D,
) -> Result<impl Future<Output = Result<FormOutcome<T>>> + Send + 'static + use<T, D>>
where
    T: Send + 'static,
    D: FnOnce(Value) -> Result<T> + 'static,
{
    let (sender, receiver) = oneshot::channel();
    register(ctx, player, form, move |_, response, _| {
        let _ = sender.send(decode(response, submitted));
        Ok(())
    })?;
    Ok(async move {
        receiver
            .await
            .map_err(|_| Error::new(FerrumStatus::MOD_DISABLED, "模组停用，表单等待已取消"))?
    })
}

struct CallbackEnv<F> {
    ctx: ModContextHandle,
    callback: Cell<Option<F>>,
}

fn register<F>(ctx: &ModContext, player: PlayerRef<'_>, form: Value, callback: F) -> Result<()>
where
    F: for<'tick> FnOnce(
            ActorRuntimeId,
            Result<FormOutcome<Value>>,
            &'tick MainThreadToken<'tick>,
        ) -> Result<()>
        + 'static,
{
    let send = host::form_api()?.send()?;
    let json = form.to_string();
    let ctx = ctx.raw_handle();
    let env = Box::into_raw(Box::new(CallbackEnv {
        ctx,
        callback: Cell::new(Some(callback)),
    }))
    .cast();
    let callback = FormCallbackV1 {
        env,
        invoke_fn: invoke::<F>,
        drop_fn: drop_callback::<F>,
    };
    let status = send(ctx, player.runtime_id(), FfiStr::new(&json), callback);
    if status.is_ok() {
        Ok(())
    } else {
        let error = host::error_from_status(status);
        drop_callback::<F>(env);
        Err(error)
    }
}

extern "C" fn invoke<F>(
    raw: *mut c_void,
    _: RawMainThreadToken,
    player: ActorRuntimeId,
    kind: FormResponseKind,
    json: FfiStr<'_>,
) -> FerrumStatus
where
    F: for<'tick> FnOnce(
        ActorRuntimeId,
        Result<FormOutcome<Value>>,
        &'tick MainThreadToken<'tick>,
    ) -> Result<()>,
{
    // SAFETY: 宿主在主线程派发，整个调用期间持有此 env。
    let env = unsafe { &*raw.cast::<CallbackEnv<F>>() };
    firewall(env.ctx, || {
        let callback = env
            .callback
            .take()
            .ok_or_else(|| Error::new(FerrumStatus::REENTRANT_CALL, "表单回调已经消费"))?;
        callback(
            player,
            parse_response(kind, json),
            &MainThreadToken::assume_main_thread(),
        )
    })
}

extern "C" fn drop_callback<F>(raw: *mut c_void) {
    // SAFETY: 发送失败时由 SDK 收回，成功后由宿主在主线程归还一次。
    let env = unsafe { Box::from_raw(raw.cast::<CallbackEnv<F>>()) };
    firewall(env.ctx, || {
        drop(env);
        Ok(())
    });
}

fn parse_response(kind: FormResponseKind, json: FfiStr<'_>) -> Result<FormOutcome<Value>> {
    let reason = match kind {
        FormResponseKind::SUBMITTED => {
            // SAFETY: 宿主保证响应 JSON 在本次回调期间可读。
            let value = serde_json::from_slice(unsafe { json.as_bytes() })
                .map_err(|error| invalid(format!("无效的表单响应 JSON：{error}")))?;
            if value != Value::Null {
                return Ok(FormOutcome::Submitted(value));
            }
            FormCancelReason::Unspecified
        }
        FormResponseKind::DISCONNECTED => {
            return Err(Error::new(
                FerrumStatus::NOT_FOUND,
                "玩家断线，表单等待已取消",
            ));
        }
        FormResponseKind::USER_CLOSED => FormCancelReason::UserClosed,
        FormResponseKind::USER_BUSY => FormCancelReason::UserBusy,
        FormResponseKind::CANCELLED => FormCancelReason::Unspecified,
        FormResponseKind(other) => FormCancelReason::Unknown(other),
    };
    Ok(FormOutcome::Cancelled(reason))
}
