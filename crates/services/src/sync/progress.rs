//! 同步状态类型（跨线程共享，从 Tokio 写入，UI 读取）
//!
//! 原定义于 `lumen` 的 `MainApp`，随同步编排一并下沉到 `services::sync`，
//! 使 `MainApp` 只持有 `Arc<Mutex<SyncStateInner>>` 引用而不再定义该类型。

use crate::sync::attachments::FileLibraryPreflight;

/// 同步状态
#[derive(Debug, Clone, PartialEq)]
pub enum SyncStatus {
    Idle,
    Syncing,
    Error(String),
}

/// 脱敏的文件同步错误类别。
///
/// 禁止携带路径、文件名、object key、URL、版本、hash、账号或 Token。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSyncErrorKind {
    /// 后端未启用或配置不可用
    BackendDisabled,
    /// 远端清单（list）获取失败或返回不确定结果
    ListFailed,
    /// 文件库探测失败（网络/解析）
    PreflightFailed,
    /// 传输阶段存在失败对象
    TransferFailed,
    /// Summary 持久化失败（不得报告 Complete）
    SummaryPersistFailed,
    /// 其他内部错误
    Internal,
}

impl FileSyncErrorKind {
    /// 用于日志与持久化 `reason` 的脱敏类别字符串。
    pub fn category(self) -> &'static str {
        match self {
            Self::BackendDisabled => "backend_disabled",
            Self::ListFailed => "list_failed",
            Self::PreflightFailed => "preflight_failed",
            Self::TransferFailed => "transfer_failed",
            Self::SummaryPersistFailed => "summary_persist_failed",
            Self::Internal => "internal",
        }
    }
}

/// 文件同步独立状态。
///
/// 与数据库状态分开保存与展示，禁止互相覆盖。
/// `InitializationRequired` / `IdentityMismatch` / `UnidentifiedRemote` /
/// `WaitingForDatabaseIdentity` / `Disabled` 与 `FileLibraryPreflight` 一一对应，
/// 由 `from_preflight` 固定映射（测试锁定）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileSyncStatus {
    Idle,
    Disabled,
    Syncing,
    Complete,
    PartialFailure,
    /// STATE-001: 存在冲突/未知分歧/无法恢复对象，需要用户关注
    NeedsAttention,
    /// STATE-001: 仅等待数据库确认或按需下载，不得显示 Complete
    Waiting,
    Error(FileSyncErrorKind),
    WaitingForDatabaseIdentity,
    InitializationRequired,
    UnidentifiedRemote,
    IdentityMismatch,
}

impl FileSyncStatus {
    /// `FileLibraryPreflight` → 文件状态的唯一映射。
    ///
    /// `Ready` 在探测成功但轮次尚未完成时仍是 `Syncing`；
    /// 最终 Complete/PartialFailure 由协调器按轮次结果收敛。
    pub fn from_preflight(preflight: &FileLibraryPreflight) -> Self {
        match preflight {
            FileLibraryPreflight::BackendDisabled => Self::Disabled,
            FileLibraryPreflight::WaitingForDatabaseIdentity => Self::WaitingForDatabaseIdentity,
            FileLibraryPreflight::InitializationRequired => Self::InitializationRequired,
            FileLibraryPreflight::UnidentifiedRemote => Self::UnidentifiedRemote,
            // ID 只用于判定，不进入状态（脱敏）
            FileLibraryPreflight::IdentityMismatch { .. } => Self::IdentityMismatch,
            FileLibraryPreflight::Ready { .. } => Self::Syncing,
            // 原始错误信息不进入状态（脱敏）
            FileLibraryPreflight::Error(_) => Self::Error(FileSyncErrorKind::PreflightFailed),
        }
    }

