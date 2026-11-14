use crate::{DimensionId, FerrumStatus, FfiBool, FfiByteWriter, FfiStr};

/// 方块整数坐标，按值传递且布局冻结；实际访问仍需主线程令牌。
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

/// 当前进程/注册表的状态 ID，不能落盘或跨版本保存。
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BlockRuntimeId(pub u32);

/// 主方块层；按维度/坐标实时解析，只访问已加载区块。
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct BlockApiV1 {
    pub struct_size: u32,
    pub type_name: extern "C" fn(DimensionId, BlockPos, *mut FfiByteWriter) -> FerrumStatus,
    pub runtime_id: extern "C" fn(DimensionId, BlockPos, *mut BlockRuntimeId) -> FerrumStatus,
    pub is_air: extern "C" fn(DimensionId, BlockPos, *mut FfiBool) -> FerrumStatus,
    /// 按名称放置默认状态，更新邻居并同步客户端；不生成破坏掉落。
    pub set_type: extern "C" fn(DimensionId, BlockPos, FfiStr<'_>) -> FerrumStatus,
}
