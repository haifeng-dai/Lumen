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
use crate::sync::attachments::{FileLibraryPreflight, FileRoundSummary, FileSyncService};
use crate::sync::progress::{
    DatabaseDisposition, FileMutationPolicy, FileSyncErrorKind, FileSyncStatus,
    FileSyncSummaryView, SyncStateInner,
};
use anyhow::Result;
use database::sqlite::FileSyncSummary;
use database::{Database, DatabaseSyncSummary, SyncConflict};
use file::LocalFileManager;
use log::{debug, info, warn};
use models::config::AppConfig;
use std::{sync::Arc, time::Instant};
use tokio::{
    sync::{Mutex, MutexGuard, mpsc},
    time::{Duration, interval, sleep},
};
use uuid::Uuid;

#[derive(Clone, Debug)]
struct SyncRunLog {
    run_id: String,
    started_at: Instant,
    /// 测试观察点：记录每条事件的原始文本；生产路径不读取，仅用于确定性验证。
    recorded: Option<Arc<std::sync::Mutex<Vec<String>>>>,
}

impl SyncRunLog {
    fn new() -> Self {
        Self {
            run_id: Uuid::new_v4().simple().to_string()[..8].to_string(),
            started_at: Instant::now(),
            recorded: None,
        }
    }

    #[cfg(test)]
    fn with_recorder() -> (Self, Arc<std::sync::Mutex<Vec<String>>>) {
        let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = Self {
            recorded: Some(recorded.clone()),
            ..Self::new()
        };
        (log, recorded)
    }

