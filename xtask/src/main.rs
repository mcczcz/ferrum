//! 仓库生成物工具。
//!
//! ```text
//! cargo xtask cbindgen [--check]     生成 cpp-shim/include/ferrum/ferrum_abi.h
//! cargo xtask layout-lock [--check]  向 crates/ferrum-abi/tests/layout_lock.rs 追加布局断言
//! ```
//!
//! `--check` 模式只比较、不写入，生成物过期时以非零退出码结束（供 CI 使用）。

mod cbindgen_gen;
mod layout_lock;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub(crate) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace root")
        .to_path_buf()
}

/// 写入或校验生成物；返回是否与期望一致。
pub(crate) fn emit(path: &Path, expected: &str, check: bool) -> Result<bool, String> {
    let current = std::fs::read_to_string(path)
        .ok()
        .map(|s| s.replace("\r\n", "\n"));
    let up_to_date = current.as_deref() == Some(expected);
    if check {
        if !up_to_date {
            eprintln!(
                "过期: {}（运行不带 --check 的命令重新生成）",
                path.display()
            );
        }
        return Ok(up_to_date);
    }
    if !up_to_date {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, expected).map_err(|e| format!("写入 {} 失败: {e}", path.display()))?;
        println!("已生成: {}", path.display());
    } else {
        println!("无变化: {}", path.display());
    }
    Ok(true)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let result = match args.first().map(String::as_str) {
        Some("cbindgen") => cbindgen_gen::run(check),
        Some("layout-lock") => layout_lock::run(check),
        _ => {
            eprintln!("用法: cargo xtask <cbindgen|layout-lock> [--check]");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("错误: {e}");
            ExitCode::FAILURE
        }
    }
}
