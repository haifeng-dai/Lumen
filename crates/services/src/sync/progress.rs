//! 同步状态类型（跨线程共享，从 Tokio 写入，UI 读取）
//!
//! 原定义于 `lumen` 的 `MainApp`，随同步编排一并下沉到 `services::sync`，
//! 使 `MainApp` 只持有 `Arc<Mutex<SyncStateInner>>` 引用而不再定义该类型。

/// 同步状态
#[derive(Debug, Clone, PartialEq)]
pub enum SyncStatus {
    Idle,
    Syncing,
    Error(String),
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
    Error(String),
}

/// 同步状态（跨线程共享）
///
/// - `sync_status` / `attachment_sync_status`：由 `engine`/`metadata`/`attachments` 在异步任务中写入。
#[derive(Debug, Clone)]
pub struct SyncStateInner {
    /// Legacy aggregate status used by the existing window-level error display.
    pub sync_status: SyncStatus,
    pub database_sync_status: DatabaseSyncStatus,
    pub attachment_sync_status: SyncStatus,
}

impl SyncStateInner {
    pub fn new() -> Self {
        Self {
            sync_status: SyncStatus::Idle,
            database_sync_status: DatabaseSyncStatus::Idle,
            attachment_sync_status: SyncStatus::Idle,
        }
    }
}

impl Default for SyncStateInner {
    fn default() -> Self {
        Self::new()
    }
}
