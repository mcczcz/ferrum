// 纳入 domain 模块，共享命令与事件的主线程资源和活跃回调计数。
pub(super) fn install_custom_events() {
    fn begin(ctx: ModContextHandle) -> FerrumStatus {
        match with_mod(ctx, |m| m.state == MockModState::Enabled) {
            Ok(true) => {
                DOMAIN.with(|d| *d.borrow_mut().active.entry(ctx.0.to_raw()).or_default() += 1);
                FerrumStatus::OK
            }
            Ok(false) => FerrumStatus::MOD_DISABLED,
            Err(s) => s,
        }
    }
    fn finish(ctx: ModContextHandle) {
        let pending = DOMAIN.with(|d| {
            let mut d = d.borrow_mut();
            let count = d.active.entry(ctx.0.to_raw()).or_default();
            *count -= 1;
            let pending = *count == 0 && d.pending.contains(&ctx);
            if pending {
                d.pending.retain(|c| *c != ctx);
            }
            pending
        });
        if pending {
            let _ = super::finish_disable_ctx(ctx);
        }
    }
    super::custom_event_core::install(super::custom_event_core::Hooks {
        main_thread: || is_main_thread().get(),
        can_subscribe: |ctx| match enabled(ctx) {
            Ok(()) => FerrumStatus::OK,
            Err(s) => s,
        },
        begin,
        finish,
        track_listener: |_, _| {},
        track_provider: |_, _| {},
        publisher_name: |ctx| with_mod(ctx, |m| m.name.clone()),
        enabled: |ctx| with_mod(ctx, |m| m.state == MockModState::Enabled) == Ok(true),
        publish_ll: None,
        attach: |_, _, _, _| FerrumStatus::OK,
        detach: |_| {},
        log: |ctx, level, text| {
            super::log(ctx, level, FfiStr::new(text));
        },
        set_error: |text| super::LAST_ERROR.with(|s| *s.borrow_mut() = Some(text)),
        take_error: || super::LAST_ERROR.with(|s| s.borrow_mut().take()),
    });
}

impl MockHost {
    /// 与原生模组相同的 C ABI 发布协议；mock 不查询真实 LL 模组身份或启用状态。
    pub fn native_event_api(&self) -> &'static NativeEventApiV1 {
        &super::custom_event_core::NATIVE_API
    }
}

