use crate::analysis::CCFService;
use crate::database_sync::{IdentityDecision, IdentityPlan};
use crate::feed::FeedService;
use crate::feed::FetcherService;
use crate::library::{AttachmentService, FolderService, LiteratureService, TagService};
use crate::sync::SyncService;
use crate::sync::SyncStateInner;
use anyhow::{Result, anyhow};
use database::DatabaseSyncSummary;
use i18n::Language;
use log::{debug, info};
use models::config::AppConfig;
use parser::export::ExportManager;
use std::sync::{Arc, Mutex};
use translate::TranslationService;

use super::MainApp;

impl MainApp {
    #[must_use]
    pub fn new(
        config: AppConfig,
        local_state_manager: Arc<database::state::LocalStateManager>,
        initial_state: models::local_state::AppUiState,
    ) -> (Self, tokio::sync::mpsc::Receiver<()>) {
        info!("开始创建 MainApp 实例...");
        let (backend_name, backend_config_json) =
            if config.webdav.enabled && !initial_state.webdav_password.is_empty() {
                let cfg = serde_json::to_string(&file::WebDavConfig {
                    enabled: true,
                    endpoint: config.webdav.endpoint.clone(),
                    username: config.webdav.username.clone(),
                    password: initial_state.webdav_password.clone(),
                    remote_path: config.webdav.remote_path.clone(),
                })
                .unwrap_or_default();
                ("webdav", cfg)
            } else if config.google_drive.enabled
                && !initial_state.google_drive_refresh_token.is_empty()
            {
                let cfg = serde_json::to_string(&file::GoogleDriveConfig {
                    enabled: true,
                    client_id: config.google_drive.client_id.clone(),
                    client_secret: config.google_drive.client_secret.clone(),
                    refresh_token: initial_state.google_drive_refresh_token.clone(),
                })
                .unwrap_or_default();
                ("google_drive", cfg)
            } else {
                ("noop", String::new())
            };
        let on_demand = config.webdav.on_demand;
        info!("MainApp 同步配置: backend={backend_name}, on_demand={on_demand}");

        // 跨线程通知通道：先建 Arc，供注入闭包捕获；UI 后续把真实 Sender 写入此 Arc。
        let refresh_tx: Arc<
            Mutex<Option<tokio::sync::broadcast::Sender<crate::notify::RefreshMsg>>>,
        > = Arc::new(Mutex::new(None));

        let notify_ui: Arc<dyn Fn() + Send + Sync> = {
            let tx = refresh_tx.clone();
            Arc::new(move || {
                if let Some(tx) = tx.lock().unwrap().as_ref() {
                    let _ = tx.send(crate::notify::RefreshMsg::UiChanged);
                }
            })
        };
        let notify_data: Arc<dyn Fn() + Send + Sync> = {
            let tx = refresh_tx.clone();
            Arc::new(move || {
                if let Some(tx) = tx.lock().unwrap().as_ref() {
                    let _ = tx.send(crate::notify::RefreshMsg::DataChanged);
                }
            })
        };
        let sync_state: Arc<Mutex<SyncStateInner>> = Arc::new(Mutex::new(SyncStateInner::new()));

        let (sync_service, sync_rx) = SyncService::new(
            &config,
            backend_name,
            &backend_config_json,
            on_demand,
            sync_state.clone(),
            notify_ui,
            notify_data,
        )
        .expect("Failed to initialize sync_service");
        let sync_service = Arc::new(sync_service);
        let db = sync_service.db.clone();
        let file_manager = sync_service.file_manager.clone();
        (
            Self {
                sync_state,
                refresh_tx,
                literature_service: Arc::new(LiteratureService::new()),
                attachment_service: Arc::new(AttachmentService::new()),
                folder_service: Arc::new(FolderService::new()),
                tag_service: Arc::new(TagService::new()),
                feed_service: Arc::new(FeedService::new()),
                db,
                file_manager,
                sync_service,
                fetcher_service: Arc::new(FetcherService::new()),
                export_manager: Arc::new(ExportManager::new()),
                ccf_service: Arc::new(CCFService::new()),
                local_state: Arc::new(std::sync::RwLock::new(initial_state.clone())),
                local_state_manager,
                translation_service: Arc::new(Mutex::new(TranslationService::new(
                    &config.translation.engine,
                    &initial_state.translation_keys,
                ))),
                config: Mutex::new(config),
            },
            sync_rx,
        )
    }

