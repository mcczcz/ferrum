use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use ferrum_abi::*;

use crate::arena::Arena;
use crate::loader::Invocation;
use crate::runtime::{self, ModState};
use crate::{last_error, shim, vtable};

struct Callback(TaskCallbackV1);

// SAFETY: 各线程只在队列中交接回调，调用和最后一个引用的释放均在主线程。
unsafe impl Send for Callback {}
// SAFETY: 后台投递不访问 env，主线程串行调用，派发期间持有独立强引用。
unsafe impl Sync for Callback {}

impl Drop for Callback {
    fn drop(&mut self) {
        (self.0.drop_fn)(self.0.env);
    }
}

struct Task {
    owner: ModContextHandle,
    callback: Arc<Callback>,
    deadline: (u64, u64),
    interval: u64,
}

struct Queue {
    tick: u64,
    next_sequence: u64,
    tasks: Arena<Task>,
    deadlines: BTreeMap<(u64, u64), GenerationalHandle>,
}

impl Queue {
    fn deadline(&mut self, due: u64) -> (u64, u64) {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        (due, sequence)
    }

    fn remove(&mut self, id: GenerationalHandle) -> Option<Task> {
        let task = self.tasks.remove(id)?;
        self.deadlines.remove(&task.deadline);
        Some(task)
    }
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue {
    tick: 0,
    next_sequence: 0,
    tasks: Arena::new(),
    deadlines: BTreeMap::new(),
});

fn enqueue(
    ctx: ModContextHandle,
    delay: u64,
    interval: u64,
    cb: TaskCallbackV1,
    out: *mut TaskHandle,
) -> FerrumStatus {
    if out.is_null() {
        return FerrumStatus::NULL_POINTER;
    }
    let entry = match runtime::lookup(ctx) {
        Ok(entry) => entry,
        Err(status) => return status,
    };
    let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    // 在队列锁内检查，确保与停用时的 revoke 串行。
    if !matches!(entry.state(), ModState::Enabling | ModState::Enabled) {
        return FerrumStatus::MOD_DISABLED;
    }
    let Some(due) = queue.tick.checked_add(delay.max(1)) else {
        return FerrumStatus::INVALID_ARGUMENT;
    };
    let deadline = queue.deadline(due);
    let id = queue.tasks.insert(Task {
        owner: ctx,
        callback: Arc::new(Callback(cb)),
        deadline,
        interval,
    });
    queue.deadlines.insert(deadline, id);
    // SAFETY: ABI 调用方提供可写的句柄输出位置，且已检查非空。
    unsafe { out.write(TaskHandle(id)) };
    FerrumStatus::OK
}

extern "C" fn post(
    ctx: ModContextHandle,
    cb: TaskCallbackV1,
    out: *mut TaskHandle,
) -> FerrumStatus {
    vtable::guard(|| enqueue(ctx, 1, 0, cb, out))
}

extern "C" fn after_ticks(
    ctx: ModContextHandle,
    delay: u64,
    cb: TaskCallbackV1,
    out: *mut TaskHandle,
) -> FerrumStatus {
    vtable::guard(|| {
        if !vtable::is_main_thread() {
            return FerrumStatus::WRONG_THREAD;
        }
        enqueue(ctx, delay, 0, cb, out)
    })
}

extern "C" fn every_ticks(
    ctx: ModContextHandle,
    interval: u64,
    cb: TaskCallbackV1,
    out: *mut TaskHandle,
) -> FerrumStatus {
    vtable::guard(|| {
        if !vtable::is_main_thread() {
            return FerrumStatus::WRONG_THREAD;
        }
        if interval == 0 {
            return FerrumStatus::INVALID_ARGUMENT;
        }
        enqueue(ctx, interval, interval, cb, out)
    })
}

extern "C" fn cancel(ctx: ModContextHandle, handle: TaskHandle) -> FerrumStatus {
    vtable::guard(|| {
        if !vtable::is_main_thread() {
            return FerrumStatus::WRONG_THREAD;
        }
        let removed = {
            let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
            match queue.tasks.get(handle.0) {
                None => return FerrumStatus::STALE_HANDLE,
                Some(task) if task.owner != ctx => return FerrumStatus::PERMISSION_DENIED,
                _ => queue.remove(handle.0),
            }
        };
        drop(removed);
        FerrumStatus::OK
    })
}

pub(crate) fn revoke(ctx: ModContextHandle) {
    let removed = {
        let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
        let ids: Vec<_> = queue
            .tasks
            .iter()
            .filter(|(_, task)| task.owner == ctx)
            .map(|(id, _)| id)
            .collect();
        ids.into_iter()
            .filter_map(|id| queue.remove(id))
            .collect::<Vec<_>>()
    };
    drop(removed);
}

fn dispatch(id: GenerationalHandle) {
    // 逆序析构：最后一个回调强引用在 Invocation 触发延迟停用前释放。
    let invocation;
    let (owner, callback) = {
        let queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
        let Some(task) = queue.tasks.get(id) else {
            return;
        };
        (task.owner, task.callback.clone())
    };
    let Ok(entry) = runtime::lookup(owner) else {
        return;
    };
    invocation = Invocation::begin(owner, entry.clone());
    let status = match &invocation {
        Ok(_) => (callback.0.invoke_fn)(callback.0.env, RawMainThreadToken::ASSERTED),
        Err(status) => *status,
    };
    if !matches!(
        status,
        FerrumStatus::OK | FerrumStatus::MOD_DISABLED | FerrumStatus::MOD_PANICKED
    ) {
        let detail = last_error::take().unwrap_or_default();
        if let Some(shim) = shim::get() {
            (shim.log)(
                entry.info.cookie,
                LogLevel::ERROR,
                FfiStr::new(&format!("调度回调返回 {status}: {detail}")),
            );
        }
    }
    let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    let now = queue.tick;
    let Some(task) = queue.tasks.get(id) else {
        return;
    };
    if status.is_ok() && task.interval != 0 {
        let due = now.saturating_add(task.interval);
        let deadline = queue.deadline(due);
        queue.tasks.get_mut(id).unwrap().deadline = deadline;
        queue.deadlines.insert(deadline, id);
    } else {
        // callback 强引用仍在栈上，移出记录后在锁外析构。
        queue.remove(id);
    }
}

pub(crate) fn tick() -> FerrumStatus {
    if !vtable::is_main_thread() {
        return FerrumStatus::WRONG_THREAD;
    }
    let ready = {
        let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
        queue.tick += 1;
        let mut ready = Vec::new();
        while let Some((&(due, _), &id)) = queue.deadlines.first_key_value() {
            if due > queue.tick {
                break;
            }
            queue.deadlines.pop_first();
            ready.push(id);
        }
        ready
    };
    for id in ready {
        dispatch(id);
    }
    FerrumStatus::OK
}

pub(crate) static API: SchedulerApiV1 = SchedulerApiV1 {
    struct_size: size_of::<SchedulerApiV1>() as u32,
    post,
    after_ticks,
    every_ticks,
    cancel,
};
