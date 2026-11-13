//! 从 `ferrum-abi` 与 `ferrum-host` 的 shim 接口生成 C 头文件。

use cbindgen::{
    Builder, Config, EnumConfig, ExportConfig, Language, ParseConfig, RenameRule, Style,
};

use crate::{emit, workspace_root};

const HEADER: &str = "\
// 由 `cargo xtask cbindgen` 从 crates/ferrum-abi 与 crates/ferrum-host/src/shim.rs 生成。
// 严禁手改；修改 Rust 定义后重新生成。";

pub fn run(check: bool) -> Result<bool, String> {
    let root = workspace_root();
    let config = Config {
        language: Language::C,
        cpp_compat: true,
        style: Style::Both,
        header: Some(HEADER.to_owned()),
        include_guard: Some("FERRUM_ABI_H".to_owned()),
        pragma_once: true,
        no_includes: true,
        sys_includes: vec!["stdint.h".to_owned(), "stddef.h".to_owned()],
        usize_is_size_t: true,
        documentation: true,
        export: ExportConfig {
            prefix: None,
            // 宿主内部函数由 exports.rs 导出，同样需要原型。
            include: vec![
                "FerrumShimApi".to_owned(),
                "ShimModInfo".to_owned(),
                "ModEntryFnV1".to_owned(),
                "FerrumModDescriptor".to_owned(),
            ],
            ..Default::default()
        },
        enumeration: EnumConfig {
            rename_variants: RenameRule::ScreamingSnakeCase,
            ..Default::default()
        },
        parse: ParseConfig {
            parse_deps: true,
            include: Some(vec!["ferrum-abi".to_owned()]),
            ..Default::default()
        },
        ..Default::default()
    };

    let bindings = Builder::new()
        .with_crate(root.join("crates/ferrum-host"))
        .with_config(config)
        .generate()
        .map_err(|e| format!("cbindgen 失败: {e}"))?;

    let mut out = Vec::new();
    bindings.write(&mut out);
    let text = String::from_utf8(out)
        .map_err(|e| e.to_string())?
        .replace("\r\n", "\n");
    emit(
        &root.join("cpp-shim/include/ferrum/ferrum_abi.h"),
        &text,
        check,
    )
}
