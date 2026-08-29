//! 附件同步服务模块
//!
//! 前端服务层：负责附件同步的业务流程编排。
//! 底层文件操作和远程协议由 `crates/file/` 实现。
//!
//! 解耦说明：不再依赖 `MainApp`。原 `app.sync_state.lock()` 改戳注入的
//! `sync_state`；`app.notify_ui_changed()` 改为注入的 `notify_ui` 闭包。

use crate::runtime::RUNTIME;
use crate::sync::progress::{SyncStateInner, SyncStatus};
use anyhow::{Result, anyhow};
use database::Database;
use file::{AttachmentBackend, LocalFileManager};
use log::{debug, error, info, warn};
use models::Attachment;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Mutex as AsyncMutex;
use unicode_normalization::UnicodeNormalization;

/// 附件同步服务
pub struct FileSyncService {
    db: Arc<Database>,
    file_manager: LocalFileManager,
    backend: Arc<tokio::sync::Mutex<Box<dyn AttachmentBackend>>>,
    on_demand: AtomicBool,
    /// 附件同步状态
    attachment_sync_status: Arc<AsyncMutex<SyncStatus>>,
    /// 待处理的远程重命名队列 (Key: Attachment ID, Value: Old Filename)
    pub pending_renames: Arc<std::sync::Mutex<HashMap<String, String>>>,
    /// 共享同步状态（由 `MainApp` 注入，跨线程）
    sync_state: Arc<Mutex<SyncStateInner>>,
    /// UI 变更通知闭包（注入）
    notify_ui: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for FileSyncService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSyncService")
            .field("db", &self.db)
            .field("file_manager", &self.file_manager)
            .field("on_demand", &self.on_demand.load(Ordering::Relaxed))
            .field("pending_renames", &"Arc<Mutex<HashMap>>")
            .finish()
    }
}

impl FileSyncService {
    pub fn new(
        db: Arc<Database>,
        file_manager: LocalFileManager,
        backend: Box<dyn AttachmentBackend>,
        pending_renames: Arc<std::sync::Mutex<HashMap<String, String>>>,
        sync_state: Arc<Mutex<SyncStateInner>>,
        notify_ui: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        info!("存储管理: [File] 正在初始化存储管理器...");

        let manager = Self {
            db,
            file_manager,
            backend: Arc::new(tokio::sync::Mutex::new(backend)),
            on_demand: AtomicBool::new(false),
            attachment_sync_status: Arc::new(AsyncMutex::new(SyncStatus::Idle)),
            pending_renames,
            sync_state,
            notify_ui,
        };

        info!("存储管理: [File] 初始化完成");
        manager
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

    async fn backend(&self) -> tokio::sync::MutexGuard<'_, Box<dyn AttachmentBackend>> {
        self.backend.lock().await
    }

    // --- 同步逻辑 ---

    /// 执行附件同步 (供外部手动触发)
    pub fn perform_attachments_sync(&self) -> Option<tokio::task::JoinHandle<()>> {
        let db = self.db.clone();
        let file_manager = self.file_manager.clone();
        let backend = self.backend.clone();
        let attachment_sync_status = self.attachment_sync_status.clone();
        let pending_renames = self.pending_renames.clone();
        let on_demand = AtomicBool::new(self.on_demand.load(Ordering::Relaxed));
        let sync_state = self.sync_state.clone();
        let notify_ui = self.notify_ui.clone();

        Some(tokio::spawn(async move {
            if !backend.lock().await.is_enabled() {
                debug!("存储管理: 后端未启用，跳过附件同步");
                return;
            }
            info!("存储管理: 开始执行附件同步 (上传+下载) ...");

            let service = FileSyncService {
                db,
                file_manager,
                backend,
                on_demand,
                attachment_sync_status,
                pending_renames,
                sync_state,
                notify_ui,
            };

            let _ = service.sync_local_to_remote_inner().await;
            let _ = service.sync_remote_to_local_inner().await;
            (service.notify_ui)();
        }))
    }

    /// 阶段 1: 上传本地变更到远程 (Push)
    pub async fn sync_local_to_remote(&self) -> Result<Vec<String>> {
        if !self.backend().await.is_enabled() {
            return Ok(Vec::new());
        }
        self.sync_local_to_remote_inner().await
    }

