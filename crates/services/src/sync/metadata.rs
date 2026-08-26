//! 元数据同步服务模块 (`MySQL`)
//!
//! 前端服务层：负责文献元数据与 `MySQL` 数据库的双向同步编排。
//!
//! 解耦说明：不再依赖 `MainApp`。原 `app.sync_state.lock()` 改戳注入的
//! `sync_state: Arc<std::sync::Mutex<SyncStateInner>>`；`app.notify_ui_changed()`
//! 改为注入的 `notify_ui` 闭包；`app.refresh_all_data()`（= `notify_data_changed`
//! = `request_sync` + `DataChanged`）改为 `notify_data` 闭包——仅发 `DataChanged`，
//! 与 `MainApp::data_changed_notify` 语义对齐，消除同步成功后的无限重触发隐患。

use crate::runtime::RUNTIME;
use crate::sync::progress::{SyncStateInner, SyncStatus};
use anyhow::{Result, anyhow};
use database::{Database, MySqlManager};
use log::{debug, error, info, warn};
use models::config::AppConfig;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;

/// 元数据同步服务
pub struct SQLSyncService {
    db: Arc<Database>,
    mysql: Arc<MySqlManager>,
    sync_status: Arc<AsyncMutex<SyncStatus>>,
    attachment_dir: Arc<std::sync::RwLock<std::path::PathBuf>>,
    /// 共享同步状态（由 `MainApp` 注入，跨线程）
    sync_state: Arc<std::sync::Mutex<SyncStateInner>>,
    /// UI 变更通知闭包（注入）
    notify_ui: Arc<dyn Fn() + Send + Sync>,
    /// 数据刷新通知闭包（注入，仅发 `DataChanged`）
    notify_data: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for SQLSyncService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SQLSyncService")
            .field("db", &self.db)
            .finish()
    }
}

impl SQLSyncService {
    pub fn new(
        db: Arc<Database>,
        config: &AppConfig,
        sync_state: Arc<std::sync::Mutex<SyncStateInner>>,
        notify_data: Arc<dyn Fn() + Send + Sync>,
        notify_ui: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        info!("存储管理: [SQL] 正在初始化存储管理器...");

        let manager = Self {
            db,
            mysql: Arc::new(MySqlManager::new(config.database.clone())),
            sync_status: Arc::new(AsyncMutex::new(SyncStatus::Idle)),
            attachment_dir: Arc::new(std::sync::RwLock::new(config.attachment_path.clone())),
            sync_state,
            notify_ui,
            notify_data,
        };

        info!("存储管理: [SQL] 初始化完成");
        Ok(manager)
    }

    pub fn update_config(&self, config: &AppConfig) {
        info!("存储管理: [SQL] 正在更新同步配置...");
        let old_path = self.attachment_dir.read().map(|r| r.clone()).ok();
        if let Some(pool) = self.mysql.update_config(config.database.clone()) {
            RUNTIME.spawn(async move {
                if let Err(e) = pool.disconnect().await {
                    error!("MySQL: 断开旧连接池失败: {e}");
                }
            });
        }
        if let Ok(mut w) = self.attachment_dir.write() {
            let new_path = config.attachment_path.clone();
            if Some(&new_path) != old_path.as_ref() {
                debug!(
                    "存储管理: [SQL] 附件目录已变更: {:?} -> {:?}",
                    old_path, new_path
                );
            }
            *w = new_path;
        }
    }

