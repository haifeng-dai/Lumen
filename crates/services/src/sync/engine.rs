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

use crate::runtime::RUNTIME;
use crate::sync::attachments::FileSyncService;
use crate::sync::metadata::SQLSyncService;
use crate::sync::progress::{SyncStateInner, SyncStatus};
use anyhow::Result;
use database::Database;
use file::LocalFileManager;
use log::{debug, error, info, warn};
use models::config::AppConfig;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::{
    sync::{Mutex, mpsc},
    time::{Duration, interval, sleep},
};

/// 同步协调器
pub struct SyncService {
    pub db: Arc<Database>,
    pub file_manager: LocalFileManager,
    file_sync: Arc<FileSyncService>,
    sql_sync: Arc<SQLSyncService>,
    auto_sync_paused: Arc<Mutex<bool>>,
    sync_trigger: mpsc::Sender<()>,
    /// 串行化标志：同一时刻只允许一个全量同步流程（上传+元数据+下载）运行，
    /// 避免并发同步共享同一 SQLite 连接互相争用导致 PULL 阶段卡死。
    sync_in_progress: Arc<std::sync::Mutex<bool>>,
    pub pending_renames: Arc<std::sync::Mutex<HashMap<String, String>>>,
}

impl std::fmt::Debug for SyncService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncService")
            .field("file_sync", &self.file_sync)
            .field("sql_sync", &self.sql_sync)
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
        notify_data: Arc<dyn Fn() + Send + Sync>,
        notify_ui: Arc<dyn Fn() + Send + Sync>,
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

        let sql_sync = SQLSyncService::new(
            db.clone(),
            config,
            sync_state.clone(),
            notify_data,
            notify_ui,
        )?;

        let (tx, rx) = mpsc::channel(32);

        let manager = Self {
            db: db.clone(),
            file_manager: file_manager.clone(),
            file_sync: Arc::new(file_sync),
            sql_sync: Arc::new(sql_sync),
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
        self.sql_sync.update_config(config);

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

        let file_sync = self.file_sync.clone();
        let sql_sync = self.sql_sync.clone();
        let auto_sync_paused = self.auto_sync_paused.clone();

        RUNTIME.spawn(async move {
            info!("存储管理: 开始执行全量同步流程 (Push -> MySQL -> Pull)...");

            let successfully_uploaded_ids = match file_sync.sync_local_to_remote().await {
                Ok(ids) => {
                    info!(
                        "存储管理: [Engine] 上传阶段完成，成功上传 {} 个附件",
                        ids.len()
                    );
                    Some(ids)
                }
                Err(e) => {
                    error!("存储管理: [Engine] 上传阶段失败: {e}");
                    Some(Vec::new())
                }
            };

            let handle =
                sql_sync.perform_full_sync(auto_sync_paused.clone(), successfully_uploaded_ids);
            match handle.await {
                Ok(()) => info!("存储管理: [Engine] 元数据同步阶段正常完成"),
                Err(e) => error!("存储管理: [Engine] 元数据同步任务崩溃: {e}"),
            }

            match sql_sync.get_sync_status().await {
                SyncStatus::Idle => info!("存储管理: [Engine] 元数据同步状态: Idle"),
                SyncStatus::Conflict(conflicts) => warn!(
                    "存储管理: [Engine] 元数据同步完成，发现 {} 个冲突",
                    conflicts.len()
                ),
                SyncStatus::Error(msg) => {
                    error!("存储管理: [Engine] 元数据同步失败: {msg}");
                    warn!("存储管理: 因同步错误暂停自动同步");
                    let mut p = auto_sync_paused.lock().await;
                    *p = true;
                }
                _ => {}
            }

            if let Err(e) = file_sync.sync_remote_to_local().await {
                error!("存储管理: [Engine] 下载阶段失败: {e}");
            }

            info!("存储管理: 全量同步流程结束");
            *sync_in_progress.lock().unwrap() = false;
        });
    }

    pub fn perform_attachments_sync(&self) {
        self.file_sync.perform_attachments_sync();
    }

    pub async fn test_backend_config(&self, name: &str, config_json: &str) -> Result<()> {
        self.file_sync.test_backend_config(name, config_json).await
    }

    pub async fn test_mysql_config(&self, config: database::DatabaseConfig) -> Result<()> {
        self.sql_sync.test_mysql_config(config).await
    }

    pub async fn clear_remote_database(&self) -> Result<()> {
        info!("存储管理: 开始清空远程数据库...");
        self.sql_sync.clear_remote_data().await?;
        // 清空远程后，标记本地全部记录为 dirty，下次同步会全量推送
        self.db.mark_all_dirty_for_sync()?;
        self.db.clear_sync_timestamps()?;
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
        let remote_rows = self.sql_sync.purge_deleted_data().await?;
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
