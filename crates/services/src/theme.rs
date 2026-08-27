//! 主题加载器（服务层）
//!
//! 从磁盘 JSON 加载主题方案，纯逻辑、不依赖 GPUI。
//! 与 `services::config`（从磁盘加载 `AppConfig`）属同一类职责。

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::Result;
use log::info;

use models::theme::ThemeScheme;

/// 已加载主题的缓存，按名称索引。
#[derive(Debug, Clone, Default)]
pub struct ThemeLoader {
    themes: HashMap<String, ThemeScheme>,
}

impl ThemeLoader {
    #[must_use]
    pub fn new() -> Self {
        Self {
            themes: HashMap::new(),
        }
    }

    /// 扫描主题目录，加载所有 `.json` 主题方案。
    pub fn load_all(&mut self, themes_dir: &Path) -> Result<()> {
        if !themes_dir.exists() {
            let _ = fs::create_dir_all(themes_dir);
            return Ok(());
        }

        for entry in fs::read_dir(themes_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let content = fs::read_to_string(&path)?;
                if let Ok(scheme) = serde_json::from_str::<ThemeScheme>(&content) {
                    info!("加载自定义主题: {}", scheme.name);
                    self.themes.insert(scheme.name.clone(), scheme);
                }
            }
        }
        Ok(())
    }

    /// 按名称取主题方案。
    #[must_use]
    pub fn get_theme(&self, name: &str) -> Option<&ThemeScheme> {
        self.themes.get(name)
    }

    /// 已加载主题名称（升序）。
    #[must_use]
    pub fn available_themes(&self) -> Vec<String> {
        let mut names: Vec<String> = self.themes.keys().cloned().collect();
        names.sort();
        names
    }

    /// 热重载单个主题文件。
    pub fn reload_theme_from_file(&mut self, path: &Path) -> Result<()> {
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let content = fs::read_to_string(path)?;
            if let Ok(scheme) = serde_json::from_str::<ThemeScheme>(&content) {
                info!("热加载主题: {}", scheme.name);
                self.themes.insert(scheme.name.clone(), scheme);
            }
        }
        Ok(())
    }
}
