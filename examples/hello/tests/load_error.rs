//! on_load 返回 Err 时加载失败，错误详情经 last-error 传回宿主。

use ferrum_host_mock::MockHost;
use ferrum_sdk::prelude::*;

struct Failing;

#[ferrum_sdk::mod_entry]
impl FerrumMod for Failing {
    fn on_load(_ctx: &ModContext) -> Result<Self> {
        Err(Error::msg("配置文件损坏"))
    }
}

#[test]
fn on_load_error_carries_message() {
    let host = MockHost::new();
    let err = host.load::<Failing>().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Failed);
    assert!(err.message().unwrap().contains("配置文件损坏"), "{err}");
}