    async fn sync_local_to_remote_inner(&self) -> Result<Vec<String>> {
        // 尝试获取锁
        {
            let mut status = self.attachment_sync_status.lock().await;
            if *status == SyncStatus::Syncing {
                debug!("存储管理: [Upload] 另一次附件同步已在运行中，跳过本次请求");
                return Ok(Vec::new());
            }
            *status = SyncStatus::Syncing;
        }

        if let Ok(mut state) = self.sync_state.lock() {
            state.attachment_sync_status = SyncStatus::Syncing;
        }
        (self.notify_ui)();

        info!("存储管理: [Upload] 开始上传阶段...");

        let res = async {
            let local_attachments = self.db.get_all_attachments_include_deleted()?;
            let mut successfully_uploaded_ids = Vec::new();
            let mut had_dirty_uploads = false;

            let remote_files: std::collections::HashMap<String, String> = {
                let backend = self.backend().await;
                match backend.list().await {
                    Ok(entries) => entries.into_iter().map(|e| (e.name, e.version)).collect(),
                    Err(e) => {
                        debug!("存储管理: [Upload] 无法获取远程文件列表，将按 etag 判断: {e}");
                        std::collections::HashMap::new()
                    }
                }
            };

            for att in local_attachments {
                let backend = self.backend().await;

                // 优先处理重命名（无论 etag 状态）
                let old_name = if let Ok(mut map) = self.pending_renames.lock() {
                    map.remove(&att.id)
                } else {
                    None
                };
                if let Some(old_name) = old_name {
                    info!(
                        "存储管理: [Upload] 尝试远程重命名: {} -> {}",
                        old_name, att.file_name
                    );
                    had_dirty_uploads = true;
                    match backend.rename(old_name, att.file_name.clone()).await {
                        Ok(()) => {
                            info!("存储管理: [Upload] 远程重命名成功，跳过文件上传");
                            // 重命名不改变文件内容，保留原有 etag
                            // is_dirty 不变，留给 MySQL push
                            successfully_uploaded_ids.push(att.id.clone());
                            continue;
                        }
                        Err(e) => {
                            warn!("存储管理: [Upload] 远程重命名失败 (将回退到标准上传): {e}");
                        }
                    }
                }

                // 已删除附件
                if att.is_deleted {
                    if att.etag.is_some() {
                        info!("存储管理: [Upload] 同步删除远程文件 '{}'", att.file_name);
                        had_dirty_uploads = true;
                        match backend.delete(att.file_name.clone()).await {
                            Ok(()) => {
                                info!("存储管理: [Upload] 远程文件删除成功 '{}'", att.file_name);
                                // 立即清除 etag，避免下次同步重复删除
                                let mut updated_att = att.clone();
                                updated_att.etag = None;
                                self.db.insert_attachment(&updated_att)?;
                                successfully_uploaded_ids.push(att.id.clone());
                            }
                            Err(e) => {
                                error!(
                                    "存储管理: [Upload] 删除远程文件失败，将在下次同步重试 '{}': {}",
                                    att.file_name, e
                                );
                            }
                        }
                    } else {
                        debug!(
                            "存储管理: [Upload] 附件从未上传到远程，跳过远程删除 '{}'",
                            att.file_name
                        );
                        // 仍加入 IDs 以便 MySQL push 能处理该删除记录
                        successfully_uploaded_ids.push(att.id.clone());
                    }
                    continue;
                }

                // 未删除且已在远程（etag 或 hash 存在）：跳过上传，加入 IDs 以便 MySQL push
                if att.etag.is_some() || att.hash.is_some() {
                    debug!(
                        "存储管理: [Upload] 附件已在远程服务器上，跳过上传 '{}'",
                        att.file_name
                    );
                    successfully_uploaded_ids.push(att.id.clone());
                    continue;
                }

                // etag 不存在时，回退到文件名匹配（仅限非 dirty 附件）
                if !att.is_dirty {
                    let normalized_name: String = att.file_name.nfc().collect();
                    if let Some(remote_etag) = remote_files.get(&normalized_name) {
                        info!(
                            "存储管理: [Upload] 附件无 etag 但远程已存在同名文件，跳过上传 '{}'",
                            att.file_name
                        );
                        let mut updated_att = att.clone();
                        updated_att.etag = Some(remote_etag.clone());
                        self.db.insert_attachment(&updated_att)?;
                        successfully_uploaded_ids.push(att.id.clone());
                        continue;
                    }
                }

                // 未删除且不在远程：上传
                had_dirty_uploads = true;
                let local_file_path = std::path::Path::new(&att.file_path);
                if local_file_path.exists() {
                    info!("存储管理: [Upload] 正在上传 '{}'", att.file_name);
                    match backend
                        .upload(local_file_path.to_path_buf(), att.file_name.clone())
                        .await
                    {
                        Ok(new_etag) => {
                            let mut updated_att = att.clone();
                            updated_att.etag = new_etag;
                            let hash_path = local_file_path.to_path_buf();
                            updated_att.hash = match tokio::task::spawn_blocking(move || {
                                compute_file_hash(&hash_path)
                            })
                            .await
                            {
                                Ok(Ok(hash)) => Some(hash),
                                Ok(Err(e)) => {
                                    warn!(
                                        "存储管理: [Upload] '{}' 上传成功，但无法计算完整 SHA-256: {e}",
                                        att.file_name
                                    );
                                    None
                                }
                                Err(e) => {
                                    error!(
                                        "存储管理: [Upload] '{}' 上传成功，但 hash 任务异常结束: {e}",
                                        att.file_name
                                    );
                                    None
                                }
                            };
                            self.db.insert_attachment(&updated_att)?;
                            debug!(
                                "存储管理: [Upload] '{}' 上传成功，等待元数据同步",
                                att.file_name
                            );
                            successfully_uploaded_ids.push(att.id.clone());
                        }
                        Err(e) => {
                            error!("存储管理: [Upload] 上传失败 '{}': {}", att.file_name, e);
                        }
                    }
                } else {
                    warn!(
                        "存储管理: [Upload] 本地文件物理缺失，跳过上传: {}",
                        att.file_path
                    );
                }
            }

            if had_dirty_uploads && successfully_uploaded_ids.is_empty() {
                anyhow::bail!("后端连接异常：所有文件上传失败，请检查配置");
            }

            Ok::<Vec<String>, anyhow::Error>(successfully_uploaded_ids)
        };

        let result = res.await;

        {
            let mut status = self.attachment_sync_status.lock().await;
            match &result {
                Err(e) => {
                    let msg = e.to_string();
                    error!("存储管理: [Upload] 失败: {msg}");
                    let friendly =
                        crate::sync::error::format_sync_error(e, "远程附件存储（WebDAV）");
                    *status = SyncStatus::Error(friendly.clone());
                    if let Ok(mut state) = self.sync_state.lock() {
                        state.attachment_sync_status = SyncStatus::Error(friendly.clone());
                    }
                }
                Ok(success_ids) => {
                    info!(
                        "存储管理: [Upload] 上传阶段完成，成功上传 {} 个附件",
                        success_ids.len()
                    );
                    *status = SyncStatus::Idle;
                    if let Ok(mut state) = self.sync_state.lock() {
                        state.attachment_sync_status = SyncStatus::Idle;
                    }
                }
            }
        }
        (self.notify_ui)();

        result
    }

