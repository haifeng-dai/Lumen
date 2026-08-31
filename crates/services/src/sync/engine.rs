//! 同步引擎模块
//!
//! 作为同步协调器，负责统一管理和调度附件同步和元数据同步任务，
//! 提供自动同步循环控制。
//!
//! 解耦说明：本模块不再依赖 `MainApp`。原 `Arc<MainApp>` 仅用于三件事——
//! 戳 `sync_state`、发 UI 变更通知、触发内存刷新——现统一改为构造时注入
//! `sync_state: Arc<Mutex<SyncStateInner>>` 与 `notify_data` / `notify_ui` 两个 `'static` 闭包。
//! `notify_data` 仅发 `DataChanged`（与 `MainApp::data_changed_notify` 语义一致，
//! 不再顺带 `request_sync`，避免同步成功后无限重触发）。

use crate::database_sync::{
    DatabaseSyncRunResult, DatabaseSyncService, DownloadResult, IdentityDecision, IdentityPlan,
};
use crate::runtime::RUNTIME;
use crate::sync::attachments::FileSyncService;
use crate::sync::progress::SyncStateInner;
use anyhow::Result;
use database::{Database, DatabaseSyncSummary, SyncConflict};
use file::LocalFileManager;
use log::{debug, info, warn};
use models::config::AppConfig;
use std::collections::HashMap;
use std::{sync::Arc, time::Instant};
use tokio::{
    sync::{Mutex, mpsc},
    time::{Duration, interval, sleep},
};
use uuid::Uuid;

#[derive(Clone, Debug)]
struct SyncRunLog {
    run_id: String,
    started_at: Instant,
}

impl SyncRunLog {
    fn new() -> Self {
        Self {
            run_id: Uuid::new_v4().simple().to_string()[..8].to_string(),
            started_at: Instant::now(),
        }
    }

    fn event(&self, stage: &str, event: &str, fields: &str) {
        let elapsed_ms = self.started_at.elapsed().as_millis();
        if fields.is_empty() {
            info!(
                "[Sync][run={}][stage={stage}] event={event} elapsed_ms={elapsed_ms}",
                self.run_id
            );
        } else {
            info!(
                "[Sync][run={}][stage={stage}] event={event} elapsed_ms={elapsed_ms} {fields}",
                self.run_id
            );
        }
    }
}

fn sync_error_category(error: &anyhow::Error) -> &'static str {
    let text = error.to_string().to_ascii_lowercase();
    if text.contains("read-only") || text.contains("not read-only") {
        "read_only"
    } else if text.contains("timeout") || text.contains("timed out") {
        "timeout"
    } else if text.contains("connection") || text.contains("connect") {
        "connection"
    } else {
        "unknown"
    }
}

fn database_sync_result_fields(result: &DatabaseSyncRunResult) -> String {
    let (downloaded, download_conflicts) = match result.download.as_ref() {
        Some(DownloadResult::Applied { records, conflicts }) => (*records, *conflicts),
        _ => (0, 0),
    };
    let (uploaded, upload_conflicts) = result
        .upload
        .as_ref()
        .map(|value| (value.uploaded, value.conflicts))
        .unwrap_or_default();
    let outcome = if result.failures == 0 {
        "complete"
    } else {
        "partial_failure"
    };
    format!(
        "outcome={outcome} downloaded={downloaded} uploaded={uploaded} conflicts={} failures={}",
        download_conflicts + upload_conflicts,
        result.failures
    )
}

/// 同步协调器
pub struct SyncService {
    pub db: Arc<Database>,
    pub file_manager: LocalFileManager,
    file_sync: Arc<FileSyncService>,
    database_sync: Arc<DatabaseSyncService>,
    auto_sync_paused: Arc<Mutex<bool>>,
    sync_trigger: mpsc::Sender<()>,
    /// 串行化标志：同一时刻只允许一个全量同步流程（上传+元数据+下载）运行，
    /// 避免并发同步共享同一 SQLite 连接互相争用导致 PULL 阶段卡死。
    sync_in_progress: Arc<std::sync::Mutex<bool>>,
    pub pending_renames: Arc<std::sync::Mutex<HashMap<String, String>>>,
}

