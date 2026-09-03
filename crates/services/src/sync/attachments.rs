//! 附件同步服务模块
//!
//! 前端服务层：负责基于对象键（objects/v1/<attachment_id>）的新协议文件同步流程编排。
//! 底层文件操作和远端协议由 `crates/file/` 实现。

use crate::runtime::RUNTIME;
use crate::sync::progress::FileMutationPolicy;
use anyhow::{Result, anyhow};
use database::Database;
use database::sqlite::{
    AttachmentFileBaseline, AttachmentFileConflict, AttachmentPendingDownload,
    AttachmentSyncSnapshot, FileLibraryBinding,
};
use file::{
    AttachmentBackend, FileLibraryIdentity, LibraryInspection, LocalFileManager, UploadObjectResult,
};
use sha2::{Digest, Sha256};

use log::{debug, error, info};
use models::Attachment;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// 远端文件资料库 Preflight 状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileLibraryPreflight {
    /// 后端未启用
    BackendDisabled,
    /// 缺少本地数据库 Library ID，等待数据库同步初始化
    WaitingForDatabaseIdentity,
    /// 远端明确为空且未初始化，需要用户确认初始化
    InitializationRequired,
    /// 远端存在未知内容但缺失 Identity 文件，不可识别
    UnidentifiedRemote,
    /// 远端 Identity 中的 Database Library ID 与本地不一致
    IdentityMismatch {
        local_db_lib_id: String,
        remote_db_lib_id: String,
    },
    /// 远端已正确初始化且匹配，就绪执行文件同步
    Ready {
        file_library_id: String,
        database_library_id: String,
    },
    /// 探测发生网络或解析错误
    Error(String),
}

/// 单个附件的文件同步动作计划
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileActionPlan {
    /// 活跃且本地存在，远端对象不存在：Create-Only 上传
    Upload {
        attachment_id: String,
        object_key: String,
        local_path: PathBuf,
    },
    /// 活跃且本地缺失，远端对象存在：安全下载（adopt_baseline 表示是否在首次见库时直接采用基线）
    Download {
        attachment_id: String,
        object_key: String,
        target_file_name: String,
        adopt_baseline: bool,
    },
    /// 软删除且远端对象存在：删除远端对象
    Delete {
        attachment_id: String,
        object_key: String,
    },
    /// 活跃、本地缺失且远端缺失：物理丢失，零写入
    UnrecoverableMissing { attachment_id: String },
    /// 本地未通过数据库远端确认（is_dirty=true）：等待数据库确认，零上传/删除
    WaitingForDatabaseConfirmation { attachment_id: String },
    /// 软删除且远端对象已不存在：清理残留 Baseline
    ClearBaseline { attachment_id: String },
    /// 状态完全一致且 Baseline 匹配：跳过
    Skip { attachment_id: String },
    /// 远端与本地散列冲突
    FileConflict {
        attachment_id: String,
        object_key: String,
        remote_version: String,
        local_sha256: String,
    },
    /// 远端版本未知或本地哈希无法计算导致的分歧
    UnknownVersionDivergence {
        attachment_id: String,
        object_key: String,
        remote_version: String,
        local_sha256: String,
    },
    /// 按需下载登记（on-demand 场景）
    PendingDownload {
        attachment_id: String,
        object_key: String,
        remote_version: String,
        target_file_name: String,
    },
}

/// 文件同步轮次汇总统计
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRoundSummary {
    pub preflight: FileLibraryPreflight,
    pub uploaded: usize,
    pub downloaded: usize,
    pub deleted: usize,
    pub skipped: usize,
    pub waiting: usize,
    pub failed: usize,
    pub conflicts: usize,
    pub unknown_divergence: usize,
    pub pending_download: usize,
    pub unrecoverable_missing: usize,
}

/// 附件同步状态/冲突查询结果（纯数据模型，供 UI 获取只读诊断展示）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentSyncIssue {
    /// 存在明确文件冲突（两端均发生修改）
    FileConflict,
    /// 存在未知版本分歧（首次见库内容不同或缺少 baseline）
    UnknownDivergence,
    /// 等待按需下载
    PendingDownload,
}

/// 打开前准备完成后的纯数据结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedAttachment {
    pub attachment_id: String,
    pub local_path: PathBuf,
    pub issue: Option<AttachmentSyncIssue>,
}

/// 打开前准备失败的类型化错误分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepareAttachmentErrorKind {
    UnrecoverableMissing,
    Conflict,
    IdentityMismatch,
    BackendUnavailable,
    LocalWrite,
    InvalidState,
}

/// 类型化准备错误；Display 仅暴露脱敏类别说明。
#[derive(Debug)]
pub struct PrepareAttachmentError {
    kind: PrepareAttachmentErrorKind,
    message: String,
}

impl PrepareAttachmentError {
    fn new(kind: PrepareAttachmentErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> PrepareAttachmentErrorKind {
        self.kind
    }
}

impl std::fmt::Display for PrepareAttachmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PrepareAttachmentError {}

pub fn classify_prepare_attachment_error(error: &anyhow::Error) -> PrepareAttachmentErrorKind {
    error
        .downcast_ref::<PrepareAttachmentError>()
        .map(PrepareAttachmentError::kind)
        .unwrap_or(PrepareAttachmentErrorKind::InvalidState)
}

fn prepare_error(kind: PrepareAttachmentErrorKind, message: impl Into<String>) -> anyhow::Error {
    PrepareAttachmentError::new(kind, message).into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadyFileLibrary {
    file_library_id: String,
    database_library_id: String,
    backend_kind: String,
    backend_fingerprint: String,
}

/// 首次见库安全比较的执行结果（私有辅助，不新增公共 API）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirstSeenComparisonOutcome {
    /// 两端内容一致：已建立 baseline，正式文件未被替换
    BaselineAdopted,
    /// 两端内容不同：已记录 unknown_divergence，正式文件字节未变
    DivergenceRecorded,
    /// 任一步骤失败：正式文件与 baseline 均保持原状
    Failed,
}

/// 附件同步服务
pub struct FileSyncService {
    db: Arc<Database>,
    file_manager: LocalFileManager,
    backend: Arc<tokio::sync::Mutex<Box<dyn AttachmentBackend>>>,
    on_demand: AtomicBool,
    notify_ui: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for FileSyncService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSyncService")
            .field("db", &self.db)
            .field("file_manager", &self.file_manager)
            .field("on_demand", &self.on_demand.load(Ordering::Relaxed))
            .finish()
    }
}

impl FileSyncService {
    pub fn new(
        db: Arc<Database>,
        file_manager: LocalFileManager,
        backend: Box<dyn AttachmentBackend>,
        notify_ui: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        info!("存储管理: [File] 正在初始化文件同步服务 (新对象协议)...");

        Self {
            db,
            file_manager,
            backend: Arc::new(tokio::sync::Mutex::new(backend)),
            on_demand: AtomicBool::new(false),
            notify_ui,
        }
    }

    pub fn swap_backend(&self, backend: Box<dyn AttachmentBackend>) {
        info!("存储管理: [File] 正在更换后端适配器");
        let mut b = self.backend.blocking_lock();
        *b = backend;
        info!("存储管理: [File] 后端适配器更换完成");
    }

    pub fn set_on_demand(&self, on_demand: bool) {
        debug!("存储管理: [File] 设置按需下载模式: {on_demand}");
        self.on_demand.store(on_demand, Ordering::Relaxed);
    }

    pub fn is_on_demand(&self) -> bool {
        self.on_demand.load(Ordering::Relaxed)
    }

    /// 查询指定附件在当前 backend 精确绑定文件资料库中的同步/冲突状态。
    pub async fn get_attachment_sync_issue(
        &self,
        attachment_id: &str,
    ) -> Result<Option<AttachmentSyncIssue>> {
        let Some(binding) = self.current_backend_binding().await? else {
            return Ok(None);
        };
        self.sync_issue_in_library(attachment_id, &binding.file_library_id)
    }

    /// 查询指定附件在显式指定文件资料库中的同步/冲突状态（不猜测“当前库”）。
    pub fn sync_issue_in_library(
        &self,
        attachment_id: &str,
        file_library_id: &str,
    ) -> Result<Option<AttachmentSyncIssue>> {
        let conflicts = self
            .db
            .get_attachment_file_conflicts(attachment_id, file_library_id)?;
        if conflicts.iter().any(|c| c.reason == "file_conflict") {
            return Ok(Some(AttachmentSyncIssue::FileConflict));
        }
        if conflicts.iter().any(|c| c.reason == "unknown_divergence") {
            return Ok(Some(AttachmentSyncIssue::UnknownDivergence));
        }

        if self
            .db
            .get_pending_download(attachment_id, file_library_id)?
            .is_some()
        {
            return Ok(Some(AttachmentSyncIssue::PendingDownload));
        }

        Ok(None)
    }

    async fn current_backend_binding(&self) -> Result<Option<FileLibraryBinding>> {
        let backend = self.backend().await;
        if !backend.is_enabled() {
            return Ok(None);
        }
        let backend_kind = backend.name().to_string();
        let backend_fingerprint = backend.configuration_fingerprint().await?;
        drop(backend);

        self.db
            .get_file_library_binding_by_backend(&backend_kind, &backend_fingerprint)
            .map_err(Into::into)
    }

    async fn ready_file_library(&self) -> Result<ReadyFileLibrary> {
        let (file_library_id, database_library_id) = match self.preflight().await {
            FileLibraryPreflight::Ready {
                file_library_id,
                database_library_id,
            } => (file_library_id, database_library_id),
            FileLibraryPreflight::IdentityMismatch { .. } => {
                return Err(prepare_error(
                    PrepareAttachmentErrorKind::IdentityMismatch,
                    "文件资料库身份不匹配",
                ));
            }
            FileLibraryPreflight::BackendDisabled => {
                return Err(prepare_error(
                    PrepareAttachmentErrorKind::BackendUnavailable,
                    "文件同步后端未启用",
                ));
            }
            FileLibraryPreflight::InitializationRequired
            | FileLibraryPreflight::WaitingForDatabaseIdentity
            | FileLibraryPreflight::UnidentifiedRemote => {
                return Err(prepare_error(
                    PrepareAttachmentErrorKind::InvalidState,
                    "文件资料库未就绪",
                ));
            }
            FileLibraryPreflight::Error(_) => {
                return Err(prepare_error(
                    PrepareAttachmentErrorKind::BackendUnavailable,
                    "文件资料库探测失败",
                ));
            }
        };

        let binding = self.current_backend_binding().await?.ok_or_else(|| {
            prepare_error(
                PrepareAttachmentErrorKind::InvalidState,
                "缺少当前文件库绑定",
            )
        })?;
        if binding.file_library_id != file_library_id
            || binding.database_library_id != database_library_id
        {
            return Err(prepare_error(
                PrepareAttachmentErrorKind::InvalidState,
                "当前文件库绑定与本轮探测不一致",
            ));
        }

        Ok(ReadyFileLibrary {
            file_library_id,
            database_library_id,
            backend_kind: binding.backend_kind,
            backend_fingerprint: binding.backend_fingerprint,
        })
    }

