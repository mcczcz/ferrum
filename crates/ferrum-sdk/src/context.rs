use std::fmt;
use std::path::PathBuf;

use ferrum_abi::{ModContextHandle, ModDirKind};

use crate::error::Result;
use crate::host;
use crate::logger::Logger;

/// 模组目录种类。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ModDir {
    /// `mods/<name>/`
    Mod,
    /// `mods/<name>/data/`
    Data,
    /// `mods/<name>/config/`
    Config,
    /// `mods/<name>/lang/`
    Lang,
}

impl ModDir {
    fn to_abi(self) -> ModDirKind {
        match self {
            Self::Mod => ModDirKind::MOD,
            Self::Data => ModDirKind::DATA,
            Self::Config => ModDirKind::CONFIG,
            Self::Lang => ModDirKind::LANG,
        }
    }
}

/// 当前模组的上下文：元数据、目录与各子系统入口。
///
/// 可以克隆并跨线程保存；模组卸载后，其上的调用返回 `Err(StaleHandle)`。
#[derive(Copy, Clone)]
pub struct ModContext {
    handle: ModContextHandle,
}

impl ModContext {
    pub(crate) fn new(handle: ModContextHandle) -> Self {
        Self { handle }
    }

    /// 底层上下文句柄（与宿主交互的扩展代码使用）。
    pub fn raw_handle(&self) -> ModContextHandle {
        self.handle
    }

    /// 本模组专属日志器。
    pub fn logger(&self) -> Logger {
        Logger::new(self.handle)
    }

    /// 命令 Builder；实际注册必须在 on_enable 或主线程回调中持令牌执行。
    pub fn commands(&self) -> crate::command::Commands {
        crate::command::Commands { ctx: self.handle }
    }

    /// `manifest.json` 中的 `name`。
    pub fn name(&self) -> Result<String> {
        host::read_string(|w| (host::core().get_mod_name)(self.handle, w))
    }

    /// `manifest.json` 中的 `version`。
    pub fn version(&self) -> Result<String> {
        host::read_string(|w| (host::core().get_mod_version)(self.handle, w))
    }

    pub fn dir(&self, kind: ModDir) -> Result<PathBuf> {
        host::read_string(|w| (host::core().get_mod_dir)(self.handle, kind.to_abi(), w))
            .map(PathBuf::from)
    }

    pub fn mod_dir(&self) -> Result<PathBuf> {
        self.dir(ModDir::Mod)
    }

    pub fn data_dir(&self) -> Result<PathBuf> {
        self.dir(ModDir::Data)
    }

    pub fn config_dir(&self) -> Result<PathBuf> {
        self.dir(ModDir::Config)
    }

    pub fn lang_dir(&self) -> Result<PathBuf> {
        self.dir(ModDir::Lang)
    }
}

impl fmt::Debug for ModContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModContext")
            .field("handle", &self.handle.0.to_raw())
            .finish()
    }
}
