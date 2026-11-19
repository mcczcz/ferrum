use crate::{FerrumStatus, FfiByteWriter, FfiStr};

/// LegacyMoney 经济接口。金额为有符号 64 位整数，账户使用 XUID 字符串。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct LlMoneyApiV1 {
    pub struct_size: u32,
    pub get: extern "C" fn(FfiStr<'_>, *mut i64) -> FerrumStatus,
    pub set: extern "C" fn(FfiStr<'_>, i64) -> FerrumStatus,
    pub add: extern "C" fn(FfiStr<'_>, i64) -> FerrumStatus,
    pub reduce: extern "C" fn(FfiStr<'_>, i64) -> FerrumStatus,
    pub transfer: extern "C" fn(FfiStr<'_>, FfiStr<'_>, i64, FfiStr<'_>) -> FerrumStatus,
    pub history: extern "C" fn(FfiStr<'_>, i32, *mut FfiByteWriter) -> FerrumStatus,
    pub clear_history: extern "C" fn(i32) -> FerrumStatus,
    /// 每行依次为小端 i64 余额、小端 u64 XUID 字节数、UTF-8 XUID。
    pub ranking: extern "C" fn(u16, *mut FfiByteWriter) -> FerrumStatus,
}
