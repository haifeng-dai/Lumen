use gpui::{App, Global};
use i18n::Language;

use database::state::LocalStateManager;
use models::config::AppConfig;

pub struct ConfigStore {
    pub inner: AppConfig,
}

impl Global for ConfigStore {}

impl ConfigStore {
    pub fn current_language(&self) -> Language {
        self.ui.language.parse::<Language>().unwrap_or_default()
    }

    /// 从本地状态库加载配置并注册为全局状态。
    ///
    /// 实际加载/解析/默认值回落逻辑委托给服务层 `services::config::load_config`，
    /// 这里只负责把结果注册为 GPUI Global。
    pub fn load_and_set(manager: &LocalStateManager, cx: &mut App) {
        let config = services::config::load_config(manager);
        cx.set_global(Self { inner: config });
    }
}

impl std::ops::Deref for ConfigStore {
    type Target = AppConfig;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