    /// 强制下载单个文件 (按需下载)
    pub async fn download_single_file(&self, attachment: &Attachment) -> Result<bool> {
        let backend = self.backend().await;
        if !backend.is_enabled() {
            return Err(anyhow::anyhow!("后端未启用"));
        }

        info!(
            "存储管理: [OnDemand] 正在按需下载 '{}'",
            attachment.file_name
        );

        let expected_dir = self.file_manager.get_attachments_dir();
        let expected_path = expected_dir.join(&attachment.file_name);

        let exists = expected_path.exists();
        info!("存储管理: [OnDemand] 检查本地文件: path={expected_path:?}, exists={exists}");

        if exists {
            info!("存储管理: [OnDemand] 检测到文件已在本地存在，执行路径修复与关联");
            let mut updated = attachment.clone();
            updated.file_path = expected_path.to_string_lossy().to_string();
            updated.is_dirty = false;

            match self.db.insert_attachment(&updated) {
                Ok(()) => {
                    info!("存储管理: [OnDemand] 数据库更新成功");
                    if let Ok(Some(lit)) = self.db.get_literature(&updated.literature_id) {
                        let _ = self.db.update_literature_row(&lit);
                    }
                }
                Err(e) => error!("存储管理: [OnDemand] 数据库更新失败: {e}"),
            }
            return Ok(true);
        }

        let db_path = std::path::Path::new(&attachment.file_path);
        let target_path = if db_path.as_os_str().is_empty()
            || !db_path.parent().is_some_and(std::path::Path::exists)
        {
            expected_path.as_path()
        } else {
            db_path
        };

        let new_version = backend
            .download(attachment.file_name.clone(), target_path.to_path_buf())
            .await?;

        if let Some(version) = new_version {
            let mut updated = attachment.clone();
            updated.etag = Some(version);
            updated.file_path = target_path.to_string_lossy().to_string();
            updated.is_dirty = false;
            self.db.insert_attachment(&updated)?;
            return Ok(true);
        }

        debug!(
            "存储管理: [OnDemand] 远程文件 '{}' 无更新版本",
            attachment.file_name
        );
        Ok(false)
    }

