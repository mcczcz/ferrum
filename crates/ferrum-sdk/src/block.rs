use crate::{MainThreadToken, Result, host};
pub use ferrum_abi::{BlockPos, BlockRuntimeId};
use ferrum_abi::{DimensionId, FfiBool};
use std::marker::PhantomData;

/// 绑定主线程同步作用域的方块位置，每次访问都按维度/坐标重新读取。
/// 原位置被替换时后续调用观察新方块，不保存 Block* 或 BlockSource*。
#[derive(Copy, Clone, Debug)]
pub struct BlockRef<'tick> {
    dimension: DimensionId,
    position: BlockPos,
    _scope: PhantomData<(&'tick (), *mut ())>,
}
impl<'tick> BlockRef<'tick> {
    pub(crate) fn new(
        _token: &'tick MainThreadToken<'tick>,
        dimension: DimensionId,
        position: BlockPos,
    ) -> Self {
        Self {
            dimension,
            position,
            _scope: PhantomData,
        }
    }
    pub fn position(&self) -> BlockPos {
        self.position
    }
    pub fn dimension(&self) -> DimensionId {
        self.dimension
    }
    pub fn type_name(&self) -> Result<String> {
        let api = host::block_api()?;
        host::read_string(|w| (api.type_name)(self.dimension, self.position, w))
    }
    /// 当前进程注册表 ID，不能跨版本或跨重启保存。
    pub fn runtime_id(&self) -> Result<BlockRuntimeId> {
        let mut out = BlockRuntimeId::default();
        host::check((host::block_api()?.runtime_id)(
            self.dimension,
            self.position,
            &mut out,
        ))?;
        Ok(out)
    }
    pub fn is_air(&self) -> Result<bool> {
        let mut out = FfiBool::FALSE;
        host::check((host::block_api()?.is_air)(
            self.dimension,
            self.position,
            &mut out,
        ))?;
        Ok(out.get())
    }
    /// 放置该类型的默认状态；更新邻居和客户端，不模拟玩家放置或破坏掉落。
    pub fn set_type(&self, name: &str) -> Result<()> {
        host::check((host::block_api()?.set_type)(
            self.dimension,
            self.position,
            name.into(),
        ))
    }
    pub fn set_air(&self) -> Result<()> {
        self.set_type("minecraft:air")
    }
}
