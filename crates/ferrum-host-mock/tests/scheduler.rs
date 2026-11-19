use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::task::{Context, Poll, Wake, Waker};

use ferrum_abi as abi;
use ferrum_host_mock::{MockHost, MockMod};
use ferrum_sdk::prelude::*;

thread_local! {
    static SCHEDULER: Cell<Option<Scheduler>> = const { Cell::new(None) };
}

struct TestMod;

#[ferrum_sdk::mod_entry]
impl FerrumMod for TestMod {
    fn on_load(ctx: &ModContext) -> Result<Self> {
        SCHEDULER.set(Some(ctx.scheduler()));
        Ok(Self)
    }
}

fn enabled() -> (MockHost, MockMod, Scheduler) {
    let host = MockHost::new();
    let module = host.load::<TestMod>().unwrap();
    host.enable(&module).unwrap();
    (host, module, SCHEDULER.take().unwrap())
}

#[test]
fn post_runs_next_tick_in_submission_order_after_slot_reuse() {
    let (host, _, scheduler) = enabled();
    let (sent, received) = mpsc::channel();
    // 先腾出一个槽位，再投递足够多的任务以同时覆盖复用槽与新槽。
    scheduler.post(|_| Ok(())).unwrap();
    host.tick().unwrap();
    std::thread::spawn(move || {
        for value in 0..32 {
            let sent = sent.clone();
            scheduler
                .post(move |_| {
                    sent.send(value).unwrap();
                    Ok(())
                })
                .unwrap();
        }
    })
    .join()
    .unwrap();
    assert!(received.try_recv().is_err());
    host.tick().unwrap();
    assert_eq!(
        received.try_iter().collect::<Vec<_>>(),
        (0..32).collect::<Vec<_>>()
    );
}

#[test]
fn delayed_and_repeating_tasks_share_fifo_deadlines_and_stop_on_error() {
    let (host, module, scheduler) = enabled();
    let (sent, received) = mpsc::channel();
    scheduler
        .post(move |token| {
            let repeated = sent.clone();
            let mut runs = 0;
            scheduler.every_ticks(token, 1, move |_| {
                repeated.send("repeat").unwrap();
                runs += 1;
                if runs == 2 {
                    return Err(Error::msg("stop repeating"));
                }
                Ok(())
            })?;
            for (delay, label) in [(0, "zero"), (2, "two")] {
                let sent = sent.clone();
                scheduler.after_ticks(token, delay, move |_| {
                    sent.send(label).unwrap();
                    Ok(())
                })?;
            }
            Ok(())
        })
        .unwrap();
    host.tick().unwrap();
    assert!(received.try_recv().is_err());
    host.tick().unwrap();
    assert_eq!(received.try_iter().collect::<Vec<_>>(), ["repeat", "zero"]);
    host.tick().unwrap();
    assert_eq!(received.try_iter().collect::<Vec<_>>(), ["two", "repeat"]);
    host.tick().unwrap();
    assert!(received.try_recv().is_err());
    assert!(
        host.logs(&module)
            .iter()
            .any(|log| log.message.contains("stop repeating"))
    );
}

#[test]
fn repeating_task_can_cancel_itself() {
    let (host, _, scheduler) = enabled();
    let (sent, received) = mpsc::channel();
    scheduler
        .post(move |token| {
            let handle = Rc::new(Cell::new(None));
            let own_handle = handle.clone();
            handle.set(Some(scheduler.every_ticks(token, 1, move |token| {
                scheduler.cancel(token, own_handle.get().unwrap())?;
                sent.send(()).unwrap();
                Ok(())
            })?));
            Ok(())
        })
        .unwrap();
    host.tick().unwrap();
    host.tick().unwrap();
    host.tick().unwrap();
    assert_eq!(received.try_iter().count(), 1);
}

struct WakeFlag(AtomicBool);