    /// 阶段 3: 从远程下载变更到本地 (Pull)
    pub async fn sync_remote_to_local(&self) -> Result<()> {
        let backend = self.backend().await;
        if !backend.is_enabled() {
            return Ok(());
        }
        // Drop backend lock before entering inner async block which takes its own lock
        drop(backend);
        self.sync_remote_to_local_inner().await
    }

    async fn sync_remote_to_local_inner(&self) -> Result<()> {
        // 尝试获取锁
        {
            let mut status = self.attachment_sync_status.lock().await;
            if *status == SyncStatus::Syncing {
                debug!("存储管理: [Download] 另一次同步占用中，跳过");
                return Ok(());
            }
            *status = SyncStatus::Syncing;
        }

        if let Ok(mut state) = self.sync_state.lock() {
            state.attachment_sync_status = SyncStatus::Syncing;
        }
        (self.notify_ui)();

        info!("存储管理: [Download] 开始下载阶段...");

        let res = async {
            // 1. 获取远程文件列表
            let remote_entries = self.backend().await.list().await;
            let remote_files: HashMap<String, String> = match remote_entries {
                Ok(entries) => entries.into_iter().map(|e| (e.name, e.version)).collect(),
                Err(e) => {
                    error!("存储管理: [Download] 获取远程列表失败 (将仅执行本地自愈): {e}");
                    HashMap::new()
                }
            };

            let local_attachments = self.db.get_all_attachments_include_deleted()?;

            for att_record in &local_attachments {
                let mut att = att_record.clone();

                // 路径自愈逻辑
                let local_path = std::path::Path::new(&att.file_path);
                let expected_dir = self.file_manager.get_attachments_dir();
                let expected_path = expected_dir.join(&att.file_name);

                let local_valid = local_path.exists() && local_path.is_file();

                if !local_valid && expected_path.exists() && expected_path.is_file() {
                    info!(
                        "存储管理: [Fix] 发现文件存在于标准目录，自动修复路径: {}",
                        att.file_name
                    );
                    att.file_path = expected_path.to_string_lossy().to_string();
                    att.is_dirty = false;

                    match self.db.insert_attachment(&att) {
                        Ok(()) => {
                            info!("存储管理: [Fix] 数据库路径修复成功");
                            if let Ok(Some(lit)) = self.db.get_literature(&att.literature_id) {
                                let _ = self.db.update_literature_row(&lit);
                            }
                            (self.notify_ui)();
                        }
                        Err(e) => error!("存储管理: [Fix] 数据库更新失败: {e}"),
                    }
                }

                if remote_files.is_empty() {
                    continue;
                }

                if att.etag.is_none() {
                    // 回退：用文件名匹配远程列表
                    let normalized_name = att.file_name.nfc().collect::<String>();
                    if let Some(r_version) = remote_files.get(&normalized_name) {
                        let local_path = std::path::Path::new(&att.file_path);
                        if !local_path.exists() && !att.is_deleted {
                            info!(
                                "存储管理: [Download] 附件无 etag，远程存在同名文件且本地缺失，正在下载 '{}'",
                                att.file_name
                            );
                            match self
                                .backend()
                                .await
                                .download(att.file_name.clone(), local_path.to_path_buf())
                                .await
                            {
                                Ok(new_version) => {
                                    let mut updated = att.clone();
                                    updated.etag = new_version.or_else(|| Some(r_version.clone()));
                                    updated.is_dirty = false;
                                    self.db.insert_attachment(&updated)?;
                                }
                                Err(e) => {
                                    error!(
                                        "存储管理: [Download] 下载失败 '{}': {}",
                                        att.file_name, e
                                    );
                                }
                            }
                        } else {
                            // 本地文件存在，仅补存 etag
                            let mut updated = att.clone();
                            updated.etag = Some(r_version.clone());
                            self.db.insert_attachment(&updated)?;
                        }
                    }
                    continue;
                }

                let normalized_name = att.file_name.nfc().collect::<String>();
                let remote_version = remote_files.get(&normalized_name);

                if let Some(r_version) = remote_version {
                    if att.is_deleted {
                        let has_active_reference = local_attachments.iter().any(|a| {
                            !a.is_deleted
                                && a.file_name.nfc().collect::<String>() == normalized_name
                                && a.id != att.id
                        });

                        if has_active_reference {
                            warn!(
                                "存储管理: [Download] 跳过删除远程文件 '{}'，因为本地仍有其他有效引用",
                                att.file_name
                            );
                            self.db.mark_attachment_synced(&att.id)?;
                        } else {
                            info!(
                                "存储管理: [Download] 补救删除远程残留文件 '{}'",
                                att.file_name
                            );
                            let _ = self.backend().await.delete(att.file_name.clone()).await;
                            self.db.mark_attachment_synced(&att.id)?;
                        }
                    } else {
                        let need_download = att.etag.as_ref() != Some(r_version);
                        let local_path = std::path::Path::new(&att.file_path);
                        let physical_missing = !local_path.exists();

                        if need_download || physical_missing {
                            if self.on_demand.load(Ordering::Relaxed) && physical_missing {
                                info!(
                                    "存储管理: [Download] 按需下载模式: 跳过自动下载 '{}', 仅更新元数据",
                                    att.file_name
                                );
                                let mut updated = att.clone();
                                updated.etag = Some(r_version.clone());
                                updated.is_dirty = false;
                                self.db.insert_attachment(&updated)?;
                            } else {
                                info!(
                                    "存储管理: [Download] 正在下载/更新 '{}' (原因: 版本变动或本地缺失)",
                                    att.file_name
                                );
                                match self
                                    .backend()
                                    .await
                                    .download(att.file_name.clone(), local_path.to_path_buf())
                                    .await
                                {
                                    Ok(new_version) => {
                                        let mut updated = att.clone();
                                        updated.etag = new_version;
                                        updated.is_dirty = false;
                                        self.db.insert_attachment(&updated)?;
                                    }
                                    Err(e) => {
                                        error!(
                                            "存储管理: [Download] 下载失败 '{}': {}",
                                            att.file_name, e
                                        );
                                    }
                                }
                            }
                        }
                    }
                } else {
                    if att.is_deleted {
                        // 双方都删除了
                    } else {
                        let local_path = std::path::Path::new(&att.file_path);
                        if local_path.exists() {
                            warn!(
                                "存储管理: [Download] 远程文件丢失，执行自愈上传 '{}'",
                                att.file_name
                            );
                            match self
                                .backend()
                                .await
                                .upload(local_path.to_path_buf(), att.file_name.clone())
                                .await
                            {
                                Ok(new_version) => {
                                    let mut updated = att.clone();
                                    updated.etag = new_version;
                                    updated.is_dirty = true;
                                    self.db.insert_attachment(&updated)?;
                                }
                                Err(e) => error!("自愈上传失败: {e}"),
                            }
                        } else {
                            error!("存储管理: [Download] 文件双重丢失: '{}'", att.file_name);
                        }
                    }
                }
            }

            info!(
                "存储管理: [Download] 下载阶段结束，共处理 {} 条附件记录",
                local_attachments.len()
            );

            Ok::<(), anyhow::Error>(())
        };

        let result = res.await;

        {
            let mut status = self.attachment_sync_status.lock().await;
            if let Err(e) = &result {
                let msg = e.to_string();
                error!("存储管理: [Download] 失败: {msg}");
                let friendly = crate::sync::error::format_sync_error(e, "远程附件存储（WebDAV）");
                *status = SyncStatus::Error(friendly.clone());
                if let Ok(mut state) = self.sync_state.lock() {
                    state.attachment_sync_status = SyncStatus::Error(friendly.clone());
                }
            } else {
                info!("存储管理: [Download] 下载阶段完成");
                *status = SyncStatus::Idle;
                if let Ok(mut state) = self.sync_state.lock() {
                    state.attachment_sync_status = SyncStatus::Idle;
                }
            }
        }
        (self.notify_ui)();

        result
    }

