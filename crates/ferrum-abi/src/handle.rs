use core::num::NonZeroU64;

/// 64 位非零代数句柄：低 32 位为槽位下标，高 32 位为代数（恒非零）。
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GenerationalHandle(pub NonZeroU64);

impl GenerationalHandle {
    /// `generation` 为 0 时返回 `None`。
    #[inline]
    pub const fn new(slot_index: u32, generation: u32) -> Option<Self> {
        match NonZeroU64::new(((generation as u64) << 32) | slot_index as u64) {
            Some(raw) if generation != 0 => Some(Self(raw)),
            _ => None,
        }
    }

    #[inline]
    pub const fn slot_index(self) -> u32 {
        self.0.get() as u32
    }

    #[inline]
    pub const fn generation(self) -> u32 {
        (self.0.get() >> 32) as u32
    }

    #[inline]
    pub const fn to_raw(self) -> u64 {
        self.0.get()
    }

    #[inline]
    pub const fn from_raw(raw: u64) -> Option<Self> {
        match NonZeroU64::new(raw) {
            Some(raw) if (raw.get() >> 32) != 0 => Some(Self(raw)),
            _ => None,
        }
    }
}

/// 宿主为每个已加载模组分配的上下文句柄，宿主据此识别调用方并校验归属。
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ModContextHandle(pub GenerationalHandle);

/// “此调用发生在主线程”的显式证明（ABI 层占位，按值传递）。
///
/// 仅宿主在确认当前线程为主线程后构造；SDK 在跳板中把它转换为 `&MainThreadToken<'tick>`。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct RawMainThreadToken {
    pub _reserved: u8,
}

impl RawMainThreadToken {
    /// 只能由宿主在主线程上调用。
    pub const ASSERTED: Self = Self { _reserved: 0 };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_roundtrip() {
        let h = GenerationalHandle::new(7, 3).unwrap();
        assert_eq!(h.slot_index(), 7);
        assert_eq!(h.generation(), 3);
        assert_eq!(GenerationalHandle::from_raw(h.to_raw()), Some(h));
        assert!(GenerationalHandle::new(7, 0).is_none());
        assert!(GenerationalHandle::from_raw(7).is_none());
    }
}