    pub fn perform_full_sync(
        &self,
        auto_sync_paused: Arc<AsyncMutex<bool>>,
        allowed_attachment_ids: Option<Vec<String>>,
    ) -> tokio::task::JoinHandle<()> {
        let status_mutex = self.sync_status.clone();
        let mysql = self.mysql.clone();
        let mysql_for_reset = mysql.clone();
        let db = self.db.clone();
        let attachment_dir = self.attachment_dir.clone();
        let sync_state = self.sync_state.clone();
        let notify_ui = self.notify_ui.clone();
        let notify_data = self.notify_data.clone();

        RUNTIME.spawn(async move {
            {
                let mut status = status_mutex.lock().await;
                if *status == SyncStatus::Syncing {
                    warn!("存储管理: 另一次同步已在运行中，跳过本次全量同步请求");
                    return;
                }
                *status = SyncStatus::Syncing;
            }
            debug!("存储管理: [SQL] 同步状态 -> Syncing");
            if let Ok(mut state) = sync_state.lock() {
                state.sync_status = SyncStatus::Syncing;
            }
            (notify_ui)();

            info!("存储管理: 开始执行全量同步 (MySQL)...");
            let start = std::time::Instant::now();

            let base_path = attachment_dir.read().unwrap().clone();
            // 单独 spawn 元数据同步，避免其内部 panic 直接杀掉本任务、
            // 导致 sync_status 永远卡在 Syncing（之前“一直同步中”的根因之一）。
            // 编排逻辑已迁至 `services::sync::remote::sync_metadata`（database 仅留存储原语）。
            let db_clone = db.clone();
            let allowed = allowed_attachment_ids.map(|v| v.to_vec());
            let sync_handle = RUNTIME.spawn(async move {
                crate::sync::remote::sync_metadata(&mysql, db_clone, &base_path, allowed.as_deref())
                    .await
            });
            let sync_result = match sync_handle.await {
                Ok(r) => r,
                Err(e) => {
                    error!("存储管理: [SQL] 元数据同步任务崩溃 (panic): {e}");
                    Err(anyhow!("元数据同步任务崩溃: {e}"))
                }
            };

            let elapsed = start.elapsed();

            let final_status = match sync_result {
                Ok(conflicts) => {
                    if conflicts.is_empty() {
                        info!("存储管理: 元数据同步成功 (耗时: {elapsed:?})，正在刷新内存数据...");
                        (notify_data)();
                        SyncStatus::Idle
                    } else {
                        warn!(
                            "存储管理: 元数据同步完成 (耗时: {elapsed:?})，但发现 {} 个冲突项",
                            conflicts.len()
                        );
                        SyncStatus::Conflict(conflicts)
                    }
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    error!("存储管理: 元数据同步失败 (耗时: {elapsed:?}): {err_msg}");
                    let cfg = mysql_for_reset.get_config();
                    let target = format!("同步数据库 {}:{}", cfg.host, cfg.port);
                    let kind = crate::sync::error::classify_sync_error(&e);
                    let friendly =
                        crate::sync::error::format_sync_error_with_kind(kind, &e, &target);
                    // 网络类错误：丢弃可能残留死连接的旧连接池，下次同步重建，
                    // 避免切换网络后一直复用已断开的 TCP 连接。
                    if kind == crate::sync::error::SyncErrorKind::Network
                        && let Some(old) = mysql_for_reset.update_config(cfg)
                    {
                        RUNTIME.spawn(async move {
                            if let Err(e) = old.disconnect().await {
                                error!("MySQL: 断开旧连接池失败: {e}");
                            }
                        });
                    }
                    *auto_sync_paused.lock().await = true;
                    SyncStatus::Error(friendly)
                }
            };

            *status_mutex.lock().await = final_status.clone();
            debug!("存储管理: [SQL] 同步状态 -> {:?}", final_status);
            if let Ok(mut state) = sync_state.lock() {
                state.sync_status = final_status;
            }
            (notify_ui)();
            info!("存储管理: 全量同步流程执行结束");
        })
    }

    pub async fn test_mysql_config(&self, config: database::DatabaseConfig) -> anyhow::Result<()> {
        let host = config.host.clone();
        debug!("存储管理: 正在测试 MySQL 连接配置 (Host: {host})");
        let handle =
            RUNTIME.spawn(async move { MySqlManager::new(config).test_connection().await });
        let result = handle.await.map_err(|e| anyhow::anyhow!("任务失败: {e}"))?;
        match &result {
            Ok(()) => info!("存储管理: MySQL 连接测试通过 (Host: {host})"),
            Err(e) => error!("存储管理: MySQL 连接测试失败 (Host: {host}): {e}"),
        }
        result
    }

    pub async fn clear_remote_data(&self) -> anyhow::Result<()> {
        info!("存储管理: 开始清空远程数据...");
        let mysql = self.mysql.clone();
        let db = self.db.clone();
        let handle = RUNTIME.spawn(async move {
            debug!("存储管理: [SQL] 正在清空 MySQL 端所有数据...");
            let r = mysql.clear_all_data().await;
            match &r {
                Ok(()) => {
                    info!("存储管理: [SQL] 远程数据已清空");
                    if let Err(e) = db.clear_sync_timestamps() {
                        error!("存储管理: 清空同步时间戳失败: {e}");
                    }
                }
                Err(e) => error!("存储管理: [SQL] 清空远程数据失败: {e}"),
            }
            r
        });
        handle.await.map_err(|e| anyhow::anyhow!("任务失败: {e}"))?
    }

    pub async fn purge_deleted_data(&self) -> anyhow::Result<usize> {
        self.mysql.purge_deleted_data().await
    }

    pub async fn get_sync_status(&self) -> SyncStatus {
        self.sync_status.lock().await.clone()
    }
}