    pub async fn test_backend_config(&self, name: &str, config_json: &str) -> anyhow::Result<()> {
        info!("存储管理: [File] 正在测试后端配置 ({name})");
        let backend = file::create_backend(name, config_json);
        if !backend.is_enabled() {
            warn!("存储管理: [File] 后端未启用，跳过测试");
            anyhow::bail!("后端未启用，请先填写配置");
        }
        let handle = RUNTIME.spawn(async move { backend.test_connection().await });
        let result = handle.await.map_err(|e| anyhow!("任务失败: {e}"))?;
        match &result {
            Ok(()) => info!("存储管理: [File] 后端配置测试通过 ({name})"),
            Err(e) => error!("存储管理: [File] 后端配置测试失败 ({name}): {e}"),
        }
        result
    }

    /// 清空远程文件
    pub async fn clear_remote_files(&self) -> anyhow::Result<()> {
        info!("存储管理: 开始清空远程文件...");
        let entries = self.backend().await.list().await?;
        let total = entries.len();
        for (i, entry) in entries.into_iter().enumerate() {
            debug!(
                "存储管理: 正在删除远程文件 [{}/{}] '{}'",
                i + 1,
                total,
                entry.name
            );
            self.backend().await.delete(entry.name).await?;
        }
        info!("存储管理: 远程文件清空完成，共删除 {total} 个文件");
        Ok(())
    }

