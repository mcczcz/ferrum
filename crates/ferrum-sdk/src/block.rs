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
        let type_name = host::block_api()?.type_name()?;
        host::read_string(|w| type_name(self.dimension, self.position, w))
    }
    /// 读取主方块层的状态 NBT（名称、状态属性及版本）。
    pub fn nbt(&self) -> Result<crate::nbt::NbtCompound> {
        let read = host::block_api()?.nbt()?;
        let bytes = host::read_bytes(|out| read(self.dimension, self.position, out))
            .map_err(host::error_from_status)?;
        crate::nbt::decode(&bytes)
    }
    /// 读取箱子、告示牌等方块实体的存档。普通方块返回 None，区块未加载则返回错误。
    pub fn block_entity_nbt(&self) -> Result<Option<crate::nbt::NbtCompound>> {
        let read = host::block_api()?.block_entity_nbt()?;
        let bytes = host::read_bytes(|out| read(self.dimension, self.position, out))
            .map_err(host::error_from_status)?;
        if bytes.is_empty() {
            return Ok(None);
        }
        crate::nbt::decode(&bytes).map(Some)
    }
    /// 当前进程注册表 ID，不能跨版本或跨重启保存。
    pub fn runtime_id(&self) -> Result<BlockRuntimeId> {
        let mut out = BlockRuntimeId::default();
        host::check((host::block_api()?.runtime_id()?)(
            self.dimension,
            self.position,
            &mut out,
        ))?;
        Ok(out)
    }
    pub fn is_air(&self) -> Result<bool> {
        let mut out = FfiBool::FALSE;
        host::check((host::block_api()?.is_air()?)(
            self.dimension,
            self.position,
            &mut out,
        ))?;
        Ok(out.get())
    }
    /// 放置该类型的默认状态；更新邻居和客户端，不模拟玩家放置或破坏掉落。
    pub fn set_type(&self, name: &str) -> Result<()> {
        host::check((host::block_api()?.set_type()?)(
            self.dimension,
            self.position,
            name.into(),
        ))
    }
    pub fn set_air(&self) -> Result<()> {
        self.set_type("minecraft:air")
    }
}