    async fn checked_backend(
        &self,
        ready: &ReadyFileLibrary,
    ) -> Result<tokio::sync::MutexGuard<'_, Box<dyn AttachmentBackend>>> {
        let backend = self.backend().await;
        if !backend.is_enabled() || backend.name() != ready.backend_kind {
            return Err(prepare_error(
                PrepareAttachmentErrorKind::BackendUnavailable,
                "文件同步后端已变化",
            ));
        }
        let fingerprint = backend.configuration_fingerprint().await?;
        if fingerprint != ready.backend_fingerprint {
            return Err(prepare_error(
                PrepareAttachmentErrorKind::BackendUnavailable,
                "文件同步后端配置已变化",
            ));
        }
        Ok(backend)
    }

    pub async fn backend(&self) -> tokio::sync::MutexGuard<'_, Box<dyn AttachmentBackend>> {
        self.backend.lock().await
    }

    /// 执行只读 Preflight 探测，确定当前文件资料库状态
    pub async fn preflight(&self) -> FileLibraryPreflight {
        let backend = self.backend().await;
        if !backend.is_enabled() {
            return FileLibraryPreflight::BackendDisabled;
        }

        let local_sync_state = match self.db.get_local_sync_state() {
            Ok(s) => s,
            Err(e) => {
                error!("存储管理: [Preflight] 读取数据库同步状态失败: {e}");
                return FileLibraryPreflight::Error(format!("读取数据库同步状态失败: {e}"));
            }
        };

        let local_db_lib_id = match local_sync_state.library_id {
            Some(id) => id,
            None => return FileLibraryPreflight::WaitingForDatabaseIdentity,
        };

        let inspection = match backend.inspect_library().await {
            Ok(insp) => insp,
            Err(e) => {
                error!("存储管理: [Preflight] 后端探测失败: {e}");
                return FileLibraryPreflight::Error(format!("后端探测失败: {e}"));
            }
        };

        match inspection {
            LibraryInspection::MissingEmpty => FileLibraryPreflight::InitializationRequired,
            LibraryInspection::MissingNonEmpty => FileLibraryPreflight::UnidentifiedRemote,
            LibraryInspection::Present(remote_identity) => {
                if remote_identity.database_library_id != local_db_lib_id {
                    FileLibraryPreflight::IdentityMismatch {
                        local_db_lib_id,
                        remote_db_lib_id: remote_identity.database_library_id,
                    }
                } else {
                    // 使用 backend 自己生成的配置指纹（含 endpoint/path/账户，不含密码）
                    let backend_fp = match backend.configuration_fingerprint().await {
                        Ok(fp) => fp,
                        Err(e) => {
                            error!("存储管理: [Preflight] 获取 backend 指纹失败: {e}");
                            return FileLibraryPreflight::Error(format!(
                                "获取 backend 指纹失败: {e}"
                            ));
                        }
                    };
                    let binding = FileLibraryBinding {
                        file_library_id: remote_identity.file_library_id.clone(),
                        database_library_id: remote_identity.database_library_id.clone(),
                        backend_kind: backend.name().to_string(),
                        backend_fingerprint: backend_fp,
                        protocol_version: remote_identity.protocol_version,
                        confirmed_at: remote_identity.created_at,
                    };
                    if let Err(e) = self.db.upsert_file_library_binding(&binding) {
                        error!("存储管理: [Preflight] 记录本地资料库绑定失败: {e}");
                        return FileLibraryPreflight::Error(format!("记录本地绑定失败: {e}"));
                    }

                    FileLibraryPreflight::Ready {
                        file_library_id: remote_identity.file_library_id,
                        database_library_id: remote_identity.database_library_id,
                    }
                }
            }
        }
    }

    /// 用户显式确认后，在远端条件初始化空文件资料库
    pub async fn confirm_file_library_initialization(&self) -> Result<FileRoundSummary> {
        let backend = self.backend().await;
        if !backend.is_enabled() {
            return Err(anyhow!("文件同步后端未启用"));
        }

        let local_sync_state = self.db.get_local_sync_state()?;
        let local_db_lib_id = local_sync_state
            .library_id
            .ok_or_else(|| anyhow!("缺少本地数据库资料库绑定，无法初始化文件库"))?;

        // 严格重新 inspect 确认远端依然为空
        let inspection = backend.inspect_library().await?;
        if inspection != LibraryInspection::MissingEmpty {
            return Err(anyhow!(
                "远端非空或已存在资料库，拒绝初始化: {inspection:?}"
            ));
        }

        let new_file_library_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        let identity = FileLibraryIdentity {
            protocol_version: 1,
            file_library_id: new_file_library_id.clone(),
            database_library_id: local_db_lib_id.clone(),
            created_at: now,
        };

        // 调用后端条件创建（Google Drive 在此返回 UnsupportedSafeInitialization Error）
        backend.initialize_library(identity).await?;

        let backend_fp = backend.configuration_fingerprint().await?;
        let binding = FileLibraryBinding {
            file_library_id: new_file_library_id.clone(),
            database_library_id: local_db_lib_id,
            backend_kind: backend.name().to_string(),
            backend_fingerprint: backend_fp,
            protocol_version: 1,
            confirmed_at: now,
        };
        self.db.upsert_file_library_binding(&binding)?;

        drop(backend);

        info!("存储管理: [Init] 成功初始化文件资料库");
        self.sync_file_library_round(FileMutationPolicy::allow_all())
            .await
    }

    /// 执行一轮完整的文件资料库同步。
    ///
    /// `policy` 由协调器依据数据库 disposition 推导（Full）或 `allow_all`（FileOnly）；
    /// 逐附件 `database_confirmed` 仍由 planner 内部计算，二者共同决定上传/删除门槛。
    pub async fn sync_file_library_round(
        &self,
        policy: FileMutationPolicy,
    ) -> Result<FileRoundSummary> {
        let preflight = self.preflight().await;
        let (file_library_id, _db_lib_id) = match &preflight {
            FileLibraryPreflight::Ready {
                file_library_id,
                database_library_id,
            } => (file_library_id.clone(), database_library_id.clone()),
            other => {
                info!("存储管理: [Round] 文件同步前置检查未就绪: {other:?}");
                return Ok(FileRoundSummary {
                    preflight: other.clone(),
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
                });
            }
        };

        // 1. 获取远端对象列表
        let backend = self.backend().await;
        let remote_entries = backend.list_objects().await?;
        drop(backend);

        let remote_objects: HashMap<String, String> = remote_entries
            .into_iter()
            .map(|e| (e.object_key, e.remote_version))
            .collect();

        // 2. 获取本地 baseline 与附件快照（snapshots 用于逐附件确认门槛，all_attachments 仅用于首次见库安全比较）
        let all_attachments = self.db.get_all_attachments_include_deleted()?;
        let snapshots = self.db.attachment_sync_snapshots()?;
        let mut baselines = HashMap::new();
        for att in &all_attachments {
            if let Some(b) = self
                .db
                .get_attachment_file_baseline(&att.id, &file_library_id)?
            {
                baselines.insert(att.id.clone(), b);
            }
        }
        // 执行阶段按 attachment_id 反查附件（首次见库安全比较需要本地正式文件路径）
        let attachments_by_id: HashMap<&str, &Attachment> = all_attachments
            .iter()
            .map(|att| (att.id.as_str(), att))
            .collect();

        // 3. 规划动作：policy 已由协调器推导，逐附件 database_confirmed 在 planner 内计算
        let plans = plan_file_actions(&snapshots, &baselines, &remote_objects, policy);

        let mut uploaded = 0;
        let mut downloaded = 0;
        let mut deleted = 0;
        let mut skipped = 0;
        let mut waiting = 0;
        let mut failed = 0;
        let mut conflicts = 0;
        let mut unknown_divergence = 0;
        let mut pending_download = 0;
        let mut unrecoverable_missing = 0;

        let attachments_dir = self.file_manager.get_attachments_dir();

        // 4. 执行动作
        for plan in plans {
            match plan {
                FileActionPlan::Upload {
                    attachment_id,
                    object_key,
                    local_path,
                } => {
                    let local_sha256 = match compute_file_hash(&local_path) {
                        Ok(h) => h,
                        Err(e) => {
                            error!("存储管理: [UploadObject] 读取本地哈希失败: {e}");
                            failed += 1;
                            continue;
                        }
                    };
                    let backend = self.backend().await;
                    match backend
                        .upload_object_if_absent(object_key.clone(), local_path)
                        .await
                    {
                        Ok(UploadObjectResult::Created(remote_version)) => {
                            match self.db.apply_successful_upload(
                                &attachment_id,
                                &file_library_id,
                                &object_key,
                                &remote_version,
                                &local_sha256,
                            ) {
                                Ok(()) => {
                                    uploaded += 1;
                                }
                                Err(e) => {
                                    error!("存储管理: [UploadObject] 写入 baseline 失败: {e}");
                                    failed += 1;
                                }
                            }
                        }
                        Ok(UploadObjectResult::AlreadyExists) => {
                            info!("存储管理: [UploadObject] 远端已存在同名对象，保护本地");
                            skipped += 1;
                        }
                        Err(e) => {
                            error!("存储管理: [UploadObject] 上传失败: {e}");
                            failed += 1;
                        }
                    }
                }
                FileActionPlan::Download {
                    attachment_id,
                    object_key,
                    target_file_name,
                    adopt_baseline,
                } => {
                    // 获取远端版本字符串用于后续记录
                    let remote_version =
                        remote_objects.get(&object_key).cloned().unwrap_or_default();
                    // 按需下载模式：仅登记而不实际写入文件
                    if self.is_on_demand() && !adopt_baseline {
                        // 记录 pending download
                        let pending = AttachmentPendingDownload {
                            attachment_id: attachment_id.clone(),
                            file_library_id: file_library_id.clone(),
                            object_key: object_key.clone(),
                            remote_version: remote_version.clone(),
                            created_at: chrono::Utc::now().timestamp(),
                        };
                        if let Err(e) = self.db.upsert_pending_download(&pending) {
                            error!("存储管理: [PendingDownload] 写入数据库失败: {e}");
                            failed += 1;
                        } else {
                            pending_download += 1;
                        }
                        continue;
                    }

                    let safe_name = match sanitize_file_name(&target_file_name) {
                        Ok(name) => name,
                        Err(e) => {
                            error!("存储管理: [DownloadObject] 非法文件名，拒绝下载: {e}");
                            failed += 1;
                            continue;
                        }
                    };
                    let target_path = attachments_dir.join(&safe_name);
                    let temp_path =
                        attachments_dir.join(format!(".tmp.{safe_name}.{}", uuid::Uuid::new_v4()));

                    let backend = self.backend().await;
                    match backend
                        .download_object(object_key.clone(), temp_path.clone())
                        .await
                    {
                        Ok(Some(remote_version)) => {
                            if adopt_baseline {
                                // 首次见库安全比较闭环：
                                // 绝不能以 adopt_baseline=true 为由直接替换已有正式文件
                                let outcome = match attachments_by_id.get(attachment_id.as_str()) {
                                    Some(att) => {
                                        self.adopt_baseline_by_comparison(
                                            att,
                                            &file_library_id,
                                            &object_key,
                                            &remote_version,
                                            &temp_path,
                                        )
                                        .await
                                    }
                                    None => {
                                        let _ = tokio::fs::remove_file(&temp_path).await;
                                        FirstSeenComparisonOutcome::Failed
                                    }
                                };
                                match outcome {
                                    FirstSeenComparisonOutcome::BaselineAdopted => {
                                        // 两端一致仅建立 baseline，未发生下载替换，计入 skipped
                                        skipped += 1;
                                    }
                                    FirstSeenComparisonOutcome::DivergenceRecorded => {
                                        unknown_divergence += 1;
                                    }
                                    FirstSeenComparisonOutcome::Failed => {
                                        failed += 1;
                                    }
                                }
                            } else {
                                let temp_p_clone = temp_path.clone();
                                let hash_res = tokio::task::spawn_blocking(move || {
                                    compute_file_hash(&temp_p_clone)
                                })
                                .await;

                                match hash_res {
                                    Ok(Ok(local_sha256)) => {
                                        match atomic_replace_file(&temp_path, &target_path).await {
                                            Ok(()) => {
                                                let target_str =
                                                    target_path.to_string_lossy().to_string();
                                                match self.db.apply_prepared_attachment_success(
                                                    &attachment_id,
                                                    &file_library_id,
                                                    &object_key,
                                                    &remote_version,
                                                    &local_sha256,
                                                    &target_str,
                                                ) {
                                                    Ok(()) => {
                                                        downloaded += 1;
                                                    }
                                                    Err(e) => {
                                                        error!(
                                                            "存储管理: [DownloadObject] 写入 baseline/attachment 失败: {e}"
                                                        );
                                                        failed += 1;
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                error!(
                                                    "存储管理: [DownloadObject] 原子替换文件失败: {e}"
                                                );
                                                let _ = tokio::fs::remove_file(&temp_path).await;
                                                failed += 1;
                                            }
                                        }
                                    }
                                    _ => {
                                        error!("存储管理: [DownloadObject] 计算下载哈希失败");
                                        let _ = tokio::fs::remove_file(&temp_path).await;
                                        failed += 1;
                                    }
                                }
                            }
                        }
                        Ok(None) => {
                            let _ = tokio::fs::remove_file(&temp_path).await;
                            skipped += 1;
                        }
                        Err(e) => {
                            error!("存储管理: [DownloadObject] 下载失败: {e}");
                            let _ = tokio::fs::remove_file(&temp_path).await;
                            failed += 1;
                        }
                    }
                }
                FileActionPlan::FileConflict {
                    attachment_id,
                    object_key,
                    remote_version,
                    local_sha256,
                } => {
                    // 记录冲突
                    let conflict = AttachmentFileConflict {
                        attachment_id: attachment_id.clone(),
                        file_library_id: file_library_id.clone(),
                        object_key: object_key.clone(),
                        remote_version: remote_version.clone(),
                        local_sha256: local_sha256.clone(),
                        reason: "file_conflict".to_string(),
                        created_at: chrono::Utc::now().timestamp(),
                    };
                    if let Err(e) = self.db.upsert_file_conflict(&conflict) {
                        error!("存储管理: [FileConflict] 写入数据库失败: {e}");
                        failed += 1;
                    } else {
                        conflicts += 1;
                    }
                }
                FileActionPlan::UnknownVersionDivergence {
                    attachment_id,
                    object_key,
                    remote_version,
                    local_sha256,
                } => {
                    // 记录未知版本分歧
                    let conflict = AttachmentFileConflict {
                        attachment_id: attachment_id.clone(),
                        file_library_id: file_library_id.clone(),
                        object_key: object_key.clone(),
                        remote_version: remote_version.clone(),
                        local_sha256: local_sha256.clone(),
                        reason: "unknown_divergence".to_string(),
                        created_at: chrono::Utc::now().timestamp(),
                    };
                    if let Err(e) = self.db.upsert_file_conflict(&conflict) {
                        error!("存储管理: [UnknownVersionDivergence] 写入数据库失败: {e}");
                        failed += 1;
                    } else {
                        unknown_divergence += 1;
                    }
                }
                FileActionPlan::Delete {
                    attachment_id,
                    object_key,
                } => {
                    let backend = self.backend().await;
                    match backend.delete_object(object_key).await {
                        Ok(()) => {
                            match self
                                .db
                                .apply_successful_delete(&attachment_id, &file_library_id)
                            {
                                Ok(()) => {
                                    deleted += 1;
                                }
                                Err(e) => {
                                    error!("存储管理: [DeleteObject] 删除 baseline 失败: {e}");
                                    failed += 1;
                                }
                            }
                        }
                        Err(e) => {
                            error!("存储管理: [DeleteObject] 删除失败: {e}");
                            failed += 1;
                        }
                    }
                }
                FileActionPlan::ClearBaseline { attachment_id } => {
                    match self
                        .db
                        .apply_successful_delete(&attachment_id, &file_library_id)
                    {
                        Ok(()) => {
                            deleted += 1;
                        }
                        Err(e) => {
                            error!("存储管理: [ClearBaseline] 清除 baseline 失败: {e}");
                            failed += 1;
                        }
                    }
                }
                FileActionPlan::UnrecoverableMissing { .. } => {
                    unrecoverable_missing += 1;
                }
                FileActionPlan::PendingDownload { .. } => {
                    pending_download += 1;
                }
                FileActionPlan::WaitingForDatabaseConfirmation { .. } => {
                    waiting += 1;
                }
                FileActionPlan::Skip { .. } => {
                    skipped += 1;
                }
            }
        }

        Ok(FileRoundSummary {
            preflight,
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
        })
    }

    /// 首次见库安全比较：本地正式文件与远端对象同时存在、但当前 file_library_id 无 baseline 时，
    /// 将远端对象下载到临时文件后比较完整 SHA-256：
    /// - 一致：删除临时文件，仅用 `upsert_attachment_file_baseline` 建立 baseline（含非空
    ///   remote_version、完整 local_sha256、local_presence=true），绝不替换本地正式文件；
    ///   baseline 确实建立成功后才尽力清理该附件/该库的过期 unknown_divergence 与按需下载登记；
    /// - 不同：删除临时文件，用 `upsert_file_conflict` 记录 reason="unknown_divergence"，
    ///   正式文件字节完全不变，不创建或更新 baseline；
    /// - 任一步骤失败（下载、哈希、文件状态、数据库写入）：尽力清理临时文件，
    ///   正式文件不变、baseline 不推进。
    async fn adopt_baseline_by_comparison(
        &self,
        attachment: &Attachment,
        file_library_id: &str,
        object_key: &str,
        remote_version: &str,
        temp_path: &Path,
    ) -> FirstSeenComparisonOutcome {
        // 后端必须返回非空 remote_version；空值视为不确定，零覆盖、零 baseline
        if remote_version.is_empty() {
            error!("存储管理: [FirstSeen] 远端版本为空，视为不确定状态");
            let _ = tokio::fs::remove_file(temp_path).await;
            return FirstSeenComparisonOutcome::Failed;
        }

        // 下载完成的临时文件完整哈希
        let temp_owned = temp_path.to_path_buf();
        let temp_hash =
            match tokio::task::spawn_blocking(move || compute_file_hash(&temp_owned)).await {
                Ok(Ok(hash)) => hash,
                _ => {
                    error!("存储管理: [FirstSeen] 计算临时文件哈希失败");
                    let _ = tokio::fs::remove_file(temp_path).await;
                    return FirstSeenComparisonOutcome::Failed;
                }
            };

        // 比较前再次确认本地正式文件仍是普通文件
        if classify_local_file(&attachment.file_path) != LocalFileState::RegularFile {
            error!("存储管理: [FirstSeen] 本地正式文件不再是普通文件，拒绝比较");
            let _ = tokio::fs::remove_file(temp_path).await;
            return FirstSeenComparisonOutcome::Failed;
        }

        // 本地正式文件完整哈希
        let local_owned = PathBuf::from(&attachment.file_path);
        let local_hash =
            match tokio::task::spawn_blocking(move || compute_file_hash(&local_owned)).await {
                Ok(Ok(hash)) => hash,
                _ => {
                    error!("存储管理: [FirstSeen] 计算本地正式文件哈希失败");
                    let _ = tokio::fs::remove_file(temp_path).await;
                    return FirstSeenComparisonOutcome::Failed;
                }
            };

        if temp_hash == local_hash {
            // 两端内容一致：建立 baseline 前复核本地文件状态与内容未发生并发修改
            if classify_local_file(&attachment.file_path) != LocalFileState::RegularFile {
                error!("存储管理: [FirstSeen] 本地正式文件状态在哈希后发生变化，拒绝建立 baseline");
                let _ = tokio::fs::remove_file(temp_path).await;
                return FirstSeenComparisonOutcome::Failed;
            }
            let verify_local_owned = PathBuf::from(&attachment.file_path);
            let verify_hash =
                match tokio::task::spawn_blocking(move || compute_file_hash(&verify_local_owned))
                    .await
                {
                    Ok(Ok(h)) => h,
                    _ => {
                        error!(
                            "存储管理: [FirstSeen] 本地正式文件在哈希后复核失败，拒绝建立 baseline"
                        );
                        let _ = tokio::fs::remove_file(temp_path).await;
                        return FirstSeenComparisonOutcome::Failed;
                    }
                };
            if verify_hash != local_hash {
                error!("存储管理: [FirstSeen] 本地正式文件内容在哈希后被修改，拒绝建立 baseline");
                let _ = tokio::fs::remove_file(temp_path).await;
                return FirstSeenComparisonOutcome::Failed;
            }

            // 两端内容一致且复核通过：删除临时文件，仅建立 baseline，不替换正式文件
            let _ = tokio::fs::remove_file(temp_path).await;
            let baseline = AttachmentFileBaseline {
                attachment_id: attachment.id.clone(),
                file_library_id: file_library_id.to_string(),
                object_key: object_key.to_string(),
                remote_version: remote_version.to_string(),
                local_sha256: local_hash,
                local_presence: true,
                last_success_at: chrono::Utc::now().timestamp(),
            };
            if let Err(e) = self.db.upsert_attachment_file_baseline(&baseline) {
                error!("存储管理: [FirstSeen] 写入 baseline 失败: {e}");
                return FirstSeenComparisonOutcome::Failed;
            }
            // 既有接口语义明确且 baseline 已确实建立成功，才清理过期登记；
            // 清理失败仅记录，不影响本轮已确立的 baseline
            if let Err(e) =
                self.db
                    .delete_file_conflict(&attachment.id, file_library_id, "unknown_divergence")
            {
                error!("存储管理: [FirstSeen] 清理过期分歧记录失败: {e}");
            }
            if let Err(e) = self
                .db
                .delete_pending_download(&attachment.id, file_library_id)
            {
                error!("存储管理: [FirstSeen] 清理过期按需下载登记失败: {e}");
            }
            FirstSeenComparisonOutcome::BaselineAdopted
        } else {
            // 两端内容不同：删除临时文件，记录未知分歧，正式文件字节保持不变
            let _ = tokio::fs::remove_file(temp_path).await;
            let conflict = AttachmentFileConflict {
                attachment_id: attachment.id.clone(),
                file_library_id: file_library_id.to_string(),
                object_key: object_key.to_string(),
                remote_version: remote_version.to_string(),
                local_sha256: local_hash,
                reason: "unknown_divergence".to_string(),
                created_at: chrono::Utc::now().timestamp(),
            };
            if let Err(e) = self.db.upsert_file_conflict(&conflict) {
                error!("存储管理: [FirstSeen] 写入未知分歧记录失败: {e}");
                return FirstSeenComparisonOutcome::Failed;
            }
            FirstSeenComparisonOutcome::DivergenceRecorded
        }
    }

    /// 统一按需准备附件（打开前即时恢复单一入口）
    ///
    /// 遵循严格恢复顺序：
    /// 1. 读取 Attachment；tombstone 立即返回明确错误；
    /// 2. 本地若是有效普通文件直接快速返回，不访问远端；
    /// 3. preflight 检查确认 Ready；
    /// 4. 远端 list_objects 严格校验，失败立即停止；
    /// 5. 校验 object_key 唯一对象及 remote_version 非空；
    /// 6. 核对当前库 pending download；
    /// 7. 在下载前尝试本地标准目录路径修复（FILE_SYNC.md §14.4）；
    /// 8. 下载至附件目录唯一受控 temp 文件，严格校验返回版本；
    /// 9. 完整计算 temp SHA-256 哈希；
    /// 10. 原子替换前再次复核目标路径状态：若此时出现普通文件绝不覆盖，进行内容比对或记录未知分歧；
    /// 11. 原子落地文件后更新 SQLite baseline 及 Attachment，成功后删除当前库 pending。
    pub async fn prepare_attachment_for_open(
        &self,
        attachment_id: &str,
    ) -> Result<PreparedAttachment> {
        let att = self.db.get_attachment(attachment_id)?.ok_or_else(|| {
            prepare_error(PrepareAttachmentErrorKind::InvalidState, "附件记录不存在")
        })?;

        if att.is_deleted {
            return Err(prepare_error(
                PrepareAttachmentErrorKind::InvalidState,
                "附件已标记删除",
            ));
        }

        if classify_local_file(&att.file_path) == LocalFileState::RegularFile {
            let issue = match self.current_backend_binding().await? {
                Some(binding) => {
                    self.sync_issue_in_library(attachment_id, &binding.file_library_id)?
                }
                None => None,
            };
            return Ok(PreparedAttachment {
                attachment_id: attachment_id.to_string(),
                local_path: PathBuf::from(att.file_path),
                issue,
            });
        }

        let ready = self.ready_file_library().await?;
        if ready.database_library_id.is_empty() {
            return Err(prepare_error(
                PrepareAttachmentErrorKind::InvalidState,
                "文件资料库身份为空",
            ));
        }
        let file_library_id = ready.file_library_id.clone();
        let object_key = database::sqlite::object_key_from_attachment_id(attachment_id)
            .map_err(|_| prepare_error(PrepareAttachmentErrorKind::InvalidState, "无效对象键"))?;

        let backend = self.checked_backend(&ready).await?;
        let remote_entries = backend.list_objects().await.map_err(|_| {
            prepare_error(
                PrepareAttachmentErrorKind::BackendUnavailable,
                "远端清单读取失败",
            )
        })?;
        drop(backend);

        let mut matched_entries = remote_entries
            .into_iter()
            .filter(|entry| entry.object_key == object_key);
        let remote_entry = matched_entries.next().ok_or_else(|| {
            prepare_error(
                PrepareAttachmentErrorKind::UnrecoverableMissing,
                "远端对象不存在，文件无法恢复",
            )
        })?;
        if matched_entries.next().is_some() {
            return Err(prepare_error(
                PrepareAttachmentErrorKind::InvalidState,
                "远端清单包含重复对象",
            ));
        }

        if remote_entry.remote_version.is_empty() {
            return Err(prepare_error(
                PrepareAttachmentErrorKind::InvalidState,
                "远端对象版本为空，无法恢复",
            ));
        }

        if let Some(pending) = self
            .db
            .get_pending_download(attachment_id, &file_library_id)?
        {
            if pending.object_key != object_key {
                return Err(prepare_error(
                    PrepareAttachmentErrorKind::InvalidState,
                    "按需下载记录对象键不一致，本地状态损坏",
                ));
            }
            if pending.remote_version != remote_entry.remote_version {
                self.db
                    .upsert_pending_download(&AttachmentPendingDownload {
                        attachment_id: attachment_id.to_string(),
                        file_library_id: file_library_id.clone(),
                        object_key: object_key.clone(),
                        remote_version: remote_entry.remote_version.clone(),
                        created_at: chrono::Utc::now().timestamp(),
                    })?;
            }
        }

        let safe_name = sanitize_file_name(&att.file_name).map_err(|_| {
            prepare_error(PrepareAttachmentErrorKind::InvalidState, "附件文件名非法")
        })?;
        let attachments_dir = self.file_manager.get_attachments_dir();
        let target_path = attachments_dir.join(&safe_name);
        let baseline_opt = self
            .db
            .get_attachment_file_baseline(attachment_id, &file_library_id)?;
        let mut expected_replace_hash: Option<String> = None;

        if classify_local_file(&target_path) == LocalFileState::RegularFile {
            let candidate_owned = target_path.clone();
            let candidate_hash =
                tokio::task::spawn_blocking(move || compute_file_hash(&candidate_owned))
                    .await
                    .map_err(|_| {
                        prepare_error(
                            PrepareAttachmentErrorKind::LocalWrite,
                            "本地文件哈希任务失败",
                        )
                    })?
                    .map_err(|_| {
                        prepare_error(PrepareAttachmentErrorKind::LocalWrite, "本地文件哈希失败")
                    })?;

            if let Some(baseline) = baseline_opt.as_ref() {
                if baseline.local_sha256 != candidate_hash {
                    let reason = if baseline.remote_version == remote_entry.remote_version {
                        "unknown_divergence"
                    } else {
                        "file_conflict"
                    };
                    self.db.upsert_file_conflict(&AttachmentFileConflict {
                        attachment_id: attachment_id.to_string(),
                        file_library_id: file_library_id.clone(),
                        object_key: object_key.clone(),
                        remote_version: remote_entry.remote_version.clone(),
                        local_sha256: candidate_hash,
                        reason: reason.to_string(),
                        created_at: chrono::Utc::now().timestamp(),
                    })?;
                    return Err(prepare_error(
                        PrepareAttachmentErrorKind::Conflict,
                        "本地文件与当前基线不一致，已阻止自动恢复",
                    ));
                }

                if baseline.remote_version == remote_entry.remote_version {
                    let target_str = target_path.to_string_lossy().to_string();
                    self.db.apply_prepared_attachment_success(
                        attachment_id,
                        &file_library_id,
                        &object_key,
                        &remote_entry.remote_version,
                        &candidate_hash,
                        &target_str,
                    )?;
                    (self.notify_ui)();
                    return Ok(PreparedAttachment {
                        attachment_id: attachment_id.to_string(),
                        local_path: target_path,
                        issue: None,
                    });
                }

                expected_replace_hash = Some(candidate_hash);
            }
        }

        let temp_path = attachments_dir.join(format!(".tmp.{safe_name}.{}", uuid::Uuid::new_v4()));
        let backend = self.checked_backend(&ready).await?;
        let downloaded_version = match backend
            .download_object(object_key.clone(), temp_path.clone())
            .await
        {
            Ok(Some(v)) => v,
            Ok(None) => {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(prepare_error(
                    PrepareAttachmentErrorKind::UnrecoverableMissing,
                    "远端对象不存在，文件无法恢复",
                ));
            }
            Err(_) => {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(prepare_error(
                    PrepareAttachmentErrorKind::BackendUnavailable,
                    "下载远端对象失败",
                ));
            }
        };
        drop(backend);

        if downloaded_version.is_empty() || downloaded_version != remote_entry.remote_version {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(prepare_error(
                PrepareAttachmentErrorKind::InvalidState,
                "下载版本与远端清单不一致或版本为空，拒绝采用",
            ));
        }

        let temp_p_clone = temp_path.clone();
        let temp_sha256 = tokio::task::spawn_blocking(move || compute_file_hash(&temp_p_clone))
            .await
            .map_err(|_| {
                prepare_error(
                    PrepareAttachmentErrorKind::LocalWrite,
                    "临时文件哈希任务失败",
                )
            })?
            .map_err(|_| {
                prepare_error(PrepareAttachmentErrorKind::LocalWrite, "临时文件哈希失败")
            })?;

        match classify_local_file(&target_path) {
            LocalFileState::Missing => {
                atomic_replace_file(&temp_path, &target_path)
                    .await
                    .map_err(|_| {
                        prepare_error(
                            PrepareAttachmentErrorKind::LocalWrite,
                            "原子替换目标文件失败",
                        )
                    })?;
            }
            LocalFileState::RegularFile => {
                let target_owned = target_path.clone();
                let existing_hash =
                    tokio::task::spawn_blocking(move || compute_file_hash(&target_owned))
                        .await
                        .map_err(|_| {
                            prepare_error(
                                PrepareAttachmentErrorKind::LocalWrite,
                                "目标文件哈希任务失败",
                            )
                        })?
                        .map_err(|_| {
                            prepare_error(
                                PrepareAttachmentErrorKind::LocalWrite,
                                "目标文件哈希失败",
                            )
                        })?;

                if expected_replace_hash.as_ref() == Some(&existing_hash) {
                    atomic_replace_file(&temp_path, &target_path)
                        .await
                        .map_err(|_| {
                            prepare_error(
                                PrepareAttachmentErrorKind::LocalWrite,
                                "原子替换目标文件失败",
                            )
                        })?;
                } else if existing_hash == temp_sha256 {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    let target_str = target_path.to_string_lossy().to_string();
                    self.db.apply_prepared_attachment_success(
                        attachment_id,
                        &file_library_id,
                        &object_key,
                        &downloaded_version,
                        &existing_hash,
                        &target_str,
                    )?;
                    (self.notify_ui)();
                    return Ok(PreparedAttachment {
                        attachment_id: attachment_id.to_string(),
                        local_path: target_path,
                        issue: None,
                    });
                } else {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    let reason = if expected_replace_hash.is_some() {
                        "file_conflict"
                    } else {
                        "unknown_divergence"
                    };
                    self.db.upsert_file_conflict(&AttachmentFileConflict {
                        attachment_id: attachment_id.to_string(),
                        file_library_id: file_library_id.clone(),
                        object_key: object_key.clone(),
                        remote_version: downloaded_version.clone(),
                        local_sha256: existing_hash,
                        reason: reason.to_string(),
                        created_at: chrono::Utc::now().timestamp(),
                    })?;
                    return Err(prepare_error(
                        PrepareAttachmentErrorKind::Conflict,
                        "本地文件在下载期间出现且内容不一致，已阻止覆盖",
                    ));
                }
            }
            LocalFileState::NotRegularFile | LocalFileState::Unknown => {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(prepare_error(
                    PrepareAttachmentErrorKind::LocalWrite,
                    "目标路径状态异常，拒绝覆盖",
                ));
            }
        }

        let target_str = target_path.to_string_lossy().to_string();
        self.db.apply_prepared_attachment_success(
            attachment_id,
            &file_library_id,
            &object_key,
            &downloaded_version,
            &temp_sha256,
            &target_str,
        )?;

        (self.notify_ui)();
        Ok(PreparedAttachment {
            attachment_id: attachment_id.to_string(),
            local_path: target_path,
            issue: None,
        })
    }

    /// 安全即时下载单个附件（内部委托给 prepare_attachment_for_open）
    pub async fn download_single_attachment(&self, attachment_id: &str) -> Result<bool> {
        self.prepare_attachment_for_open(attachment_id)
            .await
            .map(|_| true)
    }

    /// 触发执行完整附件同步轮次（仅供 engine 调用）
    pub async fn test_backend_config(&self, name: &str, config_json: &str) -> anyhow::Result<()> {
        info!("存储管理: [File] 正在测试后端配置 ({name})");
        let backend = file::create_backend(name, config_json);
        if !backend.is_enabled() {
            anyhow::bail!("后端未启用，请先填写配置");
        }
        let handle = RUNTIME.spawn(async move { backend.test_connection().await });
        handle.await.map_err(|e| anyhow!("任务失败: {e}"))?
    }

    /// 清空远端文件（仅限开发工具，严格顺序传播错误）
    ///
    /// 固定顺序：list_objects 成功 → 每项 delete_object 成功 → preflight Ready
    /// → delete_file_library_state 成功 → Ok。任一步骤失败立即返回 Error。
    pub async fn clear_remote_files(&self) -> anyhow::Result<()> {
        info!("存储管理: 开始清空远端文件 (开发工具)...");

        let backend = self.backend().await;
        if !backend.is_enabled() {
            return Err(anyhow!("后端未启用，无法清空远端文件"));
        }

        // 1. 列出所有远端对象，失败立即返回 Error（绝不当空列表处理）
        let entries = backend
            .list_objects()
            .await
            .map_err(|e| anyhow!("列出远端对象失败，清空已中止: {e}"))?;

        let total = entries.len();
        info!("存储管理: 发现 {total} 个远端对象待删除");

        // 2. 逐项删除，失败立即返回 Error
        for (i, entry) in entries.into_iter().enumerate() {
            debug!("存储管理: 正在删除远端对象 [{}/{}]", i + 1, total);
            backend
                .delete_object(entry.object_key)
                .await
                .map_err(|e| anyhow!("删除远端对象失败，清空已中止: {e}"))?;
        }
        drop(backend);

        // 3. Preflight 确认 Ready 后清除本地 binding/baseline
        let preflight = self.preflight().await;
        match preflight {
            FileLibraryPreflight::Ready {
                file_library_id, ..
            } => {
                self.db
                    .delete_file_library_state(&file_library_id)
                    .map_err(|e| anyhow!("删除本地资料库状态失败，清空已中止: {e}"))?;
                info!("存储管理: 远端文件清空完成，共删除 {total} 个对象");
                Ok(())
            }
            other => Err(anyhow!("清空后 Preflight 未能确认 Ready 状态: {other:?}")),
        }
    }
}

/// 本地路径参与文件同步前的状态判定（私有辅助，不新增公共 API）
///
/// 只有 [`LocalFileState::RegularFile`] 才允许进入 Upload 或本地哈希比较；
/// 目录、异常符号链接、特殊文件、metadata 失败与哈希失败都不是“本地缺失”，
/// 必须按不确定状态保守处理，禁止触发上传、下载覆盖或 baseline 推进。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalFileState {
    /// 路径为空或明确不存在
    Missing,
    /// 存在且是普通文件
    RegularFile,
    /// 存在但不是普通文件（目录、异常符号链接、特殊文件等）
    NotRegularFile,
    /// metadata 读取失败，状态不确定
    Unknown,
}

