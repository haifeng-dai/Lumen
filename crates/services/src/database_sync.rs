//! 阶段 2：独立的数据库只下载编排。
//! 不连接文件同步、不上传，也不接入自动同步控制器。

use crate::sync::progress::{DatabaseSyncStatus, SyncStateInner};
use anyhow::{Result, anyhow};
use database::{
    Database, DatabaseSyncSummary, LocalDirtyRecord, MySqlManager, RemoteRecord, SyncConflict,
    SyncEntityType, UploadConfirmation, VersionedWriteResult,
};
use log::{info, warn};
use sha2::{Digest, Sha256};
use std::{future::Future, sync::Arc, time::Instant};
use tokio::time::{Duration, interval};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub(crate) struct SyncLogContext {
    run_id: String,
    started_at: Instant,
}

impl SyncLogContext {
    pub(crate) fn new(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
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

    fn slow(&self, stage: &str, fields: &str) {
        let elapsed_ms = self.started_at.elapsed().as_millis();
        warn!(
            "[Sync][run={}][stage={stage}] event=slow elapsed_ms={elapsed_ms} {fields}",
            self.run_id
        );
    }
}

fn error_category(error: &anyhow::Error) -> &'static str {
    let text = error.to_string().to_ascii_lowercase();
    if text.contains("invalid_remote_record") {
        "invalid_remote_record"
    } else if text.contains("read-only") || text.contains("not read-only") {
        "read_only"
    } else if text.contains("timeout") || text.contains("timed out") {
        "timeout"
    } else if text.contains("connection") || text.contains("connect") {
        "connection"
    } else if text.contains("schema") || text.contains("column") || text.contains("table") {
        "schema"
    } else {
        "unknown"
    }
}

fn invalid_record_detail(error: &anyhow::Error) -> Option<String> {
    for cause in error.chain() {
        let text = cause.to_string();
        if let Some(pos) = text.find("invalid_remote_record") {
            return Some(text[pos..].trim().to_string());
        }
    }
    None
}

fn should_log_upload_progress(completed: usize, elapsed: Duration) -> bool {
    completed > 0 && (completed % 50 == 0 || elapsed >= Duration::from_secs(10))
}

async fn get_library_info(
    mysql: &MySqlManager,
    context: Option<&SyncLogContext>,
) -> Result<Option<database::RemoteLibraryInfo>> {
    mysql
        .get_library_info_with_context(context.map(|value| value.run_id.as_str()))
        .await
}