    /// 持久化 `state` 字段使用的脱敏类别字符串。
    pub fn state_name(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Disabled => "disabled",
            Self::Syncing => "syncing",
            Self::Complete => "complete",
            Self::PartialFailure => "partial_failure",
            Self::NeedsAttention => "needs_attention",
            Self::Waiting => "waiting",
            Self::Error(_) => "error",
            Self::WaitingForDatabaseIdentity => "waiting_for_database_identity",
            Self::InitializationRequired => "initialization_required",
            Self::UnidentifiedRemote => "unidentified_remote",
            Self::IdentityMismatch => "identity_mismatch",
        }
    }

    /// 是否属于身份阻断状态（不得写成 Idle）。
    pub fn is_identity_blocked(&self) -> bool {
        matches!(
            self,
            Self::InitializationRequired
                | Self::IdentityMismatch
                | Self::UnidentifiedRemote
                | Self::WaitingForDatabaseIdentity
        )
    }

    /// 由持久化的 `state` / `reason` 还原类型化状态。
    ///
    /// 未知类别返回 `None`（由调用方视为损坏，不得静默降级为 Idle）。
    pub fn from_state_name(state: &str, reason: Option<&str>) -> Option<Self> {
        let status = match state {
            "idle" => Self::Idle,
            "disabled" => Self::Disabled,
            "syncing" => Self::Syncing,
            "complete" => Self::Complete,
            "partial_failure" => Self::PartialFailure,
            "needs_attention" => Self::NeedsAttention,
            "waiting" => Self::Waiting,
            "waiting_for_database_identity" => Self::WaitingForDatabaseIdentity,
            "initialization_required" => Self::InitializationRequired,
            "unidentified_remote" => Self::UnidentifiedRemote,
            "identity_mismatch" => Self::IdentityMismatch,
            "error" => Self::Error(match reason {
                Some("backend_disabled") => FileSyncErrorKind::BackendDisabled,
                Some("list_failed") => FileSyncErrorKind::ListFailed,
                Some("preflight_failed") => FileSyncErrorKind::PreflightFailed,
                Some("transfer_failed") => FileSyncErrorKind::TransferFailed,
                Some("summary_persist_failed") => FileSyncErrorKind::SummaryPersistFailed,
                _ => FileSyncErrorKind::Internal,
            }),
            _ => return None,
        };
        Some(status)
    }
}

/// 数据库阶段的类型化结果。
///
/// 协调器不得再用 `failures == 0` 推断一切：必须区分未配置与失败，
/// 并明确本轮是否允许远端删除。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseDisposition {
    /// 未配置远端数据库
    Disabled,
    /// 本轮数据库同步成功完成
    CompleteReady,
    /// 部分成功（存在失败记录，但已有确认状态可用）
    PartialFailure,
    PendingLocalChanges,
    /// DB-004: remote versions regressed below local synced_version
    RemoteVersionRegression,
    /// 需要用户确认远端初始化
    InitializationRequired,
    /// 需要用户确认远端采纳
    AdoptionRequired,
    /// 远端 database_library_id 与本地不一致
    IdentityMismatch,
    /// 数据库同步阶段错误
    Error,
}

impl DatabaseDisposition {
    /// 仅完整成功的一轮才允许远端删除（其余一律禁止 Delete）。
    pub fn allows_remote_delete(&self) -> bool {
        matches!(self, Self::CompleteReady)
    }

    /// 文件恢复下载：身份阻断/未配置远端时禁止；失败/冲突/回退仍允许只读恢复。
    pub fn allows_file_recovery(&self) -> bool {
        !matches!(
            self,
            Self::InitializationRequired
                | Self::AdoptionRequired
                | Self::IdentityMismatch
                | Self::Disabled
        )
    }

    /// COORD-001: 已确认附件对象的条件上传/更新。
    /// 身份阻断与 Disabled 禁止；PartialFailure/Pending/Regression/Error 允许
    /// （逐附件仍须 database_confirmed）。
    pub fn allows_confirmed_file_upload(&self) -> bool {
        matches!(
            self,
            Self::CompleteReady
                | Self::PartialFailure
                | Self::PendingLocalChanges
                | Self::RemoteVersionRegression
                | Self::Error
        )
    }

