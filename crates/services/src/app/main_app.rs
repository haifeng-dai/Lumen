use crate::analysis::CCFService;
use crate::feed::FeedService;
use crate::feed::FetcherService;
use crate::library::{AttachmentService, FolderService, LiteratureService, TagService};
use crate::sync::SyncService;
use crate::sync::SyncStateInner;
use database::Database;
use file::LocalFileManager;
use models::config::AppConfig;
use parser::export::ExportManager;
use std::sync::{Arc, Mutex};
use translate::TranslationService;

mod core;
mod data;
mod files;
mod literature;
mod smart;
mod subscription;

pub use literature::{BatchRenameSaveError, BatchRenameSummary};

/// 主应用控制器
///
/// 负责协调数据、UI 和业务逻辑
pub struct MainApp {
    /// 同步状态（跨线程共享）
    pub sync_state: Arc<Mutex<SyncStateInner>>,
    /// 应用配置
    pub config: Mutex<AppConfig>,
    /// 文献服务
    pub literature_service: Arc<LiteratureService>,
    /// 文件夹服务
    pub folder_service: Arc<FolderService>,
    /// 订阅服务
    pub feed_service: Arc<FeedService>,
    /// 标签服务
    pub tag_service: Arc<TagService>,
    /// 附件服务
    pub attachment_service: Arc<AttachmentService>,
    /// 数据库
    pub db: Arc<Database>,
    /// 本地文件管理器
    pub file_manager: LocalFileManager,
    /// 存储与同步管理器 (持久化)
    pub sync_service: Arc<SyncService>,
    /// 解析管理器
    pub fetcher_service: Arc<FetcherService>,
    /// 导出管理器
    pub export_manager: Arc<ExportManager>,
    /// CCF 分级管理器
    pub ccf_service: Arc<CCFService>,
    /// 本地 UI 状态 (内存中)
    pub local_state: Arc<std::sync::RwLock<models::local_state::AppUiState>>,
    /// 本地状态管理器 (数据库)
    pub local_state_manager: Arc<database::state::LocalStateManager>,
    /// 翻译服务
    pub translation_service: Arc<Mutex<TranslationService>>,
    /// 跨线程通知通道发送端（桥接 GPUI !Send 限制）
    /// 用 `Arc` 包装，便于在构造 `SyncService` 时把同一份通道借给注入的通知闭包，
    /// 且后续 UI 注入真实 `Sender` 时所有闭包即时可见。
    pub refresh_tx: Arc<Mutex<Option<tokio::sync::broadcast::Sender<crate::notify::RefreshMsg>>>>,
}
