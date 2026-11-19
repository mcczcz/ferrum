use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::MaybeUninit;

use ferrum_abi::{FerrumStatus, ModContextHandle, RawMainThreadToken, TaskCallbackV1};
use futures_channel::oneshot;

use crate::firewall::firewall;
use crate::{Error, MainThreadToken, Result, host};

pub use ferrum_abi::TaskHandle;

#[derive(Clone, Copy, Debug)]
pub struct Scheduler {
    pub(crate) ctx: ModContextHandle,
}

impl Scheduler {
    /// 从任意线程投递一次，在下一 Tick 执行。
    pub fn post<F>(self, f: F) -> Result<TaskHandle>
    where
        F: for<'t> FnOnce(&'t MainThreadToken<'t>) -> Result<()> + Send + 'static,
    {
        let post = host::scheduler_api()?.post()?;
        self.register(once(f), |cb, out| post(self.ctx, cb, out))
    }

    /// 延迟 ticks 个游戏 Tick，零延迟也在下一 Tick 执行。
    pub fn after_ticks<F>(
        self,
        _token: &MainThreadToken<'_>,
        ticks: u64,
        f: F,
    ) -> Result<TaskHandle>
    where
        F: for<'t> FnOnce(&'t MainThreadToken<'t>) -> Result<()> + 'static,
    {
        let after = host::scheduler_api()?.after_ticks()?;
        self.register(once(f), |cb, out| after(self.ctx, ticks, cb, out))
    }

    /// 每隔 interval 个 Tick 执行。回调返回错误或 panic 时结束任务。
    pub fn every_ticks<F>(
        self,
        _token: &MainThreadToken<'_>,
        interval: u64,
        f: F,
    ) -> Result<TaskHandle>
    where
        F: for<'t> FnMut(&'t MainThreadToken<'t>) -> Result<()> + 'static,
    {
        let every = host::scheduler_api()?.every_ticks()?;
        self.register(f, |cb, out| every(self.ctx, interval, cb, out))
    }

    pub fn cancel(self, _token: &MainThreadToken<'_>, task: TaskHandle) -> Result<()> {
        host::check((host::scheduler_api()?.cancel()?)(self.ctx, task))
    }

    /// 在主线程运行闭包，将结果交回调用方的异步运行时。停用时唤醒等待方。
    pub async fn on_main_thread<T, F>(self, f: F) -> Result<T>
    where
        F: for<'t> FnOnce(&'t MainThreadToken<'t>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let (sender, receiver) = oneshot::channel();
        self.post(move |token| {
            if sender.is_canceled() {
                return Ok(());
            }
            let mut result = None;
            let status = firewall(self.ctx, || {
                result = Some(f(token));
                Ok(())
            });
            let _ = sender
                .send(result.unwrap_or_else(|| Err(Error::new(status, "主线程任务发生 panic"))));
            Ok(())
        })?;
        receiver
            .await
            .map_err(|_| Error::new(FerrumStatus::MOD_DISABLED, "模组停用，主线程任务已取消"))?
    }

    fn register<F>(
        self,
        f: F,
        register: impl FnOnce(TaskCallbackV1, *mut TaskHandle) -> FerrumStatus,
    ) -> Result<TaskHandle>
    where
        F: for<'t> FnMut(&'t MainThreadToken<'t>) -> Result<()> + 'static,
    {
        let env = Box::into_raw(Box::new(TaskEnv {
            ctx: self.ctx,
            callback: RefCell::new(f),
        }))
        .cast();
        let cb = TaskCallbackV1 {
            env,
            invoke_fn: invoke::<F>,
            drop_fn: drop_task::<F>,
        };
        let mut out = MaybeUninit::uninit();
        let status = register(cb, out.as_mut_ptr());
        if !status.is_ok() {
            let error = host::error_from_status(status);
            drop_task::<F>(env);
            return Err(error);
        }
        // SAFETY: 成功注册后宿主填入完整任务句柄并接管回调。
        Ok(unsafe { out.assume_init() })
    }
}

fn once<F>(f: F) -> impl for<'t> FnMut(&'t MainThreadToken<'t>) -> Result<()>
where
    F: for<'t> FnOnce(&'t MainThreadToken<'t>) -> Result<()>,
{
    let mut f = Some(f);
    move |token| f.take().expect("one-shot task invoked twice")(token)
}

struct TaskEnv<F> {
    ctx: ModContextHandle,
    callback: RefCell<F>,
}

extern "C" fn invoke<F>(raw: *mut c_void, _: RawMainThreadToken) -> FerrumStatus
where
    F: for<'t> FnMut(&'t MainThreadToken<'t>) -> Result<()>,
{
    // SAFETY: 宿主仅在主线程派发，调用期间持有 env 的强引用。
    let env = unsafe { &*raw.cast::<TaskEnv<F>>() };
    firewall(env.ctx, || {
        let mut callback = env
            .callback
            .try_borrow_mut()
            .map_err(|_| Error::new(FerrumStatus::REENTRANT_CALL, "任务回调重入"))?;
        callback(&MainThreadToken::assume_main_thread())
    })
}

extern "C" fn drop_task<F>(raw: *mut c_void) {
    // SAFETY: 注册失败时在提交线程回收，否则宿主在主线程仅归还一次。
    let env = unsafe { Box::from_raw(raw.cast::<TaskEnv<F>>()) };
    firewall(env.ctx, || {
        drop(env);
        Ok(())
    });
}