extern "C" fn subscribe_event(
    ctx: ModContextHandle,
    sub: *const EventSubscriptionV1,
    callback: EventCallbackV1,
    out: *mut ListenerHandle,
) -> FerrumStatus {
    guard(|| {
        if let Err(s) = enabled(ctx) {
            return s;
        }
        if sub.is_null() || out.is_null() {
            return FerrumStatus::NULL_POINTER;
        }
        // SAFETY: SDK 提供至少包含 struct_size 的可读头。
        if unsafe { (*sub).struct_size } < size_of::<EventSubscriptionV1>() as u32 {
            return FerrumStatus::ABI_INCOMPATIBLE;
        }
        // SAFETY: 结构头已校验，SDK 保证本次同步调用内可读。
        let sub = unsafe { &*sub };
        if !matches!(
            sub.kind,
            EventKind::PLAYER_JOIN | EventKind::PLAYER_DISCONNECT | EventKind::PLAYER_CHAT
        ) {
            return FerrumStatus::UNSUPPORTED;
        }
        if !matches!(
            sub.priority,
            EventPriority::HIGHEST
                | EventPriority::HIGH
                | EventPriority::NORMAL
                | EventPriority::LOW
                | EventPriority::LOWEST
        ) {
            return FerrumStatus::INVALID_ARGUMENT;
        }
        DOMAIN.with(|d| {
            let mut d = d.borrow_mut();
            let h = ListenerHandle(serial(&mut d));
            d.listeners.insert(
                h.0.to_raw(),
                Rc::new(Listener {
                    owner: ctx,
                    kind: sub.kind,
                    priority: sub.priority,
                    callback,
                    warned_reentrant: Cell::new(false),
                }),
            );
            put(out, h)
        })
    })
}
extern "C" fn unsubscribe_event(ctx: ModContextHandle, h: ListenerHandle) -> FerrumStatus {
    guard(|| {
        if let Err(s) = with_mod(ctx, |_| ()) {
            return s;
        }
        let removed = DOMAIN.with(|d| {
            let mut d = d.borrow_mut();
            match d.listeners.get(&h.0.to_raw()) {
                Some(l) if l.owner != ctx => Err(FerrumStatus::PERMISSION_DENIED),
                None => Err(FerrumStatus::STALE_HANDLE),
                _ => Ok(d.listeners.remove(&h.0.to_raw())),
            }
        });
        match removed {
            Ok(l) => {
                drop(l);
                FerrumStatus::OK
            }
            Err(s) => s,
        }
    })
}
fn event_scope<T>(
    ctx: ModContextHandle,
    h: EventScopeHandle,
    f: impl FnOnce(&mut EventScope) -> Result<T, FerrumStatus>,
) -> Result<T, FerrumStatus> {
    DOMAIN.with(|d| {
        let mut d = d.borrow_mut();
        match d.event_scopes.get_mut(&h.0.to_raw()) {
            Some(s) if s.owner == ctx => f(s),
            Some(_) => Err(FerrumStatus::PERMISSION_DENIED),
            None => Err(FerrumStatus::STALE_HANDLE),
        }
    })
}
extern "C" fn event_player(
    c: ModContextHandle,
    h: EventScopeHandle,
    out: *mut ActorRuntimeId,
) -> FerrumStatus {
    guard(
        || match event_scope(c, h, |s| Ok(s.player)).and_then(|id| player(id, |_| id)) {
            Ok(id) => put(out, id),
            Err(s) => s,
        },
    )
}
extern "C" fn event_cancelled(
    c: ModContextHandle,
    h: EventScopeHandle,
    out: *mut FfiBool,
) -> FerrumStatus {
    guard(|| {
        match event_scope(c, h, |s| {
            if s.kind == EventKind::PLAYER_DISCONNECT {
                Err(FerrumStatus::INVALID_ARGUMENT)
            } else {
                Ok(s.cancelled)
            }
        }) {
            Ok(v) => put(out, v.into()),
            Err(s) => s,
        }
    })
}
extern "C" fn event_cancel(c: ModContextHandle, h: EventScopeHandle) -> FerrumStatus {
    guard(|| {
        match event_scope(c, h, |s| {
            if s.kind == EventKind::PLAYER_DISCONNECT {
                return Err(FerrumStatus::INVALID_ARGUMENT);
            }
            s.cancelled = true;
            Ok(())
        }) {
            Ok(()) => FerrumStatus::OK,
            Err(s) => s,
        }
    })
}
extern "C" fn event_message(
    c: ModContextHandle,
    h: EventScopeHandle,
    out: *mut FfiByteWriter,
) -> FerrumStatus {
    guard(|| {
        match event_scope(c, h, |s| {
            s.message.clone().ok_or(FerrumStatus::INVALID_ARGUMENT)
        }) {
            Ok(text) => write(out, text.as_bytes()),
            Err(s) => s,
        }
    })
}
pub(super) static EVENT: EventApiV1 = EventApiV1 {
    struct_size: size_of::<EventApiV1>() as u32,
    subscribe: subscribe_event,
    unsubscribe: unsubscribe_event,
    player: event_player,
    is_cancelled: event_cancelled,
    cancel: event_cancel,
    chat_message: event_message,
};