#[cfg(test)]
mod tests {
    use super::{SyncRunLog, database_sync_result_fields};
    use crate::database_sync::{DatabaseSyncRunResult, DownloadResult, UploadResult};

    #[test]
    fn run_log_uses_a_short_id_shared_by_all_events() {
        let log = SyncRunLog::new();
        assert_eq!(log.run_id.len(), 8);
        assert!(log.run_id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(log.run_id, log.run_id.clone());
    }

    #[test]
    fn database_result_log_marks_partial_failure_and_counts_outcomes() {
        let fields = database_sync_result_fields(&DatabaseSyncRunResult {
            download: Some(DownloadResult::Applied {
                records: 3,
                conflicts: 1,
            }),
            upload: Some(UploadResult {
                uploaded: 2,
                conflicts: 1,
                failures: 1,
                ..Default::default()
            }),
            failures: 1,
            ..Default::default()
        });
        assert!(fields.contains("outcome=partial_failure"));
        assert!(fields.contains("downloaded=3"));
        assert!(fields.contains("uploaded=2"));
        assert!(fields.contains("conflicts=2"));
        assert!(fields.contains("failures=1"));
    }
}

impl std::fmt::Debug for SyncService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncService")
            .field("file_sync", &self.file_sync)
            .field("database_sync", &self.database_sync)
            .finish()
    }
}

impl SyncService {
    /// 创建新的同步协调器
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: &AppConfig,
        backend_name: &str,
        backend_config_json: &str,
        on_demand: bool,
        sync_state: Arc<std::sync::Mutex<SyncStateInner>>,
        notify_ui: Arc<dyn Fn() + Send + Sync>,
        notify_data: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<(Self, mpsc::Receiver<()>)> {
        info!("存储管理: 正在初始化同步协调器...");
        let db = Arc::new(Database::new(config.get_database_path())?);
        let file_manager = LocalFileManager::new(config.attachment_path.clone())?;

        info!(
            "存储管理: 共享资源初始化完成 (数据库: {:?})",
            config.get_database_path()
        );

        let pending_renames = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let backend = file::create_backend(backend_name, backend_config_json);

        let file_sync = FileSyncService::new(
            db.clone(),
            file_manager.clone(),
            backend,
            pending_renames.clone(),
            sync_state.clone(),
            notify_ui.clone(),
        );
        file_sync.set_on_demand(on_demand);

        let manager = Arc::new(database::MySqlManager::new(config.database.clone()));
        let database_sync = DatabaseSyncService::with_progress(
            db.clone(),
            manager,
            sync_state.clone(),
            notify_ui.clone(),
            notify_data,
        );

        let (tx, rx) = mpsc::channel(32);

        let manager = Self {
            db: db.clone(),
            file_manager: file_manager.clone(),
            file_sync: Arc::new(file_sync),
            database_sync: Arc::new(database_sync),
            auto_sync_paused: Arc::new(Mutex::new(false)),
            sync_trigger: tx,
            sync_in_progress: Arc::new(std::sync::Mutex::new(false)),
            pending_renames,
        };

        info!("存储管理: 同步协调器初始化完成");
        Ok((manager, rx))
    }

    pub fn update_config(
        &self,
        config: &AppConfig,
        backend_name: &str,
        backend_config_json: &str,
        on_demand: bool,
    ) {
        info!("存储管理: 正在更新同步配置...");
        let backend = file::create_backend(backend_name, backend_config_json);
        self.file_sync.swap_backend(backend);
        self.file_sync.set_on_demand(on_demand);
        // MySQL manager keeps local identity/sequence; updating configuration only
        // retires the old pool and never resets synchronization state.
        self.database_sync.update_config(config);

        let paused = self.auto_sync_paused.clone();
        RUNTIME.spawn(async move {
            let mut p = paused.lock().await;
            if *p {
                info!("存储管理: 检测到配置更新，正在恢复自动同步任务");
                *p = false;
            } else {
                debug!("存储管理: 配置更新完成，自动同步状态未变更");
            }
        });
    }

