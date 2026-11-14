/// `"FERRUMAB"`，用于校验根表指针确实来自 Ferrum 宿主。
pub const FERRUM_ABI_MAGIC: u64 = 0x4645_5252_554D_4142;

/// ABI 主版本；破坏性变更必须使用新的入口符号（如 `ferrum_mod_entry_v2`）。
pub const FERRUM_ABI_MAJOR: u16 = 1;

/// 当前 ABI 次版本；每次向任意已发布 VTable 尾追加字段时递增。
pub const FERRUM_ABI_MINOR: u16 = 1;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AbiVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u32,
}

impl AbiVersion {
    /// 本 crate 所描述的 ABI 版本。
    pub const CURRENT: Self = Self::new(FERRUM_ABI_MAJOR, FERRUM_ABI_MINOR, 0);
    /// v1.0：生命周期、日志、错误上报。
    pub const V1_0: Self = Self::new(1, 0, 0);

    pub const fn new(major: u16, minor: u16, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// 以本版本为“宿主版本”，判断能否满足 `required` 的需求。
    pub const fn satisfies(self, required: Self) -> bool {
        self.major == required.major && self.minor >= required.minor
    }
}

impl core::fmt::Display for AbiVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}
