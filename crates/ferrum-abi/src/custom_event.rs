use crate::{
    EventCallbackV1, EventPriority, EventScopeHandle, FerrumStatus, FfiBool, FfiByteWriter,
    FfiSlice, FfiStr, GenerationalHandle, ModContextHandle, PlayerUuid,
};

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CustomListenerHandle(pub GenerationalHandle);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct EventProviderHandle(pub GenerationalHandle);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CustomEventHandle(pub GenerationalHandle);

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EventValueKind(pub u32);

impl EventValueKind {
    pub const I64: Self = Self(0);
    pub const U64: Self = Self(1);
    pub const F64: Self = Self(2);
    pub const BOOL: Self = Self(3);
    pub const STRING: Self = Self(4);
    pub const BYTES: Self = Self(5);
    pub const UUID: Self = Self(6);
}

/// 冻结的字段描述；name 和 kind 在事件版本内不变。
#[repr(C)]
#[derive(Copy, Clone)]
pub struct EventFieldV1<'a> {
    pub name: FfiStr<'a>,
    pub kind: EventValueKind,
    pub reserved: u32,
}

#[repr(C)]
pub struct CustomEventDefinitionV1<'a> {
    pub struct_size: u32,
    /// 完整名称必须以发布者模组名加点号开头，如 economy.balance_changing.v1。
    pub name: FfiStr<'a>,
    pub cancellable: FfiBool,
    pub fields: FfiSlice<'a, EventFieldV1<'a>>,
}

/// 冻结的发布字段。I64 为二进制补码，F64 为 IEEE 754 位模式，BOOL 为零/非零。
/// STRING/BYTES/UUID 使用 bytes（UUID 恰为 16 字节），其余使用 scalar。
/// 所有借用数据在 publish 返回前有效；宿主在调用任何监听器前复制载荷。
#[repr(C)]
#[derive(Copy, Clone)]
pub struct EventValueV1<'a> {
    pub name: FfiStr<'a>,
    pub kind: EventValueKind,
    pub reserved: u32,
    pub scalar: u64,
    pub bytes: FfiSlice<'a, u8>,
}

