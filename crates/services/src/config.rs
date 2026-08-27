//! 配置读写的服务层编排
//!
//! 职责：把“从 `LocalStateManager` 读取/写入配置 JSON”的原始 CRUD，封装为带
//! 解析、默认值回落、序列化错误处理的服务层操作；并集中承载配置相关的平台/IO
//! 行为（`get_app_root_dir` / `apply_proxy_config` / `ensure_dirs` /
//! `clean_old_logs` / `default_app_config`），与 `models::config`（纯类型 + serde）
//! 分离。UI 与启动路径只调用本模块，不直接碰 `database` 的 `load_config` /
//! `save_config`。

use std::path::PathBuf;
use std::{env, fs};

use anyhow::Context;
use log::debug;

use database::state::LocalStateManager;
use models::config::{
    AppConfig, CitationConfig, DatabaseConfig, GoogleDriveConfig, PdfViewerConfig, ProxyConfig,
    TranslationConfig, UiConfig, WebDavConfig,
};

/// 从本地状态库加载配置；不存在或解析失败时回落默认配置，并把默认配置写回。
#[must_use]
pub fn load_config(manager: &LocalStateManager) -> AppConfig {
    manager
        .load_config()
        .ok()
        .flatten()
        .and_then(|blob| serde_json::from_str(&blob).ok())
        .unwrap_or_else(|| {
            let default = default_app_config();
            if let Ok(blob) = serde_json::to_string(&default) {
                let _ = manager.save_config(&blob);
            }
            default
        })
}

/// 将配置序列化并写入本地状态库。
pub fn save_config(manager: &LocalStateManager, config: &AppConfig) -> anyhow::Result<()> {
    let blob = serde_json::to_string(config)?;
    manager.save_config(&blob)?;
    Ok(())
}

/// 应用平台相关的默认配置（等价于原 `AppConfig::default()`，但置于服务层，
/// 避免 `models` 反向依赖平台代码）。
#[must_use]
pub fn default_app_config() -> AppConfig {
    let base_dir = get_app_root_dir();
    AppConfig {
        attachment_path: base_dir.join("storage"),
        base_dir,
        filename_template: "{author}{year}-{title}".to_string(),
        log_level: "info".to_string(),
        notification_level: "all".to_string(),
        ui: UiConfig {
            theme_mode: "light".to_string(),
            theme_style: "default".to_string(),
            language: "zh-CN".to_string(),
            ui_scale: 1.0,
        },
        webdav: WebDavConfig {
            enabled: false,
            endpoint: String::new(),
            username: String::new(),
            remote_path: "/".to_string(),
            on_demand: false,
        },
        google_drive: GoogleDriveConfig::default(),
        database: DatabaseConfig {
            use_remote: false,
            host: "localhost".to_string(),
            port: 3306,
            database: "lumen".to_string(),
            username: "root".to_string(),
            password: String::new(),
            use_ssl: false,
        },
        pdf_viewer: PdfViewerConfig::default(),
        translation: TranslationConfig::default(),
        proxy: ProxyConfig::default(),
        citation: CitationConfig::default(),
    }
}

/// 获取应用根目录（平台相关）。
///
/// 使用 `directories::ProjectDirs` 遵循各平台标准目录规范，
/// `ProjectDirs::from("", "", "Lumen")` 的路径与历史手写逻辑逐平台一致：
///
/// - Windows   → `%APPDATA%\Lumen`
/// - macOS     → `~/Library/Application Support/Lumen`
/// - Linux     → `$XDG_CONFIG_HOME/Lumen` 或 `~/.config/Lumen`
///
/// 环境变量缺失等极端情况下回退到当前目录下的 `.Lumen`。
pub fn get_app_root_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "Lumen")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".Lumen"))
}

/// 应用代理配置到当前进程的环境变量中
pub fn apply_proxy_config(config: &ProxyConfig) {
    if config.enabled && !config.url.trim().is_empty() {
        let url = config.url.trim();
        unsafe {
            env::set_var("HTTP_PROXY", url);
            env::set_var("HTTPS_PROXY", url);
            env::set_var("ALL_PROXY", url); // 同时支持 socks 代理等
        }
        log::info!("自定义代理已启用并在当前进程生效: {}", url);
    } else {
        unsafe {
            env::remove_var("HTTP_PROXY");
            env::remove_var("HTTPS_PROXY");
            env::remove_var("ALL_PROXY");
        }
        log::info!("自定义代理已从当前进程禁用（恢复系统默认网络环境）");
    }
}

/// 确保配置中定义的所有物理目录都已创建
pub fn ensure_dirs(config: &AppConfig) -> anyhow::Result<()> {
    let dirs = [
        config.attachment_path.clone(),
        config.base_dir.clone(),
        config.log_dir(),
        config.themes_dir(),
    ];
    for dir in dirs {
        if !dir.exists() {
            fs::create_dir_all(&dir)
                .with_context(|| format!("Failed to create directory: {dir:?}"))?;
        }
    }
    Ok(())
}

/// 清理旧日志文件，只保留最近的 20 个
pub fn clean_old_logs(config: &AppConfig) {
    const MAX_LOG_FILES: usize = 20;
    let log_dir = config.log_dir();

    if let Ok(entries) = fs::read_dir(&log_dir) {
        let mut log_files: Vec<PathBuf> = entries
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|s| s.starts_with("log_") && s.ends_with(".log"))
            })
            .collect();

        // 按文件名排序 (因为文件名包含时间戳 YYYYMMDD_HHMMSS，所以字典序即时间序)
        log_files.sort();

        // 如果超过限制，删除最旧的
        if log_files.len() > MAX_LOG_FILES {
            let to_remove = log_files.len() - MAX_LOG_FILES;
            debug!(
                "日志清理: 需删除 {} 个旧日志文件 (共 {} 个, 上限 {})",
                to_remove,
                log_files.len(),
                MAX_LOG_FILES
            );
            for path in log_files.iter().take(to_remove) {
                if let Err(e) = fs::remove_file(path) {
                    debug!("日志清理: 删除旧日志文件失败 {path:?}: {e}");
                } else {
                    debug!("日志清理: 已删除旧日志文件 {path:?}");
                }
            }
        }
    }
}