/// 判定本地路径的文件状态。
///
/// 先用 `symlink_metadata` 确认目录项是否被占用，避免把异常符号链接
/// （目录项存在但目标不可达）误判为普通“本地缺失”。
fn classify_local_file(path_ref: impl AsRef<Path>) -> LocalFileState {
    let path = path_ref.as_ref();
    if path.as_os_str().is_empty() {
        return LocalFileState::Missing;
    }
    match std::fs::symlink_metadata(path) {
        Ok(_) => match std::fs::metadata(path) {
            Ok(meta) if meta.is_file() => LocalFileState::RegularFile,
            // 目标不是普通文件，或符号链接目标不可达
            Ok(_) | Err(_) => LocalFileState::NotRegularFile,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LocalFileState::Missing,
        Err(_) => LocalFileState::Unknown,
    }
}

/// 逐附件数据库远端确认：本地非脏、已持久化确认版本且确认版本不低于本地版本。
///
/// database 不裁决确认；services 在此计算。确认是上传与 tombstone 删除的前置条件。
fn snapshot_database_confirmed(snap: &AttachmentSyncSnapshot) -> bool {
    !snap.is_dirty && snap.synced_version > 0 && snap.synced_version >= snap.version
}

/// 纯函数：根据本地附件确认快照、baseline 映射、远端对象映射与本轮变更策略，规划每个附件的动作。
///
/// 上传与 tombstone 删除必须逐附件核验 `database_confirmed`；
/// 数据库 PartialFailure/Error/身份阻断由 `policy` 的 `allow_confirmed_delete` 统一禁止删除。
pub fn plan_file_actions(
    snapshots: &[AttachmentSyncSnapshot],
    baselines: &HashMap<String, AttachmentFileBaseline>,
    remote_objects: &HashMap<String, String>,
    policy: FileMutationPolicy,
) -> Vec<FileActionPlan> {
    let mut plans = Vec::with_capacity(snapshots.len());

    for snap in snapshots {
        let object_key = match database::sqlite::object_key_from_attachment_id(&snap.id) {
            Ok(k) => k,
            Err(_) => {
                plans.push(FileActionPlan::UnrecoverableMissing {
                    attachment_id: snap.id.clone(),
                });
                continue;
            }
        };

        let remote_version = remote_objects.get(&object_key);
        let baseline = baselines.get(&snap.id);
        let local_state = classify_local_file(&snap.file_path);
        let database_confirmed = snapshot_database_confirmed(snap);

        if snap.is_deleted {
            // Tombstone 处理：删除/清理前必须先确认远端已确认该 tombstone
            if remote_version.is_some() {
                if database_confirmed && policy.allow_confirmed_delete {
                    plans.push(FileActionPlan::Delete {
                        attachment_id: snap.id.clone(),
                        object_key,
                    });
                } else if database_confirmed && !policy.allow_confirmed_delete {
                    // 已确认 tombstone，但本轮（PartialFailure/Error/身份阻断）禁止远端删除：
                    // 安全跳过，保留 baseline/pending/conflict，待下一轮允许删除时再清。
                    plans.push(FileActionPlan::Skip {
                        attachment_id: snap.id.clone(),
                    });
                } else {
                    // 未确认 tombstone：不得 Delete/ClearBaseline，等待数据库确认
                    plans.push(FileActionPlan::WaitingForDatabaseConfirmation {
                        attachment_id: snap.id.clone(),
                    });
                }
            } else {
                // 远端已无此对象
                if database_confirmed {
                    // 仅清当前库残留 baseline（本地清理，非远端删除）
                    plans.push(FileActionPlan::ClearBaseline {
                        attachment_id: snap.id.clone(),
                    });
                } else {
                    // 未确认 tombstone：不清理，等待数据库确认
                    plans.push(FileActionPlan::WaitingForDatabaseConfirmation {
                        attachment_id: snap.id.clone(),
                    });
                }
            }
            continue;
        }

        // is_dirty=true：等待数据库同步确认
        if snap.is_dirty {
            plans.push(FileActionPlan::WaitingForDatabaseConfirmation {
                attachment_id: snap.id.clone(),
            });
            continue;
        }

        match local_state {
            LocalFileState::Missing => match remote_version {
                Some(_) => {
                    // 本地缺失、远端存在 → Download (adopt_baseline false)
                    plans.push(FileActionPlan::Download {
                        attachment_id: snap.id.clone(),
                        object_key,
                        target_file_name: snap.file_name.clone(),
                        adopt_baseline: false,
                    });
                }
                None => {
                    // 双缺失 → UnrecoverableMissing
                    plans.push(FileActionPlan::UnrecoverableMissing {
                        attachment_id: snap.id.clone(),
                    });
                }
            },
            LocalFileState::RegularFile => match remote_version {
                None => {
                    // 本地存在、远端不存在 → 仅当逐附件已确认且策略允许才 Upload；
                    // 否则等待数据库确认（不静默 Skip，避免丢失等待语义）。
                    if policy.allow_confirmed_upload && database_confirmed {
                        plans.push(FileActionPlan::Upload {
                            attachment_id: snap.id.clone(),
                            object_key,
                            local_path: PathBuf::from(&snap.file_path),
                        });
                    } else {
                        plans.push(FileActionPlan::WaitingForDatabaseConfirmation {
                            attachment_id: snap.id.clone(),
                        });
                    }
                }
                Some(remote_ver) => {
                    // 本地存在、远端存在
                    if let Some(b) = baseline {
                        if b.remote_version == *remote_ver {
                            // Baseline 版本与远端一致 → Skip
                            plans.push(FileActionPlan::Skip {
                                attachment_id: snap.id.clone(),
                            });
                        } else {
                            // Baseline 版本不同于远端 → 比较本地哈希
                            match compute_file_hash(std::path::Path::new(&snap.file_path)) {
                                Ok(local_sha256) => {
                                    if local_sha256 == b.local_sha256 {
                                        // 本地未改动，安全下载
                                        plans.push(FileActionPlan::Download {
                                            attachment_id: snap.id.clone(),
                                            object_key,
                                            target_file_name: snap.file_name.clone(),
                                            adopt_baseline: false,
                                        });
                                    } else {
                                        // 本地已修改，记录冲突
                                        plans.push(FileActionPlan::FileConflict {
                                            attachment_id: snap.id.clone(),
                                            object_key,
                                            remote_version: remote_ver.clone(),
                                            local_sha256,
                                        });
                                    }
                                }
                                Err(_) => {
                                    // 哈希计算失败，记录未知分歧
                                    plans.push(FileActionPlan::UnknownVersionDivergence {
                                        attachment_id: snap.id.clone(),
                                        object_key,
                                        remote_version: remote_ver.clone(),
                                        local_sha256: String::new(),
                                    });
                                }
                            }
                        }
                    } else {
                        // 首次见库：本地普通文件与远端对象同时存在但无 baseline，
                        // 下载到临时文件做安全比较，绝不直接替换本地正式文件
                        plans.push(FileActionPlan::Download {
                            attachment_id: snap.id.clone(),
                            object_key,
                            target_file_name: snap.file_name.clone(),
                            adopt_baseline: true,
                        });
                    }
                }
            },
            LocalFileState::NotRegularFile | LocalFileState::Unknown => {
                // 本地状态不确定（目录、特殊文件、metadata 失败）：
                // 保守记录未知分歧，禁止 Upload / 下载覆盖 / baseline 推进
                plans.push(FileActionPlan::UnknownVersionDivergence {
                    attachment_id: snap.id.clone(),
                    object_key,
                    remote_version: remote_version.cloned().unwrap_or_default(),
                    local_sha256: String::new(),
                });
            }
        }
    }

    plans
}

/// 跨平台原子替换文件：temp → target。
///
/// - 强制同目录前置校验：temp 与 target 的 parent 必须完全一致，且 temp 必须存在；
/// - Unix：同一文件系统 rename 替换（原子）；
/// - Windows：target 已存在时使用 ReplaceFileW；target 不存在时使用 MoveFileExW (MOVEFILE_WRITE_THROUGH，严禁 MOVEFILE_COPY_ALLOWED)；
/// - 替换失败时保留旧 target，删除 temp，并返回 Error；
/// - 成功后 temp 不再存在，target 内容为 temp 内容。
pub async fn atomic_replace_file(temp: &Path, target: &Path) -> Result<()> {
    let temp_parent = temp
        .parent()
        .ok_or_else(|| anyhow!("temp 路径缺少父目录"))?;
    let target_parent = target
        .parent()
        .ok_or_else(|| anyhow!("target 路径缺少父目录"))?;

    if temp_parent != target_parent {
        return Err(anyhow!(
            "temp 与 target 必须在同一目录下以确保原子替换，拒绝跨目录操作"
        ));
    }

    if !temp.exists() {
        return Err(anyhow!("源临时文件不存在，无法执行原子替换"));
    }

    #[cfg(target_os = "windows")]
    {
        let temp_owned = temp.to_path_buf();
        let target_owned = target.to_path_buf();
        let result =
            tokio::task::spawn_blocking(move || windows_atomic_replace(&temp_owned, &target_owned))
                .await
                .map_err(|e| anyhow!("原子替换任务异常结束: {e}"))?;
        if let Err(e) = result {
            let _ = tokio::fs::remove_file(temp).await;
            return Err(e);
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Unix rename 在同文件系统上是原子操作
        if let Err(e) = tokio::fs::rename(temp, target).await {
            let _ = tokio::fs::remove_file(temp).await;
            return Err(anyhow!("原子替换文件失败: {e}"));
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn windows_atomic_replace(temp: &Path, target: &Path) -> Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            lpReplacedFileName: *const u16,
            lpReplacementFileName: *const u16,
            lpBackupFileName: *const u16,
            dwReplaceFlags: u32,
            lpExclude: *const std::ffi::c_void,
            lpReserved: *const std::ffi::c_void,
        ) -> i32;

        fn MoveFileExW(
            lpExistingFileName: *const u16,
            lpNewFileName: *const u16,
            dwFlags: u32,
        ) -> i32;

        fn GetLastError() -> u32;
    }

    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    fn to_wide(s: &Path) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let temp_wide = to_wide(temp);
    let target_wide = to_wide(target);

    if target.exists() {
        // target 存在：使用 ReplaceFileW 执行同卷原子替换
        let result = unsafe {
            ReplaceFileW(
                target_wide.as_ptr(),
                temp_wide.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if result == 0 {
            let err_code = unsafe { GetLastError() };
            return Err(anyhow!(
                "Windows ReplaceFileW 原子替换已有目标失败，错误码: {err_code}"
            ));
        }
    } else {
        // target 不存在：使用 MoveFileExW (MOVEFILE_WRITE_THROUGH，严禁 COPY_ALLOWED)
        let result = unsafe {
            MoveFileExW(
                temp_wide.as_ptr(),
                target_wide.as_ptr(),
                MOVEFILE_WRITE_THROUGH,
            )
        };
        if result == 0 {
            let err_code = unsafe { GetLastError() };
            return Err(anyhow!(
                "Windows MoveFileExW 创建新目标失败，错误码: {err_code}"
            ));
        }
    }

    Ok(())
}

/// 净化并严格校验本地文件名，拒绝非法路径与危险命名（严格禁止静默改写）
pub fn sanitize_file_name(raw: &str) -> Result<String> {
    // 1. 检查是否为空或纯空白
    if raw.trim().is_empty() {
        return Err(anyhow!("文件名为空，拒绝处理"));
    }

    // 2. 严禁前导或尾随空白（禁止静默改名，若有则必须报错拒绝）
    if raw != raw.trim() {
        return Err(anyhow!("文件名包含前导或尾随空白字符，拒绝处理"));
    }

    // 3. 拒绝点目录
    if raw == "." || raw == ".." {
        return Err(anyhow!("文件名不能为 '.' 或 '..' 点目录"));
    }

    // 4. 拒绝路径分隔符
    if raw.contains('/') || raw.contains('\\') {
        return Err(anyhow!("文件名包含路径分隔符，拒绝路径逃逸"));
    }

    // 5. 拒绝 NUL 及控制字符
    if raw.contains('\0') || raw.chars().any(|c| c.is_control()) {
        return Err(anyhow!("文件名包含 NUL 或非法控制字符"));
    }

    // 6. 拒绝跨平台非法字符 (Windows: < > : " | ? *)
    if raw
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return Err(anyhow!("文件名包含跨平台非法字符 (<, >, :, \", |, ?, *)"));
    }

    // 7. 拒绝尾随点或空格（Windows 会静默截断）
    if raw.ends_with('.') || raw.ends_with(' ') {
        return Err(anyhow!("文件名不能以点或空格结尾"));
    }

    // 8. 拒绝 Windows 保留设备名 (CON, PRN, AUX, NUL, COM1..9, LPT1..9)
    let stem = match raw.find('.') {
        Some(idx) => &raw[..idx],
        None => raw,
    };
    let upper_stem = stem.to_ascii_uppercase();
    const RESERVED_DEVICE_NAMES: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED_DEVICE_NAMES.contains(&upper_stem.as_str()) {
        return Err(anyhow!("文件名使用了系统保留设备名: {stem}"));
    }

    // 9. 确保作为单个 Path 分段解析后与原始文件名严格一致
    let path = Path::new(raw);
    let single_component = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("无法解析为有效的文件名分段"))?;

    if single_component != raw {
        return Err(anyhow!("文件名包含多级路径分段，拒绝处理"));
    }

    Ok(raw.to_string())
}

const HASH_BUFFER_SIZE: usize = 64 * 1024;

/// 计算文件完整内容的 SHA-256 摘要（十六进制字符串）。
///
/// 调用方负责在阻塞任务中运行此函数。
fn compute_file_hash(path: &std::path::Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; HASH_BUFFER_SIZE];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let result = hasher.finalize();
    Ok(result.iter().map(|b| format!("{:02x}", b)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File, OpenOptions};
    use std::future::Future;
    use std::io::{Seek, SeekFrom, Write};
    use std::pin::Pin;
    use std::sync::Mutex;

    use file::RemoteObjectEntry;

    fn test_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("lumen-{name}-{}.bin", uuid::Uuid::new_v4()))
    }

    fn write_repeated(file: &mut File, byte: u8, len: usize) -> std::io::Result<()> {
        let chunk = [byte; HASH_BUFFER_SIZE];
        let mut remaining = len;
        while remaining > 0 {
            let bytes_to_write = remaining.min(chunk.len());
            file.write_all(&chunk[..bytes_to_write])?;
            remaining -= bytes_to_write;
        }
        Ok(())
    }

    #[test]
    fn hashes_empty_files_with_standard_sha256() {
        let path = test_path("empty-hash");
        File::create(&path).unwrap();

        let hash = compute_file_hash(&path).unwrap();

        fs::remove_file(&path).unwrap();
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn hashes_bytes_after_the_legacy_ten_megabyte_boundary() {
        const LEGACY_HASH_READ_SIZE: usize = 10 * 1024 * 1024;
        let path = test_path("full-file-hash");
        let mut file = File::create(&path).unwrap();
        write_repeated(&mut file, 0xA5, LEGACY_HASH_READ_SIZE).unwrap();
        file.write_all(b"first suffix").unwrap();
        file.flush().unwrap();

        let first_hash = compute_file_hash(&path).unwrap();

        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(LEGACY_HASH_READ_SIZE as u64))
            .unwrap();
        file.write_all(b"other suffix").unwrap();
        file.flush().unwrap();

        let second_hash = compute_file_hash(&path).unwrap();

        fs::remove_file(&path).unwrap();
        assert_ne!(first_hash, second_hash);
    }

    #[test]
    fn reports_file_read_errors_to_the_caller() {
        let path = test_path("missing-hash");

        let error = compute_file_hash(&path).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }

    fn sample_attachment(
        id: &str,
        is_deleted: bool,
        is_dirty: bool,
        file_path: &str,
    ) -> Attachment {
        Attachment {
            id: id.to_string(),
            literature_id: "lit-1".to_string(),
            file_name: "paper.pdf".to_string(),
            file_path: file_path.to_string(),
            file_size: 1024,
            mime_type: Some("application/pdf".to_string()),
            etag: None,
            hash: None,
            is_main: true,
            is_dirty,
            is_deleted,
            version: 1,
            created_at: 100,
            updated_at: 100,
        }
    }

    /// 规划器测试用：返回 `AttachmentSyncSnapshot`（含 `synced_version` 以表达逐附件数据库确认）。
    fn sample_snapshot(
        id: &str,
        is_deleted: bool,
        is_dirty: bool,
        file_path: &str,
        synced_version: i64,
    ) -> AttachmentSyncSnapshot {
        AttachmentSyncSnapshot {
            id: id.to_string(),
            version: 1,
            synced_version,
            is_dirty,
            is_deleted,
            file_path: file_path.to_string(),
            file_name: "paper.pdf".to_string(),
        }
    }

    #[test]
    fn test_plan_file_actions_matrix() {
        let att_uuid = "550e8400-e29b-41d4-a716-446655440000";
        let obj_key = format!("objects/v1/{att_uuid}");

        // 创建临时测试文件
        let tmp_file =
            std::env::temp_dir().join(format!("lumen-test-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_file, b"test content").unwrap();
        let tmp_path_str = tmp_file.to_string_lossy().to_string();

        let mut baselines = HashMap::new();
        let mut remote_objects = HashMap::new();

        // 1. 活跃、普通本地文件、对象不存在、已确认 -> Upload
        let att1 = sample_snapshot(att_uuid, false, false, &tmp_path_str, 1);
        let atts = [att1];
        let plans = plan_file_actions(
            &atts,
            &baselines,
            &remote_objects,
            FileMutationPolicy::allow_all(),
        );
        assert_eq!(
            plans,
            vec![FileActionPlan::Upload {
                attachment_id: att_uuid.to_string(),
                object_key: obj_key.clone(),
                local_path: tmp_file.clone(),
            }]
        );

        // 2. 活跃、对象不存在、is_dirty=true -> WaitingForDatabaseConfirmation
        let att_dirty = sample_snapshot(att_uuid, false, true, &tmp_path_str, 0);
        let plans_dirty = plan_file_actions(
            &[att_dirty],
            &baselines,
            &remote_objects,
            FileMutationPolicy::allow_all(),
        );
        assert_eq!(
            plans_dirty,
            vec![FileActionPlan::WaitingForDatabaseConfirmation {
                attachment_id: att_uuid.to_string(),
            }]
        );

        // 3. 活跃、本地缺失、对象存在 -> Download（恢复下载不受确认门槛限制）
        remote_objects.insert(obj_key.clone(), "v1".to_string());
        let att_missing = sample_snapshot(att_uuid, false, false, "/non/existent/path.pdf", 1);
        let plans_download = plan_file_actions(
            &[att_missing],
            &baselines,
            &remote_objects,
            FileMutationPolicy::allow_all(),
        );
        assert_eq!(
            plans_download,
            vec![FileActionPlan::Download {
                attachment_id: att_uuid.to_string(),
                object_key: obj_key.clone(),
                target_file_name: "paper.pdf".to_string(),
                adopt_baseline: false,
            }]
        );

        // 4. 活跃、本地存在、对象存在、有同版本 baseline -> Skip
        baselines.insert(
            att_uuid.to_string(),
            AttachmentFileBaseline {
                attachment_id: att_uuid.to_string(),
                file_library_id: "flib-1".to_string(),
                object_key: obj_key.clone(),
                remote_version: "v1".to_string(),
                local_sha256: "hash".to_string(),
                local_presence: true,
                last_success_at: 100,
            },
        );
        let plans_skip = plan_file_actions(
            &atts,
            &baselines,
            &remote_objects,
            FileMutationPolicy::allow_all(),
        );
        assert_eq!(
            plans_skip,
            vec![FileActionPlan::Skip {
                attachment_id: att_uuid.to_string(),
            }]
        );

        // 5. 活跃、本地存在、对象存在、版本不同且本地哈希不同 -> FileConflict
        remote_objects.insert(obj_key.clone(), "v2-different".to_string());
        let plans_existing = plan_file_actions(
            &atts,
            &baselines,
            &remote_objects,
            FileMutationPolicy::allow_all(),
        );
        // Verify we got a FileConflict and that the local_sha256 differs from the placeholder "hash"
        assert_eq!(plans_existing.len(), 1);
        match &plans_existing[0] {
            FileActionPlan::FileConflict {
                attachment_id,
                object_key,
                remote_version,
                local_sha256,
            } => {
                assert_eq!(attachment_id, &att_uuid.to_string());
                assert_eq!(object_key, &obj_key);
                assert_eq!(remote_version, "v2-different");
                assert_ne!(local_sha256, "hash");
            }
            other => panic!("expected FileConflict, got {:?}", other),
        }

        // 6. 活跃、本地缺失、对象不存在 -> UnrecoverableMissing
        remote_objects.clear();
        let att_unrec = sample_snapshot(att_uuid, false, false, "/non/existent/path.pdf", 1);
        let plans_unrec = plan_file_actions(
            &[att_unrec],
            &baselines,
            &remote_objects,
            FileMutationPolicy::allow_all(),
        );
        assert_eq!(
            plans_unrec,
            vec![FileActionPlan::UnrecoverableMissing {
                attachment_id: att_uuid.to_string(),
            }]
        );

        // 7. tombstone、对象存在、已确认 -> Delete
        remote_objects.insert(obj_key.clone(), "v1".to_string());
        let att_del = sample_snapshot(att_uuid, true, false, &tmp_path_str, 1);
        let plans_del = plan_file_actions(
            &[att_del],
            &baselines,
            &remote_objects,
            FileMutationPolicy::allow_all(),
        );
        assert_eq!(
            plans_del,
            vec![FileActionPlan::Delete {
                attachment_id: att_uuid.to_string(),
                object_key: obj_key.clone(),
            }]
        );

        // 8. tombstone、对象不存在、已确认 -> ClearBaseline
        remote_objects.clear();
        let att_clear = sample_snapshot(att_uuid, true, false, &tmp_path_str, 1);
        let plans_clear = plan_file_actions(
            &[att_clear],
            &baselines,
            &remote_objects,
            FileMutationPolicy::allow_all(),
        );
        assert_eq!(
            plans_clear,
            vec![FileActionPlan::ClearBaseline {
                attachment_id: att_uuid.to_string(),
            }]
        );

        // 清理临时测试文件
        let _ = std::fs::remove_file(&tmp_file);
    }

    #[test]
    fn planner_blocks_unconfirmed_upload_despite_allow_policy() {
        // 活跃普通文件、远端不存在，但 synced_version=0（从未确认）→ 即使策略允许也不得 Upload
        let att_uuid = "550e8400-e29b-41d4-a716-446655440000".to_string();
        let _obj_key = format!("objects/v1/{att_uuid}");
        let tmp_file =
            std::env::temp_dir().join(format!("lumen-test-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_file, b"test content").unwrap();

        let snap = sample_snapshot(&att_uuid, false, false, &tmp_file.to_string_lossy(), 0);
        let plans = plan_file_actions(
            &[snap],
            &HashMap::new(),
            &HashMap::new(),
            FileMutationPolicy::allow_all(),
        );
        assert_eq!(
            plans,
            vec![FileActionPlan::WaitingForDatabaseConfirmation {
                attachment_id: att_uuid.clone()
            }]
        );

        // synced_version 落后于本地版本（确认版本 < 本地版本）→ 同样不得 Upload
        let snap_stale = sample_snapshot(&att_uuid, false, false, &tmp_file.to_string_lossy(), 0);
        // 本地 version=1，synced_version=0 < version → 未确认
        let plans_stale = plan_file_actions(
            &[snap_stale],
            &HashMap::new(),
            &HashMap::new(),
            FileMutationPolicy::allow_all(),
        );
        assert_eq!(plans_stale.len(), 1);
        assert!(matches!(
            plans_stale[0],
            FileActionPlan::WaitingForDatabaseConfirmation { .. }
        ));

        let _ = std::fs::remove_file(&tmp_file);
    }

    #[test]
    fn planner_blocks_unconfirmed_tombstone_delete_and_clear() {
        // 未确认 tombstone（synced_version=0）即使远端存在、策略允许删除，也不得 Delete
        let att_uuid = "550e8400-e29b-41d4-a716-446655440000".to_string();
        let obj_key = format!("objects/v1/{att_uuid}");
        let mut remote_objects = HashMap::new();
        remote_objects.insert(obj_key.clone(), "v1".to_string());

        let tomb_unconfirmed = sample_snapshot(&att_uuid, true, false, "/x.pdf", 0);
        let plans = plan_file_actions(
            &[tomb_unconfirmed],
            &HashMap::new(),
            &remote_objects,
            FileMutationPolicy::allow_all(),
        );
        assert_eq!(
            plans,
            vec![FileActionPlan::WaitingForDatabaseConfirmation {
                attachment_id: att_uuid.clone()
            }]
        );

        // 未确认 tombstone 远端缺失也不得 ClearBaseline
        remote_objects.clear();
        let tomb_unconfirmed2 = sample_snapshot(&att_uuid, true, false, "/x.pdf", 0);
        let plans2 = plan_file_actions(
            &[tomb_unconfirmed2],
            &HashMap::new(),
            &remote_objects,
            FileMutationPolicy::allow_all(),
        );
        assert_eq!(
            plans2,
            vec![FileActionPlan::WaitingForDatabaseConfirmation {
                attachment_id: att_uuid.clone()
            }]
        );
    }

    #[test]
    fn planner_denies_confirmed_tombstone_delete_when_policy_blocks() {
        // database PartialFailure/Error：tombstone 已确认但本轮禁止删除 → 安全 Skip（Delete=0）
        let att_uuid = "550e8400-e29b-41d4-a716-446655440000".to_string();
        let obj_key = format!("objects/v1/{att_uuid}");
        let mut remote_objects = HashMap::new();
        remote_objects.insert(obj_key.clone(), "v1".to_string());

        let tomb_confirmed = sample_snapshot(&att_uuid, true, false, "/x.pdf", 1);
        let plans = plan_file_actions(
            &[tomb_confirmed],
            &HashMap::new(),
            &remote_objects,
            FileMutationPolicy::deny_all(),
        );
        assert_eq!(
            plans,
            vec![FileActionPlan::Skip {
                attachment_id: att_uuid.clone()
            }]
        );
    }

    #[tokio::test]
    async fn atomic_replace_file_replaces_existing_target() {
        let temp = test_path("atomic-temp");
        let target = test_path("atomic-target");

        // 写入目标文件（旧内容）
        std::fs::write(&target, b"old content").unwrap();
        // 写入 temp（新内容）
        std::fs::write(&temp, b"new content").unwrap();

        atomic_replace_file(&temp, &target).await.unwrap();

        let content = std::fs::read(&target).unwrap();
        assert_eq!(content, b"new content");
        assert!(!temp.exists(), "temp 应已被删除");

        let _ = std::fs::remove_file(&target);
    }

    #[tokio::test]
    async fn atomic_replace_file_rejects_different_parents() {
        let temp = test_path("atomic-temp-p1");
        let dir2 = std::env::temp_dir().join(format!("lumen-sub-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir2).unwrap();
        let target = dir2.join("target.bin");

        std::fs::write(&temp, b"test").unwrap();

        let res = atomic_replace_file(&temp, &target).await;
        assert!(res.is_err(), "跨目录替换必须拒绝以保证同卷原子性");

        let _ = std::fs::remove_file(&temp);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[tokio::test]
    async fn atomic_replace_file_rejects_missing_temp() {
        let temp = test_path("atomic-missing-temp");
        let target = test_path("atomic-target-missing");

        let res = atomic_replace_file(&temp, &target).await;
        assert!(res.is_err(), "源 temp 缺失必须报错");
    }

    #[test]
    fn sanitize_file_name_validations() {
        // 空白与点目录
        assert!(sanitize_file_name("").is_err());
        assert!(sanitize_file_name("   ").is_err());
        assert!(sanitize_file_name(".").is_err());
        assert!(sanitize_file_name("..").is_err());

        // 严禁静默改写：前导/尾随空格必须拒绝
        assert!(sanitize_file_name(" paper.pdf").is_err());
        assert!(sanitize_file_name("paper.pdf ").is_err());
        assert!(sanitize_file_name("  paper.pdf  ").is_err());

        // 路径分隔符与控制字符
        assert!(sanitize_file_name("foo/bar.pdf").is_err());
        assert!(sanitize_file_name("foo\\bar.pdf").is_err());
        assert!(sanitize_file_name("foo\0bar.pdf").is_err());
        assert!(sanitize_file_name("foo\nbar.pdf").is_err());

        // 跨平台非法字符
        assert!(sanitize_file_name("foo:bar.pdf").is_err());
        assert!(sanitize_file_name("foo*bar.pdf").is_err());
        assert!(sanitize_file_name("foo?bar.pdf").is_err());
        assert!(sanitize_file_name("foo\"bar.pdf").is_err());
        assert!(sanitize_file_name("foo<bar.pdf").is_err());
        assert!(sanitize_file_name("foo>bar.pdf").is_err());
        assert!(sanitize_file_name("foo|bar.pdf").is_err());

        // 尾随点
        assert!(sanitize_file_name("paper.").is_err());
        assert!(sanitize_file_name("paper.pdf.").is_err());

        // Windows 系统保留设备名
        assert!(sanitize_file_name("CON").is_err());
        assert!(sanitize_file_name("con.txt").is_err());
        assert!(sanitize_file_name("aux.pdf").is_err());
        assert!(sanitize_file_name("NUL.doc").is_err());
        assert!(sanitize_file_name("com1.png").is_err());
        assert!(sanitize_file_name("lpt1.pdf").is_err());

        // 合法文件名（保持原始字符串完全不变）
        assert_eq!(sanitize_file_name("paper.pdf").unwrap(), "paper.pdf");
        assert_eq!(
            sanitize_file_name("论文 2026 (v1).pdf").unwrap(),
            "论文 2026 (v1).pdf"
        );
        assert_eq!(
            sanitize_file_name("Nature_Medicine_2026_Review.pdf").unwrap(),
            "Nature_Medicine_2026_Review.pdf"
        );
    }

    // ---------- 规划器：文件状态判定矩阵 ----------

    #[test]
    fn planner_adopts_baseline_via_download_when_no_baseline_and_both_sides_exist() {
        let att_uuid = uuid::Uuid::new_v4().to_string();
        let obj_key = format!("objects/v1/{att_uuid}");
        let tmp_file =
            std::env::temp_dir().join(format!("lumen-planner-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_file, b"local data").unwrap();

        let mut remote_objects = HashMap::new();
        remote_objects.insert(obj_key.clone(), "remote-v1".to_string());

        let att = sample_snapshot(&att_uuid, false, false, &tmp_file.to_string_lossy(), 1);
        let plans = plan_file_actions(
            &[att],
            &HashMap::new(),
            &remote_objects,
            FileMutationPolicy::allow_all(),
        );

        assert_eq!(
            plans,
            vec![FileActionPlan::Download {
                attachment_id: att_uuid,
                object_key: obj_key,
                target_file_name: "paper.pdf".to_string(),
                adopt_baseline: true,
            }]
        );

        let _ = std::fs::remove_file(&tmp_file);
    }

    #[test]
    fn planner_downloads_when_version_changed_and_local_hash_matches_baseline() {
        let att_uuid = uuid::Uuid::new_v4().to_string();
        let obj_key = format!("objects/v1/{att_uuid}");
        let tmp_file =
            std::env::temp_dir().join(format!("lumen-planner-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_file, b"unchanged local content").unwrap();
        let local_hash = compute_file_hash(&tmp_file).unwrap();

        let mut baselines = HashMap::new();
        baselines.insert(
            att_uuid.clone(),
            AttachmentFileBaseline {
                attachment_id: att_uuid.clone(),
                file_library_id: "flib-1".to_string(),
                object_key: obj_key.clone(),
                remote_version: "v1".to_string(),
                local_sha256: local_hash,
                local_presence: true,
                last_success_at: 100,
            },
        );
        let mut remote_objects = HashMap::new();
        remote_objects.insert(obj_key.clone(), "v2".to_string());

        let att = sample_snapshot(&att_uuid, false, false, &tmp_file.to_string_lossy(), 1);
        let plans = plan_file_actions(
            &[att],
            &baselines,
            &remote_objects,
            FileMutationPolicy::allow_all(),
        );

        assert_eq!(
            plans,
            vec![FileActionPlan::Download {
                attachment_id: att_uuid,
                object_key: obj_key,
                target_file_name: "paper.pdf".to_string(),
                adopt_baseline: false,
            }]
        );

        let _ = std::fs::remove_file(&tmp_file);
    }

    #[test]
    fn planner_records_unknown_divergence_for_non_regular_local_file_on_version_change() {
        // 本地路径是目录（非普通文件）且 baseline 版本落后 → 未知分歧，不得 Download/Upload
        let att_uuid = uuid::Uuid::new_v4().to_string();
        let obj_key = format!("objects/v1/{att_uuid}");
        let dir = std::env::temp_dir().join(format!("lumen-planner-dir-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut baselines = HashMap::new();
        baselines.insert(
            att_uuid.clone(),
            AttachmentFileBaseline {
                attachment_id: att_uuid.clone(),
                file_library_id: "flib-1".to_string(),
                object_key: obj_key.clone(),
                remote_version: "v1".to_string(),
                local_sha256: "hash".to_string(),
                local_presence: true,
                last_success_at: 100,
            },
        );
        let mut remote_objects = HashMap::new();
        remote_objects.insert(obj_key, "v2".to_string());

        let att = sample_snapshot(&att_uuid, false, false, &dir.to_string_lossy(), 1);
        let plans = plan_file_actions(
            &[att],
            &baselines,
            &remote_objects,
            FileMutationPolicy::allow_all(),
        );

        match &plans[0] {
            FileActionPlan::UnknownVersionDivergence {
                remote_version,
                local_sha256,
                ..
            } => {
                assert_eq!(remote_version, "v2");
                assert_eq!(local_sha256, "");
            }
            other => panic!("expected UnknownVersionDivergence, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn planner_never_uploads_when_local_path_is_not_a_regular_file() {
        // 本地目录 + 远端不存在 + 策略允许 → 仍不得 Upload（本地状态不确定）
        let att_uuid = uuid::Uuid::new_v4().to_string();
        let dir = std::env::temp_dir().join(format!("lumen-planner-dir-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let att = sample_snapshot(&att_uuid, false, false, &dir.to_string_lossy(), 1);
        let plans = plan_file_actions(
            &[att],
            &HashMap::new(),
            &HashMap::new(),
            FileMutationPolicy::allow_all(),
        );

        match &plans[0] {
            FileActionPlan::UnknownVersionDivergence { remote_version, .. } => {
                assert_eq!(remote_version, "");
            }
            other => panic!("expected UnknownVersionDivergence, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn planner_policy_does_not_block_safe_restore_or_baseline_adoption() {
        let att_uuid = uuid::Uuid::new_v4().to_string();
        let obj_key = format!("objects/v1/{att_uuid}");
        let tmp_file =
            std::env::temp_dir().join(format!("lumen-planner-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_file, b"data").unwrap();
        let path_str = tmp_file.to_string_lossy().to_string();

        let mut remote_objects = HashMap::new();
        remote_objects.insert(obj_key.clone(), "v1".to_string());

        // a) 本地缺失 + 远端存在 + 策略禁止变更 → 仍 Download（安全恢复不受上传/删除开关限制）
        let att_missing = sample_snapshot(&att_uuid, false, false, "/non/existent/path.pdf", 1);
        let plans = plan_file_actions(
            &[att_missing],
            &HashMap::new(),
            &remote_objects,
            FileMutationPolicy::deny_all(),
        );
        assert_eq!(
            plans,
            vec![FileActionPlan::Download {
                attachment_id: att_uuid.clone(),
                object_key: obj_key.clone(),
                target_file_name: "paper.pdf".to_string(),
                adopt_baseline: false,
            }]
        );

        // b) 本地普通文件 + 远端存在 + 无 baseline + 策略禁止变更 → 仍 Download(adopt_baseline=true)
        let att_present = sample_snapshot(&att_uuid, false, false, &path_str, 1);
        let plans = plan_file_actions(
            &[att_present],
            &HashMap::new(),
            &remote_objects,
            FileMutationPolicy::deny_all(),
        );
        assert_eq!(
            plans,
            vec![FileActionPlan::Download {
                attachment_id: att_uuid.clone(),
                object_key: obj_key.clone(),
                target_file_name: "paper.pdf".to_string(),
                adopt_baseline: true,
            }]
        );

        // c) tombstone 已确认 + 远端存在 + 策略禁止删除 → Skip（不 Delete，Delete=0）
        let att_tomb = sample_snapshot(&att_uuid, true, false, &path_str, 1);
        let plans = plan_file_actions(
            &[att_tomb],
            &HashMap::new(),
            &remote_objects,
            FileMutationPolicy::deny_all(),
        );
        assert_eq!(
            plans,
            vec![FileActionPlan::Skip {
                attachment_id: att_uuid.clone(),
            }]
        );

        // d) 本地普通文件 + 远端不存在 + 已确认但策略禁止上传 → 等待数据库确认（不 Upload 也不静默 Skip）
        let att_active = sample_snapshot(&att_uuid, false, false, &path_str, 1);
        let plans = plan_file_actions(
            &[att_active],
            &HashMap::new(),
            &HashMap::new(),
            FileMutationPolicy::deny_all(),
        );
        assert_eq!(
            plans,
            vec![FileActionPlan::WaitingForDatabaseConfirmation {
                attachment_id: att_uuid.clone(),
            }]
        );

        let _ = std::fs::remove_file(&tmp_file);
    }

    #[test]
    fn existing_object_without_baseline_variant_is_absent_from_source() {
        // 已废弃的“存在对象但无 baseline”独立变体不得在源码（含本测试模块）中复现；
        // 字符串分段拼接避免本测试自身命中检索
        let banned = ["Existing", "ObjectWithoutBaseline"].concat();
        assert!(!include_str!("attachments.rs").contains(&banned));
    }

    // ---------- 首次见库安全比较：可控测试后端（仅存在于测试模块） ----------

    const TEST_FILE_LIBRARY_ID: &str = "0f1e2d3c-4b5a-6978-8a9b-0c1d2e3f4a5b";
    const TEST_DB_LIBRARY_ID: &str = "db-lib-test-1";

    struct TestRemoteObject {
        content: Vec<u8>,
        version: String,
    }

    struct TestBackend {
        objects: Mutex<HashMap<String, TestRemoteObject>>,
        fail_download: AtomicBool,
        empty_remote_version: AtomicBool,
        temp_as_directory: AtomicBool,
        corrupt_local_path: Mutex<Option<PathBuf>>,
        /// 下载期间把该目录设为只读，模拟后续数据库写入失败
        readonly_dir: Mutex<Option<PathBuf>>,
        concurrent_write: Mutex<Option<(PathBuf, Vec<u8>)>>,
    }

    impl TestBackend {
        fn new() -> Self {
            Self {
                objects: Mutex::new(HashMap::new()),
                fail_download: AtomicBool::new(false),
                empty_remote_version: AtomicBool::new(false),
                temp_as_directory: AtomicBool::new(false),
                corrupt_local_path: Mutex::new(None),
                readonly_dir: Mutex::new(None),
                concurrent_write: Mutex::new(None),
            }
        }

        fn put_object(&self, object_key: &str, content: &[u8], version: &str) {
            self.objects.lock().unwrap().insert(
                object_key.to_string(),
                TestRemoteObject {
                    content: content.to_vec(),
                    version: version.to_string(),
                },
            );
        }
    }

    impl AttachmentBackend for TestBackend {
        fn name(&self) -> &str {
            "test"
        }

        fn is_enabled(&self) -> bool {
            true
        }

        fn test_connection(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
            Box::pin(async { Ok(()) })
        }

        fn inspect_library(
            &self,
        ) -> Pin<Box<dyn Future<Output = Result<LibraryInspection>> + Send>> {
            Box::pin(async {
                Ok(LibraryInspection::Present(FileLibraryIdentity {
                    protocol_version: 1,
                    file_library_id: TEST_FILE_LIBRARY_ID.to_string(),
                    database_library_id: TEST_DB_LIBRARY_ID.to_string(),
                    created_at: 0,
                }))
            })
        }

        fn initialize_library(
            &self,
            _identity: FileLibraryIdentity,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
            Box::pin(async { Ok(()) })
        }

        fn list_objects(
            &self,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<RemoteObjectEntry>>> + Send>> {
            // trait 返回的 future 是 'static：在 future 外快照后端状态
            let entries: Vec<RemoteObjectEntry> = self
                .objects
                .lock()
                .unwrap()
                .iter()
                .map(|(key, obj)| RemoteObjectEntry {
                    object_key: key.clone(),
                    remote_version: obj.version.clone(),
                })
                .collect();
            Box::pin(async move { Ok(entries) })
        }

        fn upload_object_if_absent(
            &self,
            object_key: String,
            local_path: PathBuf,
        ) -> Pin<Box<dyn Future<Output = Result<UploadObjectResult>> + Send>> {
            // trait 返回的 future 是 'static：同步完成后返回就绪 future
            let mut objects = self.objects.lock().unwrap();
            let result = match objects.entry(object_key) {
                std::collections::hash_map::Entry::Occupied(_) => {
                    Ok(UploadObjectResult::AlreadyExists)
                }
                std::collections::hash_map::Entry::Vacant(slot) => match std::fs::read(&local_path)
                {
                    Ok(content) => {
                        slot.insert(TestRemoteObject {
                            content,
                            version: "uploaded-v1".to_string(),
                        });
                        Ok(UploadObjectResult::Created("uploaded-v1".to_string()))
                    }
                    Err(e) => Err(e.into()),
                },
            };
            Box::pin(async move { result })
        }

        fn download_object(
            &self,
            object_key: String,
            temporary_path: PathBuf,
        ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send>> {
            // trait 返回的 future 是 'static：在 future 外快照后端状态
            let fail_download = self.fail_download.load(Ordering::Relaxed);
            let empty_remote_version = self.empty_remote_version.load(Ordering::Relaxed);
            let temp_as_directory = self.temp_as_directory.load(Ordering::Relaxed);
            let corrupt_local_path = self.corrupt_local_path.lock().unwrap().clone();
            let readonly_dir = self.readonly_dir.lock().unwrap().clone();
            let concurrent_write = self.concurrent_write.lock().unwrap().clone();
            let object = self
                .objects
                .lock()
                .unwrap()
                .get(&object_key)
                .map(|obj| (obj.content.clone(), obj.version.clone()));

            Box::pin(async move {
                if fail_download {
                    // 模拟传输失败：先写入部分内容再报错，验证调用方清理临时文件
                    let _ = std::fs::write(&temporary_path, b"partial");
                    anyhow::bail!("injected download failure");
                }
                // 可选副作用：下载期间把本地正式文件替换为空目录，
                // 模拟比较期间本地文件状态变化
                if let Some(path) = corrupt_local_path {
                    let _ = std::fs::remove_file(&path);
                    let _ = std::fs::create_dir_all(&path);
                }
                // 可选副作用：下载完成后（返回前）把数据库所在目录设为只读，
                // 使本轮后续数据库写入（baseline）失败
                if let Some(dir) = readonly_dir {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ =
                            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555));
                    }
                    #[cfg(windows)]
                    {
                        if let Ok(meta) = std::fs::metadata(&dir) {
                            let mut perms = meta.permissions();
                            perms.set_readonly(true);
                            let _ = std::fs::set_permissions(&dir, perms);
                        }
                    }
                }
                let Some((content, version)) = object else {
                    return Ok(None);
                };
                if temp_as_directory {
                    // 模拟临时文件异常：temp 位置是目录而非普通文件，导致 temp 哈希失败
                    std::fs::create_dir_all(&temporary_path)?;
                    return Ok(Some(version));
                }
                std::fs::write(&temporary_path, &content)?;
                if let Some((path, bytes)) = concurrent_write {
                    let _ = std::fs::write(&path, bytes);
                }
                if empty_remote_version {
                    return Ok(Some(String::new()));
                }
                Ok(Some(version))
            })
        }

        fn delete_object(
            &self,
            object_key: String,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
            // trait 返回的 future 是 'static：同步完成后返回就绪 future
            self.objects.lock().unwrap().remove(&object_key);
            Box::pin(async { Ok(()) })
        }

        fn configuration_fingerprint(
            &self,
        ) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
            Box::pin(async { Ok("test-fingerprint".to_string()) })
        }
    }

    fn temp_test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-fsync-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn create_test_service(dir: &Path, backend: TestBackend) -> FileSyncService {
        // 默认使用内存数据库；需要模拟数据库写入失败的测试传入文件路径
        let db = Arc::new(Database::new(":memory:").unwrap());
        db.set_local_library_id(TEST_DB_LIBRARY_ID).unwrap();
        let file_manager = LocalFileManager::new(dir).unwrap();
        let notify_ui: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        FileSyncService::new(db, file_manager, Box::new(backend), notify_ui)
    }

    fn create_file_backed_test_service(
        dir: &Path,
        db_path: &Path,
        backend: TestBackend,
    ) -> FileSyncService {
        let db = Arc::new(Database::new(db_path).unwrap());
        db.set_local_library_id(TEST_DB_LIBRARY_ID).unwrap();
        let file_manager = LocalFileManager::new(dir).unwrap();
        let notify_ui: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        FileSyncService::new(db, file_manager, Box::new(backend), notify_ui)
    }

    fn no_temp_leftovers(dir: &Path) -> bool {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .all(|entry| !entry.file_name().to_string_lossy().starts_with(".tmp."))
    }

    #[tokio::test]
    async fn first_seen_equal_content_establishes_baseline_without_replacing_local_file() {
        // 计数语义：两端一致仅建立 baseline，未发生下载替换，计入 skipped
        let dir = temp_test_dir("first-seen-equal");
        let att_id = uuid::Uuid::new_v4().to_string();
        let object_key = format!("objects/v1/{att_id}");
        let formal = dir.join("paper.pdf");
        std::fs::write(&formal, b"same content").unwrap();

        let backend = TestBackend::new();
        backend.put_object(&object_key, b"same content", "remote-v1");

        let service = create_test_service(&dir, backend);
        let att = sample_attachment(&att_id, false, false, &formal.to_string_lossy());
        service.db.insert_attachment(&att).unwrap();

        // 策略禁止变更：既有对象的安全恢复（Download）不受上传/删除开关限制
        let summary = service
            .sync_file_library_round(FileMutationPolicy::deny_all())
            .await
            .unwrap();

        assert_eq!(
            summary.preflight,
            FileLibraryPreflight::Ready {
                file_library_id: TEST_FILE_LIBRARY_ID.to_string(),
                database_library_id: TEST_DB_LIBRARY_ID.to_string(),
            }
        );
        assert_eq!(summary.skipped, 1, "两端一致仅建立 baseline，计入 skipped");
        assert_eq!(summary.downloaded, 0);
        assert_eq!(summary.unknown_divergence, 0);
        assert_eq!(summary.failed, 0);
        // 正式文件字节完全不变
        assert_eq!(std::fs::read(&formal).unwrap(), b"same content");
        // 当前库 baseline 建立，remote_version/hash 正确且非空
        let baseline = service
            .db
            .get_attachment_file_baseline(&att_id, TEST_FILE_LIBRARY_ID)
            .unwrap()
            .expect("baseline 应已建立");
        assert_eq!(baseline.remote_version, "remote-v1");
        assert!(!baseline.remote_version.is_empty());
        assert_eq!(baseline.local_sha256, compute_file_hash(&formal).unwrap());
        assert!(!baseline.local_sha256.is_empty());
        assert!(baseline.local_presence);
        // 无冲突记录
        assert!(
            service
                .db
                .list_file_conflicts(TEST_FILE_LIBRARY_ID)
                .unwrap()
                .is_empty()
        );
        // 临时文件已删除
        assert!(no_temp_leftovers(&dir));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn first_seen_different_content_records_unknown_divergence_without_overwrite() {
        let dir = temp_test_dir("first-seen-different");
        let att_id = uuid::Uuid::new_v4().to_string();
        let object_key = format!("objects/v1/{att_id}");
        let formal = dir.join("paper.pdf");
        std::fs::write(&formal, b"local original").unwrap();

        let backend = TestBackend::new();
        backend.put_object(&object_key, b"remote different", "remote-v1");

        let service = create_test_service(&dir, backend);
        let att = sample_attachment(&att_id, false, false, &formal.to_string_lossy());
        service.db.insert_attachment(&att).unwrap();

        let summary = service
            .sync_file_library_round(FileMutationPolicy::deny_all())
            .await
            .unwrap();

        assert_eq!(summary.unknown_divergence, 1);
        assert_eq!(summary.downloaded, 0);
        assert_eq!(summary.failed, 0);
        // 正式文件逐字节不变
        assert_eq!(std::fs::read(&formal).unwrap(), b"local original");
        // 无 baseline
        assert!(
            service
                .db
                .get_attachment_file_baseline(&att_id, TEST_FILE_LIBRARY_ID)
                .unwrap()
                .is_none()
        );
        // 存在 unknown_divergence 记录，且保存了本地完整哈希与非空远端版本
        let conflicts = service
            .db
            .list_file_conflicts(TEST_FILE_LIBRARY_ID)
            .unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].reason, "unknown_divergence");
        assert_eq!(
            conflicts[0].local_sha256,
            compute_file_hash(&formal).unwrap()
        );
        assert_eq!(conflicts[0].remote_version, "remote-v1");
        // 临时文件已删除
        assert!(no_temp_leftovers(&dir));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn first_seen_download_failure_keeps_local_file_and_baseline_untouched() {
        let dir = temp_test_dir("first-seen-dl-fail");
        let att_id = uuid::Uuid::new_v4().to_string();
        let object_key = format!("objects/v1/{att_id}");
        let formal = dir.join("paper.pdf");
        std::fs::write(&formal, b"local original").unwrap();

        let backend = TestBackend::new();
        backend.put_object(&object_key, b"remote content", "remote-v1");
        backend.fail_download.store(true, Ordering::Relaxed);

        let service = create_test_service(&dir, backend);
        let att = sample_attachment(&att_id, false, false, &formal.to_string_lossy());
        service.db.insert_attachment(&att).unwrap();

        let summary = service
            .sync_file_library_round(FileMutationPolicy::deny_all())
            .await
            .unwrap();

        assert_eq!(summary.failed, 1);
        assert_eq!(summary.downloaded, 0);
        assert_eq!(summary.skipped, 0);
        // 正式文件不变、无 baseline
        assert_eq!(std::fs::read(&formal).unwrap(), b"local original");
        assert!(
            service
                .db
                .get_attachment_file_baseline(&att_id, TEST_FILE_LIBRARY_ID)
                .unwrap()
                .is_none()
        );
        assert!(
            service
                .db
                .list_file_conflicts(TEST_FILE_LIBRARY_ID)
                .unwrap()
                .is_empty()
        );
        // 部分写入的临时文件已被尽力清理
        assert!(no_temp_leftovers(&dir));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn first_seen_temp_hash_failure_keeps_local_file_and_baseline_untouched() {
        let dir = temp_test_dir("first-seen-temp-hash");
        let att_id = uuid::Uuid::new_v4().to_string();
        let object_key = format!("objects/v1/{att_id}");
        let formal = dir.join("paper.pdf");
        std::fs::write(&formal, b"local original").unwrap();

        let backend = TestBackend::new();
        backend.put_object(&object_key, b"remote content", "remote-v1");
        backend.temp_as_directory.store(true, Ordering::Relaxed);

        let service = create_test_service(&dir, backend);
        let att = sample_attachment(&att_id, false, false, &formal.to_string_lossy());
        service.db.insert_attachment(&att).unwrap();

        let summary = service
            .sync_file_library_round(FileMutationPolicy::deny_all())
            .await
            .unwrap();

        assert_eq!(summary.failed, 1);
        assert_eq!(summary.downloaded, 0);
        // 正式文件不变、无 baseline
        assert_eq!(std::fs::read(&formal).unwrap(), b"local original");
        assert!(
            service
                .db
                .get_attachment_file_baseline(&att_id, TEST_FILE_LIBRARY_ID)
                .unwrap()
                .is_none()
        );
        assert!(
            service
                .db
                .list_file_conflicts(TEST_FILE_LIBRARY_ID)
                .unwrap()
                .is_empty()
        );
        // 注意：temp 位置是目录而非普通文件，“尽力清理”只负责普通临时文件，
        // 此处不断言该目录被移除

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn first_seen_local_state_change_during_comparison_prevents_overwrite() {
        let dir = temp_test_dir("first-seen-state-change");
        let att_id = uuid::Uuid::new_v4().to_string();
        let object_key = format!("objects/v1/{att_id}");
        let formal = dir.join("paper.pdf");
        std::fs::write(&formal, b"local original").unwrap();

        let backend = TestBackend::new();
        backend.put_object(&object_key, b"remote different", "remote-v1");
        // 下载期间把本地正式文件替换为空目录，模拟比较期间文件状态变化
        *backend.corrupt_local_path.lock().unwrap() = Some(formal.clone());

        let service = create_test_service(&dir, backend);
        let att = sample_attachment(&att_id, false, false, &formal.to_string_lossy());
        service.db.insert_attachment(&att).unwrap();

        let summary = service
            .sync_file_library_round(FileMutationPolicy::deny_all())
            .await
            .unwrap();

        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 0);
        assert_eq!(summary.downloaded, 0);
        // 正式路径未被写入任何远端字节：仍是测试注入的空目录
        assert!(formal.is_dir());
        assert_eq!(std::fs::read_dir(&formal).unwrap().count(), 0);
        // 无 baseline
        assert!(
            service
                .db
                .get_attachment_file_baseline(&att_id, TEST_FILE_LIBRARY_ID)
                .unwrap()
                .is_none()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn first_seen_baseline_write_failure_is_not_counted_as_success() {
        // 场景：本地正式文件与远端对象内容一致，但 baseline 写入数据库失败
        //（下载完成后将数据库所在目录设为只读，SQLite 无法创建 journal，写入失败），
        // 必须计为 failed，不得替换正式文件、不得推进 baseline。
        let dir = temp_test_dir("first-seen-db-fail");
        let att_id = uuid::Uuid::new_v4().to_string();
        let object_key = format!("objects/v1/{att_id}");
        let formal = dir.join("attach").join("paper.pdf");
        std::fs::create_dir_all(dir.join("attach")).unwrap();
        std::fs::write(&formal, b"same content").unwrap();

        let backend = TestBackend::new();
        backend.put_object(&object_key, b"same content", "remote-v1");
        // 数据库文件放在 dir 根下（附件目录在子目录，保持可写以便临时文件创建）
        *backend.readonly_dir.lock().unwrap() = Some(dir.clone());

        let service = create_file_backed_test_service(
            &dir.join("attach"),
            &dir.join("lumen-test.db"),
            backend,
        );
        let att = sample_attachment(&att_id, false, false, &formal.to_string_lossy());
        service.db.insert_attachment(&att).unwrap();

        let summary = service
            .sync_file_library_round(FileMutationPolicy::deny_all())
            .await
            .expect("轮次本身应完成，单对象失败不阻断");

        // 两端内容一致但 baseline 写入失败：不得计为成功
        assert_eq!(summary.failed, 1, "baseline 写入失败必须计为 failed");
        assert_eq!(summary.skipped, 0);
        assert_eq!(summary.downloaded, 0);
        // 正式文件不替换
        assert_eq!(std::fs::read(&formal).unwrap(), b"same content");
        // baseline 未推进
        assert!(
            service
                .db
                .get_attachment_file_baseline(&att_id, TEST_FILE_LIBRARY_ID)
                .unwrap()
                .is_none()
        );
        // 临时文件已清理
        assert!(no_temp_leftovers(&dir.join("attach")));

        // 恢复目录权限以便清理
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755));
        }
        #[cfg(windows)]
        {
            if let Ok(meta) = std::fs::metadata(&dir) {
                let mut perms = meta.permissions();
                perms.set_readonly(false);
                let _ = std::fs::set_permissions(&dir, perms);
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn first_seen_empty_remote_version_fails_without_overwrite_or_baseline() {
        let dir = temp_test_dir("first-seen-empty-version");
        let att_id = uuid::Uuid::new_v4().to_string();
        let object_key = format!("objects/v1/{att_id}");
        let formal = dir.join("paper.pdf");
        std::fs::write(&formal, b"local original").unwrap();

        let backend = TestBackend::new();
        backend.put_object(&object_key, b"remote content", "remote-v1");
        backend.empty_remote_version.store(true, Ordering::Relaxed);

        let service = create_test_service(&dir, backend);
        let att = sample_attachment(&att_id, false, false, &formal.to_string_lossy());
        service.db.insert_attachment(&att).unwrap();

        let summary = service
            .sync_file_library_round(FileMutationPolicy::deny_all())
            .await
            .unwrap();

        // 空版本视为不确定：failed 加一，零覆盖、零 baseline
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.downloaded, 0);
        assert_eq!(summary.skipped, 0);
        assert_eq!(std::fs::read(&formal).unwrap(), b"local original");
        assert!(
            service
                .db
                .get_attachment_file_baseline(&att_id, TEST_FILE_LIBRARY_ID)
                .unwrap()
                .is_none()
        );
        assert!(
            service
                .db
                .list_file_conflicts(TEST_FILE_LIBRARY_ID)
                .unwrap()
                .is_empty()
        );
        // 临时文件已删除
        assert!(no_temp_leftovers(&dir));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn first_seen_local_file_change_after_hash_prevents_baseline_adoption() {
        // 场景：本地正式文件在初次哈希计算与建立 baseline 之间被外部篡改，
        // 必须触发复核失败，拒绝建立 baseline，删除临时文件并计为 failed。
        let dir = temp_test_dir("first-seen-modify-after-hash");
        let att_id = uuid::Uuid::new_v4().to_string();
        let object_key = format!("objects/v1/{att_id}");
        let formal = dir.join("paper.pdf");
        std::fs::write(&formal, b"initial content").unwrap();

        let backend = TestBackend::new();
        backend.put_object(&object_key, b"initial content", "remote-v1");

        let service = create_test_service(&dir, backend);
        let att = sample_attachment(&att_id, false, false, &formal.to_string_lossy());
        service.db.insert_attachment(&att).unwrap();

        // 篡改本地正式文件内容
        std::fs::write(&formal, b"modified content").unwrap();

        let summary = service
            .sync_file_library_round(FileMutationPolicy::deny_all())
            .await
            .unwrap();

        // 由于本地文件内容已变，或在哈希阶段判定不匹配或复核阶段拒绝建立 baseline，
        // 最终均不得建立 baseline，临时文件必须清理
        assert_eq!(summary.downloaded, 0);
        assert_eq!(std::fs::read(&formal).unwrap(), b"modified content");
        assert!(
            service
                .db
                .get_attachment_file_baseline(&att_id, TEST_FILE_LIBRARY_ID)
                .unwrap()
                .is_none()
        );
        assert!(no_temp_leftovers(&dir));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn prepare_attachment_fast_path_when_local_regular_file_exists() {
        let dir = temp_test_dir("prepare-fast-path");
        let att_id = uuid::Uuid::new_v4().to_string();
        let formal = dir.join("local_ready.pdf");
        std::fs::write(&formal, b"already downloaded").unwrap();

        let backend = TestBackend::new();
        let service = create_test_service(&dir, backend);
        let att = sample_attachment(&att_id, false, false, &formal.to_string_lossy());
        service.db.insert_attachment(&att).unwrap();

        let prepared = service.prepare_attachment_for_open(&att_id).await.unwrap();
        assert_eq!(prepared.local_path, formal);
        assert_eq!(prepared.issue, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn prepare_attachment_fails_when_remote_missing_and_returns_unrecoverable() {
        let dir = temp_test_dir("prepare-unrecoverable");
        let att_id = uuid::Uuid::new_v4().to_string();
        let missing_path = dir.join("non_existent.pdf");

        let backend = TestBackend::new();
        let service = create_test_service(&dir, backend);
        let att = sample_attachment(&att_id, false, false, &missing_path.to_string_lossy());
        service.db.insert_attachment(&att).unwrap();

        let res = service.prepare_attachment_for_open(&att_id).await;
        assert!(res.is_err());
        let err_msg = res.err().unwrap().to_string();
        assert!(err_msg.contains("远端对象不存在") || err_msg.contains("无法恢复"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn prepare_attachment_path_repair_when_standard_dir_has_matching_file() {
        let dir = temp_test_dir("prepare-path-repair");
        let att_id = uuid::Uuid::new_v4().to_string();
        let old_missing = dir.join("old_dir").join("repaired.pdf");
        let standard_file = dir.join("repaired.pdf");
        std::fs::write(&standard_file, b"content for repair").unwrap();
        let file_hash = compute_file_hash(&standard_file).unwrap();

        let backend = TestBackend::new();
        let object_key = format!("objects/v1/{att_id}");
        backend.put_object(&object_key, b"content for repair", "v1");

        let service = create_test_service(&dir, backend);
        let mut att = sample_attachment(&att_id, false, false, &old_missing.to_string_lossy());
        att.file_name = "repaired.pdf".to_string();
        service.db.insert_attachment(&att).unwrap();

        // 建立预先的 baseline
        let baseline = AttachmentFileBaseline {
            attachment_id: att_id.clone(),
            file_library_id: TEST_FILE_LIBRARY_ID.to_string(),
            object_key: object_key.clone(),
            remote_version: "v1".to_string(),
            local_sha256: file_hash.clone(),
            local_presence: true,
            last_success_at: 1000,
        };
        service
            .db
            .upsert_attachment_file_baseline(&baseline)
            .unwrap();

        let prepared = service.prepare_attachment_for_open(&att_id).await.unwrap();
        assert_eq!(prepared.local_path, standard_file);
        assert_eq!(prepared.issue, None);

        // 验证数据库内附件路径已被修复为标准目录文件
        let updated_att = service.db.get_attachment(&att_id).unwrap().unwrap();
        assert_eq!(
            updated_att.file_path,
            standard_file.to_string_lossy().to_string()
        );
        assert_eq!(updated_att.hash, Some(file_hash));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn prepare_attachment_prevents_overwrite_when_file_reappears_during_download() {
        let dir = temp_test_dir("prepare-concurrent-reappear");
        let att_id = uuid::Uuid::new_v4().to_string();
        let target_path = dir.join("reappear.pdf");
        let object_key = format!("objects/v1/{att_id}");

        let backend = TestBackend::new();
        backend.put_object(&object_key, b"remote new version", "v2");
        *backend.concurrent_write.lock().unwrap() =
            Some((target_path.clone(), b"concurrent local file".to_vec()));

        let service = create_test_service(&dir, backend);
        let mut att = sample_attachment(&att_id, false, false, &target_path.to_string_lossy());
        att.file_name = "reappear.pdf".to_string();
        service.db.insert_attachment(&att).unwrap();

        // 在准备前目标文件尚不存在
        assert!(!target_path.exists());

        // 触发准备：应在下载期间由 backend 触发并发写入，从而在替换前检测到目标路径出现普通文件且哈希不一致，阻止覆盖并报错
        let res = service.prepare_attachment_for_open(&att_id).await;
        assert!(res.is_err());
        assert_eq!(
            std::fs::read(&target_path).unwrap(),
            b"concurrent local file"
        );
        assert!(no_temp_leftovers(&dir));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn on_demand_round_registers_pending_download_and_unrecoverable_is_counted() {
        let dir = temp_test_dir("on-demand-pending");
        let att_id1 = uuid::Uuid::new_v4().to_string();
        let att_id2 = uuid::Uuid::new_v4().to_string();

        let backend = TestBackend::new();
        let object_key1 = format!("objects/v1/{att_id1}");
        backend.put_object(&object_key1, b"remote content 1", "v1");

        let service = create_test_service(&dir, backend);
        service.set_on_demand(true);

        // att1: 本地缺失、远端存在 -> on-demand 下记录 pending download
        let mut att1 = sample_attachment(
            &att_id1,
            false,
            false,
            &dir.join("missing1.pdf").to_string_lossy(),
        );
        att1.literature_id = "lit-1".to_string();
        service.db.insert_attachment(&att1).unwrap();

        // att2: 本地缺失、远端缺失 -> UnrecoverableMissing
        let mut att2 = sample_attachment(
            &att_id2,
            false,
            false,
            &dir.join("missing2.pdf").to_string_lossy(),
        );
        att2.literature_id = "lit-2".to_string();
        service.db.insert_attachment(&att2).unwrap();

        let summary = service
            .sync_file_library_round(FileMutationPolicy::deny_all())
            .await
            .unwrap();
        assert_eq!(summary.pending_download, 1);
        assert_eq!(summary.unrecoverable_missing, 1);
        assert_eq!(summary.downloaded, 0);

        // 检查 pending 记录
        let pending = service
            .db
            .get_pending_download(&att_id1, TEST_FILE_LIBRARY_ID)
            .unwrap();
        assert!(pending.is_some());
        assert_eq!(pending.unwrap().remote_version, "v1");

        // 检查诊断接口
        let issue1 = service.get_attachment_sync_issue(&att_id1).await.unwrap();
        assert_eq!(issue1, Some(AttachmentSyncIssue::PendingDownload));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn prepare_attachment_downloads_remote_new_version_instead_of_returning_old_candidate() {
        // 候选文件哈希与 baseline 一致，但远端版本更新：必须继续下载远端新版，
        // 不得把旧候选文件当成准备完成的结果。
        let dir = temp_test_dir("prepare-path-repair-remote-newer");
        let att_id = uuid::Uuid::new_v4().to_string();
        let object_key = format!("objects/v1/{att_id}");
        let old_missing = dir.join("old_dir").join("paper.pdf");
        let candidate = dir.join("paper.pdf");
        std::fs::write(&candidate, b"old content").unwrap();
        let candidate_hash = compute_file_hash(&candidate).unwrap();

        let backend = TestBackend::new();
        backend.put_object(&object_key, b"remote new content", "v2");

        let service = create_test_service(&dir, backend);
        let mut att = sample_attachment(&att_id, false, false, &old_missing.to_string_lossy());
        att.file_name = "paper.pdf".to_string();
        att.version = 5;
        service.db.insert_attachment(&att).unwrap();
        service
            .db
            .upsert_attachment_file_baseline(&AttachmentFileBaseline {
                attachment_id: att_id.clone(),
                file_library_id: TEST_FILE_LIBRARY_ID.to_string(),
                object_key: object_key.clone(),
                remote_version: "v1".to_string(),
                local_sha256: candidate_hash,
                local_presence: true,
                last_success_at: 1000,
            })
            .unwrap();

        let prepared = service.prepare_attachment_for_open(&att_id).await.unwrap();
        assert_eq!(prepared.local_path, candidate);
        assert_eq!(std::fs::read(&candidate).unwrap(), b"remote new content");

        let baseline = service
            .db
            .get_attachment_file_baseline(&att_id, TEST_FILE_LIBRARY_ID)
            .unwrap()
            .unwrap();
        assert_eq!(baseline.remote_version, "v2");
        assert_eq!(
            baseline.local_sha256,
            compute_file_hash(&candidate).unwrap()
        );

        // 路径修复不得改变 version / is_dirty / is_deleted
        let updated = service.db.get_attachment(&att_id).unwrap().unwrap();
        assert_eq!(updated.version, 5);
        assert!(!updated.is_dirty);
        assert!(!updated.is_deleted);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn prepare_attachment_rejects_pending_object_key_mismatch_and_keeps_record() {
        let dir = temp_test_dir("prepare-pending-key-mismatch");
        let att_id = uuid::Uuid::new_v4().to_string();
        let object_key = format!("objects/v1/{att_id}");

        let backend = TestBackend::new();
        backend.put_object(&object_key, b"remote content", "v1");

        let service = create_test_service(&dir, backend);
        let att = sample_attachment(
            &att_id,
            false,
            false,
            &dir.join("gone.pdf").to_string_lossy(),
        );
        service.db.insert_attachment(&att).unwrap();
        // 构造对象键不匹配的损坏 pending：先为另一个附件登记，再把记录迁移到本附件
        let wrong_owner = uuid::Uuid::new_v4().to_string();
        service
            .db
            .upsert_pending_download(&AttachmentPendingDownload {
                attachment_id: wrong_owner.clone(),
                file_library_id: TEST_FILE_LIBRARY_ID.to_string(),
                object_key: format!("objects/v1/{wrong_owner}"),
                remote_version: "v1".to_string(),
                created_at: 100,
            })
            .unwrap();
        service
            .db
            .rebind_pending_download_for_test(&wrong_owner, &att_id)
            .unwrap();

        let res = service.prepare_attachment_for_open(&att_id).await;
        assert!(res.is_err());
        assert_eq!(
            classify_prepare_attachment_error(&res.unwrap_err()),
            PrepareAttachmentErrorKind::InvalidState
        );
        // 损坏记录必须保留，等待人工/后续轮次处理
        assert!(
            service
                .db
                .get_pending_download(&att_id, TEST_FILE_LIBRARY_ID)
                .unwrap()
                .is_some()
        );
        assert!(no_temp_leftovers(&dir));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn prepare_attachment_updates_stale_pending_version_before_download() {
        let dir = temp_test_dir("prepare-pending-version-stale");
        let att_id = uuid::Uuid::new_v4().to_string();
        let object_key = format!("objects/v1/{att_id}");

        let backend = TestBackend::new();
        backend.put_object(&object_key, b"remote new content", "v2");

        let service = create_test_service(&dir, backend);
        let mut att = sample_attachment(
            &att_id,
            false,
            false,
            &dir.join("gone.pdf").to_string_lossy(),
        );
        att.file_name = "paper.pdf".to_string();
        service.db.insert_attachment(&att).unwrap();
        service
            .db
            .upsert_pending_download(&AttachmentPendingDownload {
                attachment_id: att_id.clone(),
                file_library_id: TEST_FILE_LIBRARY_ID.to_string(),
                object_key: object_key.clone(),
                remote_version: "v1".to_string(),
                created_at: 100,
            })
            .unwrap();

        let prepared = service.prepare_attachment_for_open(&att_id).await.unwrap();
        assert_eq!(
            std::fs::read(&prepared.local_path).unwrap(),
            b"remote new content"
        );
        // 恢复成功后只清理当前库的 pending
        assert!(
            service
                .db
                .get_pending_download(&att_id, TEST_FILE_LIBRARY_ID)
                .unwrap()
                .is_none()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn current_backend_binding_is_chosen_by_backend_identity_not_confirmed_at() {
        // A（当前 backend）confirmed_at 更早，B 更晚：仍必须选择当前 backend 绑定的 A。
        let dir = temp_test_dir("binding-by-backend-identity");
        let att_id = uuid::Uuid::new_v4().to_string();
        let object_key = format!("objects/v1/{att_id}");

        let backend = TestBackend::new();
        let service = create_test_service(&dir, backend);
        service
            .db
            .upsert_file_library_binding(&FileLibraryBinding {
                file_library_id: TEST_FILE_LIBRARY_ID.to_string(),
                database_library_id: TEST_DB_LIBRARY_ID.to_string(),
                backend_kind: "test".to_string(),
                backend_fingerprint: "test-fingerprint".to_string(),
                protocol_version: 1,
                confirmed_at: 100,
            })
            .unwrap();
        service
            .db
            .upsert_file_library_binding(&FileLibraryBinding {
                file_library_id: "flib-other-backend".to_string(),
                database_library_id: TEST_DB_LIBRARY_ID.to_string(),
                backend_kind: "webdav".to_string(),
                backend_fingerprint: "other-fingerprint".to_string(),
                protocol_version: 1,
                confirmed_at: 9_999,
            })
            .unwrap();

        let binding = service.current_backend_binding().await.unwrap().unwrap();
        assert_eq!(binding.file_library_id, TEST_FILE_LIBRARY_ID);

        // A/B 隔离：B 库的 pending 不会出现在 A 库的诊断中
        service
            .db
            .upsert_pending_download(&AttachmentPendingDownload {
                attachment_id: att_id.clone(),
                file_library_id: "flib-other-backend".to_string(),
                object_key,
                remote_version: "v1".to_string(),
                created_at: 100,
            })
            .unwrap();
        assert_eq!(
            service
                .sync_issue_in_library(&att_id, TEST_FILE_LIBRARY_ID)
                .unwrap(),
            None
        );
        assert_eq!(
            service
                .sync_issue_in_library(&att_id, "flib-other-backend")
                .unwrap(),
            Some(AttachmentSyncIssue::PendingDownload)
        );
        assert_eq!(
            service.get_attachment_sync_issue(&att_id).await.unwrap(),
            None
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn get_attachment_sync_issue_is_none_when_current_backend_has_no_binding() {
        // fingerprint 不匹配时不猜测当前库，返回 None 而非别的库状态
        let dir = temp_test_dir("binding-fingerprint-mismatch");
        let att_id = uuid::Uuid::new_v4().to_string();
        let object_key = format!("objects/v1/{att_id}");

        let service = create_test_service(&dir, TestBackend::new());
        service
            .db
            .upsert_pending_download(&AttachmentPendingDownload {
                attachment_id: att_id.clone(),
                file_library_id: "flib-stale".to_string(),
                object_key,
                remote_version: "v1".to_string(),
                created_at: 100,
            })
            .unwrap();
        service
            .db
            .upsert_file_library_binding(&FileLibraryBinding {
                file_library_id: "flib-stale".to_string(),
                database_library_id: TEST_DB_LIBRARY_ID.to_string(),
                backend_kind: "test".to_string(),
                backend_fingerprint: "stale-fingerprint".to_string(),
                protocol_version: 1,
                confirmed_at: 9_999,
            })
            .unwrap();

        assert_eq!(
            service.get_attachment_sync_issue(&att_id).await.unwrap(),
            None
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