    pub fn category(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::CompleteReady => "complete_ready",
            Self::PartialFailure => "partial_failure",
            Self::PendingLocalChanges => "pending_local_changes",
            Self::RemoteVersionRegression => "remote_version_regression",
            Self::InitializationRequired => "initialization_required",
            Self::AdoptionRequired => "adoption_required",
            Self::IdentityMismatch => "identity_mismatch",
            Self::Error => "error",
        }
    }
}

/// 本轮文件变更策略，替换粗粒度的上传/删除布尔开关。
///
/// 具体每个对象是否可上传/删除，仍由逐附件 `database_confirmed` 决定。
/// COORD-001 矩阵：
/// - CompleteReady: 确认上传 + 确认删除 + 恢复
/// - PartialFailure / Pending / Regression / Error: 确认上传，禁止删除，允许恢复
/// - Identity* / Disabled: 禁止确认上传/删除；恢复见 `allow_file_recovery`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMutationPolicy {
    pub allow_confirmed_upload: bool,
    pub allow_confirmed_delete: bool,
    /// 缺失文件的安全恢复下载（身份阻断时为 false）
    pub allow_file_recovery: bool,
}

impl FileMutationPolicy {
    /// 完全成功（CompleteReady）的一轮：已确认对象可上传、已确认 tombstone 可删除。
    pub fn allow_all() -> Self {
        Self {
            allow_confirmed_upload: true,
            allow_confirmed_delete: true,
            allow_file_recovery: true,
        }
    }

    /// 禁止变更（上传/删除），仍允许安全恢复 — 用于策略测试与部分失败语义。
    pub fn deny_all() -> Self {
        Self {
            allow_confirmed_upload: false,
            allow_confirmed_delete: false,
            allow_file_recovery: true,
        }
    }

    /// 身份阻断/远端未配置：变更与恢复均禁止。
    pub fn block_mutations_and_recovery() -> Self {
        Self {
            allow_confirmed_upload: false,
            allow_confirmed_delete: false,
            allow_file_recovery: false,
        }
    }

    /// 由数据库 disposition 推导本轮策略。
    pub fn from_disposition(disposition: DatabaseDisposition) -> Self {
        Self {
            allow_confirmed_upload: disposition.allows_confirmed_file_upload(),
            allow_confirmed_delete: disposition.allows_remote_delete(),
            allow_file_recovery: disposition.allows_file_recovery(),
        }
    }
}

/// UI-facing 只读文件同步结果视图。
///
/// UI 不得直接访问 database；services 负责从持久化 DTO 映射为类型化状态。
/// 由 `SyncService::file_sync_summary` 构造，UI 只读展示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSyncSummaryView {
    pub uploaded: usize,
    pub downloaded: usize,
    pub deleted: usize,
    pub skipped: usize,
    pub waiting: usize,
    pub pending_download: usize,
    pub unrecoverable_missing: usize,
    pub conflicts: usize,
    pub unknown_divergence: usize,
    pub failures: usize,
    pub state: FileSyncStatus,
    pub run_id: String,
    pub file_library_id: Option<String>,
    pub updated_at: i64,
}