    pub fn request_sync(&self) {
        debug!("存储管理: 接收到手动同步请求");
        let tx = self.sync_trigger.clone();
        RUNTIME.spawn(async move {
            let _ = tx.send(()).await;
        });
    }

    pub fn start_auto_sync_loop(self: Arc<Self>, mut receiver: mpsc::Receiver<()>) {
        let manager = self.clone();
        RUNTIME.spawn(async move {
            info!("自动同步控制循环已启动");
            sleep(Duration::from_secs(5)).await;
            manager.perform_full_sync();
            let mut heartbeat = interval(Duration::from_secs(60));
            // 吃掉 interval 首次立即返回的 tick，避免与上面的初始同步同时触发双发
            heartbeat.tick().await;
            loop {
                tokio::select! {
                    Some(()) = receiver.recv() => {
                        if *manager.auto_sync_paused.lock().await { debug!("自动同步已暂停，忽略变更信号"); continue; }
                        info!("检测到本地变更，准备同步 (15秒防抖)...");
                        sleep(Duration::from_secs(15)).await;
                        while receiver.try_recv().is_ok() {}
                        manager.perform_full_sync();
                    }
                    _ = heartbeat.tick() => {
                        if *manager.auto_sync_paused.lock().await {
                            debug!("自动同步已暂停，跳过心跳同步");
                            continue;
                        }
                        info!("执行周期性同步心跳...");
                        manager.perform_full_sync();
                    }
                }
            }
        });
    }

    pub async fn force_sync(&self) {
        info!("存储管理: 正在强制执行全量同步...");
        {
            let mut paused = self.auto_sync_paused.lock().await;
            *paused = false;
        }
        self.perform_full_sync();
    }

    pub fn perform_full_sync(&self) {
        // 串行化：同一时刻只允许一个全量同步流程
        if *self.sync_in_progress.lock().unwrap() {
            debug!("存储管理: 已有全量同步流程在运行，跳过本次请求");
            return;
        }
        *self.sync_in_progress.lock().unwrap() = true;
        let sync_in_progress = self.sync_in_progress.clone();
        let run_log = SyncRunLog::new();

        let file_sync = self.file_sync.clone();
        let database_sync = self.database_sync.clone();

        RUNTIME.spawn(async move {
            run_log.event("full_sync", "start", "");
            run_log.event("attachment_push", "start", "");
            match file_sync.sync_local_to_remote().await {
                Ok(ids) => run_log.event(
                    "attachment_push",
                    "complete",
                    &format!("uploaded={}", ids.len()),
                ),
                Err(error) => run_log.event(
                    "attachment_push",
                    "failed",
                    &format!(
                        "operation=attachment_push error_category={}",
                        sync_error_category(&error)
                    ),
                ),
            }

            run_log.event("database_sync", "start", "");
            match database_sync.run_with_context(&run_log.run_id).await {
                Ok(result) => run_log.event(
                    "database_sync",
                    "complete",
                    &database_sync_result_fields(&result),
                ),
                Err(error) => run_log.event(
                    "database_sync",
                    "failed",
                    &format!(
                        "operation=database_sync outcome=failed_before_transfer error_category={}",
                        sync_error_category(&error)
                    ),
                ),
            }

            run_log.event("attachment_pull", "start", "");
            match file_sync.sync_remote_to_local().await {
                Ok(()) => run_log.event("attachment_pull", "complete", ""),
                Err(error) => run_log.event(
                    "attachment_pull",
                    "failed",
                    &format!(
                        "operation=attachment_pull error_category={}",
                        sync_error_category(&error)
                    ),
                ),
            }

            run_log.event("full_sync", "complete", "");
            *sync_in_progress.lock().unwrap() = false;
        });
    }