impl Wake for WakeFlag {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[test]
fn disable_wakes_pending_main_thread_future_and_revokes_task() {
    let (host, module, scheduler) = enabled();
    let (sent, received) = mpsc::channel();
    let mut future = Box::pin(scheduler.on_main_thread(move |_| {
        sent.send(()).unwrap();
        Ok(())
    }));
    let wake = Arc::new(WakeFlag(AtomicBool::new(false)));
    let waker = Waker::from(wake.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(future.as_mut().poll(&mut cx).is_pending());
    host.disable(&module).unwrap();
    assert!(wake.0.load(Ordering::SeqCst));
    let Poll::Ready(Err(error)) = future.as_mut().poll(&mut cx) else {
        panic!("停用后应立即返回错误");
    };
    assert_eq!(error.kind(), ErrorKind::ModDisabled);
    host.enable(&module).unwrap();
    host.tick().unwrap();
    assert_eq!(received.try_recv(), Err(mpsc::TryRecvError::Disconnected));
}

#[test]
fn periodic_panic_logs_one_error_and_stops() {
    let (host, module, scheduler) = enabled();
    scheduler
        .post(move |token| {
            scheduler.every_ticks(token, 1, |_| panic!("scheduler panic"))?;
            Ok(())
        })
        .unwrap();
    host.tick().unwrap();
    host.tick().unwrap();
    host.tick().unwrap();
    assert_eq!(host.panics(&module).len(), 1);
    let errors: Vec<_> = host
        .logs(&module)
        .into_iter()
        .filter(|log| log.level == Level::Error)
        .collect();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("scheduler panic"));
}

// 在同一进程加载第二个 ABI 模组。它只使用宿主子表，无需 SDK 生命周期实例。
struct OtherMod;

impl FerrumMod for OtherMod {
    fn on_load(_: &ModContext) -> Result<Self> {
        Ok(Self)
    }
}

impl ferrum_sdk::ModEntryPoint for OtherMod {
    const ENTRY: abi::ModEntryFnV1 = other_entry;
}

unsafe extern "C" fn other_entry(
    _: *const abi::FerrumHostApi,
    _: abi::ModContextHandle,
    out: *mut abi::FerrumModDescriptor,
) -> abi::FerrumStatus {
    extern "C" fn load(_: abi::ModContextHandle) -> abi::FerrumStatus {
        abi::FerrumStatus::OK
    }
    // SAFETY: 此测试使用同版本 MockHost 提供的完整描述符缓冲区。
    unsafe {
        out.write(abi::FerrumModDescriptor {
            struct_size: size_of::<abi::FerrumModDescriptor>() as u32,
            flags: abi::FerrumModFlags::NONE,
            required_abi_version: abi::AbiVersion::CURRENT,
            sdk_version: ferrum_sdk::SDK_VERSION,
            on_load: load,
            on_enable: None,
            on_disable: None,
            on_unload: None,
        })
    };
    abi::FerrumStatus::OK
}

#[test]
fn another_enabled_mod_cannot_cancel_the_owners_task() {
    let (host, _, scheduler) = enabled();
    let other = host.load::<OtherMod>().unwrap();
    host.enable(&other).unwrap();
    let (sent, received) = mpsc::channel();
    let task = scheduler
        .post(move |_| {
            sent.send(()).unwrap();
            Ok(())
        })
        .unwrap();
    let api = ferrum_host::vtable::host_api();
    let table = (api.query_interface)("ferrum.scheduler.v1".into(), 0);
    assert!(!table.is_null());
    // SAFETY: 此接口在本测试的 ABI 1.2 宿主中指向完整的静态 SchedulerApiV1。
    let scheduler_api = unsafe { &*table.cast::<abi::SchedulerApiV1>() };
    assert_eq!(
        (scheduler_api.cancel)(other.context_handle(), task),
        abi::FerrumStatus::PERMISSION_DENIED
    );
    host.tick().unwrap();
    received.try_recv().unwrap();
}
