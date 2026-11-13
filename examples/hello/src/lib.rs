//! 最小 Ferrum 示例模组：演示生命周期、日志与目录 API。
//!
//! 在 `config/hello.txt` 中写入 `panic` 可演示 `on_enable` panic 被防火墙拦截、模组进入停用状态。

use std::fs;

use ferrum_sdk::prelude::*;

pub struct HelloFerrum {
    logger: Logger,
    enable_count: u32,
}

#[ferrum_sdk::mod_entry]
impl FerrumMod for HelloFerrum {
    fn on_load(ctx: &ModContext) -> Result<Self> {
        let logger = ctx.logger();
        logger.info(format!(
            "HelloFerrum v{} 正在加载，配置目录：{}",
            ctx.version()?,
            ctx.config_dir()?.display()
        ));
        Ok(Self {
            logger,
            enable_count: 0,
        })
    }

    fn on_enable(&mut self, ctx: &ModContext, _token: &MainThreadToken<'_>) -> Result<()> {
        self.enable_count += 1;
        let trigger = ctx.config_dir()?.join("hello.txt");
        if fs::read_to_string(&trigger).is_ok_and(|s| s.trim() == "panic") {
            panic!("按 {} 的要求在 on_enable 中 panic", trigger.display());
        }
        self.logger
            .info(format!("HelloFerrum 已启用（第 {} 次）", self.enable_count));
        Ok(())
    }

    fn on_disable(&mut self, _ctx: &ModContext, _token: &MainThreadToken<'_>) -> Result<()> {
        self.logger.info("HelloFerrum 已停用");
        Ok(())
    }

    fn on_unload(self, _ctx: &ModContext) -> Result<()> {
        self.logger.info("HelloFerrum 已卸载");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use ferrum_host_mock::{MockHost, MockModState};

    use super::HelloFerrum;

    #[test]
    fn full_lifecycle() {
        let host = MockHost::new();
        let m = host
            .load_named::<HelloFerrum>("hello-ferrum", "0.1.0")
            .unwrap();
        host.enable(&m).unwrap();
        host.disable(&m).unwrap();
        host.enable(&m).unwrap();
        host.unload(&m).unwrap();
        assert_eq!(host.state(&m), Some(MockModState::Unloaded));

        let logs: Vec<String> = host.logs(&m).into_iter().map(|l| l.message).collect();
        assert!(logs[0].contains("v0.1.0"), "{logs:?}");
        assert!(logs.iter().any(|l| l.contains("第 2 次")), "{logs:?}");
        assert!(logs.last().unwrap().contains("已卸载"));
        assert!(host.panics(&m).is_empty());
    }
}