    fn event(&self, stage: &str, event: &str, fields: &str) {
        let elapsed_ms = self.started_at.elapsed().as_millis();
        let line = if fields.is_empty() {
            format!(
                "[Sync][run={}][stage={stage}] event={event} elapsed_ms={elapsed_ms}",
                self.run_id
            )
        } else {
            format!(
                "[Sync][run={}][stage={stage}] event={event} elapsed_ms={elapsed_ms} {fields}",
                self.run_id
            )
        };
        info!("{line}");
        if let Some(recorded) = &self.recorded {
            if let Ok(mut entries) = recorded.lock() {
                entries.push(line);
            }
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

/// 本轮整体 outcome（overall 阶段）：Error 视为失败，PartialFailure 单列；
/// 身份阻断/禁用状态单列，绝不记为 complete。
fn overall_outcome(status: &FileSyncStatus) -> &'static str {
    match status {
        FileSyncStatus::Complete => "complete",
        FileSyncStatus::PartialFailure => "partial_failure",
        FileSyncStatus::Error(_) => "failed",
        FileSyncStatus::Disabled => "disabled",
        FileSyncStatus::WaitingForDatabaseIdentity => "waiting_database_identity",
        FileSyncStatus::InitializationRequired => "initialization_required",
        FileSyncStatus::UnidentifiedRemote => "unidentified_remote",
        FileSyncStatus::IdentityMismatch => "identity_mismatch",
        // Idle/Syncing 不应作为一轮的终态出现，单列以便发现问题
        FileSyncStatus::Idle | FileSyncStatus::Syncing => "unexpected_terminal_state",
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

/// 统一协调模式：Full 固定“数据库先、文件后”；FileOnly 仅跑文件轮次。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyncMode {
    Full,
    FileOnly,
}

/// 协调入口的运行结果：调用方必须消费，Busy 时不得当作已启动。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum SyncRunOutcome {
    /// 本轮同步已启动
    Started,
    /// 已有同步流程在运行，本轮请求未启动（未覆盖任何状态，也未启动第二轮）
    SkippedBusy,
}

/// 同步协调器
#[derive(Clone)]
pub struct SyncService {
    pub db: Arc<Database>,
    pub file_manager: LocalFileManager,
    file_sync: Arc<FileSyncService>,
    database_sync: Arc<DatabaseSyncService>,
    auto_sync_paused: Arc<Mutex<bool>>,
    sync_trigger: mpsc::Sender<()>,
    /// 统一协调锁（RAII）：Full 与 FileOnly 共用同一把锁，guard 在整个协调过程中持有，
    /// 提前返回 / 任务取消 / panic 时自动释放，禁止手工复位 bool。
    coordinator_lock: Arc<Mutex<()>>,
    /// 共享同步状态（数据库/file 状态真源），由协调器与内部服务共同写入。
    sync_state: Arc<std::sync::Mutex<SyncStateInner>>,
    notify_ui: Arc<dyn Fn() + Send + Sync>,
    notify_data: Arc<dyn Fn() + Send + Sync>,
    /// 测试注入点（仅测试构建存在）：非 None 时替代真实 Summary 写库，
    /// 用于确定性注入持久化失败，不依赖文件系统权限等平台语义。
    #[cfg(test)]
    summary_persist_override:
        Option<Arc<dyn Fn(&database::sqlite::FileSyncSummary) -> Result<()> + Send + Sync>>,
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

        let backend = file::create_backend(backend_name, backend_config_json);

        let file_sync =
            FileSyncService::new(db.clone(), file_manager.clone(), backend, notify_ui.clone());
        file_sync.set_on_demand(on_demand);

        let manager = Arc::new(database::MySqlManager::new(config.database.clone()));
        let database_sync = DatabaseSyncService::with_progress(
            db.clone(),
            manager,
            sync_state.clone(),
            notify_ui.clone(),
            notify_data.clone(),
        );

        let (tx, rx) = mpsc::channel(32);

        let manager = Self {
            db: db.clone(),
            file_manager: file_manager.clone(),
            file_sync: Arc::new(file_sync),
            database_sync: Arc::new(database_sync),
            auto_sync_paused: Arc::new(Mutex::new(false)),
            sync_trigger: tx,
            coordinator_lock: Arc::new(Mutex::new(())),
            sync_state: sync_state.clone(),
            notify_ui: notify_ui.clone(),
            notify_data,
            #[cfg(test)]
            summary_persist_override: None,
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
            // 自动同步：锁冲突只需日志可观测，不弹 UI 通知，避免反复打扰
            let _ = manager.perform_full_sync().await;
            let mut heartbeat = interval(Duration::from_secs(60));
            // 吃了 interval 首次立即返回的 tick，避免与上面的初始同步同时触发双发
            heartbeat.tick().await;
            loop {
                tokio::select! {
                    Some(()) = receiver.recv() => {
                        if *manager.auto_sync_paused.lock().await { debug!("自动同步已暂停，忽略变更信号"); continue; }
                        info!("检测到本地变更，准备同步 (15秒防抖)...");
                        sleep(Duration::from_secs(15)).await;
                        while receiver.try_recv().is_ok() {}
                        let _ = manager.perform_full_sync().await;
                    }
                    _ = heartbeat.tick() => {
                        if *manager.auto_sync_paused.lock().await {
                            debug!("自动同步已暂停，跳过心跳同步");
                            continue;
                        }
                        info!("执行周期性同步心跳...");
                        let _ = manager.perform_full_sync().await;
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
        let outcome = self.perform_full_sync().await;
        if outcome == SyncRunOutcome::SkippedBusy {
            debug!("存储管理: 强制同步请求因同步进行中被跳过");
        }
    }

    /// 启动 Full 同步并等待其完成；返回运行结果供调用方消费。
    ///
    /// RAII 锁：未拿到锁说明已有同步流程在运行，跳过本次请求（绝不静默启动第二轮），
    /// guard 在整个协调过程中持有，提前返回/任务取消/panic 时自动释放。
    pub async fn perform_full_sync(&self) -> SyncRunOutcome {
        let guard = match self.coordinator_lock.try_lock() {
            Ok(guard) => guard,
            Err(_) => return Self::log_skipped_busy(SyncMode::Full),
        };
        self.run_sync(SyncMode::Full, guard).await;
        SyncRunOutcome::Started
    }

    /// 启动 FileOnly 同步并等待其完成；语义同 `perform_full_sync`。
    pub async fn perform_file_only_sync(&self) -> SyncRunOutcome {
        let guard = match self.coordinator_lock.try_lock() {
            Ok(guard) => guard,
            Err(_) => return Self::log_skipped_busy(SyncMode::FileOnly),
        };
        self.run_sync(SyncMode::FileOnly, guard).await;
        SyncRunOutcome::Started
    }

    /// 锁冲突：本轮请求未启动。通过日志（coordinator/outcome=skipped_busy）与返回值
    /// 双通道可观测；绝不覆盖正在运行的 Syncing 状态，也绝不启动第二轮。
    fn log_skipped_busy(mode: SyncMode) -> SyncRunOutcome {
        SyncRunLog::new().event("coordinator", "skipped_busy", &format!("mode={mode:?}"));
        SyncRunOutcome::SkippedBusy
    }

    /// 统一协调入口：持有 coordinator_lock 期间完成数据库阶段、文件阶段、结果持久化、
    /// 状态写入与通知；Result/Summary 一律不得丢弃。
    async fn run_sync(&self, mode: SyncMode, _coordinator_guard: MutexGuard<'_, ()>) {
        let run_log = SyncRunLog::new();
        let run_id = run_log.run_id.clone();
        run_log.event("coordinator", "start", &format!("mode={mode:?}"));

        // 1. 数据库阶段（仅 Full）：保留类型化结果，不得压缩为 failures==0
        let disposition = if mode == SyncMode::Full {
            run_log.event("database", "start", "");
            match self.database_sync.run_with_context(&run_id).await {
                Ok(result) => {
                    run_log.event(
                        "database",
                        "complete",
                        &database_sync_result_fields(&result),
                    );
                    Self::disposition_from_db_result(&result)
                }
                Err(error) => {
                    run_log.event(
                        "database",
                        "failed",
                        &format!(
                            "operation=database_sync outcome=failed_before_transfer error_category={}",
                            sync_error_category(&error)
                        ),
                    );
                    DatabaseDisposition::Error
                }
            }
        } else {
            // FileOnly 不运行数据库阶段；逐附件确认后允许变更（策略 allow_all）。
            DatabaseDisposition::Disabled
        };

        // 2. 由数据库 disposition 推导本轮文件变更策略（Full）；FileOnly 直接 allow_all
        let policy = if mode == SyncMode::Full {
            FileMutationPolicy::from_disposition(disposition)
        } else {
            FileMutationPolicy::allow_all()
        };

        // 3. 文件阶段：先置 Syncing，round 内部重新读取数据库确认后的快照。
        // preflight/list/plan/transfer 在文件引擎内部无法准确拆分，统一使用 file 阶段。
        run_log.event("file", "start", "");
        if let Ok(mut st) = self.sync_state.lock() {
            st.file_sync_status = FileSyncStatus::Syncing;
        }
        let status = match self.file_sync.sync_file_library_round(policy).await {
            Ok(summary) => {
                run_log.event(
                    "file",
                    "complete",
                    &format!(
                        "uploaded={} downloaded={} deleted={} skipped={} waiting={} pending={} conflicts={} unknown={} unrecoverable={} failed={}",
                        summary.uploaded,
                        summary.downloaded,
                        summary.deleted,
                        summary.skipped,
                        summary.waiting,
                        summary.pending_download,
                        summary.conflicts,
                        summary.unknown_divergence,
                        summary.unrecoverable_missing,
                        summary.failed
                    ),
                );
                self.finalize_round_status(&run_log, &summary).await
            }
            Err(error) => {
                run_log.event(
                    "file",
                    "failed",
                    &format!(
                        "operation=attachment_round error_category={}",
                        sync_error_category(&error)
                    ),
                );
                // 文件轮次失败：写入终态并通知 UI（不覆盖数据库状态）
                let status = FileSyncStatus::Error(FileSyncErrorKind::TransferFailed);
                if let Ok(mut st) = self.sync_state.lock() {
                    st.file_sync_status = status.clone();
                }
                (self.notify_data)();
                (self.notify_ui)();
                status
            }
        };

        run_log.event(
            "overall",
            overall_outcome(&status),
            &format!("file_state={}", status.state_name()),
        );
    }

    /// 由文件轮次结果收敛终态：持久化成功保留轮次终态；持久化失败降级为
    /// `SummaryPersistFailed`，绝不报告 Complete。
    /// 无论持久化成败，都写入内存终态并通知 UI。
    async fn finalize_round_status(
        &self,
        run_log: &SyncRunLog,
        round: &FileRoundSummary,
    ) -> FileSyncStatus {
        let (status, reason) = Self::file_status_from_round(round);
        let status = match self.persist_file_sync_summary(
            &run_log.run_id,
            round,
            &status,
            reason.as_deref(),
        ) {
            Ok(()) => {
                run_log.event("summary_persist", "complete", "");
                status
            }
            Err(error) => {
                run_log.event(
                    "summary_persist",
                    "failed",
                    &format!(
                        "operation=summary_persist error_category={}",
                        sync_error_category(&error)
                    ),
                );
                FileSyncStatus::Error(FileSyncErrorKind::SummaryPersistFailed)
            }
        };
        // 写文件终态（不覆盖数据库状态；数据库状态由 database_sync 服务维护）
        if let Ok(mut st) = self.sync_state.lock() {
            st.file_sync_status = status.clone();
        }
        (self.notify_data)();
        (self.notify_ui)();
        status
    }

    /// 由数据库同步结果推导类型化 disposition（区分成功与部分失败，禁止仅看 failures==0）。
    fn disposition_from_db_result(result: &DatabaseSyncRunResult) -> DatabaseDisposition {
        if result.failures == 0 {
            DatabaseDisposition::CompleteReady
        } else {
            DatabaseDisposition::PartialFailure
        }
    }

    /// 由文件轮次结果推导类型化文件状态，返回 (状态, 脱敏原因)。
    ///
    /// preflight 非 Ready 直接映射身份/禁用状态；Ready 时按计数判定 Complete/PartialFailure/Error。
    fn file_status_from_round(summary: &FileRoundSummary) -> (FileSyncStatus, Option<String>) {
        match &summary.preflight {
            FileLibraryPreflight::Ready { .. } => {
                if summary.failed == 0 {
                    (FileSyncStatus::Complete, None)
                } else if summary.uploaded
                    + summary.downloaded
                    + summary.deleted
                    + summary.skipped
                    + summary.waiting
                    + summary.conflicts
                    + summary.unknown_divergence
                    + summary.pending_download
                    + summary.unrecoverable_missing
                    > 0
                {
                    (
                        FileSyncStatus::PartialFailure,
                        Some("partial_failure".to_string()),
                    )
                } else {
                    (
                        FileSyncStatus::Error(FileSyncErrorKind::TransferFailed),
                        Some("transfer_failed".to_string()),
                    )
                }
            }
            other => (FileSyncStatus::from_preflight(other), None),
        }
    }

    /// 持久化完整文件同步结果。
    ///
    /// 失败以 `Result` 上抛给 `finalize_round_status`，由其降级终态并记录
    /// `summary_persist` 事件；本函数只做原语写入，不吞错误、不打日志。
    /// 日志字段禁止携带数据库错误正文/路径/SQLite 内容。
    fn persist_file_sync_summary(
        &self,
        run_id: &str,
        round: &FileRoundSummary,
        status: &FileSyncStatus,
        reason: Option<&str>,
    ) -> Result<()> {
        let file_library_id = match &round.preflight {
            FileLibraryPreflight::Ready {
                file_library_id, ..
            } => Some(file_library_id.clone()),
            _ => None,
        };
        let summary = FileSyncSummary {
            uploaded: round.uploaded,
            downloaded: round.downloaded,
            deleted: round.deleted,
            skipped: round.skipped,
            waiting: round.waiting,
            pending_download: round.pending_download,
            unrecoverable_missing: round.unrecoverable_missing,
            conflicts: round.conflicts,
            unknown_divergence: round.unknown_divergence,
            failures: round.failed,
            state: status.state_name().to_string(),
            reason: reason.map(|s| s.to_string()),
            run_id: run_id.to_string(),
            file_library_id,
            updated_at: chrono::Utc::now().timestamp(),
        };
        #[cfg(test)]
        if let Some(override_write) = &self.summary_persist_override {
            return override_write(&summary);
        }
        self.db
            .set_file_sync_summary(&summary)
            .map_err(anyhow::Error::from)
    }

    /// 只读接口：最近一次文件同步摘要（UI 不得直接访问 database）。
    ///
    /// 无记录返回 `Ok(None)`；记录损坏（状态类别无法识别）返回 `Err`，
    /// 调用方必须呈现不可用状态，不得降级为 Complete。
    pub fn file_sync_summary(&self) -> Result<Option<FileSyncSummaryView>> {
        match self.db.get_file_sync_summary()? {
            Some(summary) => Ok(Some(FileSyncSummaryView::from_persisted(&summary)?)),
            None => Ok(None),
        }
    }

    pub async fn test_backend_config(&self, name: &str, config_json: &str) -> Result<()> {
        self.file_sync.test_backend_config(name, config_json).await
    }

    pub async fn test_mysql_config(&self, config: models::DatabaseConfig) -> Result<()> {
        self.database_sync.test_mysql_config(config).await
    }

    /// UI-facing database synchronization operations.
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
        info!("存储管理: 开始清空远端数据库...");
        self.database_sync.clear_remote_data().await?;
        self.db.reset_database_sync_state_after_remote_clear()?;
        self.db.clear_attachment_etags()?;
        info!("存储管理: 远端数据库清空完成，本地已标记全量重推");
        Ok(())
    }

    pub async fn clear_remote_files(&self) -> Result<()> {
        self.file_sync.clear_remote_files().await
    }

    pub async fn purge_deleted_data(&self) -> Result<usize> {
        let remote_rows = self.database_sync.purge_deleted_data().await?;
        let (local_rows, attachment_paths) = self.db.purge_all_deleted()?;
        for path in attachment_paths {
            if let Err(e) = self.file_manager.trash_file(&path) {
                warn!("存储管理: [Purge] 删除本地附件失败 '{}': {e}", path);
            }
        }
        info!(
            "存储管理: [Purge] 清理完成，远端记录 {} 条，本地记录 {} 条",
            remote_rows, local_rows
        );
        Ok(remote_rows + local_rows)
    }

    pub async fn file_library_preflight(&self) -> crate::sync::attachments::FileLibraryPreflight {
        self.file_sync.preflight().await
    }

    pub async fn confirm_file_library_initialization(
        &self,
    ) -> Result<crate::sync::attachments::FileRoundSummary> {
        self.file_sync.confirm_file_library_initialization().await
    }

    pub async fn download_single_attachment(&self, attachment_id: &str) -> Result<bool> {
        self.file_sync
            .download_single_attachment(attachment_id)
            .await
    }

    pub async fn prepare_attachment_for_open(
        &self,
        attachment_id: &str,
    ) -> Result<crate::sync::attachments::PreparedAttachment> {
        self.file_sync
            .prepare_attachment_for_open(attachment_id)
            .await
    }

    pub async fn get_attachment_sync_issue(
        &self,
        attachment_id: &str,
    ) -> Result<Option<crate::sync::attachments::AttachmentSyncIssue>> {
        self.file_sync
            .get_attachment_sync_issue(attachment_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SyncRunLog, SyncRunOutcome, SyncService, database_sync_result_fields, overall_outcome,
    };
    use crate::database_sync::{
        DatabaseSyncRunResult, DatabaseSyncService, DownloadResult, UploadResult,
    };
    use crate::sync::attachments::{FileLibraryPreflight, FileRoundSummary, FileSyncService};
    use crate::sync::progress::{
        DatabaseDisposition, FileSyncErrorKind, FileSyncStatus, SyncStateInner,
    };
    use database::Database;
    use database::sqlite::FileSyncSummary;
    use file::LocalFileManager;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[allow(clippy::too_many_arguments)]
    fn ready_round(
        failed: usize,
        uploaded: usize,
        downloaded: usize,
        deleted: usize,
        skipped: usize,
        waiting: usize,
        conflicts: usize,
        unknown_divergence: usize,
        pending_download: usize,
        unrecoverable_missing: usize,
    ) -> FileRoundSummary {
        FileRoundSummary {
            preflight: FileLibraryPreflight::Ready {
                file_library_id: "flib-1".to_string(),
                database_library_id: "db-1".to_string(),
            },
            uploaded,
            downloaded,
            deleted,
            skipped,
            waiting,
            failed,
            conflicts,
            unknown_divergence,
            pending_download,
            unrecoverable_missing,
        }
    }

    #[test]
    fn disposition_from_db_result_maps_failures_to_typed_disposition() {
        assert_eq!(
            SyncService::disposition_from_db_result(&DatabaseSyncRunResult {
                download: None,
                upload: None,
                failures: 0,
                ..Default::default()
            }),
            DatabaseDisposition::CompleteReady
        );
        assert_eq!(
            SyncService::disposition_from_db_result(&DatabaseSyncRunResult {
                download: None,
                upload: None,
                failures: 2,
                ..Default::default()
            }),
            DatabaseDisposition::PartialFailure
        );
    }

    #[test]
    fn file_status_from_round_maps_counts_to_typed_status() {
        // Ready + 无失败 → Complete
        assert_eq!(
            SyncService::file_status_from_round(&ready_round(0, 1, 0, 0, 0, 0, 0, 0, 0, 0)).0,
            FileSyncStatus::Complete
        );

        // Ready + 有失败但也有成功传输 → PartialFailure
        assert_eq!(
            SyncService::file_status_from_round(&ready_round(1, 2, 0, 0, 0, 0, 0, 0, 0, 0)).0,
            FileSyncStatus::PartialFailure
        );

        // Ready + 仅失败、无任何传输/跳过 → Error（非 Complete）
        assert_eq!(
            SyncService::file_status_from_round(&ready_round(1, 0, 0, 0, 0, 0, 0, 0, 0, 0)).0,
            FileSyncStatus::Error(crate::sync::progress::FileSyncErrorKind::TransferFailed)
        );

        // preflight 非 Ready → 直接映射身份/禁用状态，不覆盖为 Complete
        let disabled = FileRoundSummary {
            preflight: FileLibraryPreflight::BackendDisabled,
            uploaded: 0,
            downloaded: 0,
            deleted: 0,
            skipped: 0,
            waiting: 0,
            failed: 0,
            conflicts: 0,
            unknown_divergence: 0,
            pending_download: 0,
            unrecoverable_missing: 0,
        };
        assert_eq!(
            SyncService::file_status_from_round(&disabled).0,
            FileSyncStatus::Disabled
        );
    }

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

    // ---------- 测试构造辅助 ----------

    struct TestNotifyCounter(Arc<AtomicUsize>);

    impl TestNotifyCounter {
        fn new() -> (Self, Arc<dyn Fn() + Send + Sync>) {
            let counter = Arc::new(AtomicUsize::new(0));
            let callback = {
                let counter = counter.clone();
                Arc::new(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                }) as Arc<dyn Fn() + Send + Sync>
            };
            (Self(counter), callback)
        }

        fn count(&self) -> usize {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-engine-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 直接构造协调器（测试模块可访问私有字段）；noop 后端保证不触网。
    fn test_sync_service(
        db: Arc<Database>,
        attachments_dir: &Path,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> SyncService {
        let file_manager = LocalFileManager::new(attachments_dir).unwrap();
        let backend = file::create_backend("noop", "");
        let file_sync =
            FileSyncService::new(db.clone(), file_manager.clone(), backend, notify.clone());
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        SyncService {
            db: db.clone(),
            file_manager,
            file_sync: Arc::new(file_sync),
            database_sync: Arc::new(DatabaseSyncService::new(db.clone())),
            auto_sync_paused: Arc::new(tokio::sync::Mutex::new(false)),
            sync_trigger: tx,
            coordinator_lock: Arc::new(tokio::sync::Mutex::new(())),
            sync_state: Arc::new(std::sync::Mutex::new(SyncStateInner::new())),
            notify_ui: notify.clone(),
            notify_data: notify,
            summary_persist_override: None,
        }
    }

    // ---------- A：Summary 持久化失败降级 ----------

    #[tokio::test]
    async fn summary_persist_success_keeps_round_terminal_status_and_notifies() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        let dir = unique_temp_dir("persist-ok");
        let (counter, notify) = TestNotifyCounter::new();
        let service = test_sync_service(db.clone(), &dir, notify);
        let run_log = SyncRunLog::new();
        let round = ready_round(0, 1, 2, 0, 0, 0, 0, 0, 0, 0);

        let status = service.finalize_round_status(&run_log, &round).await;

        assert_eq!(status, FileSyncStatus::Complete);
        // 内存终态已写入且 UI 已收到通知（成功路径同样通知）
        let state = service.sync_state.lock().unwrap();
        assert_eq!(state.file_sync_status, FileSyncStatus::Complete);
        drop(state);
        assert!(counter.count() >= 1);
        // 持久化记录可被只读接口读回
        let view = service.file_sync_summary().unwrap().unwrap();
        assert_eq!(view.state, FileSyncStatus::Complete);
        assert_eq!(view.uploaded, 1);
        assert_eq!(view.downloaded, 2);
        assert_eq!(view.failures, 0);
    }

    #[tokio::test]
    async fn summary_persist_failure_forces_summary_persist_failed_and_notifies() {
        let dir = unique_temp_dir("persist-fail");
        let db = Arc::new(Database::new(":memory:").unwrap());
        let (counter, notify) = TestNotifyCounter::new();
        let mut service = test_sync_service(db.clone(), &dir, notify);
        // 确定性注入持久化失败（测试专用 override，不依赖平台文件权限）
        service.summary_persist_override = Some(Arc::new(|_summary| {
            Err(anyhow::anyhow!("simulated summary persist failure"))
        }));
        let run_log = SyncRunLog::new();
        let round = ready_round(0, 3, 0, 0, 0, 0, 0, 0, 0, 0);

        let status = service.finalize_round_status(&run_log, &round).await;

        // 文件轮次本身零失败（本应 Complete），持久化失败必须降级为 SummaryPersistFailed
        assert_eq!(
            status,
            FileSyncStatus::Error(FileSyncErrorKind::SummaryPersistFailed)
        );
        // 内存终态写入 + UI 通知（不得静默）
        let state = service.sync_state.lock().unwrap();
        assert_eq!(
            state.file_sync_status,
            FileSyncStatus::Error(FileSyncErrorKind::SummaryPersistFailed)
        );
        drop(state);
        assert!(counter.count() >= 1);
    }

    // ---------- B：锁冲突可观测 ----------

    #[tokio::test]
    async fn full_sync_returns_skipped_busy_while_file_only_holds_lock() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        let dir = unique_temp_dir("busy-1");
        let (_counter, notify) = TestNotifyCounter::new();
        let service = test_sync_service(db.clone(), &dir, notify);
        let _guard = service.coordinator_lock.try_lock().unwrap();

        assert_eq!(
            service.perform_full_sync().await,
            SyncRunOutcome::SkippedBusy,
            "FileOnly 持锁时 Full 不得启动第二轮"
        );
    }

    #[tokio::test]
    async fn file_only_returns_skipped_busy_while_full_holds_lock() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        let dir = unique_temp_dir("busy-2");
        let (_counter, notify) = TestNotifyCounter::new();
        let service = test_sync_service(db.clone(), &dir, notify);
        let _guard = service.coordinator_lock.try_lock().unwrap();

        assert_eq!(
            service.perform_file_only_sync().await,
            SyncRunOutcome::SkippedBusy,
            "Full 持锁时 FileOnly 不得启动第二轮"
        );
    }

    #[tokio::test]
    async fn skipped_busy_preserves_running_syncing_state() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        let dir = unique_temp_dir("busy-3");
        let (_counter, notify) = TestNotifyCounter::new();
        let service = test_sync_service(db.clone(), &dir, notify);
        {
            let mut state = service.sync_state.lock().unwrap();
            state.file_sync_status = FileSyncStatus::Syncing;
        }
        let _guard = service.coordinator_lock.try_lock().unwrap();

        let outcome = service.perform_file_only_sync().await;

        assert_eq!(outcome, SyncRunOutcome::SkippedBusy);
        // Busy 跳过不得把正在运行的 Syncing 覆盖为 Idle/Error
        let state = service.sync_state.lock().unwrap();
        assert_eq!(state.file_sync_status, FileSyncStatus::Syncing);
    }

    // ---------- C：只读摘要接口 ----------

    #[tokio::test]
    async fn file_sync_summary_returns_none_when_missing_and_err_when_corrupt() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        let dir = unique_temp_dir("summary-view");
        let (_counter, notify) = TestNotifyCounter::new();
        let service = test_sync_service(db.clone(), &dir, notify);

        // 无记录：Ok(None)，不得伪造成功
        assert!(service.file_sync_summary().unwrap().is_none());

        // 损坏记录：Err，不得降级为 Complete
        let corrupt = FileSyncSummary {
            uploaded: 0,
            downloaded: 0,
            deleted: 0,
            skipped: 0,
            waiting: 0,
            pending_download: 0,
            unrecoverable_missing: 0,
            conflicts: 0,
            unknown_divergence: 0,
            failures: 0,
            state: "bogus-state".to_string(),
            reason: None,
            run_id: "run-corrupt".to_string(),
            file_library_id: None,
            updated_at: 0,
        };
        db.set_file_sync_summary(&corrupt).unwrap();
        let result = service.file_sync_summary();
        assert!(result.is_err(), "损坏记录必须报错，不得静默降级");
        let message = result.unwrap_err().to_string();
        assert!(message.contains("状态类别"), "错误需说明状态类别无法识别");
    }

    // ---------- D：日志阶段与 run_id ----------

    #[test]
    fn run_log_events_share_run_id_and_cover_all_stages() {
        let (log, recorder) = SyncRunLog::with_recorder();
        log.event("coordinator", "start", "mode=Full");
        log.event("database", "complete", "downloaded=1 failures=0");
        log.event("file", "complete", "uploaded=1 failed=0");
        log.event("summary_persist", "failed", "error_category=read_only");
        log.event("overall", "failed", "file_state=error");

        let entries = recorder.lock().unwrap().clone();
        assert_eq!(entries.len(), 5);
        let stages: Vec<&str> = entries
            .iter()
            .filter_map(|line| {
                let start = line.find("stage=")? + "stage=".len();
                let rest = &line[start..];
                Some(rest.split(']').next().unwrap())
            })
            .collect();
        assert_eq!(
            stages,
            vec![
                "coordinator",
                "database",
                "file",
                "summary_persist",
                "overall"
            ]
        );
        // 同一 run_id 贯穿全部阶段
        for line in &entries {
            assert!(line.contains(&format!("[run={}]", log.run_id)));
        }
        // 脱敏：事件行不得携带 ID/路径/密钥类字段
        for line in &entries {
            for banned in [
                "attachment_id=",
                "object_key=",
                "sha256=",
                "etag=",
                "token=",
                "password=",
                "file_library_id=",
                "database_library_id=",
                "/",
            ] {
                assert!(!line.contains(banned), "日志行泄漏敏感内容: {line}");
            }
        }
    }

    #[test]
    fn overall_outcome_matches_typed_status() {
        assert_eq!(overall_outcome(&FileSyncStatus::Complete), "complete");
        assert_eq!(
            overall_outcome(&FileSyncStatus::PartialFailure),
            "partial_failure"
        );
        assert_eq!(
            overall_outcome(&FileSyncStatus::Error(FileSyncErrorKind::TransferFailed)),
            "failed"
        );
        assert_eq!(
            overall_outcome(&FileSyncStatus::Error(
                FileSyncErrorKind::SummaryPersistFailed
            )),
            "failed"
        );
        // 阻断/禁用状态必须单列，不得记为 complete
        assert_eq!(overall_outcome(&FileSyncStatus::Disabled), "disabled");
        assert_eq!(
            overall_outcome(&FileSyncStatus::WaitingForDatabaseIdentity),
            "waiting_database_identity"
        );
        assert_eq!(
            overall_outcome(&FileSyncStatus::InitializationRequired),
            "initialization_required"
        );
        assert_eq!(
            overall_outcome(&FileSyncStatus::UnidentifiedRemote),
            "unidentified_remote"
        );
        assert_eq!(
            overall_outcome(&FileSyncStatus::IdentityMismatch),
            "identity_mismatch"
        );
        // 终态不应出现的 Idle/Syncing 也不得记为 complete
        assert_eq!(
            overall_outcome(&FileSyncStatus::Idle),
            "unexpected_terminal_state"
        );
        assert_eq!(
            overall_outcome(&FileSyncStatus::Syncing),
            "unexpected_terminal_state"
        );
    }
}