impl FileSyncSummaryView {
    /// 由 database 持久化 DTO 映射为类型化 UI 视图。
    ///
    /// `state` 类别无法识别时返回错误，不得静默降级为 Idle/Complete。
    pub fn from_persisted(summary: &database::sqlite::FileSyncSummary) -> anyhow::Result<Self> {
        let state = FileSyncStatus::from_state_name(&summary.state, summary.reason.as_deref())
            .ok_or_else(|| anyhow::anyhow!("未知的文件同步状态类别，拒绝降级展示"))?;
        Ok(Self {
            uploaded: summary.uploaded,
            downloaded: summary.downloaded,
            deleted: summary.deleted,
            skipped: summary.skipped,
            waiting: summary.waiting,
            pending_download: summary.pending_download,
            unrecoverable_missing: summary.unrecoverable_missing,
            conflicts: summary.conflicts,
            unknown_divergence: summary.unknown_divergence,
            failures: summary.failures,
            state,
            run_id: summary.run_id.clone(),
            file_library_id: summary.file_library_id.clone(),
            updated_at: summary.updated_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DatabaseSyncStatus {
    Idle,
    Syncing,
    NeedsRemoteInitialization,
    NeedsRemoteAdoption,
    IdentityMismatch,
    Conflict,
    PartialFailure,
    PendingLocalChanges,
    /// DB-004: remote versions regressed; local was not overwritten.
    RemoteVersionRegression,
    Error(String),
}

/// STATE-003: 正交复合状态 flags — 与主 status 并存，避免互斥遮蔽。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncCompositeFlags {
    pub running: bool,
    pub identity_blocked: bool,
    pub partial_failure_count: usize,
    pub database_conflict_count: usize,
    pub file_conflict_count: usize,
    pub waiting_count: usize,
    pub unrecoverable_count: usize,
    pub last_error_category: Option<&'static str>,
}

impl SyncCompositeFlags {
    pub fn from_statuses(
        db: &DatabaseSyncStatus,
        file: &FileSyncStatus,
        db_conflicts: usize,
        file_conflicts: usize,
        waiting: usize,
        unrecoverable: usize,
        partial_failures: usize,
    ) -> Self {
        let identity_blocked = matches!(
            db,
            DatabaseSyncStatus::NeedsRemoteInitialization
                | DatabaseSyncStatus::NeedsRemoteAdoption
                | DatabaseSyncStatus::IdentityMismatch
        ) || matches!(
            file,
            FileSyncStatus::WaitingForDatabaseIdentity
                | FileSyncStatus::InitializationRequired
                | FileSyncStatus::IdentityMismatch
                | FileSyncStatus::UnidentifiedRemote
        );
        let running =
            matches!(db, DatabaseSyncStatus::Syncing) || matches!(file, FileSyncStatus::Syncing);
        let last_error_category = match db {
            DatabaseSyncStatus::Error(_) => Some("database_error"),
            _ => match file {
                FileSyncStatus::Error(kind) => Some(kind.category()),
                _ => None,
            },
        };
        Self {
            running,
            identity_blocked,
            partial_failure_count: partial_failures,
            database_conflict_count: db_conflicts,
            file_conflict_count: file_conflicts,
            waiting_count: waiting,
            unrecoverable_count: unrecoverable,
            last_error_category,
        }
    }

    /// 是否存在任何需要用户关注的非空 flags。
    pub fn has_attention(&self) -> bool {
        self.identity_blocked
            || self.partial_failure_count > 0
            || self.database_conflict_count > 0
            || self.file_conflict_count > 0
            || self.unrecoverable_count > 0
            || self.last_error_category.is_some()
    }
}

/// 同步状态（跨线程共享）
///
/// - `sync_status`：顶层聚合状态，由 `database_sync` 在异步任务中写入，供窗口级错误展示。
/// - `database_sync_status` / `file_sync_status`：数据库与文件各自独立真源，互不覆盖。
/// - `composite`：STATE-003 正交 flags，与主 status 并存。
#[derive(Debug, Clone)]
pub struct SyncStateInner {
    /// 顶层聚合状态，供窗口级错误展示（database_sync 写入）。
    pub sync_status: SyncStatus,
    pub database_sync_status: DatabaseSyncStatus,
    /// 类型化文件同步状态（阶段 4 真源），独立于 `database_sync_status`。
    pub file_sync_status: FileSyncStatus,
    pub composite: SyncCompositeFlags,
}

impl SyncStateInner {
    pub fn new() -> Self {
        Self {
            sync_status: SyncStatus::Idle,
            database_sync_status: DatabaseSyncStatus::Idle,
            file_sync_status: FileSyncStatus::Idle,
            composite: SyncCompositeFlags::default(),
        }
    }

    /// STATE-003: 按当前主状态与计数刷新 composite flags。
    pub fn refresh_composite(
        &mut self,
        db_conflicts: usize,
        file_conflicts: usize,
        waiting: usize,
        unrecoverable: usize,
        partial_failures: usize,
    ) {
        self.composite = SyncCompositeFlags::from_statuses(
            &self.database_sync_status,
            &self.file_sync_status,
            db_conflicts,
            file_conflicts,
            waiting,
            unrecoverable,
            partial_failures,
        );
    }
}

impl Default for SyncStateInner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::sqlite::FileSyncSummary;

    // ---------- FileLibraryPreflight → FileSyncStatus 一一映射 ----------

    #[test]
    fn file_sync_status_maps_every_preflight_variant() {
        assert_eq!(
            FileSyncStatus::from_preflight(&FileLibraryPreflight::BackendDisabled),
            FileSyncStatus::Disabled
        );
        assert_eq!(
            FileSyncStatus::from_preflight(&FileLibraryPreflight::WaitingForDatabaseIdentity),
            FileSyncStatus::WaitingForDatabaseIdentity
        );
        assert_eq!(
            FileSyncStatus::from_preflight(&FileLibraryPreflight::InitializationRequired),
            FileSyncStatus::InitializationRequired
        );
        assert_eq!(
            FileSyncStatus::from_preflight(&FileLibraryPreflight::UnidentifiedRemote),
            FileSyncStatus::UnidentifiedRemote
        );
        assert_eq!(
            FileSyncStatus::from_preflight(&FileLibraryPreflight::Ready {
                file_library_id: "flib-A".to_string(),
                database_library_id: "dblib-1".to_string(),
            }),
            FileSyncStatus::Syncing
        );
    }

    #[test]
    fn file_sync_status_mapping_drops_sensitive_payloads() {
        // IdentityMismatch 的两个 library ID 不得进入状态
        let mismatched = FileSyncStatus::from_preflight(&FileLibraryPreflight::IdentityMismatch {
            local_db_lib_id: "local-secret".to_string(),
            remote_db_lib_id: "remote-secret".to_string(),
        });
        assert_eq!(mismatched, FileSyncStatus::IdentityMismatch);
        assert!(!format!("{mismatched:?}").contains("local-secret"));
        assert!(!format!("{mismatched:?}").contains("remote-secret"));

        // 原始错误信息不得进入状态
        let errored = FileSyncStatus::from_preflight(&FileLibraryPreflight::Error(
            "connection refused at /secret/path".to_string(),
        ));
        assert_eq!(
            errored,
            FileSyncStatus::Error(FileSyncErrorKind::PreflightFailed)
        );
        assert!(!format!("{errored:?}").contains("/secret/path"));
    }

    #[test]
    fn file_sync_status_identity_blocked_variants() {
        assert!(FileSyncStatus::InitializationRequired.is_identity_blocked());
        assert!(FileSyncStatus::IdentityMismatch.is_identity_blocked());
        assert!(FileSyncStatus::UnidentifiedRemote.is_identity_blocked());
        assert!(FileSyncStatus::WaitingForDatabaseIdentity.is_identity_blocked());
        assert!(!FileSyncStatus::Complete.is_identity_blocked());
        assert!(!FileSyncStatus::PartialFailure.is_identity_blocked());
        assert!(!FileSyncStatus::Idle.is_identity_blocked());
    }

    // ---------- state / reason 编解码 ----------

    #[test]
    fn file_sync_status_state_name_round_trip() {
        let statuses = [
            FileSyncStatus::Idle,
            FileSyncStatus::Disabled,
            FileSyncStatus::Syncing,
            FileSyncStatus::Complete,
            FileSyncStatus::PartialFailure,
            FileSyncStatus::NeedsAttention,
            FileSyncStatus::Waiting,
            FileSyncStatus::WaitingForDatabaseIdentity,
            FileSyncStatus::InitializationRequired,
            FileSyncStatus::UnidentifiedRemote,
            FileSyncStatus::IdentityMismatch,
            FileSyncStatus::Error(FileSyncErrorKind::ListFailed),
            FileSyncStatus::Error(FileSyncErrorKind::SummaryPersistFailed),
        ];
        for status in statuses {
            let reason = match status {
                FileSyncStatus::Error(kind) => Some(kind.category().to_string()),
                _ => None,
            };
            let decoded =
                FileSyncStatus::from_state_name(status.state_name(), reason.as_deref()).unwrap();
            assert_eq!(decoded, status, "{status:?} 必须可 round-trip");
        }
    }

    #[test]
    fn file_sync_status_unknown_state_name_is_rejected() {
        assert!(FileSyncStatus::from_state_name("totally-unknown", None).is_none());
        // 损坏不得静默降级为 Idle/Complete
        let decoded = FileSyncStatus::from_state_name("totally-unknown", None);
        assert_ne!(decoded, Some(FileSyncStatus::Idle));
        assert_ne!(decoded, Some(FileSyncStatus::Complete));
    }

    // ---------- disposition / policy 矩阵 ----------

    #[test]
    fn database_disposition_delete_matrix() {
        // 仅完整成功允许远端删除
        assert!(DatabaseDisposition::CompleteReady.allows_remote_delete());
        assert!(!DatabaseDisposition::PartialFailure.allows_remote_delete());
        assert!(!DatabaseDisposition::Error.allows_remote_delete());
        assert!(!DatabaseDisposition::Disabled.allows_remote_delete());
        assert!(!DatabaseDisposition::InitializationRequired.allows_remote_delete());
        assert!(!DatabaseDisposition::AdoptionRequired.allows_remote_delete());
        assert!(!DatabaseDisposition::IdentityMismatch.allows_remote_delete());
        assert!(!DatabaseDisposition::RemoteVersionRegression.allows_remote_delete());
        assert!(DatabaseDisposition::RemoteVersionRegression.allows_file_recovery());
    }

    #[test]
    fn database_disposition_recovery_matrix() {
        // 数据库失败时文件恢复仍可继续（身份阻断除外）
        assert!(DatabaseDisposition::CompleteReady.allows_file_recovery());
        assert!(DatabaseDisposition::PartialFailure.allows_file_recovery());
        assert!(DatabaseDisposition::Error.allows_file_recovery());
        assert!(!DatabaseDisposition::InitializationRequired.allows_file_recovery());
        assert!(!DatabaseDisposition::AdoptionRequired.allows_file_recovery());
        assert!(!DatabaseDisposition::IdentityMismatch.allows_file_recovery());
        // COORD-001: Disabled 不得冒充 CompleteReady
        assert!(!DatabaseDisposition::Disabled.allows_file_recovery());
        assert!(!DatabaseDisposition::Disabled.allows_confirmed_file_upload());
        assert!(DatabaseDisposition::PartialFailure.allows_confirmed_file_upload());
        assert!(DatabaseDisposition::PendingLocalChanges.allows_confirmed_file_upload());
    }

    #[test]
    fn state_003_composite_flags_coexist_with_primary_status() {
        let mut state = SyncStateInner::new();
        state.database_sync_status = DatabaseSyncStatus::Conflict;
        state.file_sync_status = FileSyncStatus::NeedsAttention;
        state.refresh_composite(3, 2, 4, 1, 0);
        assert_eq!(state.composite.database_conflict_count, 3);
        assert_eq!(state.composite.file_conflict_count, 2);
        assert_eq!(state.composite.waiting_count, 4);
        assert_eq!(state.composite.unrecoverable_count, 1);
        assert!(state.composite.has_attention());
        // 主 status 仍为 Conflict，不丢失
        assert_eq!(state.database_sync_status, DatabaseSyncStatus::Conflict);
        assert_eq!(state.file_sync_status, FileSyncStatus::NeedsAttention);

        state.database_sync_status = DatabaseSyncStatus::IdentityMismatch;
        state.file_sync_status = FileSyncStatus::WaitingForDatabaseIdentity;
        state.refresh_composite(1, 0, 0, 0, 0);
        assert!(state.composite.identity_blocked);
        assert_eq!(state.composite.database_conflict_count, 1);
    }

    #[test]
    fn file_mutation_policy_follows_disposition() {
        let ready = FileMutationPolicy::from_disposition(DatabaseDisposition::CompleteReady);
        assert!(ready.allow_confirmed_upload);
        assert!(ready.allow_confirmed_delete);

        let partial = FileMutationPolicy::from_disposition(DatabaseDisposition::PartialFailure);
        assert!(partial.allow_confirmed_upload);
        assert!(!partial.allow_confirmed_delete, "部分失败禁止删除");

        let errored = FileMutationPolicy::from_disposition(DatabaseDisposition::Error);
        assert!(errored.allow_confirmed_upload);
        assert!(!errored.allow_confirmed_delete);

        let mismatch = FileMutationPolicy::from_disposition(DatabaseDisposition::IdentityMismatch);
        assert!(!mismatch.allow_confirmed_upload);
        assert!(!mismatch.allow_confirmed_delete);
        assert!(!mismatch.allow_file_recovery);

        let disabled = FileMutationPolicy::from_disposition(DatabaseDisposition::Disabled);
        assert!(!disabled.allow_confirmed_upload);
        assert!(!disabled.allow_confirmed_delete);
        assert!(!disabled.allow_file_recovery);

        let denied = FileMutationPolicy::deny_all();
        assert!(!denied.allow_confirmed_upload);
        assert!(!denied.allow_confirmed_delete);
        assert!(denied.allow_file_recovery, "deny_all 仍允许安全恢复");
    }

    // ---------- 持久化 DTO → UI 视图 ----------

    fn persisted(state: &str, reason: Option<&str>) -> FileSyncSummary {
        FileSyncSummary {
            uploaded: 1,
            downloaded: 2,
            deleted: 3,
            skipped: 4,
            waiting: 5,
            pending_download: 6,
            unrecoverable_missing: 7,
            conflicts: 8,
            unknown_divergence: 9,
            failures: 10,
            state: state.to_string(),
            reason: reason.map(|r| r.to_string()),
            run_id: "run-abcd1234".to_string(),
            file_library_id: Some("flib-A".to_string()),
            updated_at: 1700000000,
        }
    }

    #[test]
    fn file_sync_summary_view_maps_all_counters_from_persisted() {
        let view =
            FileSyncSummaryView::from_persisted(&persisted("partial_failure", None)).unwrap();
        assert_eq!(view.state, FileSyncStatus::PartialFailure);
        assert_eq!(view.uploaded, 1);
        assert_eq!(view.downloaded, 2);
        assert_eq!(view.deleted, 3);
        assert_eq!(view.skipped, 4);
        assert_eq!(view.waiting, 5);
        assert_eq!(view.pending_download, 6);
        assert_eq!(view.unrecoverable_missing, 7);
        assert_eq!(view.conflicts, 8);
        assert_eq!(view.unknown_divergence, 9);
        assert_eq!(view.failures, 10);
        assert_eq!(view.run_id, "run-abcd1234");
        assert_eq!(view.file_library_id.as_deref(), Some("flib-A"));
        assert_eq!(view.updated_at, 1700000000);
    }

    #[test]
    fn file_sync_summary_view_rejects_unknown_state_instead_of_degrading() {
        let result = FileSyncSummaryView::from_persisted(&persisted("bogus-state", None));
        assert!(result.is_err(), "未知状态必须报错，不得降级展示");
    }

    #[test]
    fn file_sync_summary_view_restores_typed_error_kind() {
        let view = FileSyncSummaryView::from_persisted(&persisted(
            "error",
            Some("summary_persist_failed"),
        ))
        .unwrap();
        assert_eq!(
            view.state,
            FileSyncStatus::Error(FileSyncErrorKind::SummaryPersistFailed)
        );
    }
}