/// 原生发布端协议。注册及发布均要求主线程；函数表由 loader 导出入口取得。
/// 关闭发布者立即撤销其全部事件；再次启用需重新注册。宿主不保存原生函数指针。
#[repr(C)]
pub struct NativeEventApiV1 {
    pub struct_size: u32,
    pub open_provider: extern "C" fn(FfiStr<'_>, *mut EventProviderHandle) -> FerrumStatus,
    pub close_provider: extern "C" fn(EventProviderHandle) -> FerrumStatus,
    pub register_event: extern "C" fn(
        EventProviderHandle,
        *const CustomEventDefinitionV1<'_>,
        *mut CustomEventHandle,
    ) -> FerrumStatus,
    pub unregister_event: extern "C" fn(EventProviderHandle, CustomEventHandle) -> FerrumStatus,
    /// OK 时完整写出取消结果；回调错误被隔离。发布者被重入撤销时返回 MOD_DISABLED。
    pub publish: extern "C" fn(
        EventProviderHandle,
        CustomEventHandle,
        FfiSlice<'_, EventValueV1<'_>>,
        *mut FfiBool,
    ) -> FerrumStatus,
    pub take_last_error: extern "C" fn(*mut FfiByteWriter) -> FerrumStatus,
}

/// Rust 订阅端；按名称订阅允许早于原生发布者注册，发布者重载后继续接收同名事件。
#[repr(C)]
pub struct CustomEventApiV1 {
    pub struct_size: u32,
    pub subscribe: extern "C" fn(
        ModContextHandle,
        FfiStr<'_>,
        EventPriority,
        EventCallbackV1,
        *mut CustomListenerHandle,
    ) -> FerrumStatus,
    pub unsubscribe: extern "C" fn(ModContextHandle, CustomListenerHandle) -> FerrumStatus,
    pub name: extern "C" fn(ModContextHandle, EventScopeHandle, *mut FfiByteWriter) -> FerrumStatus,
    pub get_i64:
        extern "C" fn(ModContextHandle, EventScopeHandle, FfiStr<'_>, *mut i64) -> FerrumStatus,
    pub get_u64:
        extern "C" fn(ModContextHandle, EventScopeHandle, FfiStr<'_>, *mut u64) -> FerrumStatus,
    pub get_f64:
        extern "C" fn(ModContextHandle, EventScopeHandle, FfiStr<'_>, *mut f64) -> FerrumStatus,
    pub get_bool:
        extern "C" fn(ModContextHandle, EventScopeHandle, FfiStr<'_>, *mut FfiBool) -> FerrumStatus,
    pub get_string: extern "C" fn(
        ModContextHandle,
        EventScopeHandle,
        FfiStr<'_>,
        *mut FfiByteWriter,
    ) -> FerrumStatus,
    pub get_bytes: extern "C" fn(
        ModContextHandle,
        EventScopeHandle,
        FfiStr<'_>,
        *mut FfiByteWriter,
    ) -> FerrumStatus,
    pub get_uuid: extern "C" fn(
        ModContextHandle,
        EventScopeHandle,
        FfiStr<'_>,
        *mut PlayerUuid,
    ) -> FerrumStatus,
    pub is_cancelled:
        extern "C" fn(ModContextHandle, EventScopeHandle, *mut FfiBool) -> FerrumStatus,
    pub cancel: extern "C" fn(ModContextHandle, EventScopeHandle) -> FerrumStatus,
}

/// Rust 自定义事件发布端，独立查询 ferrum.custom_event_publisher.v1。
#[repr(C)]
pub struct CustomEventPublisherApiV1 {
    pub struct_size: u32,
    /// 注册到当前 Rust 模组名下；启用失败、停用与卸载时自动撤销。
    pub register_event: extern "C" fn(
        ModContextHandle,
        *const CustomEventDefinitionV1<'_>,
        *mut CustomEventHandle,
    ) -> FerrumStatus,
    pub unregister_event: extern "C" fn(ModContextHandle, CustomEventHandle) -> FerrumStatus,
    /// 同步派发，OK 时写出取消结果。仅已启用模组可以发布。
    pub publish: extern "C" fn(
        ModContextHandle,
        CustomEventHandle,
        FfiSlice<'_, EventValueV1<'_>>,
        *mut FfiBool,
    ) -> FerrumStatus,
}

/// shim 与原生 LL 监听器的作用域访问表；不包含游戏对象指针。
/// 作用域只在同步 publish 期间有效，保存后再次访问返回 STALE_HANDLE。
#[repr(C)]
pub struct CustomEventViewApiV1 {
    pub struct_size: u32,
    pub name: extern "C" fn(EventScopeHandle, *mut FfiByteWriter) -> FerrumStatus,
    pub get_i64: extern "C" fn(EventScopeHandle, FfiStr<'_>, *mut i64) -> FerrumStatus,
    pub get_u64: extern "C" fn(EventScopeHandle, FfiStr<'_>, *mut u64) -> FerrumStatus,
    pub get_f64: extern "C" fn(EventScopeHandle, FfiStr<'_>, *mut f64) -> FerrumStatus,
    pub get_bool: extern "C" fn(EventScopeHandle, FfiStr<'_>, *mut FfiBool) -> FerrumStatus,
    pub get_string: extern "C" fn(EventScopeHandle, FfiStr<'_>, *mut FfiByteWriter) -> FerrumStatus,
    pub get_bytes: extern "C" fn(EventScopeHandle, FfiStr<'_>, *mut FfiByteWriter) -> FerrumStatus,
    pub get_uuid: extern "C" fn(EventScopeHandle, FfiStr<'_>, *mut PlayerUuid) -> FerrumStatus,
    pub is_cancelled: extern "C" fn(EventScopeHandle, *mut FfiBool) -> FerrumStatus,
    pub cancel: extern "C" fn(EventScopeHandle) -> FerrumStatus,
    pub is_active: extern "C" fn(EventScopeHandle) -> FerrumStatus,
}