    pub fn perform_attachments_sync(&self) {
        self.file_sync.perform_attachments_sync();
    }

    pub async fn test_backend_config(&self, name: &str, config_json: &str) -> Result<()> {
        self.file_sync.test_backend_config(name, config_json).await
    }

    pub async fn test_mysql_config(&self, config: models::DatabaseConfig) -> Result<()> {
        self.database_sync.test_mysql_config(config).await
    }

    /// UI-facing database synchronization operations.  These delegates keep
    /// database/mysql access below the services layer.
    pub async fn database_sync_preflight(&self) -> Result<IdentityDecision> {
        self.database_sync.preflight_identity().await
    }

    pub async fn confirm_remote_initialization(&self) -> Result<IdentityPlan> {
        self.database_sync.confirm_remote_initialization().await
    }

    pub async fn confirm_remote_adoption(&self) -> Result<IdentityPlan> {
        self.database_sync.confirm_remote_adoption().await
    }

    pub fn list_database_sync_conflicts(&self) -> Result<Vec<SyncConflict>> {
        self.database_sync.list_conflicts()
    }

    pub fn choose_remote_database_conflict(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<()> {
        self.database_sync.choose_remote(entity_type, entity_id)
    }

    pub fn keep_local_database_conflict(&self, entity_type: &str, entity_id: &str) -> Result<()> {
        self.database_sync
            .keep_local_or_merged(entity_type, entity_id)
    }

    pub fn database_sync_summary(&self) -> Result<Option<DatabaseSyncSummary>> {
        self.database_sync.last_summary()
    }

    pub async fn clear_remote_database(&self) -> Result<()> {
        info!("存储管理: 开始清空远程数据库...");
        self.database_sync.clear_remote_data().await?;
        // 清空远程后，重置新同步位置并让所有本地数据库记录重新进入
        // 安全的版本对照/上传流程；资料库身份仍需后续用户确认。
        self.db.reset_database_sync_state_after_remote_clear()?;
        self.db.clear_attachment_etags()?;
        info!("存储管理: 远程数据库清空完成，本地已标记全量重推");
        Ok(())
    }

    pub async fn clear_remote_files(&self) -> Result<()> {
        let file_sync = self.file_sync.clone();
        RUNTIME
            .spawn(async move { file_sync.clear_remote_files().await })
            .await
            .map_err(|e| anyhow::anyhow!("清空远程文件任务失败: {e}"))?
    }

    pub async fn purge_deleted_data(&self) -> Result<usize> {
        let remote_files = self.file_sync.purge_deleted_files().await?;
        let remote_rows = self.database_sync.purge_deleted_data().await?;
        let (local_rows, attachment_paths) = self.db.purge_all_deleted()?;
        for path in attachment_paths {
            if let Err(e) = self.file_manager.trash_file(&path) {
                warn!("存储管理: [Purge] 删除本地附件失败 '{}': {e}", path);
            }
        }
        info!(
            "存储管理: [Purge] 清理完成，远程文件 {} 个，远程记录 {} 条，本地记录 {} 条",
            remote_files, remote_rows, local_rows
        );
        Ok(remote_files + remote_rows + local_rows)
    }

    pub async fn delete_remote_file(&self, filename: &str) -> Result<()> {
        self.file_sync.delete_remote_file(filename).await
    }

    pub async fn rename_remote_file(&self, old_name: &str, new_name: &str) -> Result<()> {
        self.file_sync.rename_remote_file(old_name, new_name).await
    }

    pub fn queue_remote_rename(&self, attachment_id: &str, old_filename: &str) {
        debug!(
            "存储管理: [Engine] 排队等待远程重命名: {} -> {old_filename}",
            attachment_id
        );
        if let Ok(mut map) = self.pending_renames.lock() {
            map.insert(attachment_id.to_string(), old_filename.to_string());
        }
    }

    pub async fn download_single_file(
        &self,
        attachment: &models::Attachment,
    ) -> anyhow::Result<bool> {
        self.file_sync.download_single_file(attachment).await
    }
}
