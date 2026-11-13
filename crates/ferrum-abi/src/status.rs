/// 跨 FFI 的统一状态码（`0` 为成功）。
///
/// 取值一经发布永久冻结；解码方必须容忍未知值。
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct FerrumStatus(pub i32);

impl FerrumStatus {
    pub const OK: Self = Self(0);

    // 通用参数与契约错误 (1..=99)
    pub const INVALID_ARGUMENT: Self = Self(1);
    pub const NULL_POINTER: Self = Self(2);
    pub const INVALID_UTF8: Self = Self(3);
    pub const BUFFER_TOO_SMALL: Self = Self(4);
    pub const OUT_OF_MEMORY: Self = Self(5);
    /// 宿主不支持该函数/字段，或游戏特性已移除
    pub const UNSUPPORTED: Self = Self(6);
    /// 一般性失败（如模组生命周期钩子返回的自定义错误），详情见 last-error
    pub const FAILED: Self = Self(7);

    // ABI 与生命周期状态错误 (100..=199)
    pub const ABI_INCOMPATIBLE: Self = Self(100);
    /// 模组已停用、停用中或已卸载
    pub const MOD_DISABLED: Self = Self(101);
    /// 违反主线程约束
    pub const WRONG_THREAD: Self = Self(102);
    /// `FnMut` 回调重入被守卫拦截
    pub const REENTRANT_CALL: Self = Self(103);

    // 句柄与游戏对象错误 (200..=299)
    /// 代数不匹配或已销毁
    pub const STALE_HANDLE: Self = Self(200);
    /// 跨模组越权操作他人句柄
    pub const PERMISSION_DENIED: Self = Self(201);
    /// 实体/玩家/维度/服务不存在或已离线
    pub const NOT_FOUND: Self = Self(202);
    /// Level 或 CommandRegistry 尚未初始化
    pub const SERVICE_UNAVAILABLE: Self = Self(203);
    /// 重复注册同名命令/枚举/服务
    pub const ALREADY_EXISTS: Self = Self(204);

    // 异常与故障隔离码 (500..=599)
    /// C++ 侧捕获到异常或 Expected 错误
    pub const CPP_EXCEPTION: Self = Self(500);
    /// 模组侧发生 Rust Panic
    pub const MOD_PANICKED: Self = Self(501);
    /// 回调此前 panic 过，已被毒化
    pub const CALLBACK_POISONED: Self = Self(502);
    pub const INTERNAL_ERROR: Self = Self(599);

    #[inline]
    pub const fn is_ok(self) -> bool {
        self.0 == Self::OK.0
    }

    /// 已知状态码的稳定名称；未知值返回 `None`。
    pub const fn name(self) -> Option<&'static str> {
        Some(match self.0 {
            0 => "OK",
            1 => "INVALID_ARGUMENT",
            2 => "NULL_POINTER",
            3 => "INVALID_UTF8",
            4 => "BUFFER_TOO_SMALL",
            5 => "OUT_OF_MEMORY",
            6 => "UNSUPPORTED",
            7 => "FAILED",
            100 => "ABI_INCOMPATIBLE",
            101 => "MOD_DISABLED",
            102 => "WRONG_THREAD",
            103 => "REENTRANT_CALL",
            200 => "STALE_HANDLE",
            201 => "PERMISSION_DENIED",
            202 => "NOT_FOUND",
            203 => "SERVICE_UNAVAILABLE",
            204 => "ALREADY_EXISTS",
            500 => "CPP_EXCEPTION",
            501 => "MOD_PANICKED",
            502 => "CALLBACK_POISONED",
            599 => "INTERNAL_ERROR",
            _ => return None,
        })
    }
}

impl core::fmt::Display for FerrumStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.name() {
            Some(name) => f.write_str(name),
            None => write!(f, "UNKNOWN({})", self.0),
        }
    }
}