async fn observe_stage<T, F, Fut>(
    context: &SyncLogContext,
    stage: &str,
    fields: impl Fn() -> String,
    future: F,
) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    context.event(stage, "start", &fields());
    let started = Instant::now();
    let mut future = std::pin::pin!(future());
    let mut ticker = interval(Duration::from_secs(30));
    ticker.tick().await;
    loop {
        tokio::select! {
            result = &mut future => {
                match &result {
                    Ok(_) => context.event(stage, "complete", &fields()),
                    Err(error) => {
                        let category = error_category(error);
                        let detail = invalid_record_detail(error)
                            .map(|d| format!(" {d}"))
                            .unwrap_or_default();
                        context.event(
                            stage,
                            "failed",
                            &format!("{} operation={stage} error_category={category}{detail}", fields()),
                        );
                    }
                }
                return result;
            }
            _ = ticker.tick() => {
                if started.elapsed() >= Duration::from_secs(30) {
                    context.slow(stage, &fields());
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RemoteBatch {
    pub library_id: String,
    pub last_sequence: i64,
    pub records: Vec<RemoteRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadResult {
    Applied {
        records: usize,
        conflicts: usize,
        version_regressions: usize,
    },
    IdentityRequired,
    IdentityMismatch,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UploadResult {
    pub uploaded: usize,
    pub superseded: usize,
    pub conflicts: usize,
    pub failures: usize,
    pub complete: bool,
}

#[derive(Default)]
struct UploadProgress {
    total: usize,
    completed: usize,
    uploaded: usize,
    conflicts: usize,
    superseded: usize,
    failures: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityDecision {
    NeedsRemoteInitialization,
    NeedsRemoteAdoption { library_id: String },
    Mismatch { local_id: String, remote_id: String },
    Ready { full_snapshot_required: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityPlan {
    pub library_id: String,
    pub remote_fingerprint: String,
    pub reset_sequence: bool,
    pub full_snapshot_required: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityConfirmation {
    None,
    InitializeRemote,
    AdoptRemote,
}

fn remote_fingerprint(config: &models::DatabaseConfig) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!(
        "{}\0{}\0{}\0{}\0{}",
        config.host, config.port, config.database, config.username, config.use_ssl
    ));
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub struct DatabaseSyncService {
    db: Arc<Database>,
    mysql: Option<Arc<MySqlManager>>,
    sync_state: Option<Arc<std::sync::Mutex<SyncStateInner>>>,
    notify_ui: Option<Arc<dyn Fn() + Send + Sync>>,
    notify_data: Option<Arc<dyn Fn() + Send + Sync>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DatabaseSyncRunResult {
    pub download: Option<DownloadResult>,
    pub upload: Option<UploadResult>,
    pub identity: Option<IdentityDecision>,
    pub failures: usize,
}

/// UI-facing aliases kept in the services layer so UI code does not import
/// database implementation types.
pub type UiDatabaseSyncConflict = SyncConflict;
pub type UiDatabaseSyncSummary = DatabaseSyncSummary;

impl std::fmt::Debug for DatabaseSyncService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseSyncService")
            .field("db", &self.db)
            .field("mysql", &self.mysql.is_some())
            .finish()
    }
}

impl DatabaseSyncService {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            mysql: None,
            sync_state: None,
            notify_ui: None,
            notify_data: None,
        }
    }

    pub fn with_mysql(db: Arc<Database>, mysql: Arc<MySqlManager>) -> Self {
        Self {
            db,
            mysql: Some(mysql),
            sync_state: None,
            notify_ui: None,
            notify_data: None,
        }
    }

    pub fn with_progress(
        db: Arc<Database>,
        mysql: Arc<MySqlManager>,
        sync_state: Arc<std::sync::Mutex<SyncStateInner>>,
        notify_ui: Arc<dyn Fn() + Send + Sync>,
        notify_data: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            db,
            mysql: Some(mysql),
            sync_state: Some(sync_state),
            notify_ui: Some(notify_ui),
            notify_data: Some(notify_data),
        }
    }

    fn set_status(&self, status: DatabaseSyncStatus) {
        if let Some(state) = &self.sync_state {
            if let Ok(mut state) = state.lock() {
                if state.database_sync_status != status {
                    state.database_sync_status = status;
                    if let Some(notify) = &self.notify_ui {
                        notify();
                    }
                }
                // STATE-003: 刷新正交 flags
                let db_conflicts = self.db.list_sync_conflicts().map(|v| v.len()).unwrap_or(0);
                let file_conflicts = self.db.count_attachment_file_conflicts().unwrap_or(0);
                let partial = matches!(
                    state.database_sync_status,
                    DatabaseSyncStatus::PartialFailure
                ) as usize;
                state.refresh_composite(db_conflicts, file_conflicts, 0, 0, partial);
            }
        }
    }

    pub fn update_config(&self, config: &models::config::AppConfig) {
        if let Some(mysql) = &self.mysql {
            if mysql.update_config(config.database.clone()) {
                let mysql = mysql.clone();
                crate::runtime::RUNTIME.spawn(async move {
                    if let Err(error) = mysql.disconnect_retired_pools().await {
                        log::error!("MySQL: failed to disconnect retired pools: {error}");
                    }
                });
            }
        }
    }

    pub async fn test_mysql_config(&self, config: models::DatabaseConfig) -> Result<()> {
        MySqlManager::new(config).test_connection().await
    }

    pub async fn clear_remote_data(&self) -> Result<()> {
        let mysql = self
            .mysql
            .as_ref()
            .ok_or_else(|| anyhow!("MySQL is not configured"))?;
        mysql.clear_all_data().await
    }

    /// DB-003: remote tombstone physical purge is disabled until a safe
    /// device-watermark / retention protocol exists. Do not fall through to
    /// MySQL DELETE or local purge_all_deleted from this entry point.
    pub async fn purge_deleted_data(&self) -> Result<usize> {
        Err(anyhow!(
            "remote tombstone purge is currently unsupported: missing device watermark and retention protocol"
        ))
    }

    /// COORD-001: 远端数据库是否启用（无 MySQL 或 use_remote=false 时为 Disabled）。
    pub fn remote_database_enabled(&self) -> bool {
        self.mysql
            .as_ref()
            .map(|m| m.get_config().use_remote)
            .unwrap_or(false)
    }

    /// Run one database-only cycle. Identity transitions are deliberately
    /// reported and never confirmed implicitly.
    pub async fn run(&self) -> Result<DatabaseSyncRunResult> {
        self.run_with_context("standalone").await
    }

    pub(crate) async fn run_with_context(&self, run_id: &str) -> Result<DatabaseSyncRunResult> {
        let context = SyncLogContext::new(run_id);
        let Some(mysql) = &self.mysql else {
            self.save_summary(DatabaseSyncSummary {
                complete: true,
                identity_error: Some("Disabled".into()),
                updated_at: chrono::Utc::now().timestamp(),
                ..Default::default()
            })?;
            self.set_status(DatabaseSyncStatus::Idle);
            return Ok(DatabaseSyncRunResult::default());
        };
        if !mysql.get_config().use_remote {
            self.save_summary(DatabaseSyncSummary {
                complete: true,
                identity_error: Some("Disabled".into()),
                updated_at: chrono::Utc::now().timestamp(),
                ..Default::default()
            })?;
            self.set_status(DatabaseSyncStatus::Idle);
            return Ok(DatabaseSyncRunResult::default());
        }
        self.set_status(DatabaseSyncStatus::Syncing);
        let schema_result = observe_stage(
            &context,
            "schema_ensure",
            || String::new(),
            || mysql.ensure_remote_tables(),
        )
        .await;
        if let Err(error) = schema_result {
            self.set_status(DatabaseSyncStatus::Error(error.to_string()));
            return Err(error);
        }
        let identity = match observe_stage(
            &context,
            "identity_preflight",
            || "operation=get_library_info".into(),
            || self.preflight_identity_with_context(Some(&context)),
        )
        .await
        {
            Ok(identity) => identity,
            Err(error) => {
                self.set_status(DatabaseSyncStatus::Error(error.to_string()));
                return Err(error);
            }
        };
        let mut result = DatabaseSyncRunResult {
            identity: Some(identity.clone()),
            ..Default::default()
        };
        match identity {
            IdentityDecision::NeedsRemoteInitialization
            | IdentityDecision::NeedsRemoteAdoption { .. }
            | IdentityDecision::Mismatch { .. } => {
                self.set_status(match identity {
                    IdentityDecision::NeedsRemoteInitialization => {
                        DatabaseSyncStatus::NeedsRemoteInitialization
                    }
                    IdentityDecision::NeedsRemoteAdoption { .. } => {
                        DatabaseSyncStatus::NeedsRemoteAdoption
                    }
                    _ => DatabaseSyncStatus::IdentityMismatch,
                });
                result.failures = 1;
            }
            IdentityDecision::Ready {
                full_snapshot_required,
            } => {
                let download = if full_snapshot_required {
                    self.reconcile_same_library_with_context(Some(&context))
                        .await
                        .map(|(d, _)| d)
                } else {
                    self.download_from_remote_with_context(Some(&context)).await
                };
                match download {
                    Ok(value) => result.download = Some(value),
                    Err(error) => {
                        warn!(
                            "database sync download failed: error_category={}",
                            error_category(&error)
                        );
                        result.failures += 1;
                    }
                }
                match self.upload_to_remote_with_context(Some(&context)).await {
                    Ok(value) => {
                        result.failures += value.failures;
                        result.upload = Some(value);
                    }
                    Err(error) => {
                        warn!(
                            "database sync upload failed: error_category={}",
                            error_category(&error)
                        );
                        result.failures += 1;
                    }
                }
            }
        }
        let mut status = status_for_run(&result);
        // STATE-002: 终态必须由持久化冲突真源派生
        let persistent_conflicts = self.db.list_sync_conflicts().map(|v| v.len()).unwrap_or(0);
        status = apply_persistent_db_conflict_overlay(status, persistent_conflicts);
        self.set_status(status);
        // SUMMARY-001: 整轮只提交一次摘要；失败不得把状态标为成功
        if let Err(error) = self.persist_round_summary(&result) {
            self.set_status(DatabaseSyncStatus::Error(error.to_string()));
            return Err(error);
        }
        Ok(result)
    }

    /// STATE-002: 应用启动/恢复时从持久化冲突重算数据库同步状态。
    pub fn restore_status_from_persistent_conflicts(&self) -> Result<DatabaseSyncStatus> {
        let persistent = self.db.list_sync_conflicts()?.len();
        let file_conflicts = self.db.count_attachment_file_conflicts().unwrap_or(0);
        let base = if persistent > 0 || file_conflicts > 0 {
            DatabaseSyncStatus::Conflict
        } else {
            match self.last_summary().ok().flatten() {
                Some(summary) if !summary.complete && summary.failures > 0 => {
                    DatabaseSyncStatus::PartialFailure
                }
                Some(summary) if summary.version_regressions > 0 => {
                    DatabaseSyncStatus::RemoteVersionRegression
                }
                Some(summary) if summary.superseded > 0 => DatabaseSyncStatus::PendingLocalChanges,
                _ => DatabaseSyncStatus::Idle,
            }
        };
        let status = apply_persistent_db_conflict_overlay(base, persistent.max(file_conflicts));
        self.set_status(status.clone());
        Ok(status)
    }

    /// Read-only identity preflight. No local or remote data is written.
    pub async fn preflight_identity(&self) -> Result<IdentityDecision> {
        self.preflight_identity_with_context(None).await
    }

    async fn preflight_identity_with_context(
        &self,
        context: Option<&SyncLogContext>,
    ) -> Result<IdentityDecision> {
        let mysql = self
            .mysql
            .as_ref()
            .ok_or_else(|| anyhow!("remote reader is not configured"))?;
        let local = self.db.get_local_sync_state()?;
        let remote = get_library_info(mysql, context).await?;
        let decision = match (local.library_id, remote) {
            (_, None) => IdentityDecision::NeedsRemoteInitialization,
            (None, Some(remote)) => IdentityDecision::NeedsRemoteAdoption {
                library_id: remote.library_id,
            },
            (Some(local_id), Some(remote)) if local_id != remote.library_id => {
                IdentityDecision::Mismatch {
                    local_id,
                    remote_id: remote.library_id,
                }
            }
            (Some(_), Some(_remote)) => {
                let fingerprint = remote_fingerprint(&mysql.get_config());
                IdentityDecision::Ready {
                    full_snapshot_required: local.last_sequence == 0
                        || local.remote_fingerprint.as_deref() != Some(fingerprint.as_str()),
                }
            }
        };
        Ok(decision)
    }

    /// Build a write plan after an explicit identity decision. This pure
    /// planning step is shared by the confirmation paths and unit tests.
    pub fn plan_identity(
        local: &database::LocalSyncState,
        remote: Option<&database::RemoteLibraryInfo>,
        fingerprint: String,
        confirm: IdentityConfirmation,
    ) -> Result<IdentityPlan> {
        match (local.library_id.as_deref(), remote, confirm) {
            (_, None, IdentityConfirmation::InitializeRemote) => {
                let id = local
                    .library_id
                    .clone()
                    .unwrap_or_else(|| Uuid::new_v4().to_string());
                Ok(IdentityPlan {
                    library_id: id,
                    remote_fingerprint: fingerprint.clone(),
                    reset_sequence: true,
                    full_snapshot_required: true,
                })
            }
            (None, Some(remote), IdentityConfirmation::AdoptRemote) => Ok(IdentityPlan {
                library_id: remote.library_id.clone(),
                remote_fingerprint: fingerprint,
                reset_sequence: true,
                full_snapshot_required: true,
            }),
            (Some(local_id), Some(remote), IdentityConfirmation::None)
                if local_id == remote.library_id =>
            {
                Ok(IdentityPlan {
                    library_id: local_id.to_string(),
                    remote_fingerprint: fingerprint.clone(),
                    reset_sequence: local.remote_fingerprint.as_deref()
                        != Some(fingerprint.as_str()),
                    full_snapshot_required: local.last_sequence == 0
                        || local.remote_fingerprint.as_deref() != Some(fingerprint.as_str()),
                })
            }
            (Some(local_id), Some(remote), _) if local_id != remote.library_id => {
                Err(anyhow!("database library identity mismatch"))
            }
            _ => Err(anyhow!("explicit identity confirmation required")),
        }
    }

    /// Confirm creation of the remote library identity, then persist local
    /// identity only after MySQL accepts the insert.
    pub async fn confirm_remote_initialization(&self) -> Result<IdentityPlan> {
        let mysql = self
            .mysql
            .as_ref()
            .ok_or_else(|| anyhow!("remote writer is not configured"))?;
        let local = self.db.get_local_sync_state()?;
        let fingerprint = remote_fingerprint(&mysql.get_config());
        let remote = mysql.get_library_info().await?;
        let plan = Self::plan_identity(
            &local,
            remote.as_ref(),
            fingerprint,
            IdentityConfirmation::InitializeRemote,
        )?;
        mysql
            .initialize_library_info(&database::RemoteLibraryInfo {
                library_id: plan.library_id.clone(),
                schema_version: 1,
                created_at: chrono::Utc::now().timestamp(),
            })
            .await?;
        self.db
            .set_identity_state(&plan.library_id, &plan.remote_fingerprint, 0)?;
        self.set_status(DatabaseSyncStatus::Idle);
        if let Some(notify) = &self.notify_ui {
            notify();
        }
        Ok(plan)
    }

    /// Confirm adoption of an existing remote library. No upload is performed.
    pub async fn confirm_remote_adoption(&self) -> Result<IdentityPlan> {
        let mysql = self
            .mysql
            .as_ref()
            .ok_or_else(|| anyhow!("remote reader is not configured"))?;
        let local = self.db.get_local_sync_state()?;
        let remote = mysql
            .get_library_info()
            .await?
            .ok_or_else(|| anyhow!("remote library identity is missing"))?;
        let fingerprint = remote_fingerprint(&mysql.get_config());
        let plan = Self::plan_identity(
            &local,
            Some(&remote),
            fingerprint,
            IdentityConfirmation::AdoptRemote,
        )?;
        self.db
            .set_identity_state(&plan.library_id, &plan.remote_fingerprint, 0)?;
        self.set_status(DatabaseSyncStatus::Idle);
        if let Some(notify) = &self.notify_ui {
            notify();
        }
        Ok(plan)
    }

    /// Same-library full comparison, explicitly downloading a consistent
    /// snapshot before the existing typed upload path.
    pub async fn reconcile_same_library(&self) -> Result<(DownloadResult, UploadResult)> {
        self.reconcile_same_library_with_context(None).await
    }

    async fn reconcile_same_library_with_context(
        &self,
        context: Option<&SyncLogContext>,
    ) -> Result<(DownloadResult, UploadResult)> {
        let mysql = self
            .mysql
            .as_ref()
            .ok_or_else(|| anyhow!("remote reader is not configured"))?;
        let local = self.db.get_local_sync_state()?;
        let remote = get_library_info(mysql, context)
            .await?
            .ok_or_else(|| anyhow!("remote library identity is missing"))?;
        let fingerprint = remote_fingerprint(&mysql.get_config());
        let plan = Self::plan_identity(
            &local,
            Some(&remote),
            fingerprint,
            IdentityConfirmation::None,
        )?;
        if let Some(context) = context {
            context.event("remote_download", "start", "mode=snapshot");
        }
        let snapshot = match context {
            Some(context) => observe_stage(
                context,
                "remote_download_fetch",
                || "mode=snapshot".into(),
                || async {
                    mysql
                        .read_sync_snapshot_with_context(Some(&context.run_id))
                        .await
                },
            )
            .await
            .map_err(|error| {
                context.event(
                    "remote_download",
                    "failed",
                    &format!(
                        "mode=snapshot operation=remote_download_fetch error_category={}",
                        error_category(&error)
                    ),
                );
                error
            })?,
            None => mysql.read_sync_snapshot().await?,
        };
        let batch = RemoteBatch {
            library_id: plan.library_id.clone(),
            last_sequence: snapshot.last_sequence,
            records: snapshot.records,
        };
        let result = match context {
            Some(context) => {
                match observe_stage(
                    context,
                    "sqlite_apply_remote_batch",
                    || format!("records={}", batch.records.len()),
                    || async {
                        self.download_with_identity(
                            Some(&plan.library_id),
                            &batch,
                            Some((&plan.library_id, &plan.remote_fingerprint)),
                        )
                        .map_err(Into::into)
                    },
                )
                .await
                {
                    Ok(result) => result,
                    Err(error) => {
                        let category = error_category(&error);
                        let detail = invalid_record_detail(&error)
                            .map(|d| format!(" {d}"))
                            .unwrap_or_default();
                        context.event(
                            "remote_download",
                            "failed",
                            &format!(
                                "mode=snapshot operation=sqlite_apply_remote_batch error_category={category}{detail}"
                            ),
                        );
                        return Err(error);
                    }
                }
            }
            None => self.download_with_identity(
                Some(&plan.library_id),
                &batch,
                Some((&plan.library_id, &plan.remote_fingerprint)),
            )?,
        };
        if let Some(context) = context {
            let (applied, conflicts) = match &result {
                DownloadResult::Applied {
                    records, conflicts, ..
                } => (*records, *conflicts),
                _ => (0, 0),
            };
            context.event(
                "remote_download",
                "complete",
                &format!(
                    "mode=snapshot records={} applied={applied} conflicts={conflicts} to_sequence={}",
                    batch.records.len(),
                    batch.last_sequence
                ),
            );
        }
        Ok((result, UploadResult::default()))
    }

    /// Upload dirty database records only. This path deliberately has no file
    /// backend or download dependency and is not connected to the old engine.
    pub async fn upload_to_remote(&self) -> Result<UploadResult> {
        let result = self.upload_to_remote_with_context(None).await?;
        self.persist_round_summary(&DatabaseSyncRunResult {
            upload: Some(result.clone()),
            ..Default::default()
        })?;
        Ok(result)
    }

    async fn upload_to_remote_with_context(
        &self,
        context: Option<&SyncLogContext>,
    ) -> Result<UploadResult> {
        let mysql = self
            .mysql
            .as_ref()
            .ok_or_else(|| anyhow!("remote writer is not configured"))?;
        let local = self.db.get_local_sync_state()?;
        let remote = get_library_info(mysql, context)
            .await?
            .ok_or_else(|| anyhow!("remote library identity is missing"))?;
        if !upload_identity_matches(local.library_id.as_deref(), &remote.library_id) {
            // SUMMARY-001: 阶段不写最终摘要
            return Err(anyhow!("database library identity mismatch"));
        }
        let records = match context {
            Some(context) => {
                observe_stage(
                    context,
                    "collect_dirty_records",
                    || String::new(),
                    || async { self.db.collect_dirty_records().map_err(Into::into) },
                )
                .await?
            }
            None => self.db.collect_dirty_records()?,
        };
        let progress = Arc::new(std::sync::Mutex::new(UploadProgress {
            total: records.len(),
            ..Default::default()
        }));
        let progress_for_log = progress.clone();
        let upload = || async {
            self.upload_dirty_records(mysql, records, context, progress)
                .await
        };
        let result = match context {
            Some(context) => {
                observe_stage(
                    context,
                    "remote_upload",
                    move || {
                        let progress = progress_for_log.lock().unwrap();
                        format!(
                            "total={} completed={} uploaded={} conflicts={} failures={}",
                            progress.total,
                            progress.completed,
                            progress.uploaded,
                            progress.conflicts,
                            progress.failures
                        )
                    },
                    upload,
                )
                .await?
            }
            None => upload().await?,
        };
        // SUMMARY-001: 上传阶段不写最终摘要；由 run_with_context / 公共入口聚合提交
        Ok(result)
    }

    async fn upload_dirty_records(
        &self,
        mysql: &MySqlManager,
        records: Vec<LocalDirtyRecord>,
        context: Option<&SyncLogContext>,
        progress: Arc<std::sync::Mutex<UploadProgress>>,
    ) -> Result<UploadResult> {
        let mut result = UploadResult {
            complete: true,
            ..Default::default()
        };
        let mut last_progress = Instant::now();
        for record in records {
            let entity_type = record.entity.as_str();
            match self.upload_one(mysql, record, context).await {
                Ok(UploadOne::Uploaded) => result.uploaded += 1,
                Ok(UploadOne::Superseded) => {
                    result.superseded += 1;
                    result.complete = false;
                }
                Ok(UploadOne::Conflict) => result.conflicts += 1,
                Err(error) => {
                    if let Some(context) = context {
                        context.event(
                            "remote_upload",
                            "failed",
                            &format!(
                                "operation=upload_one entity_type={entity_type} error_category={}",
                                error_category(&error)
                            ),
                        );
                    } else {
                        warn!(
                            "database sync upload failed; continuing: entity_type={entity_type} error_category={}",
                            error_category(&error)
                        );
                    }
                    result.failures += 1;
                    result.complete = false;
                }
            }
            let mut current = progress.lock().unwrap();
            current.completed += 1;
            current.uploaded = result.uploaded;
            current.conflicts = result.conflicts;
            current.superseded = result.superseded;
            current.failures = result.failures;
            let should_log = should_log_upload_progress(current.completed, last_progress.elapsed());
            let completed = current.completed;
            drop(current);
            if let Some(context) = context {
                if should_log {
                    context.event(
                        "remote_upload",
                        "progress",
                        &format!(
                            "total={} completed={} uploaded={} conflicts={} failures={}",
                            progress.lock().unwrap().total,
                            completed,
                            result.uploaded,
                            result.conflicts,
                            result.failures
                        ),
                    );
                    last_progress = Instant::now();
                }
            }
        }
        Ok(result)
    }

    async fn upload_one(
        &self,
        mysql: &MySqlManager,
        record: LocalDirtyRecord,
        context: Option<&SyncLogContext>,
    ) -> Result<UploadOne> {
        let outcome = mysql
            .write_versioned_entity_with_context(
                &record.payload,
                record.expected_remote_version,
                context.map(|value| value.run_id.as_str()),
            )
            .await?;
        apply_upload_outcome(&self.db, record, outcome)
    }

    /// 从 MySQL 读取资料库身份，并按本地 sequence 选择完整快照或增量变化。
    pub async fn download_from_remote(&self) -> Result<DownloadResult> {
        self.download_from_remote_with_context(None).await
    }

    async fn download_from_remote_with_context(
        &self,
        context: Option<&SyncLogContext>,
    ) -> Result<DownloadResult> {
        let mysql = self
            .mysql
            .as_ref()
            .ok_or_else(|| anyhow!("remote reader is not configured"))?;
        let local = self.db.get_local_sync_state()?;
        let remote = get_library_info(mysql, context)
            .await?
            .ok_or_else(|| anyhow!("remote library identity is missing"))?;
        if local.library_id.as_deref() != Some(remote.library_id.as_str()) {
            let result = if local.library_id.is_none() {
                DownloadResult::IdentityRequired
            } else {
                DownloadResult::IdentityMismatch
            };
            // SUMMARY-001: 阶段不写最终摘要
            return Ok(result);
        }
        let remote_batch = if local.last_sequence == 0 {
            if let Some(context) = context {
                context.event("remote_download", "start", "mode=snapshot");
            }
            match context {
                Some(context) => observe_stage(
                    context,
                    "remote_download_fetch",
                    || "mode=snapshot".into(),
                    || async {
                        mysql
                            .read_sync_snapshot_with_context(Some(&context.run_id))
                            .await
                    },
                )
                .await
                .map_err(|error| {
                    context.event(
                        "remote_download",
                        "failed",
                        &format!(
                            "mode=snapshot operation=remote_download_fetch error_category={}",
                            error_category(&error)
                        ),
                    );
                    error
                })?,
                None => mysql.read_sync_snapshot().await?,
            }
        } else {
            if let Some(context) = context {
                context.event(
                    "remote_download",
                    "start",
                    &format!(
                        "mode=incremental from_sequence={} limit=1000",
                        local.last_sequence
                    ),
                );
            }
            match context {
                Some(context) => observe_stage(
                    context,
                    "remote_download_fetch",
                    || {
                        format!(
                            "mode=incremental from_sequence={} limit=1000",
                            local.last_sequence
                        )
                    },
                    || async {
                        mysql
                            .read_sync_changes_after_with_context(
                                local.last_sequence as u64,
                                1000,
                                Some(&context.run_id),
                            )
                            .await
                    },
                )
                .await
                .map_err(|error| {
                    context.event(
                        "remote_download",
                        "failed",
                        &format!(
                            "mode=incremental operation=remote_download_fetch error_category={}",
                            error_category(&error)
                        ),
                    );
                    error
                })?,
                None => {
                    mysql
                        .read_sync_changes_after(local.last_sequence as u64, 1000)
                        .await?
                }
            }
        };
        let download_mode = if local.last_sequence == 0 {
            "mode=snapshot".to_string()
        } else {
            format!(
                "mode=incremental from_sequence={} limit=1000",
                local.last_sequence
            )
        };
        let batch = RemoteBatch {
            library_id: remote.library_id,
            last_sequence: remote_batch.last_sequence.max(local.last_sequence),
            records: remote_batch.records,
        };
        let result = match context {
            Some(context) => {
                match observe_stage(
                    context,
                    "sqlite_apply_remote_batch",
                    || format!("records={}", batch.records.len()),
                    || async {
                        self.download_with_identity(
                            Some(local.library_id.as_deref().unwrap()),
                            &batch,
                            None,
                        )
                        .map_err(Into::into)
                    },
                )
                .await
                {
                    Ok(result) => result,
                    Err(error) => {
                        let category = error_category(&error);
                        let detail = invalid_record_detail(&error)
                            .map(|d| format!(" {d}"))
                            .unwrap_or_default();
                        context.event(
                            "remote_download",
                            "failed",
                            &format!(
                                "{download_mode} operation=sqlite_apply_remote_batch error_category={category}{detail}"
                            ),
                        );
                        return Err(error);
                    }
                }
            }
            None => self.download_with_identity(
                Some(local.library_id.as_deref().unwrap()),
                &batch,
                None,
            )?,
        };
        if let Some(context) = context {
            let (applied, conflicts) = match &result {
                DownloadResult::Applied {
                    records, conflicts, ..
                } => (*records, *conflicts),
                _ => (0, 0),
            };
            context.event(
                "remote_download",
                "complete",
                &format!(
                    "{download_mode} records={} applied={applied} conflicts={conflicts} to_sequence={}",
                    batch.records.len(),
                    batch.last_sequence
                ),
            );
        }
        Ok(result)
    }

    /// Return persistent conflicts for presentation by a later UI/controller.
    pub fn list_conflicts(&self) -> Result<Vec<SyncConflict>> {
        Ok(self.db.list_sync_conflicts()?)
    }

    pub fn last_summary(&self) -> Result<Option<DatabaseSyncSummary>> {
        self.db.get_database_sync_summary().map_err(Into::into)
    }

    /// Accept the remote record and atomically clear the persisted conflict.
    pub fn choose_remote(&self, entity_type: &str, entity_id: &str) -> Result<()> {
        self.db
            .resolve_sync_conflict_remote(entity_type, entity_id)?;
        self.update_conflict_status()?;
        notify_data_and_ui(self.notify_data.as_ref(), self.notify_ui.as_ref());
        Ok(())
    }

    /// Keep the local/manual result, acknowledge the remote version, and
    /// atomically clear the persisted conflict.
    pub fn keep_local_or_merged(&self, entity_type: &str, entity_id: &str) -> Result<()> {
        self.db
            .resolve_sync_conflict_local(entity_type, entity_id)?;
        self.update_conflict_status()?;
        notify_data_and_ui(self.notify_data.as_ref(), self.notify_ui.as_ref());
        Ok(())
    }

    fn update_conflict_status(&self) -> Result<()> {
        if self.db.list_sync_conflicts()?.is_empty() {
            self.set_status(DatabaseSyncStatus::Idle);
        } else {
            self.set_status(DatabaseSyncStatus::Conflict);
        }
        Ok(())
    }

    /// 应用完整快照或 sequence 增量。资料库身份尚未初始化时拒绝写入。
    pub fn download(
        &self,
        local_library_id: Option<&str>,
        batch: &RemoteBatch,
    ) -> Result<DownloadResult> {
        let result = self.download_with_identity(local_library_id, batch, None)?;
        // 独立调用路径：按本次下载结果提交单侧摘要
        self.persist_round_summary(&DatabaseSyncRunResult {
            download: Some(result.clone()),
            ..Default::default()
        })?;
        Ok(result)
    }

    fn download_with_identity(
        &self,
        local_library_id: Option<&str>,
        batch: &RemoteBatch,
        identity: Option<(&str, &str)>,
    ) -> Result<DownloadResult> {
        // SUMMARY-001: 阶段函数只返回类型化结果，不写最终摘要
        match local_library_id {
            None => return Ok(DownloadResult::IdentityRequired),
            Some(id) if id != batch.library_id => return Ok(DownloadResult::IdentityMismatch),
            Some(_) => {}
        }
        // DB-002: decide + apply + conflict persist + sequence/identity in one
        // SQLite transaction inside database. No services-side state pre-read.
        let outcome = self.db.apply_remote_download_atomically(
            batch.last_sequence,
            &batch.records,
            identity,
        )?;
        if outcome.version_regressions > 0 {
            warn!(
                "database sync download skipped remote version regressions: {}",
                outcome.version_regressions
            );
        }
        if outcome.applied > 0 {
            if let Some(notify) = &self.notify_data {
                notify();
            }
        }
        Ok(DownloadResult::Applied {
            records: outcome.applied,
            conflicts: outcome.conflicts,
            version_regressions: outcome.version_regressions,
        })
    }

    /// SUMMARY-001 + STATE-002: 聚合整轮结果并一次性持久化数据库同步摘要。
    /// 持久化冲突存在时摘要不得 complete，冲突计数不低于真源总数。
    pub fn persist_round_summary(&self, result: &DatabaseSyncRunResult) -> Result<()> {
        let mut summary = Self::summarize_round(result);
        let persistent_db = self.db.list_sync_conflicts().map(|v| v.len()).unwrap_or(0);
        let persistent_file = self.db.count_attachment_file_conflicts().unwrap_or(0);
        if persistent_db > summary.conflicts {
            summary.conflicts = persistent_db;
        }
        if persistent_db > 0 || persistent_file > 0 {
            summary.complete = false;
        }
        self.save_summary(summary)
    }

    fn summarize_round(result: &DatabaseSyncRunResult) -> DatabaseSyncSummary {
        let (downloaded, download_conflicts, version_regressions, download_identity) =
            match result.download.as_ref() {
                Some(DownloadResult::Applied {
                    records,
                    conflicts,
                    version_regressions,
                }) => (*records, *conflicts, *version_regressions, None),
                Some(other) => (0, 0, 0, Some(format!("download:{other:?}"))),
                None => (0, 0, 0, None),
            };
        let upload = result.upload.clone().unwrap_or_default();
        let identity_error = download_identity.or_else(|| match result.identity.as_ref() {
            Some(IdentityDecision::NeedsRemoteInitialization) => {
                Some("NeedsRemoteInitialization".to_string())
            }
            Some(IdentityDecision::NeedsRemoteAdoption { .. }) => {
                Some("NeedsRemoteAdoption".to_string())
            }
            Some(IdentityDecision::Mismatch { .. }) => Some("IdentityMismatch".to_string()),
            _ => None,
        });
        let mut failures = result.failures.max(upload.failures);
        if identity_error.is_some() && failures == 0 {
            failures = 1;
        }
        let complete = failures == 0
            && upload.conflicts == 0
            && upload.superseded == 0
            && download_conflicts == 0
            && version_regressions == 0
            && identity_error.is_none();
        DatabaseSyncSummary {
            uploaded: upload.uploaded,
            superseded: upload.superseded,
            downloaded,
            conflicts: download_conflicts + upload.conflicts,
            failures,
            version_regressions,
            complete,
            updated_at: chrono::Utc::now().timestamp(),
            identity_error,
        }
    }

    fn save_summary(&self, summary: DatabaseSyncSummary) -> Result<()> {
        self.db.set_database_sync_summary(&summary)?;
        Ok(())
    }
}

enum UploadOne {
    Uploaded,
    Superseded,
    Conflict,
}

fn apply_upload_outcome(
    db: &Database,
    record: LocalDirtyRecord,
    outcome: VersionedWriteResult,
) -> Result<UploadOne> {
    match outcome {
        VersionedWriteResult::Applied { version, .. } => {
            match db.confirm_uploaded_snapshot(
                record.entity,
                &record.key,
                record.local_generation,
                record.expected_remote_version,
                version,
            )? {
                UploadConfirmation::Confirmed => Ok(UploadOne::Uploaded),
                UploadConfirmation::Superseded => Ok(UploadOne::Superseded),
                confirmation => Err(anyhow!("upload confirmation failed: {confirmation:?}")),
            }
        }
        VersionedWriteResult::VersionConflict {
            current_version,
            remote_record,
        } => {
            db.save_sync_conflict(&SyncConflict {
                entity_type: entity_name(record.entity).to_string(),
                entity_id: database::canonical_key(&record.key),
                local_record: serde_json::to_string(&record.payload.value())?,
                remote_record: remote_record
                    .as_ref()
                    .map(|r| r.payload.to_string())
                    .unwrap_or_else(|| "null".to_string()),
                remote_version: i64::from(
                    current_version.unwrap_or(record.expected_remote_version),
                ),
                detected_at: chrono::Utc::now().timestamp(),
            })?;
            Ok(UploadOne::Conflict)
        }
    }
}

fn upload_identity_matches(local: Option<&str>, remote: &str) -> bool {
    local == Some(remote)
}

fn apply_persistent_db_conflict_overlay(
    status: DatabaseSyncStatus,
    persistent_conflicts: usize,
) -> DatabaseSyncStatus {
    if persistent_conflicts == 0 {
        return status;
    }
    match status {
        DatabaseSyncStatus::NeedsRemoteInitialization
        | DatabaseSyncStatus::NeedsRemoteAdoption
        | DatabaseSyncStatus::IdentityMismatch
        | DatabaseSyncStatus::Error(_) => status,
        // 冲突真源优先于 Idle/Pending/Regression，不得被安静轮次吞掉
        _ => DatabaseSyncStatus::Conflict,
    }
}

fn status_for_run(result: &DatabaseSyncRunResult) -> DatabaseSyncStatus {
    // Identity setup is an expected, user-confirmable transition.  Keep it
    // ahead of the generic failure count so the UI can open the identity
    // dialog instead of reducing the run to a retry-only partial failure.
    match result.identity.as_ref() {
        Some(IdentityDecision::NeedsRemoteInitialization) => {
            return DatabaseSyncStatus::NeedsRemoteInitialization;
        }
        Some(IdentityDecision::NeedsRemoteAdoption { .. }) => {
            return DatabaseSyncStatus::NeedsRemoteAdoption;
        }
        Some(IdentityDecision::Mismatch { .. }) => {
            return DatabaseSyncStatus::IdentityMismatch;
        }
        Some(IdentityDecision::Ready { .. }) | None => {}
    }

    // SUMMARY-001 / review: download 阶段的 identity 结果不得被 Ready 预检吞掉
    match result.download.as_ref() {
        Some(DownloadResult::IdentityRequired) => {
            return DatabaseSyncStatus::NeedsRemoteInitialization;
        }
        Some(DownloadResult::IdentityMismatch) => {
            return DatabaseSyncStatus::IdentityMismatch;
        }
        _ => {}
    }

    if result.failures > 0 {
        DatabaseSyncStatus::PartialFailure
    } else if result.upload.as_ref().is_some_and(|u| u.conflicts > 0)
        || result.download.as_ref().is_some_and(
            |d| matches!(d, DownloadResult::Applied { conflicts, .. } if *conflicts > 0),
        )
    {
        DatabaseSyncStatus::Conflict
    } else if result.download.as_ref().is_some_and(
        |d| matches!(d, DownloadResult::Applied { version_regressions, .. } if *version_regressions > 0),
    ) {
        DatabaseSyncStatus::RemoteVersionRegression
    } else if result.upload.as_ref().is_some_and(|u| u.superseded > 0) {
        DatabaseSyncStatus::PendingLocalChanges
    } else {
        DatabaseSyncStatus::Idle
    }
}

fn notify_data_and_ui(
    notify_data: Option<&Arc<dyn Fn() + Send + Sync>>,
    notify_ui: Option<&Arc<dyn Fn() + Send + Sync>>,
) {
    if let Some(notify) = notify_data {
        notify();
    }
    if let Some(notify) = notify_ui {
        notify();
    }
}

#[cfg(test)]
fn summarize_upload_attempts(attempts: &[Option<UploadOne>]) -> UploadResult {
    let mut result = UploadResult {
        complete: true,
        ..Default::default()
    };
    for attempt in attempts {
        match attempt {
            Some(UploadOne::Uploaded) => result.uploaded += 1,
            Some(UploadOne::Superseded) => {
                result.superseded += 1;
                result.complete = false;
            }
            Some(UploadOne::Conflict) => result.conflicts += 1,
            None => {
                result.failures += 1;
                result.complete = false;
            }
        }
    }
    result
}

fn entity_name(entity: SyncEntityType) -> &'static str {
    match entity {
        SyncEntityType::Literature => "literatures",
        SyncEntityType::Publication => "publications",
        SyncEntityType::Author => "authors",
        SyncEntityType::LiteratureAuthor => "literature_authors",
        SyncEntityType::Folder => "folders",
        SyncEntityType::LiteratureFolder => "literature_folders",
        SyncEntityType::Tag => "tags",
        SyncEntityType::LiteratureTag => "literature_tags",
        SyncEntityType::Attachment => "attachments",
        SyncEntityType::Feed => "feeds",
        SyncEntityType::FeedItem => "feed_items",
        SyncEntityType::Citation => "literature_citations",
        SyncEntityType::Annotation => "annotations",
        SyncEntityType::LiteratureNote => "literature_notes",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::{Database, LocalSyncState, SyncEntityKey, SyncEntityType};
    use models::Attachment;

    #[test]
    fn upload_progress_has_a_nonduplicating_fifty_record_boundary() {
        assert!(!should_log_upload_progress(49, Duration::from_secs(0)));
        assert!(should_log_upload_progress(50, Duration::from_secs(0)));
        assert!(should_log_upload_progress(1, Duration::from_secs(10)));
    }

    fn remote_info(id: &str) -> database::RemoteLibraryInfo {
        database::RemoteLibraryInfo {
            library_id: id.into(),
            schema_version: 1,
            created_at: 0,
        }
    }

    #[test]
    fn identity_plan_requires_explicit_confirmation_for_empty_remote() {
        let local = database::LocalSyncState::default();
        assert!(
            DatabaseSyncService::plan_identity(
                &local,
                None,
                "fingerprint".into(),
                IdentityConfirmation::None
            )
            .is_err()
        );
        let plan = DatabaseSyncService::plan_identity(
            &local,
            None,
            "fingerprint".into(),
            IdentityConfirmation::InitializeRemote,
        )
        .unwrap();
        assert!(Uuid::parse_str(&plan.library_id).is_ok());
        assert!(plan.reset_sequence && plan.full_snapshot_required);
    }

    #[test]
    fn identity_plan_blocks_mismatched_remote_without_local_change() {
        let local = database::LocalSyncState {
            library_id: Some("local".into()),
            last_sequence: 7,
            remote_fingerprint: Some("old".into()),
        };
        assert!(
            DatabaseSyncService::plan_identity(
                &local,
                Some(&remote_info("remote")),
                "new".into(),
                IdentityConfirmation::None
            )
            .is_err()
        );
        assert_eq!(local.last_sequence, 7);
    }

    #[test]
    fn identity_plan_adoption_resets_sequence_only_after_confirmation() {
        let local = database::LocalSyncState::default();
        assert!(
            DatabaseSyncService::plan_identity(
                &local,
                Some(&remote_info("remote")),
                "fp".into(),
                IdentityConfirmation::None
            )
            .is_err()
        );
        let plan = DatabaseSyncService::plan_identity(
            &local,
            Some(&remote_info("remote")),
            "fp".into(),
            IdentityConfirmation::AdoptRemote,
        )
        .unwrap();
        assert_eq!(plan.library_id, "remote");
        assert!(plan.reset_sequence && plan.full_snapshot_required);
    }

    #[test]
    fn identity_plan_requires_full_snapshot_when_remote_address_changes() {
        let local = database::LocalSyncState {
            library_id: Some("same".into()),
            last_sequence: 12,
            remote_fingerprint: Some("old".into()),
        };
        let changed = DatabaseSyncService::plan_identity(
            &local,
            Some(&remote_info("same")),
            "new".into(),
            IdentityConfirmation::None,
        )
        .unwrap();
        assert!(changed.reset_sequence && changed.full_snapshot_required);
        let unchanged = DatabaseSyncService::plan_identity(
            &LocalSyncState {
                remote_fingerprint: Some("same-fp".into()),
                ..local
            },
            Some(&remote_info("same")),
            "same-fp".into(),
            IdentityConfirmation::None,
        )
        .unwrap();
        assert!(!unchanged.reset_sequence && !unchanged.full_snapshot_required);
    }

    fn record(id: &str, version: i64) -> RemoteRecord {
        RemoteRecord {
            entity_type: SyncEntityType::Tag,
            version,
            payload: serde_json::json!({"id":id,"name":"remote","color":null,"is_deleted":false,"version":version,"created_at":0,"updated_at":0}),
        }
    }

    #[test]
    fn identity_refusal_does_not_write_data() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        let service = DatabaseSyncService::new(db.clone());
        let batch = RemoteBatch {
            library_id: "remote".into(),
            last_sequence: 1,
            records: vec![record("tag-1", 1)],
        };
        assert_eq!(
            service.download(None, &batch).unwrap(),
            DownloadResult::IdentityRequired
        );
        assert_eq!(db.get_local_sync_state().unwrap().last_sequence, 0);
    }

    #[test]
    fn upload_applied_confirms_dirty_and_preserves_business_fields() {
        let db = Database::new(":memory:").unwrap();
        let tag = db.create_tag("local", None).unwrap();
        let before = db
            .get_all_tags_with_counts()
            .unwrap()
            .into_iter()
            .find(|(t, _)| t.id == tag.id)
            .unwrap()
            .0;
        let record = LocalDirtyRecord {
            entity: SyncEntityType::Tag,
            key: SyncEntityKey::Id(tag.id.clone()),
            local_generation: i64::from(before.version),
            expected_remote_version: 0,
            payload: database::SyncEntityPayload::Tag(before.clone()),
        };
        assert!(matches!(
            apply_upload_outcome(
                &db,
                record,
                VersionedWriteResult::Applied {
                    sequence: 1,
                    version: 7
                }
            )
            .unwrap(),
            UploadOne::Uploaded
        ));
        assert_eq!(
            db.get_download_state(SyncEntityType::Tag, &SyncEntityKey::Id(tag.id.clone()))
                .unwrap(),
            Some((7, false))
        );
        let after = db
            .get_all_tags_with_counts()
            .unwrap()
            .into_iter()
            .find(|(t, _)| t.id == tag.id)
            .unwrap()
            .0;
        assert_eq!(after.name, before.name);
        assert_eq!(after.color, before.color);
    }

    #[test]
    fn upload_conflict_preserves_dirty_and_saves_complete_remote_payload() {
        let db = Database::new(":memory:").unwrap();
        let tag = db.create_tag("local", None).unwrap();
        let local = db
            .get_all_tags_with_counts()
            .unwrap()
            .into_iter()
            .find(|(t, _)| t.id == tag.id)
            .unwrap()
            .0;
        let dirty = LocalDirtyRecord {
            entity: SyncEntityType::Tag,
            key: SyncEntityKey::Id(tag.id.clone()),
            local_generation: i64::from(local.version),
            expected_remote_version: 1,
            payload: database::SyncEntityPayload::Tag(local),
        };
        let remote = super::tests::record("remote-id", 9);
        assert!(matches!(
            apply_upload_outcome(
                &db,
                dirty,
                VersionedWriteResult::VersionConflict {
                    current_version: Some(9),
                    remote_record: Some(remote.clone())
                }
            )
            .unwrap(),
            UploadOne::Conflict
        ));
        assert_eq!(
            db.get_download_state(SyncEntityType::Tag, &SyncEntityKey::Id(tag.id.clone()))
                .unwrap()
                .unwrap()
                .1,
            true
        );
        let conflict = db.list_sync_conflicts().unwrap().pop().unwrap();
        assert_eq!(conflict.remote_version, 9);
        assert_eq!(conflict.remote_record, remote.payload.to_string());
    }

    #[test]
    fn deleted_dirty_upload_conflict_keeps_tombstone_and_persists_conflict() {
        let db = Database::new(":memory:").unwrap();
        let tag = db.create_tag("local", None).unwrap();
        db.delete_tag(&tag.id).unwrap();
        let local = models::Tag {
            is_deleted: true,
            is_dirty: true,
            ..tag.clone()
        };
        let outcome = apply_upload_outcome(
            &db,
            LocalDirtyRecord {
                entity: SyncEntityType::Tag,
                key: SyncEntityKey::Id(tag.id.clone()),
                local_generation: i64::from(tag.version + 1),
                expected_remote_version: 1,
                payload: database::SyncEntityPayload::Tag(local),
            },
            VersionedWriteResult::VersionConflict {
                current_version: Some(7),
                remote_record: Some(record("remote", 7)),
            },
        )
        .unwrap();
        assert!(matches!(outcome, UploadOne::Conflict));
        assert!(!db
            .collect_dirty_records()
            .unwrap()
            .iter()
            .any(|record| matches!(&record.payload, database::SyncEntityPayload::Tag(value) if value.id == tag.id)));
        assert_eq!(db.list_sync_conflicts().unwrap().len(), 1);
        db.resolve_sync_conflict_local("tags", &tag.id).unwrap();
        assert!(db
            .collect_dirty_records()
            .unwrap()
            .iter()
            .any(|record| matches!(&record.payload, database::SyncEntityPayload::Tag(value) if value.id == tag.id)));
    }

    #[test]
    fn upload_failure_summary_continues_to_later_applied_record() {
        let result = summarize_upload_attempts(&[None, Some(UploadOne::Uploaded)]);
        assert_eq!(
            result,
            UploadResult {
                uploaded: 1,
                superseded: 0,
                conflicts: 0,
                failures: 1,
                complete: false
            }
        );
    }

    #[test]
    fn upload_identity_rejection_performs_no_local_write() {
        let db = Database::new(":memory:").unwrap();
        let tag = db.create_tag("local", None).unwrap();
        assert!(!upload_identity_matches(None, "remote"));
        assert_eq!(
            db.get_download_state(SyncEntityType::Tag, &SyncEntityKey::Id(tag.id.clone()))
                .unwrap()
                .unwrap()
                .1,
            true
        );
    }

    #[test]
    fn dirty_attachment_is_database_record_without_file_access() {
        let db = Database::new(":memory:").unwrap();
        let attachment = Attachment {
            id: "a".into(),
            literature_id: "l".into(),
            file_path: "/does/not/exist.pdf".into(),
            file_name: "x.pdf".into(),
            file_size: 0,
            mime_type: None,
            etag: None,
            hash: None,
            is_main: true,
            is_dirty: true,
            is_deleted: false,
            version: 1,
            created_at: 0,
            updated_at: 0,
        };
        db.insert_attachment(&attachment).unwrap();
        let records = db.collect_dirty_records().unwrap();
        assert!(
            records
                .iter()
                .any(|r| matches!(r.payload, database::SyncEntityPayload::Attachment(_)))
        );
    }

    #[test]
    fn remote_delete_clean_record_is_applied_as_clean_tombstone() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        let tag = db.create_tag("local", None).unwrap();
        db.confirm_uploaded_snapshot(
            SyncEntityType::Tag,
            &SyncEntityKey::Id(tag.id.clone()),
            1,
            0,
            1,
        )
        .unwrap();
        let mut remote = record(&tag.id, 8);
        remote.payload["is_deleted"] = serde_json::json!(true);
        let service = DatabaseSyncService::new(db.clone());
        assert_eq!(
            service
                .download(
                    Some("same"),
                    &RemoteBatch {
                        library_id: "same".into(),
                        last_sequence: 8,
                        records: vec![remote]
                    }
                )
                .unwrap(),
            DownloadResult::Applied {
                records: 1,
                conflicts: 0,
                version_regressions: 0,
            }
        );
        assert_eq!(
            db.get_synced_version(SyncEntityType::Tag, &SyncEntityKey::Id(tag.id.clone()))
                .unwrap(),
            Some(8)
        );
        assert_eq!(
            db.get_download_state(SyncEntityType::Tag, &SyncEntityKey::Id(tag.id.clone()))
                .unwrap(),
            Some((8, false))
        );
    }

    #[test]
    fn remote_delete_dirty_record_is_saved_as_delete_conflict() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        let tag = db.create_tag("local", None).unwrap();
        db.update_tag_name(&tag.id, "edited").unwrap();
        let mut remote = record(&tag.id, 8);
        remote.payload["is_deleted"] = serde_json::json!(true);
        let service = DatabaseSyncService::new(db.clone());
        assert_eq!(
            service
                .download(
                    Some("same"),
                    &RemoteBatch {
                        library_id: "same".into(),
                        last_sequence: 8,
                        records: vec![remote]
                    }
                )
                .unwrap(),
            DownloadResult::Applied {
                records: 0,
                conflicts: 1,
                version_regressions: 0,
            }
        );
        assert_eq!(
            db.get_download_state(SyncEntityType::Tag, &SyncEntityKey::Id(tag.id.clone()))
                .unwrap(),
            Some((0, true))
        );
        let conflict = db.list_sync_conflicts().unwrap().pop().unwrap();
        assert!(conflict.remote_record.contains("\"is_deleted\":true"));
    }

    #[test]
    fn service_persists_partial_upload_and_conflict_summary() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        let service = DatabaseSyncService::new(db.clone());
        service
            .persist_round_summary(&DatabaseSyncRunResult {
                upload: Some(UploadResult {
                    uploaded: 2,
                    superseded: 0,
                    conflicts: 1,
                    failures: 1,
                    complete: false,
                }),
                failures: 1,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            db.get_database_sync_summary().unwrap(),
            Some(DatabaseSyncSummary {
                uploaded: 2,
                superseded: 0,
                conflicts: 1,
                failures: 1,
                version_regressions: 0,
                complete: false,
                updated_at: db.get_database_sync_summary().unwrap().unwrap().updated_at,
                identity_error: None,
                downloaded: 0
            })
        );
    }

    #[test]
    fn summary_serde_defaults_pending_count_for_legacy_json() {
        let old = r#"{"uploaded":1,"downloaded":2,"conflicts":0,"failures":0,"complete":true,"updated_at":1,"identity_error":null}"#;
        let summary: DatabaseSyncSummary = serde_json::from_str(old).unwrap();
        assert_eq!(summary.superseded, 0);
        let mut summary = summary;
        summary.superseded = 3;
        let encoded = serde_json::to_string(&summary).unwrap();
        assert!(encoded.contains("\"superseded\":3"));
        assert!(encoded.contains("\"version_regressions\":0"));
        let old = r#"{"uploaded":1,"downloaded":2,"conflicts":0,"failures":0,"complete":true,"updated_at":1,"identity_error":null}"#;
        let legacy: DatabaseSyncSummary = serde_json::from_str(old).unwrap();
        assert_eq!(legacy.version_regressions, 0);
    }

    #[test]
    fn summary_001_round_aggregate_keeps_download_and_upload_counts() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        let service = DatabaseSyncService::new(db.clone());
        let result = DatabaseSyncRunResult {
            download: Some(DownloadResult::Applied {
                records: 4,
                conflicts: 2,
                version_regressions: 1,
            }),
            upload: Some(UploadResult {
                uploaded: 3,
                superseded: 1,
                conflicts: 1,
                failures: 0,
                complete: false,
            }),
            identity: Some(IdentityDecision::Ready {
                full_snapshot_required: false,
            }),
            failures: 0,
        };
        service.persist_round_summary(&result).unwrap();
        let summary = db.get_database_sync_summary().unwrap().unwrap();
        assert_eq!(summary.downloaded, 4);
        assert_eq!(summary.uploaded, 3);
        assert_eq!(summary.conflicts, 3, "download+upload conflicts 均须保留");
        assert_eq!(summary.superseded, 1);
        assert_eq!(summary.version_regressions, 1);
        assert!(!summary.complete);
        assert!(summary.identity_error.is_none());
    }

    #[test]
    fn state_002_persistent_conflicts_force_conflict_status_and_incomplete_summary() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        db.set_local_library_id("same").unwrap();
        let service = DatabaseSyncService::new(db.clone());

        // 先写入一条持久化冲突（真源）
        let tag = db.create_tag("conflicted", None).unwrap();
        db.save_sync_conflict(&SyncConflict {
            entity_type: "tags".to_string(),
            entity_id: tag.id.clone(),
            local_record: "{}".into(),
            remote_record: "{}".into(),
            remote_version: 2,
            detected_at: 1,
        })
        .unwrap();

        // 安静轮次：无新冲突，status_for_run 本应 Idle
        let quiet = DatabaseSyncRunResult {
            identity: Some(IdentityDecision::Ready {
                full_snapshot_required: false,
            }),
            download: Some(DownloadResult::Applied {
                records: 1,
                conflicts: 0,
                version_regressions: 0,
            }),
            upload: Some(UploadResult {
                complete: true,
                ..Default::default()
            }),
            failures: 0,
        };
        assert_eq!(status_for_run(&quiet), DatabaseSyncStatus::Idle);
        let overlaid = apply_persistent_db_conflict_overlay(
            status_for_run(&quiet),
            db.list_sync_conflicts().unwrap().len(),
        );
        assert_eq!(overlaid, DatabaseSyncStatus::Conflict);

        service.persist_round_summary(&quiet).unwrap();
        let summary = db.get_database_sync_summary().unwrap().unwrap();
        assert!(!summary.complete, "持久化冲突存在时摘要不得 complete");
        assert!(summary.conflicts >= 1);

        // 恢复入口也必须报 Conflict
        let restored = service.restore_status_from_persistent_conflicts().unwrap();
        assert_eq!(restored, DatabaseSyncStatus::Conflict);
    }

    #[test]
    fn state_002_identity_error_still_wins_over_persistent_conflicts() {
        let status = apply_persistent_db_conflict_overlay(DatabaseSyncStatus::IdentityMismatch, 3);
        assert_eq!(status, DatabaseSyncStatus::IdentityMismatch);
    }

    #[test]
    fn state_002_identity_block_is_not_complete() {
        let summary = DatabaseSyncService::summarize_round(&DatabaseSyncRunResult {
            download: Some(DownloadResult::IdentityMismatch),
            ..Default::default()
        });
        assert!(!summary.complete);
        assert_eq!(summary.failures, 1);
        assert!(
            summary
                .identity_error
                .as_deref()
                .is_some_and(|e| e.contains("IdentityMismatch"))
        );
    }

    #[test]
    fn remote_new_record_is_applied() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        db.set_local_library_id("same").unwrap();
        let service = DatabaseSyncService::new(db.clone());
        let batch = RemoteBatch {
            library_id: "same".into(),
            last_sequence: 1,
            records: vec![record("tag-1", 1)],
        };
        assert_eq!(
            service.download(Some("same"), &batch).unwrap(),
            DownloadResult::Applied {
                records: 1,
                conflicts: 0,
                version_regressions: 0,
            }
        );
        assert_eq!(db.get_local_sync_state().unwrap().last_sequence, 1);
    }

    #[test]
    fn dirty_local_record_is_preserved_and_conflict_is_saved() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        db.set_local_library_id("same").unwrap();
        let local = db.create_tag("local", None).unwrap();
        db.set_synced_version(
            database::SyncEntityType::Tag,
            &database::SyncEntityKey::Id(local.id.clone()),
            1,
        )
        .unwrap();
        let service = DatabaseSyncService::new(db.clone());
        let batch = RemoteBatch {
            library_id: "same".into(),
            last_sequence: 2,
            records: vec![record(&local.id, 3)],
        };
        assert_eq!(
            service.download(Some("same"), &batch).unwrap(),
            DownloadResult::Applied {
                records: 0,
                conflicts: 1,
                version_regressions: 0,
            }
        );
        assert_eq!(db.get_local_sync_state().unwrap().last_sequence, 2);
        assert_eq!(db.list_sync_conflicts().unwrap().len(), 1);
        assert_eq!(
            db.get_all_tags_with_counts()
                .unwrap()
                .iter()
                .find(|(tag, _)| tag.id == local.id)
                .map(|(tag, _)| tag.name.as_str())
                .unwrap(),
            "local"
        );
        let conflict = db.list_sync_conflicts().unwrap().pop().unwrap();
        let local_payload: serde_json::Value =
            serde_json::from_str(&conflict.local_record).unwrap();
        assert_eq!(local_payload["name"], "local");
        assert_eq!(local_payload["is_dirty"], true);
        let remote_payload: serde_json::Value =
            serde_json::from_str(&conflict.remote_record).unwrap();
        assert_eq!(remote_payload["name"], "remote");
    }

    #[test]
    fn clean_remote_version_regression_is_not_applied_as_download() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        db.set_local_library_id("same").unwrap();
        let local = db.create_tag("local", None).unwrap();
        db.confirm_uploaded_snapshot(
            SyncEntityType::Tag,
            &SyncEntityKey::Id(local.id.clone()),
            1,
            0,
            10,
        )
        .unwrap();
        let service = DatabaseSyncService::new(db.clone());
        let batch = RemoteBatch {
            library_id: "same".into(),
            last_sequence: 5,
            records: vec![record(&local.id, 4)],
        };
        assert_eq!(
            service.download(Some("same"), &batch).unwrap(),
            DownloadResult::Applied {
                records: 0,
                conflicts: 0,
                version_regressions: 1,
            }
        );
        assert_eq!(
            db.get_download_state(SyncEntityType::Tag, &SyncEntityKey::Id(local.id.clone()))
                .unwrap(),
            Some((10, false))
        );
        assert!(db.list_sync_conflicts().unwrap().is_empty());
        assert_eq!(
            db.get_all_tags_with_counts()
                .unwrap()
                .iter()
                .find(|(tag, _)| tag.id == local.id)
                .map(|(tag, _)| tag.name.as_str())
                .unwrap(),
            "local"
        );
        assert_eq!(
            status_for_run(&DatabaseSyncRunResult {
                download: Some(DownloadResult::Applied {
                    records: 0,
                    conflicts: 0,
                    version_regressions: 1,
                }),
                ..Default::default()
            }),
            DatabaseSyncStatus::RemoteVersionRegression
        );
    }

    #[test]
    fn dirty_remote_version_regression_is_counted_not_conflicted() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        db.set_local_library_id("same").unwrap();
        let local = db.create_tag("local", None).unwrap();
        db.set_synced_version(SyncEntityType::Tag, &SyncEntityKey::Id(local.id.clone()), 9)
            .unwrap();
        let service = DatabaseSyncService::new(db.clone());
        let batch = RemoteBatch {
            library_id: "same".into(),
            last_sequence: 4,
            records: vec![record(&local.id, 2)],
        };
        assert_eq!(
            service.download(Some("same"), &batch).unwrap(),
            DownloadResult::Applied {
                records: 0,
                conflicts: 0,
                version_regressions: 1,
            }
        );
        assert!(db.list_sync_conflicts().unwrap().is_empty());
        assert_eq!(
            db.get_download_state(SyncEntityType::Tag, &SyncEntityKey::Id(local.id.clone()))
                .unwrap(),
            Some((9, true))
        );
    }

    #[test]
    fn dirty_local_record_is_preserved_when_remote_did_not_advance() {
        let db = Arc::new(Database::new(":memory:").unwrap());
        db.set_local_library_id("same").unwrap();
        let local = db.create_tag("local", None).unwrap();
        db.set_synced_version(
            database::SyncEntityType::Tag,
            &database::SyncEntityKey::Id(local.id.clone()),
            3,
        )
        .unwrap();
        let service = DatabaseSyncService::new(db.clone());
        let batch = RemoteBatch {
            library_id: "same".into(),
            last_sequence: 3,
            records: vec![record(&local.id, 3)],
        };
        assert_eq!(
            service.download(Some("same"), &batch).unwrap(),
            DownloadResult::Applied {
                records: 0,
                conflicts: 0,
                version_regressions: 0,
            }
        );
        assert!(db.list_sync_conflicts().unwrap().is_empty());
    }

    #[test]
    fn run_status_helper_prioritizes_partial_failure_over_conflict() {
        let conflict = DatabaseSyncRunResult {
            download: Some(DownloadResult::Applied {
                records: 0,
                conflicts: 1,
                version_regressions: 0,
            }),
            upload: Some(UploadResult {
                conflicts: 1,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(status_for_run(&conflict), DatabaseSyncStatus::Conflict);
        assert_eq!(
            status_for_run(&DatabaseSyncRunResult {
                upload: Some(UploadResult {
                    superseded: 1,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            DatabaseSyncStatus::PendingLocalChanges
        );
        let mut partial = conflict;
        partial.failures = 1;
        assert_eq!(status_for_run(&partial), DatabaseSyncStatus::PartialFailure);
        assert_eq!(
            status_for_run(&DatabaseSyncRunResult {
                identity: Some(IdentityDecision::NeedsRemoteInitialization),
                failures: 1,
                ..Default::default()
            }),
            DatabaseSyncStatus::NeedsRemoteInitialization
        );
        assert_eq!(
            status_for_run(&DatabaseSyncRunResult {
                identity: Some(IdentityDecision::NeedsRemoteAdoption {
                    library_id: "remote".into(),
                }),
                failures: 1,
                ..Default::default()
            }),
            DatabaseSyncStatus::NeedsRemoteAdoption
        );
        assert_eq!(
            status_for_run(&DatabaseSyncRunResult {
                identity: Some(IdentityDecision::Mismatch {
                    local_id: "local".into(),
                    remote_id: "remote".into(),
                }),
                failures: 1,
                ..Default::default()
            }),
            DatabaseSyncStatus::IdentityMismatch
        );
        assert_eq!(
            status_for_run(&DatabaseSyncRunResult::default()),
            DatabaseSyncStatus::Idle
        );
        assert_eq!(
            status_for_run(&DatabaseSyncRunResult {
                download: Some(DownloadResult::Applied {
                    records: 0,
                    conflicts: 0,
                    version_regressions: 2,
                }),
                ..Default::default()
            }),
            DatabaseSyncStatus::RemoteVersionRegression
        );
        assert_eq!(
            status_for_run(&DatabaseSyncRunResult {
                identity: Some(IdentityDecision::Ready {
                    full_snapshot_required: false
                }),
                download: Some(DownloadResult::IdentityMismatch),
                ..Default::default()
            }),
            DatabaseSyncStatus::IdentityMismatch
        );
    }

    #[tokio::test]
    async fn purge_deleted_data_is_blocked_without_touching_mysql() {
        let service = DatabaseSyncService::new(Arc::new(Database::new(":memory:").unwrap()));
        let err = service.purge_deleted_data().await.unwrap_err();
        assert!(
            err.to_string()
                .contains("remote tombstone purge is currently unsupported"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn notification_helper_sends_data_and_ui_without_sync_request() {
        let data_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let ui_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let data = {
            let count = data_count.clone();
            Arc::new(move || {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }) as Arc<dyn Fn() + Send + Sync>
        };
        let ui = {
            let count = ui_count.clone();
            Arc::new(move || {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }) as Arc<dyn Fn() + Send + Sync>
        };
        notify_data_and_ui(Some(&data), Some(&ui));
        assert_eq!(data_count.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(ui_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
