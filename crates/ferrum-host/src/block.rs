use crate::{last_error, shim, vtable};
use ferrum_abi::*;

fn call(f: impl FnOnce(&BlockApiV1) -> FerrumStatus) -> FerrumStatus {
    vtable::guard(|| {
        if !vtable::is_main_thread() {
            return last_error::fail(FerrumStatus::WRONG_THREAD, "方块 API 必须在主线程调用");
        }
        match shim::get() {
            Some(api) => f(&api.block_v1),
            None => last_error::fail(FerrumStatus::SERVICE_UNAVAILABLE, "shim 未初始化"),
        }
    })
}
extern "C" fn type_name(dim: DimensionId, pos: BlockPos, out: *mut FfiByteWriter) -> FerrumStatus {
    call(|a| (a.type_name)(dim, pos, out))
}
extern "C" fn runtime_id(
    dim: DimensionId,
    pos: BlockPos,
    out: *mut BlockRuntimeId,
) -> FerrumStatus {
    call(|a| (a.runtime_id)(dim, pos, out))
}
extern "C" fn is_air(dim: DimensionId, pos: BlockPos, out: *mut FfiBool) -> FerrumStatus {
    call(|a| (a.is_air)(dim, pos, out))
}
extern "C" fn set_type(dim: DimensionId, pos: BlockPos, name: FfiStr<'_>) -> FerrumStatus {
    call(|a| (a.set_type)(dim, pos, name))
}
pub static BLOCK_V1: BlockApiV1 = BlockApiV1 {
    struct_size: size_of::<BlockApiV1>() as u32,
    type_name,
    runtime_id,
    is_air,
    set_type,
};
