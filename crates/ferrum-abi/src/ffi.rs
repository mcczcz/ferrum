use core::ffi::c_void;
use core::marker::PhantomData;

use crate::FerrumStatus;

/// 跨 FFI 布尔值：`0` 为 false，任何非 `0` 值为 true。
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct FfiBool(pub u8);

impl FfiBool {
    pub const FALSE: Self = Self(0);
    pub const TRUE: Self = Self(1);

    #[inline]
    pub const fn get(self) -> bool {
        self.0 != 0
    }
}

impl From<bool> for FfiBool {
    #[inline]
    fn from(value: bool) -> Self {
        Self(value as u8)
    }
}

impl From<FfiBool> for bool {
    #[inline]
    fn from(value: FfiBool) -> Self {
        value.get()
    }
}

/// UTF-8 只读借用字符串切片（不保证 `'\0'` 结尾）。
///
/// 仅在本次同步调用返回前有效。宿主 → 模组方向**不保证**是合法 UTF-8。
#[repr(C)]
#[derive(Copy, Clone)]
pub struct FfiStr<'a> {
    pub ptr: *const u8,
    pub len: usize,
    pub _marker: PhantomData<&'a str>,
}

impl<'a> FfiStr<'a> {
    pub const EMPTY: FfiStr<'static> = FfiStr {
        ptr: core::ptr::null(),
        len: 0,
        _marker: PhantomData,
    };

    #[inline]
    pub const fn new(s: &'a str) -> Self {
        Self {
            ptr: s.as_ptr(),
            len: s.len(),
            _marker: PhantomData,
        }
    }

    /// 还原为字节切片；容忍 `ptr == null`。
    ///
    /// # Safety
    /// 若 `len != 0` 且 `ptr` 非空，`ptr` 必须指向 `len` 个在 `'a` 内有效的已初始化字节。
    #[inline]
    pub unsafe fn as_bytes(&self) -> &'a [u8] {
        if self.len == 0 || self.ptr.is_null() {
            &[]
        } else {
            // SAFETY: 调用方保证 ptr 指向 len 个在 'a 内有效的字节；空指针与零长度已在上面分支排除。
            unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
        }
    }

    /// 校验 UTF-8 后还原为 `&str`。
    ///
    /// # Safety
    /// 同 [`FfiStr::as_bytes`]。
    #[inline]
    pub unsafe fn to_str(&self) -> Result<&'a str, core::str::Utf8Error> {
        // SAFETY: 透传调用方对 ptr/len 的保证。
        core::str::from_utf8(unsafe { self.as_bytes() })
    }
}

impl<'a> From<&'a str> for FfiStr<'a> {
    #[inline]
    fn from(s: &'a str) -> Self {
        Self::new(s)
    }
}

impl core::fmt::Debug for FfiStr<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FfiStr")
            .field("ptr", &self.ptr)
            .field("len", &self.len)
            .finish()
    }
}

/// 连续元素的只读借用切片（`T` 必须为 `#[repr(C)]` 且无未初始化填充字节）。
#[repr(C)]
pub struct FfiSlice<'a, T> {
    pub ptr: *const T,
    pub len: usize,
    pub _marker: PhantomData<&'a [T]>,
}

impl<T> Clone for FfiSlice<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for FfiSlice<'_, T> {}

impl<'a, T> FfiSlice<'a, T> {
    #[inline]
    pub const fn new(s: &'a [T]) -> Self {
        Self {
            ptr: s.as_ptr(),
            len: s.len(),
            _marker: PhantomData,
        }
    }

    /// 还原为切片；容忍 `ptr == null`。
    ///
    /// # Safety
    /// 若 `len != 0` 且 `ptr` 非空，`ptr` 必须指向 `len` 个在 `'a` 内有效、对齐且已初始化的 `T`。
    #[inline]
    pub unsafe fn as_slice(&self) -> &'a [T] {
        if self.len == 0 || self.ptr.is_null() {
            &[]
        } else {
            // SAFETY: 调用方保证 ptr 指向 len 个有效对齐的 T；空指针与零长度已在上面分支排除。
            unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
        }
    }
}

/// 写入器回调：在调用方（通常是模组）堆上追加 `bytes`。
pub type FfiWriteFn =
    extern "C" fn(context: *mut c_void, bytes: *const u8, len: usize) -> FerrumStatus;

/// 由调用方在栈上构造、传给被调用方用于输出变长数据的写入器。
///
/// - 被调用方可多次调用 `write_fn`（追加语义）；
/// - `write_fn` 返回非 `OK` 时，被调用方必须立即停止写入并原样返回该状态；
/// - 被调用方不得在调用返回后保留本结构或其 `context`。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct FfiByteWriter {
    pub context: *mut c_void,
    pub write_fn: FfiWriteFn,
}

impl FfiByteWriter {
    /// 调用 `write_fn` 写入一段字节。
    #[inline]
    pub fn write(&self, bytes: &[u8]) -> FerrumStatus {
        (self.write_fn)(self.context, bytes.as_ptr(), bytes.len())
    }
}
