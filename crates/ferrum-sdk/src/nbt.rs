//! 模组拥有的 NBT 值，可按字段读取、编辑或转为 SNBT。

pub use quartz_nbt::{NbtCompound, NbtList, NbtTag, io, snbt};

use crate::{Error, Result};
use ferrum_abi::FerrumStatus;

pub(crate) fn decode(mut bytes: &[u8]) -> Result<NbtCompound> {
    io::read_nbt(&mut bytes, io::Flavor::Uncompressed)
        .map(|(tag, _)| tag)
        .map_err(|e| Error::new(FerrumStatus::FAILED, format!("NBT 解码失败：{e}")))
}