impl MockHost {
    /// 返回事件是否被取消；不自动添加玩家或改变在线状态。
    pub fn emit_player_join(&self, player: ActorRuntimeId) -> Result<bool, Error> {
        self.emit_event(EventKind::PLAYER_JOIN, player, None)
    }
    /// 在移除 mock 玩家之前调用；不自动改变在线状态。
    pub fn emit_player_disconnect(&self, player: ActorRuntimeId) -> Result<(), Error> {
        self.emit_event(EventKind::PLAYER_DISCONNECT, player, None)
            .map(|_| ())
    }
    /// 返回聊天是否被取消；不模拟消息广播。
    pub fn emit_player_chat(
        &self,
        player: ActorRuntimeId,
        message: impl Into<String>,
    ) -> Result<bool, Error> {
        self.emit_event(EventKind::PLAYER_CHAT, player, Some(message.into()))
    }
    fn emit_event(
        &self,
        kind: EventKind,
        player_id: ActorRuntimeId,
        message: Option<String>,
    ) -> Result<bool, Error> {
        check(guard(|| FerrumStatus::OK))?;
        player(player_id, |_| ()).map_err(|s| Error::new(s, "mock 玩家不存在"))?;
        let mut snapshot = DOMAIN.with(|d| {
            d.borrow()
                .listeners
                .iter()
                .filter(|(_, l)| l.kind == kind)
                .map(|(h, l)| (l.priority.0, *h))
                .collect::<Vec<_>>()
        });
        snapshot.sort_unstable();
        for (_, raw) in snapshot {
            let listener = DOMAIN.with(|d| d.borrow().listeners.get(&raw).cloned());
            let Some(listener) = listener else {
                continue;
            };
            if with_mod(listener.owner, |m| m.state == MockModState::Enabled) != Ok(true) {
                continue;
            }
            let h = DOMAIN.with(|d| {
                let mut d = d.borrow_mut();
                let h = EventScopeHandle(serial(&mut d));
                d.event_scopes.insert(
                    h.0.to_raw(),
                    EventScope {
                        owner: listener.owner,
                        kind,
                        player: player_id,
                        message: message.clone(),
                        cancelled: false,
                    },
                );
                *d.active.entry(listener.owner.0.to_raw()).or_default() += 1;
                h
            });
            let status = (listener.callback.invoke_fn)(
                listener.callback.env,
                h,
                RawMainThreadToken::ASSERTED,
            );
            let cancelled = DOMAIN.with(|d| {
                d.borrow_mut()
                    .event_scopes
                    .remove(&h.0.to_raw())
                    .expect("active mock event scope")
                    .cancelled
            });
            if !status.is_ok() {
                let diagnostic = match status {
                    // mock 的 report_panic 已保存详情，与真实宿主一样不重复记录。
                    FerrumStatus::MOD_PANICKED => None,
                    FerrumStatus::REENTRANT_CALL => {
                        if listener.warned_reentrant.replace(true) {
                            None
                        } else {
                            Some((LogLevel::WARN, "事件回调重入，已跳过本次调用".to_owned()))
                        }
                    }
                    FerrumStatus::CALLBACK_POISONED => {
                        Some((LogLevel::ERROR, "事件回调已被毒化，移除监听器".to_owned()))
                    }
                    _ => {
                        let detail = super::LAST_ERROR
                            .with(|s| s.borrow_mut().take())
                            .unwrap_or_default();
                        let text = if detail.is_empty() {
                            format!("事件回调 {status}")
                        } else {
                            format!("事件回调 {status}: {detail}")
                        };
                        Some((LogLevel::ERROR, text))
                    }
                };
                if let Some((level, text)) = diagnostic {
                    super::log(listener.owner, level, FfiStr::new(&text));
                }
                if status == FerrumStatus::MOD_PANICKED || status == FerrumStatus::CALLBACK_POISONED
                {
                    let removed = DOMAIN.with(|d| d.borrow_mut().listeners.remove(&raw));
                    drop(removed);
                }
            }
            let owner = listener.owner;
            drop(listener);
            let pending = DOMAIN.with(|d| {
                let mut d = d.borrow_mut();
                let active = d.active.entry(owner.0.to_raw()).or_default();
                *active -= 1;
                let pending = *active == 0 && d.pending.contains(&owner);
                if pending {
                    d.pending.retain(|c| *c != owner);
                }
                pending
            });
            if pending {
                let _ = self.finish_disable(&super::MockMod { ctx: owner });
            }
            if cancelled {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