    pub fn update_config(&self, new_config: AppConfig) -> Result<()> {
        debug!("MainApp: 正在更新配置...");
        // 配置持久化委托给服务层 services::config::save_config（底层 CRUD 仍在 database）
        if let Err(e) = crate::config::save_config(&self.local_state_manager, &new_config) {
            debug!("MainApp: 保存配置失败: {e}");
        }
        let local_state = self.local_state.read().unwrap().clone();
        let (backend_name, backend_config_json) =
            if new_config.webdav.enabled && !local_state.webdav_password.is_empty() {
                let cfg = serde_json::to_string(&file::WebDavConfig {
                    enabled: true,
                    endpoint: new_config.webdav.endpoint.clone(),
                    username: new_config.webdav.username.clone(),
                    password: local_state.webdav_password.clone(),
                    remote_path: new_config.webdav.remote_path.clone(),
                })
                .unwrap_or_default();
                ("webdav", cfg)
            } else if new_config.google_drive.enabled
                && !local_state.google_drive_refresh_token.is_empty()
            {
                let cfg = serde_json::to_string(&file::GoogleDriveConfig {
                    enabled: true,
                    client_id: new_config.google_drive.client_id.clone(),
                    client_secret: new_config.google_drive.client_secret.clone(),
                    refresh_token: local_state.google_drive_refresh_token.clone(),
                })
                .unwrap_or_default();
                ("google_drive", cfg)
            } else {
                ("noop", String::new())
            };
        let on_demand = new_config.webdav.on_demand;
        self.sync_service
            .update_config(&new_config, backend_name, &backend_config_json, on_demand);
        let keys = self.local_state.read().unwrap().translation_keys.clone();
        self.translation_service
            .lock()
            .unwrap()
            .switch_engine(&new_config.translation.engine, &keys);
        *self.config.lock().unwrap() = new_config;
        self.notify_ui_changed();
        Ok(())
    }

    pub fn current_language(&self) -> Language {
        self.config
            .lock()
            .unwrap()
            .ui
            .language
            .parse()
            .unwrap_or_default()
    }

    pub fn notify_data_changed(&self) {
        self.sync_service.request_sync();
        if let Some(ref tx) = *self.refresh_tx.lock().unwrap() {
            let _ = tx.send(crate::notify::RefreshMsg::DataChanged);
        }
    }

    /// 构造一个后台刷新通知闭包，供服务层异步任务回调使用。
    ///
    /// 仅发送 `RefreshMsg::DataChanged`（等价于 `notify_data_changed` 的刷新半部分）；
    /// 同步请求由调用方在需要时显式触发。闭包是 `'static + Send + Sync`，可移入 Tokio 任务。
    pub(super) fn data_changed_notify(&self) -> Arc<dyn Fn() + Send + Sync> {
        let refresh_tx = self.refresh_tx.lock().unwrap().clone();
        Arc::new(move || {
            if let Some(tx) = &refresh_tx {
                let _ = tx.send(crate::notify::RefreshMsg::DataChanged);
            }
        })
    }

    pub fn notify_ui_changed(&self) {
        if let Some(ref tx) = *self.refresh_tx.lock().unwrap() {
            let _ = tx.send(crate::notify::RefreshMsg::UiChanged);
        }
    }

    pub fn perform_sync(self: Arc<Self>) {
        self.sync_service.clone().perform_full_sync();
    }
    pub fn perform_attachments_sync(self: Arc<Self>) {
        self.sync_service.clone().perform_attachments_sync();
    }