    /// 删除与本地软删除附件对应、且未被有效附件继续引用的远程文件。
    pub async fn purge_deleted_files(&self) -> anyhow::Result<usize> {
        let remote_entries = self.backend().await.list().await?;
        let local_attachments = self.db.get_all_attachments_include_deleted()?;
        let active_names: std::collections::HashSet<String> = local_attachments
            .iter()
            .filter(|att| !att.is_deleted)
            .map(|att| att.file_name.nfc().collect())
            .collect();
        let deleted_names: std::collections::HashSet<String> = local_attachments
            .iter()
            .filter(|att| att.is_deleted)
            .map(|att| att.file_name.nfc().collect())
            .collect();

        let mut deleted = 0;
        for entry in remote_entries {
            let normalized_name = entry.name.nfc().collect::<String>();
            if active_names.contains(&normalized_name) || !deleted_names.contains(&normalized_name)
            {
                continue;
            }
            info!(
                "存储管理: [Purge] 删除已软删除附件对应的远程文件 '{}'",
                entry.name
            );
            self.backend().await.delete(entry.name).await?;
            deleted += 1;
        }
        info!("存储管理: [Purge] 已删除 {deleted} 个远程孤立附件");
        Ok(deleted)
    }

    /// 删除远程文件（直接操作）
    pub async fn delete_remote_file(&self, filename: &str) -> Result<()> {
        let backend = self.backend().await;
        if !backend.is_enabled() {
            return Ok(());
        }
        info!("存储管理: [直接删除] 正在删除远程文件 '{filename}'");
        backend.delete(filename.to_string()).await?;
        Ok(())
    }

    /// 重命名远程文件（直接操作）
    pub async fn rename_remote_file(&self, old_name: &str, new_name: &str) -> Result<()> {
        let backend = self.backend().await;
        if !backend.is_enabled() {
            return Ok(());
        }
        info!("存储管理: [直接重命名] 正在重命名远程文件 '{old_name}' -> '{new_name}'");
        backend
            .rename(old_name.to_string(), new_name.to_string())
            .await?;
        Ok(())
    }
}

/// Bounded buffer size for full-file hashing. Hashing never loads a whole attachment into memory.
const HASH_BUFFER_SIZE: usize = 64 * 1024;

/// Computes a SHA-256 digest over every byte in an attachment.
///
/// The caller is responsible for running this blocking file I/O outside an async worker thread.
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
    use std::io::{Seek, SeekFrom, Write};

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
}
