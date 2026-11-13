//! 验收项：on_enable 中 panic 不崩服，模组进入 Disabled，并上报 panic 详情。
//!
//! 每个模组 crate 只能有一个 `#[mod_entry]`（它导出固定符号 `ferrum_mod_entry_v1`），
//! 因此每个测试模组放在独立的集成测试文件中。

use ferrum_host_mock::{MockHost, MockModState};
use ferrum_sdk::prelude::*;

struct Panicky;

#[ferrum_sdk::mod_entry]
impl FerrumMod for Panicky {
    fn on_load(_ctx: &ModContext) -> Result<Self> {
        Ok(Self)
    }

    fn on_enable(&mut self, _ctx: &ModContext, _token: &MainThreadToken<'_>) -> Result<()> {
        panic!("boom");
    }
}

#[test]
fn panic_in_on_enable_disables_mod() {
    let host = MockHost::new();
    let m = host.load::<Panicky>().unwrap();
    let err = host.enable(&m).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::ModPanicked);
    assert_eq!(host.state(&m), Some(MockModState::Disabled));

    let panics = host.panics(&m);
    assert_eq!(panics.len(), 1);
    assert_eq!(panics[0].message, "boom");
    assert!(
        panics[0].location.contains("panic_firewall.rs"),
        "{panics:?}"
    );

    // 再次启用：实例锁已中毒，回调被毒化而不是再次进入用户代码。
    let err = host.enable(&m).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::CallbackPoisoned);
    // 卸载仍然可以完成。
    host.unload(&m).unwrap();
}