    pub async fn test_webdav_config(
        &self,
        endpoint: String,
        username: String,
        password: String,
        remote_path: String,
    ) -> Result<(), String> {
        let config_json = serde_json::to_string(&file::WebDavConfig {
            enabled: true,
            endpoint,
            username,
            password,
            remote_path,
        })
        .map_err(|e| e.to_string())?;
        self.sync_service
            .test_backend_config("webdav", &config_json)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn test_mysql_config(&self, config: models::DatabaseConfig) -> Result<(), String> {
        let sync_service = self.sync_service.clone();
        crate::runtime::RUNTIME
            .spawn(async move { sync_service.test_mysql_config(config).await })
            .await
            .map_err(|e| format!("MySQL 测试任务失败: {e}"))?
            .map_err(|e| e.to_string())
    }

    /// Database-sync facade for UI callers. The UI never reaches database or
    /// MySQL primitives directly.
    pub async fn database_sync_preflight(&self) -> Result<IdentityDecision> {
        let sync_service = self.sync_service.clone();
        crate::runtime::RUNTIME
            .spawn(async move { sync_service.database_sync_preflight().await })
            .await
            .map_err(|e| anyhow!("数据库同步预检任务失败: {e}"))?
    }

    pub async fn confirm_remote_initialization(&self) -> Result<IdentityPlan> {
        let sync_service = self.sync_service.clone();
        let plan = crate::runtime::RUNTIME
            .spawn(async move { sync_service.confirm_remote_initialization().await })
            .await
            .map_err(|e| anyhow!("远程数据库初始化任务失败: {e}"))??;
        self.notify_data_changed();
        self.notify_ui_changed();
        Ok(plan)
    }

    pub async fn confirm_remote_adoption(&self) -> Result<IdentityPlan> {
        let sync_service = self.sync_service.clone();
        let plan = crate::runtime::RUNTIME
            .spawn(async move { sync_service.confirm_remote_adoption().await })
            .await
            .map_err(|e| anyhow!("远程数据库接管任务失败: {e}"))??;
        self.notify_data_changed();
        self.notify_ui_changed();
        Ok(plan)
    }

    pub async fn clear_remote_database(&self) -> Result<()> {
        let sync_service = self.sync_service.clone();
        crate::runtime::RUNTIME
            .spawn(async move { sync_service.clear_remote_database().await })
            .await
            .map_err(|e| anyhow!("清空远程数据库任务失败: {e}"))??;
        self.notify_data_changed();
        self.notify_ui_changed();
        Ok(())
    }

    pub async fn clear_remote_files(&self) -> Result<()> {
        let sync_service = self.sync_service.clone();
        crate::runtime::RUNTIME
            .spawn(async move { sync_service.clear_remote_files().await })
            .await
            .map_err(|e| anyhow!("清空远程文件任务失败: {e}"))??;
        self.notify_ui_changed();
        Ok(())
    }

    pub fn list_database_sync_conflicts(
        &self,
    ) -> Result<Vec<crate::database_sync::UiDatabaseSyncConflict>> {
        self.sync_service.list_database_sync_conflicts()
    }

    pub fn choose_remote_database_conflict(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<()> {
        self.sync_service
            .choose_remote_database_conflict(entity_type, entity_id)
    }

    pub fn keep_local_database_conflict(&self, entity_type: &str, entity_id: &str) -> Result<()> {
        self.sync_service
            .keep_local_database_conflict(entity_type, entity_id)
    }

    pub fn database_sync_summary(&self) -> Result<Option<DatabaseSyncSummary>> {
        self.sync_service.database_sync_summary()
    }

    /// 内部助手：消除“操作 -> 通知 -> 返回”模板代码
    pub(super) fn op_notify<R>(&self, op: impl FnOnce() -> Result<R>) -> Result<R> {
        let res = op()?;
        self.notify_data_changed();
        Ok(res)
    }
}
