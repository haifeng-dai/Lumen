use super::Database;
use rusqlite::{OptionalExtension, Result, params};
use serde::{Deserialize, Serialize};

/// `sync_meta` 中持久化文件同步最近一轮结果的固定 key。
const FILE_SYNC_LAST_SUMMARY_KEY: &str = "file_sync_last_summary";

/// 文件同步最近一轮结果的纯持久化 DTO。
///
/// database 只负责保存/读取与 JSON round-trip，不决定状态、不裁决计数。
/// `state` / `reason` 为 services 写入的脱敏类别，禁止出现路径、文件名、
/// object key、URL、版本、hash、账号或 Token。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSyncSummary {
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
    /// services 编码的脱敏状态类别（如 `complete` / `partial_failure` / `error`）
    pub state: String,
    /// services 编码的脱敏原因类别；成功轮次为 `None`
    pub reason: Option<String>,
    /// 本轮 run_id（与总轮次日志共用）
    pub run_id: String,
    /// 本轮所属文件库；身份未知时为 `None`
    pub file_library_id: Option<String>,
    pub updated_at: i64,
}

/// 附件确认状态快照，供 services 逐项裁决上传/删除门槛。
///
/// 由一次查询批量返回，禁止 N+1。database 不做确认裁决。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentSyncSnapshot {
    pub id: String,
    pub version: i64,
    pub synced_version: i64,
    pub is_dirty: bool,
    pub is_deleted: bool,
    pub file_path: String,
    pub file_name: String,
}

/// 文件资料库绑定信息
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLibraryBinding {
    pub file_library_id: String,
    pub database_library_id: String,
    pub backend_kind: String,
    pub backend_fingerprint: String,
    pub protocol_version: u32,
    pub confirmed_at: i64,
}

/// 附件在特定文件资料库下的基准线状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentFileBaseline {
    pub attachment_id: String,
    pub file_library_id: String,
    pub object_key: String,
    pub remote_version: String,
    pub local_sha256: String,
    pub local_presence: bool,
    pub last_success_at: i64,
}

/// 附件文件对象冲突 / 未知版本分歧记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentFileConflict {
    pub attachment_id: String,
    pub file_library_id: String,
    pub object_key: String,
    pub remote_version: String,
    pub local_sha256: String,
    pub reason: String,
    pub created_at: i64,
}

/// 按需下载登记记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentPendingDownload {
    pub attachment_id: String,
    pub file_library_id: String,
    pub object_key: String,
    pub remote_version: String,
    pub created_at: i64,
}

/// 纯函数：根据有效的 Attachment ID 生成唯一的对象键。
///
/// 格式为 `objects/v1/<attachment_id>`。
/// 必须是有效 UUID 字符串；空值、含路径分隔符、控制字符或非 UUID 返回错误。
pub fn object_key_from_attachment_id(attachment_id: &str) -> Result<String, rusqlite::Error> {
    if attachment_id.is_empty()
        || attachment_id.contains('/')
        || attachment_id.contains('\\')
        || attachment_id
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let parsed = uuid::Uuid::parse_str(attachment_id).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let canonical = parsed.to_string();
    if canonical != attachment_id.to_ascii_lowercase() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(format!("objects/v1/{canonical}"))
}

impl Database {
    /// 插入或更新文件库绑定
    pub fn upsert_file_library_binding(&self, binding: &FileLibraryBinding) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO file_library_bindings (
                    file_library_id, database_library_id, backend_kind,
                    backend_fingerprint, protocol_version, confirmed_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(file_library_id) DO UPDATE SET
                    database_library_id = excluded.database_library_id,
                    backend_kind = excluded.backend_kind,
                    backend_fingerprint = excluded.backend_fingerprint,
                    protocol_version = excluded.protocol_version,
                    confirmed_at = excluded.confirmed_at",
                params![
                    binding.file_library_id,
                    binding.database_library_id,
                    binding.backend_kind,
                    binding.backend_fingerprint,
                    binding.protocol_version,
                    binding.confirmed_at,
                ],
            )?;
            Ok(())
        })
    }

    /// 获取特定文件库绑定
    pub fn get_file_library_binding(
        &self,
        file_library_id: &str,
    ) -> Result<Option<FileLibraryBinding>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT file_library_id, database_library_id, backend_kind, backend_fingerprint, protocol_version, confirmed_at
                 FROM file_library_bindings WHERE file_library_id = ?1",
                [file_library_id],
                |row| {
                    Ok(FileLibraryBinding {
                        file_library_id: row.get(0)?,
                        database_library_id: row.get(1)?,
                        backend_kind: row.get(2)?,
                        backend_fingerprint: row.get(3)?,
                        protocol_version: row.get(4)?,
                        confirmed_at: row.get(5)?,
                    })
                },
            )
            .optional()
        })
    }

    /// 根据 backend_kind 与 backend_fingerprint 获取当前后端对应的文件资料库绑定
    pub fn get_file_library_binding_by_backend(
        &self,
        backend_kind: &str,
        backend_fingerprint: &str,
    ) -> Result<Option<FileLibraryBinding>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT file_library_id, database_library_id, backend_kind, backend_fingerprint, protocol_version, confirmed_at
                 FROM file_library_bindings WHERE backend_kind = ?1 AND backend_fingerprint = ?2",
                params![backend_kind, backend_fingerprint],
                |row| {
                    Ok(FileLibraryBinding {
                        file_library_id: row.get(0)?,
                        database_library_id: row.get(1)?,
                        backend_kind: row.get(2)?,
                        backend_fingerprint: row.get(3)?,
                        protocol_version: row.get(4)?,
                        confirmed_at: row.get(5)?,
                    })
                },
            )
            .optional()
        })
    }

    /// 插入或更新附件文件基准线
    ///
    /// 必须校验 object_key 与 attachment_id 的唯一映射一致，不一致则拒绝写入。
    pub fn upsert_attachment_file_baseline(&self, baseline: &AttachmentFileBaseline) -> Result<()> {
        let expected_key = object_key_from_attachment_id(&baseline.attachment_id)?;
        if baseline.object_key != expected_key {
            return Err(rusqlite::Error::InvalidQuery);
        }

        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO attachment_file_baselines (
                    attachment_id, file_library_id, object_key,
                    remote_version, local_sha256, local_presence, last_success_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT(attachment_id, file_library_id) DO UPDATE SET
                    object_key = excluded.object_key,
                    remote_version = excluded.remote_version,
                    local_sha256 = excluded.local_sha256,
                    local_presence = excluded.local_presence,
                    last_success_at = excluded.last_success_at",
                params![
                    baseline.attachment_id,
                    baseline.file_library_id,
                    baseline.object_key,
                    baseline.remote_version,
                    baseline.local_sha256,
                    baseline.local_presence,
                    baseline.last_success_at,
                ],
            )?;
            Ok(())
        })
    }

    /// 获取特定附件在特定文件库下的基准线
    pub fn get_attachment_file_baseline(
        &self,
        attachment_id: &str,
        file_library_id: &str,
    ) -> Result<Option<AttachmentFileBaseline>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT attachment_id, file_library_id, object_key, remote_version, local_sha256, local_presence, last_success_at
                 FROM attachment_file_baselines WHERE attachment_id = ?1 AND file_library_id = ?2",
                params![attachment_id, file_library_id],
                |row| {
                    Ok(AttachmentFileBaseline {
                        attachment_id: row.get(0)?,
                        file_library_id: row.get(1)?,
                        object_key: row.get(2)?,
                        remote_version: row.get(3)?,
                        local_sha256: row.get(4)?,
                        local_presence: row.get(5)?,
                        last_success_at: row.get(6)?,
                    })
                },
            )
            .optional()
        })
    }

    /// 删除特定附件在特定文件库下的基准线
    pub fn delete_attachment_file_baseline(
        &self,
        attachment_id: &str,
        file_library_id: &str,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM attachment_file_baselines WHERE attachment_id = ?1 AND file_library_id = ?2",
                params![attachment_id, file_library_id],
            )?;
            Ok(())
        })
    }

    /// 成功上传后原子记录当前 file library baseline
    pub fn apply_successful_upload(
        &self,
        attachment_id: &str,
        file_library_id: &str,
        object_key: &str,
        remote_version: &str,
        local_sha256: &str,
    ) -> Result<()> {
        let expected_key = object_key_from_attachment_id(attachment_id)?;
        if object_key != expected_key {
            return Err(rusqlite::Error::InvalidQuery);
        }

        let now = chrono::Utc::now().timestamp();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO attachment_file_baselines (
                    attachment_id, file_library_id, object_key,
                    remote_version, local_sha256, local_presence, last_success_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)
                ON CONFLICT(attachment_id, file_library_id) DO UPDATE SET
                    object_key = excluded.object_key,
                    remote_version = excluded.remote_version,
                    local_sha256 = excluded.local_sha256,
                    local_presence = 1,
                    last_success_at = excluded.last_success_at",
                params![
                    attachment_id,
                    file_library_id,
                    object_key,
                    remote_version,
                    local_sha256,
                    now,
                ],
            )?;
            Ok(())
        })
    }

    /// 成功下载后单事务原子更新 Attachment 本地 file_path/hash 并 upsert baseline，不修改 version、dirty、tombstone
    pub fn apply_successful_download(
        &self,
        attachment_id: &str,
        file_library_id: &str,
        object_key: &str,
        remote_version: &str,
        local_sha256: &str,
        new_file_path: &str,
    ) -> Result<()> {
        let expected_key = object_key_from_attachment_id(attachment_id)?;
        if object_key != expected_key {
            return Err(rusqlite::Error::InvalidQuery);
        }

        let now = chrono::Utc::now().timestamp();
        self.with_transaction(|tx| {
            tx.execute(
                "UPDATE attachments SET file_path = ?1, hash = ?2 WHERE id = ?3",
                params![new_file_path, local_sha256, attachment_id],
            )?;

            tx.execute(
                "INSERT INTO attachment_file_baselines (
                    attachment_id, file_library_id, object_key,
                    remote_version, local_sha256, local_presence, last_success_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)
                ON CONFLICT(attachment_id, file_library_id) DO UPDATE SET
                    object_key = excluded.object_key,
                    remote_version = excluded.remote_version,
                    local_sha256 = excluded.local_sha256,
                    local_presence = 1,
                    last_success_at = excluded.last_success_at",
                params![
                    attachment_id,
                    file_library_id,
                    object_key,
                    remote_version,
                    local_sha256,
                    now,
                ],
            )?;

            Ok(())
        })
    }

    /// 准备/即时恢复成功后单事务原子更新：
    /// 1. 更新 Attachment file_path/hash；
    /// 2. upsert 当前库 baseline；
    /// 3. 删除当前库 pending download；
    /// 4. 删除当前库已解决的 conflict 记录。
    pub fn apply_prepared_attachment_success(
        &self,
        attachment_id: &str,
        file_library_id: &str,
        object_key: &str,
        remote_version: &str,
        local_sha256: &str,
        new_file_path: &str,
    ) -> Result<()> {
        let expected_key = object_key_from_attachment_id(attachment_id)?;
        if object_key != expected_key {
            return Err(rusqlite::Error::InvalidQuery);
        }

        let now = chrono::Utc::now().timestamp();
        self.with_transaction(|tx| {
            tx.execute(
                "UPDATE attachments SET file_path = ?1, hash = ?2 WHERE id = ?3",
                params![new_file_path, local_sha256, attachment_id],
            )?;

            tx.execute(
                "INSERT INTO attachment_file_baselines (
                    attachment_id, file_library_id, object_key,
                    remote_version, local_sha256, local_presence, last_success_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)
                ON CONFLICT(attachment_id, file_library_id) DO UPDATE SET
                    object_key = excluded.object_key,
                    remote_version = excluded.remote_version,
                    local_sha256 = excluded.local_sha256,
                    local_presence = 1,
                    last_success_at = excluded.last_success_at",
                params![
                    attachment_id,
                    file_library_id,
                    object_key,
                    remote_version,
                    local_sha256,
                    now,
                ],
            )?;

            tx.execute(
                "DELETE FROM attachment_pending_downloads WHERE attachment_id = ?1 AND file_library_id = ?2",
                params![attachment_id, file_library_id],
            )?;

            tx.execute(
                "DELETE FROM attachment_file_conflicts WHERE attachment_id = ?1 AND file_library_id = ?2",
                params![attachment_id, file_library_id],
            )?;

            Ok(())
        })
    }

    /// 成功删除远端对象或 404 时原子清理当前 file library baseline，不修改 Attachment tombstone
    pub fn apply_successful_delete(
        &self,
        attachment_id: &str,
        file_library_id: &str,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM attachment_file_baselines WHERE attachment_id = ?1 AND file_library_id = ?2",
                params![attachment_id, file_library_id],
            )?;
            Ok(())
        })
    }

    /// 已确认的远端删除完成后，原子清理当前 file library 的 baseline、pending 和 conflict。
    ///
    /// 只影响 `file_library_id` 对应的库，不跨 A/B 清理，也不修改 Attachment tombstone。
    pub fn apply_confirmed_remote_delete(
        &self,
        attachment_id: &str,
        file_library_id: &str,
    ) -> Result<()> {
        self.with_transaction(|tx| {
            tx.execute(
                "DELETE FROM attachment_file_baselines WHERE attachment_id = ?1 AND file_library_id = ?2",
                params![attachment_id, file_library_id],
            )?;
            tx.execute(
                "DELETE FROM attachment_pending_downloads WHERE attachment_id = ?1 AND file_library_id = ?2",
                params![attachment_id, file_library_id],
            )?;
            tx.execute(
                "DELETE FROM attachment_file_conflicts WHERE attachment_id = ?1 AND file_library_id = ?2",
                params![attachment_id, file_library_id],
            )?;
            Ok(())
        })
    }

    /// 读取文件同步最近一轮结果。
    ///
    /// 不存在返回 `None`；JSON 损坏返回 `Error`，绝不把损坏当作“没有历史结果”。
    pub fn get_file_sync_summary(&self) -> Result<Option<FileSyncSummary>> {
        self.with_conn(|conn| {
            let value = conn
                .query_row(
                    "SELECT value FROM sync_meta WHERE key = ?1",
                    [FILE_SYNC_LAST_SUMMARY_KEY],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            value
                .map(|json| serde_json::from_str(&json).map_err(|_| rusqlite::Error::InvalidQuery))
                .transpose()
        })
    }

    /// 写入文件同步最近一轮结果（database 不决定状态与计数）。
    pub fn set_file_sync_summary(&self, summary: &FileSyncSummary) -> Result<()> {
        let value = serde_json::to_string(summary).map_err(|_| rusqlite::Error::InvalidQuery)?;
        self.set_sync_meta(FILE_SYNC_LAST_SUMMARY_KEY, &value)
    }

    /// 一次读取事务批量返回附件确认状态快照，禁止 N+1 查询。
    ///
    /// 只返回参与文件同步所需字段；database 不计算 `database_confirmed`。
    pub fn attachment_sync_snapshots(&self) -> Result<Vec<AttachmentSyncSnapshot>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, version, synced_version, is_dirty, is_deleted, file_path, file_name
                 FROM attachments",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(AttachmentSyncSnapshot {
                    id: row.get(0)?,
                    version: row.get(1)?,
                    synced_version: row.get(2)?,
                    is_dirty: row.get::<_, i64>(3)? != 0,
                    is_deleted: row.get::<_, i64>(4)? != 0,
                    file_path: row.get(5)?,
                    file_name: row.get(6)?,
                })
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    /// 插入或更新附件文件对象冲突 / 未知版本分歧记录
    pub fn upsert_file_conflict(&self, conflict: &AttachmentFileConflict) -> Result<()> {
        let expected_key = object_key_from_attachment_id(&conflict.attachment_id)?;
        if conflict.object_key != expected_key {
            return Err(rusqlite::Error::InvalidQuery);
        }

        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO attachment_file_conflicts (
                    attachment_id, file_library_id, object_key,
                    remote_version, local_sha256, reason, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT(attachment_id, file_library_id, reason) DO UPDATE SET
                    object_key = excluded.object_key,
                    remote_version = excluded.remote_version,
                    local_sha256 = excluded.local_sha256,
                    created_at = excluded.created_at",
                params![
                    conflict.attachment_id,
                    conflict.file_library_id,
                    conflict.object_key,
                    conflict.remote_version,
                    conflict.local_sha256,
                    conflict.reason,
                    conflict.created_at,
                ],
            )?;
            Ok(())
        })
    }

    /// 删除特定附件、特定文件库、特定原因的冲突记录
    pub fn delete_file_conflict(
        &self,
        attachment_id: &str,
        file_library_id: &str,
        reason: &str,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM attachment_file_conflicts WHERE attachment_id = ?1 AND file_library_id = ?2 AND reason = ?3",
                params![attachment_id, file_library_id, reason],
            )?;
            Ok(())
        })
    }

    /// 删除特定附件、特定文件库下的所有冲突记录
    pub fn delete_all_file_conflicts(
        &self,
        attachment_id: &str,
        file_library_id: &str,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM attachment_file_conflicts WHERE attachment_id = ?1 AND file_library_id = ?2",
                params![attachment_id, file_library_id],
            )?;
            Ok(())
        })
    }

    /// 列出特定文件库下的所有文件冲突记录
    pub fn list_file_conflicts(
        &self,
        file_library_id: &str,
    ) -> Result<Vec<AttachmentFileConflict>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT attachment_id, file_library_id, object_key, remote_version, local_sha256, reason, created_at
                 FROM attachment_file_conflicts WHERE file_library_id = ?1 ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map([file_library_id], |row| {
                Ok(AttachmentFileConflict {
                    attachment_id: row.get(0)?,
                    file_library_id: row.get(1)?,
                    object_key: row.get(2)?,
                    remote_version: row.get(3)?,
                    local_sha256: row.get(4)?,
                    reason: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?;
            let mut conflicts = Vec::new();
            for item in rows {
                conflicts.push(item?);
            }
            Ok(conflicts)
        })
    }

    /// 获取特定附件在特定文件库下的所有冲突记录（如 file_conflict 与 unknown_divergence）
    pub fn get_attachment_file_conflicts(
        &self,
        attachment_id: &str,
        file_library_id: &str,
    ) -> Result<Vec<AttachmentFileConflict>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT attachment_id, file_library_id, object_key, remote_version, local_sha256, reason, created_at
                 FROM attachment_file_conflicts WHERE attachment_id = ?1 AND file_library_id = ?2 ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map(params![attachment_id, file_library_id], |row| {
                Ok(AttachmentFileConflict {
                    attachment_id: row.get(0)?,
                    file_library_id: row.get(1)?,
                    object_key: row.get(2)?,
                    remote_version: row.get(3)?,
                    local_sha256: row.get(4)?,
                    reason: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?;
            let mut conflicts = Vec::new();
            for item in rows {
                conflicts.push(item?);
            }
            Ok(conflicts)
        })
    }

    /// 插入或更新按需下载登记记录
    pub fn upsert_pending_download(&self, pending: &AttachmentPendingDownload) -> Result<()> {
        let expected_key = object_key_from_attachment_id(&pending.attachment_id)?;
        if pending.object_key != expected_key {
            return Err(rusqlite::Error::InvalidQuery);
        }

        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO attachment_pending_downloads (
                    attachment_id, file_library_id, object_key,
                    remote_version, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(attachment_id, file_library_id) DO UPDATE SET
                    object_key = excluded.object_key,
                    remote_version = excluded.remote_version,
                    created_at = excluded.created_at",
                params![
                    pending.attachment_id,
                    pending.file_library_id,
                    pending.object_key,
                    pending.remote_version,
                    pending.created_at,
                ],
            )?;
            Ok(())
        })
    }

    /// 获取特定附件在特定文件库下的按需下载登记
    pub fn get_pending_download(
        &self,
        attachment_id: &str,
        file_library_id: &str,
    ) -> Result<Option<AttachmentPendingDownload>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT attachment_id, file_library_id, object_key, remote_version, created_at
                 FROM attachment_pending_downloads WHERE attachment_id = ?1 AND file_library_id = ?2",
                params![attachment_id, file_library_id],
                |row| {
                    Ok(AttachmentPendingDownload {
                        attachment_id: row.get(0)?,
                        file_library_id: row.get(1)?,
                        object_key: row.get(2)?,
                        remote_version: row.get(3)?,
                        created_at: row.get(4)?,
                    })
                },
            )
            .optional()
        })
    }

    /// 删除特定附件在特定文件库下的按需下载登记
    pub fn delete_pending_download(
        &self,
        attachment_id: &str,
        file_library_id: &str,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM attachment_pending_downloads WHERE attachment_id = ?1 AND file_library_id = ?2",
                params![attachment_id, file_library_id],
            )?;
            Ok(())
        })
    }

    /// 仅用于失败注入测试：把既有按需下载登记重新绑定到另一个附件，
    /// 构造“object_key 与 attachment_id 不匹配”的本地状态。
    pub fn rebind_pending_download_for_test(
        &self,
        from_attachment_id: &str,
        to_attachment_id: &str,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE attachment_pending_downloads SET attachment_id = ?1 WHERE attachment_id = ?2",
                params![to_attachment_id, from_attachment_id],
            )?;
            Ok(())
        })
    }

    /// 列出特定文件库下的所有按需下载登记记录
    pub fn list_pending_downloads(
        &self,
        file_library_id: &str,
    ) -> Result<Vec<AttachmentPendingDownload>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT attachment_id, file_library_id, object_key, remote_version, created_at
                 FROM attachment_pending_downloads WHERE file_library_id = ?1 ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map([file_library_id], |row| {
                Ok(AttachmentPendingDownload {
                    attachment_id: row.get(0)?,
                    file_library_id: row.get(1)?,
                    object_key: row.get(2)?,
                    remote_version: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?;
            let mut pendings = Vec::new();
            for item in rows {
                pendings.push(item?);
            }
            Ok(pendings)
        })
    }

    /// 单事务原子删除特定文件库的所有状态（包括 binding、baselines、conflicts 与 pending_downloads）
    pub fn delete_file_library_state(&self, file_library_id: &str) -> Result<()> {
        self.with_transaction(|tx| {
            tx.execute(
                "DELETE FROM file_library_bindings WHERE file_library_id = ?1",
                [file_library_id],
            )?;
            tx.execute(
                "DELETE FROM attachment_file_baselines WHERE file_library_id = ?1",
                [file_library_id],
            )?;
            tx.execute(
                "DELETE FROM attachment_file_conflicts WHERE file_library_id = ?1",
                [file_library_id],
            )?;
            tx.execute(
                "DELETE FROM attachment_pending_downloads WHERE file_library_id = ?1",
                [file_library_id],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_db() -> Database {
        Database::new(":memory:").unwrap()
    }

    #[test]
    fn valid_uuid_object_key_is_stable() {
        let id = "550e8400-e29b-41d4-a716-446655440000";
        let key1 = object_key_from_attachment_id(id).unwrap();
        let key2 = object_key_from_attachment_id(id).unwrap();
        assert_eq!(key1, "objects/v1/550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(key1, key2);
    }

    #[test]
    fn different_attachment_ids_produce_different_object_keys() {
        let id1 = "550e8400-e29b-41d4-a716-446655440000";
        let id2 = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        let key1 = object_key_from_attachment_id(id1).unwrap();
        let key2 = object_key_from_attachment_id(id2).unwrap();
        assert_ne!(key1, key2);
    }

    #[test]
    fn object_key_rejects_empty_path_like_and_non_uuid_ids() {
        assert!(object_key_from_attachment_id("").is_err());
        assert!(object_key_from_attachment_id("   ").is_err());
        assert!(object_key_from_attachment_id("not-a-uuid").is_err());
        assert!(object_key_from_attachment_id("../550e8400-e29b-41d4-a716-446655440000").is_err());
        assert!(object_key_from_attachment_id("550e8400-e29b-41d4-a716-446655440000/sub").is_err());
        assert!(object_key_from_attachment_id("550e8400-e29b-41d4-a716-446655440000\0").is_err());
        assert!(object_key_from_attachment_id("550e8400-e29b-41d4-a716-446655440000\n").is_err());
    }

    #[test]
    fn object_key_does_not_depend_on_display_filename_or_content_hash() {
        let id = "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11";
        let key = object_key_from_attachment_id(id).unwrap();
        assert_eq!(key, "objects/v1/a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11");
        assert!(!key.contains(".pdf"));
        assert!(!key.contains("sha256"));
    }

    #[test]
    fn baseline_is_scoped_by_file_library_id() {
        let db = create_test_db();
        let att_id = "550e8400-e29b-41d4-a716-446655440000";
        let object_key = format!("objects/v1/{att_id}");

        let baseline_a = AttachmentFileBaseline {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-A".to_string(),
            object_key: object_key.clone(),
            remote_version: "v-a1".to_string(),
            local_sha256: "hash-a".to_string(),
            local_presence: true,
            last_success_at: 1000,
        };

        let baseline_b = AttachmentFileBaseline {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-B".to_string(),
            object_key: object_key.clone(),
            remote_version: "v-b1".to_string(),
            local_sha256: "hash-b".to_string(),
            local_presence: false,
            last_success_at: 2000,
        };

        db.upsert_attachment_file_baseline(&baseline_a).unwrap();
        db.upsert_attachment_file_baseline(&baseline_b).unwrap();

        let read_a = db
            .get_attachment_file_baseline(att_id, "flib-A")
            .unwrap()
            .unwrap();
        let read_b = db
            .get_attachment_file_baseline(att_id, "flib-B")
            .unwrap()
            .unwrap();

        assert_eq!(read_a.remote_version, "v-a1");
        assert_eq!(read_a.local_sha256, "hash-a");
        assert!(read_a.local_presence);

        assert_eq!(read_b.remote_version, "v-b1");
        assert_eq!(read_b.local_sha256, "hash-b");
        assert!(!read_b.local_presence);
    }

    #[test]
    fn delete_file_library_state_removes_only_target_library_and_its_baselines() {
        let db = create_test_db();
        let att1 = "550e8400-e29b-41d4-a716-446655440000";
        let att2 = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";

        let binding_a = FileLibraryBinding {
            file_library_id: "flib-A".to_string(),
            database_library_id: "dblib-1".to_string(),
            backend_kind: "webdav".to_string(),
            backend_fingerprint: "fp-a".to_string(),
            protocol_version: 1,
            confirmed_at: 1000,
        };
        let binding_b = FileLibraryBinding {
            file_library_id: "flib-B".to_string(),
            database_library_id: "dblib-1".to_string(),
            backend_kind: "google_drive".to_string(),
            backend_fingerprint: "fp-b".to_string(),
            protocol_version: 1,
            confirmed_at: 2000,
        };

        db.upsert_file_library_binding(&binding_a).unwrap();
        db.upsert_file_library_binding(&binding_b).unwrap();

        db.upsert_attachment_file_baseline(&AttachmentFileBaseline {
            attachment_id: att1.to_string(),
            file_library_id: "flib-A".to_string(),
            object_key: format!("objects/v1/{att1}"),
            remote_version: "v-a1".to_string(),
            local_sha256: "hash1".to_string(),
            local_presence: true,
            last_success_at: 1000,
        })
        .unwrap();

        db.upsert_attachment_file_baseline(&AttachmentFileBaseline {
            attachment_id: att2.to_string(),
            file_library_id: "flib-A".to_string(),
            object_key: format!("objects/v1/{att2}"),
            remote_version: "v-a2".to_string(),
            local_sha256: "hash2".to_string(),
            local_presence: true,
            last_success_at: 1000,
        })
        .unwrap();

        db.upsert_attachment_file_baseline(&AttachmentFileBaseline {
            attachment_id: att1.to_string(),
            file_library_id: "flib-B".to_string(),
            object_key: format!("objects/v1/{att1}"),
            remote_version: "v-b1".to_string(),
            local_sha256: "hash1".to_string(),
            local_presence: true,
            last_success_at: 2000,
        })
        .unwrap();

        // Delete flib-A
        db.delete_file_library_state("flib-A").unwrap();

        // Verify flib-A is completely gone
        assert!(db.get_file_library_binding("flib-A").unwrap().is_none());
        assert!(
            db.get_attachment_file_baseline(att1, "flib-A")
                .unwrap()
                .is_none()
        );
        assert!(
            db.get_attachment_file_baseline(att2, "flib-A")
                .unwrap()
                .is_none()
        );

        // Verify flib-B is untouched
        assert!(db.get_file_library_binding("flib-B").unwrap().is_some());
        assert!(
            db.get_attachment_file_baseline(att1, "flib-B")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn baseline_rejects_object_key_for_another_attachment() {
        let db = create_test_db();
        let att_id = "550e8400-e29b-41d4-a716-446655440000";
        let wrong_key = "objects/v1/6ba7b810-9dad-11d1-80b4-00c04fd430c8";

        let baseline = AttachmentFileBaseline {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-A".to_string(),
            object_key: wrong_key.to_string(),
            remote_version: "v1".to_string(),
            local_sha256: "hash".to_string(),
            local_presence: true,
            last_success_at: 1000,
        };

        let result = db.upsert_attachment_file_baseline(&baseline);
        assert!(result.is_err());
    }

    #[test]
    fn delete_file_library_state_is_atomic_on_failure() {
        let db = create_test_db();
        let att_id = "550e8400-e29b-41d4-a716-446655440000";

        let binding = FileLibraryBinding {
            file_library_id: "flib-atomic".to_string(),
            database_library_id: "dblib-1".to_string(),
            backend_kind: "webdav".to_string(),
            backend_fingerprint: "fp".to_string(),
            protocol_version: 1,
            confirmed_at: 1000,
        };
        db.upsert_file_library_binding(&binding).unwrap();

        let baseline = AttachmentFileBaseline {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-atomic".to_string(),
            object_key: format!("objects/v1/{att_id}"),
            remote_version: "v1".to_string(),
            local_sha256: "hash".to_string(),
            local_presence: true,
            last_success_at: 1000,
        };
        db.upsert_attachment_file_baseline(&baseline).unwrap();

        // 注入 Trigger，在删除 attachment_file_baselines 时强制抛出错误（模拟第二步失败）
        db.with_conn(|conn| {
            conn.execute(
                "CREATE TRIGGER fail_baselines_delete BEFORE DELETE ON attachment_file_baselines
                 BEGIN
                     SELECT RAISE(ABORT, 'injected baseline delete failure');
                 END;",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        // 直接调用 delete_file_library_state API
        let result = db.delete_file_library_state("flib-atomic");
        assert!(
            result.is_err(),
            "delete_file_library_state 应该在第二步失败时返回错误"
        );

        // 验证原子回滚：第一步删除的 binding 和第二步尝试删除的 baseline 必须全部完好无损地存在！
        let binding_after = db.get_file_library_binding("flib-atomic").unwrap();
        assert!(
            binding_after.is_some(),
            "第一步删除的 binding 必须被事务原子回滚保留"
        );
        assert_eq!(binding_after.unwrap(), binding);

        let baseline_after = db
            .get_attachment_file_baseline(att_id, "flib-atomic")
            .unwrap();
        assert!(baseline_after.is_some(), "baseline 必须完好保留");
        assert_eq!(baseline_after.unwrap(), baseline);
    }

    #[test]
    fn apply_successful_operations_test() {
        let db = create_test_db();
        let att_id = "550e8400-e29b-41d4-a716-446655440000";
        let file_lib_id = "flib-test-ops";
        let obj_key = format!("objects/v1/{att_id}");

        // 插入测试 Attachment
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO attachments (
                    id, literature_id, file_name, file_path, file_size,
                    mime_type, hash, is_dirty, version, is_deleted,
                    created_at, updated_at
                ) VALUES (?1, 'lit-1', 'paper.pdf', '/old/path/paper.pdf', 1024, 'application/pdf', 'oldhash', 0, 1, 0, 100, 100)",
                params![att_id],
            )?;
            Ok(())
        })
        .unwrap();

        // 1. apply_successful_upload
        db.apply_successful_upload(att_id, file_lib_id, &obj_key, "v-etag-1", "sha256-1")
            .unwrap();
        let bl = db
            .get_attachment_file_baseline(att_id, file_lib_id)
            .unwrap()
            .unwrap();
        assert_eq!(bl.remote_version, "v-etag-1");
        assert_eq!(bl.local_sha256, "sha256-1");
        assert!(bl.local_presence);

        // 2. apply_successful_download
        db.apply_successful_download(
            att_id,
            file_lib_id,
            &obj_key,
            "v-etag-2",
            "sha256-2",
            "/new/path/paper.pdf",
        )
        .unwrap();
        let bl2 = db
            .get_attachment_file_baseline(att_id, file_lib_id)
            .unwrap()
            .unwrap();
        assert_eq!(bl2.remote_version, "v-etag-2");
        assert_eq!(bl2.local_sha256, "sha256-2");

        // 检查 Attachment 表中 file_path 和 hash 更新，但 is_dirty, version, is_deleted 未改变
        let att = db.get_attachment(att_id).unwrap().unwrap();
        assert_eq!(att.file_path, "/new/path/paper.pdf");
        assert_eq!(att.hash.as_deref(), Some("sha256-2"));
        assert!(!att.is_dirty);
        assert_eq!(att.version, 1);
        assert!(!att.is_deleted);

        // 3. apply_successful_delete
        db.apply_successful_delete(att_id, file_lib_id).unwrap();
        assert!(
            db.get_attachment_file_baseline(att_id, file_lib_id)
                .unwrap()
                .is_none()
        );
        // Attachment 依然保留
        assert!(db.get_attachment(att_id).unwrap().is_some());
    }

    #[test]
    fn apply_prepared_attachment_success_is_atomic_and_preserves_sync_fields() {
        let db = create_test_db();
        let att_id = "550e8400-e29b-41d4-a716-446655440000";
        let obj_key = format!("objects/v1/{att_id}");

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO attachments (
                    id, literature_id, file_name, file_path, file_size,
                    mime_type, hash, is_dirty, version, is_deleted,
                    created_at, updated_at
                ) VALUES (?1, 'lit-1', 'paper.pdf', '/old/path/paper.pdf', 1024, 'application/pdf', 'oldhash', 1, 7, 0, 100, 100)",
                params![att_id],
            )?;
            Ok(())
        })
        .unwrap();

        // 同时在两个库中登记待处理状态，并写入过期冲突
        db.upsert_pending_download(&AttachmentPendingDownload {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-A".to_string(),
            object_key: obj_key.clone(),
            remote_version: "v1".to_string(),
            created_at: 100,
        })
        .unwrap();
        db.upsert_pending_download(&AttachmentPendingDownload {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-B".to_string(),
            object_key: obj_key.clone(),
            remote_version: "v1".to_string(),
            created_at: 100,
        })
        .unwrap();
        db.upsert_file_conflict(&AttachmentFileConflict {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-A".to_string(),
            object_key: obj_key.clone(),
            remote_version: "v1".to_string(),
            local_sha256: "oldhash".to_string(),
            reason: "unknown_divergence".to_string(),
            created_at: 100,
        })
        .unwrap();

        db.apply_prepared_attachment_success(
            att_id,
            "flib-A",
            &obj_key,
            "v2",
            "sha256-new",
            "/new/path/paper.pdf",
        )
        .unwrap();

        let baseline = db
            .get_attachment_file_baseline(att_id, "flib-A")
            .unwrap()
            .unwrap();
        assert_eq!(baseline.remote_version, "v2");
        assert_eq!(baseline.local_sha256, "sha256-new");

        // 当前库的 pending 与 conflict 被清除
        assert!(db.get_pending_download(att_id, "flib-A").unwrap().is_none());
        assert!(
            db.get_attachment_file_conflicts(att_id, "flib-A")
                .unwrap()
                .is_empty()
        );
        // 另一个文件库的状态不受影响
        assert!(db.get_pending_download(att_id, "flib-B").unwrap().is_some());

        // 路径修复不得改变 version / is_dirty / is_deleted
        let att = db.get_attachment(att_id).unwrap().unwrap();
        assert_eq!(att.file_path, "/new/path/paper.pdf");
        assert_eq!(att.hash.as_deref(), Some("sha256-new"));
        assert!(att.is_dirty);
        assert_eq!(att.version, 7);
        assert!(!att.is_deleted);
    }

    #[test]
    fn apply_prepared_attachment_success_rolls_back_when_conflict_delete_fails() {
        let db = create_test_db();
        let att_id = "550e8400-e29b-41d4-a716-446655440000";
        let obj_key = format!("objects/v1/{att_id}");

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO attachments (
                    id, literature_id, file_name, file_path, file_size,
                    mime_type, hash, is_dirty, version, is_deleted,
                    created_at, updated_at
                ) VALUES (?1, 'lit-1', 'paper.pdf', '/old/path/paper.pdf', 1024, 'application/pdf', 'oldhash', 0, 1, 0, 100, 100)",
                params![att_id],
            )?;
            Ok(())
        })
        .unwrap();
        db.upsert_pending_download(&AttachmentPendingDownload {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-atomic".to_string(),
            object_key: obj_key.clone(),
            remote_version: "v1".to_string(),
            created_at: 100,
        })
        .unwrap();

        db.upsert_file_conflict(&AttachmentFileConflict {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-atomic".to_string(),
            object_key: obj_key.clone(),
            remote_version: "v1".to_string(),
            local_sha256: "oldhash".to_string(),
            reason: "unknown_divergence".to_string(),
            created_at: 100,
        })
        .unwrap();

        // 注入确定性失败：删除冲突记录时触发器报错
        db.with_conn(|conn| {
            conn.execute(
                "CREATE TRIGGER IF NOT EXISTS inject_conflict_delete_failure
                 BEFORE DELETE ON attachment_file_conflicts
                 BEGIN
                     SELECT RAISE(ABORT, 'injected conflict delete failure');
                 END;",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let result = db.apply_prepared_attachment_success(
            att_id,
            "flib-atomic",
            &obj_key,
            "v2",
            "sha256-new",
            "/new/path/paper.pdf",
        );
        assert!(
            result.is_err(),
            "冲突清理失败必须让整个事务失败，不能返回成功"
        );

        // 事务回滚：pending 保留、baseline 不建立、Attachment 路径不变
        assert!(
            db.get_pending_download(att_id, "flib-atomic")
                .unwrap()
                .is_some(),
            "pending 必须保留"
        );
        assert!(
            db.get_attachment_file_baseline(att_id, "flib-atomic")
                .unwrap()
                .is_none(),
            "baseline 必须未建立"
        );
        let att = db.get_attachment(att_id).unwrap().unwrap();
        assert_eq!(att.file_path, "/old/path/paper.pdf");
    }

    #[test]
    fn file_conflict_crud_and_reason_coexistence() {
        let db = create_test_db();
        let att_id = "550e8400-e29b-41d4-a716-446655440000";
        let obj_key = format!("objects/v1/{att_id}");

        let conflict1 = AttachmentFileConflict {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-A".to_string(),
            object_key: obj_key.clone(),
            remote_version: "v1".to_string(),
            local_sha256: "hash1".to_string(),
            reason: "file_conflict".to_string(),
            created_at: 100,
        };

        // 1. upsert 后 list 能取回且字段一致
        db.upsert_file_conflict(&conflict1).unwrap();
        let list1 = db.list_file_conflicts("flib-A").unwrap();
        assert_eq!(list1.len(), 1);
        assert_eq!(list1[0], conflict1);

        // 再次 upsert 同 reason 覆盖（数量不变）
        let conflict1_updated = AttachmentFileConflict {
            remote_version: "v2".to_string(),
            local_sha256: "hash2".to_string(),
            created_at: 200,
            ..conflict1.clone()
        };
        db.upsert_file_conflict(&conflict1_updated).unwrap();
        let list1_updated = db.list_file_conflicts("flib-A").unwrap();
        assert_eq!(list1_updated.len(), 1);
        assert_eq!(list1_updated[0], conflict1_updated);

        // 2. 不同 reason 可并存（file_conflict 与 unknown_divergence 各一条）
        let conflict2 = AttachmentFileConflict {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-A".to_string(),
            object_key: obj_key.clone(),
            remote_version: "v3".to_string(),
            local_sha256: "".to_string(),
            reason: "unknown_divergence".to_string(),
            created_at: 300,
        };
        db.upsert_file_conflict(&conflict2).unwrap();
        let list2 = db.list_file_conflicts("flib-A").unwrap();
        assert_eq!(list2.len(), 2);
        assert_eq!(list2[0], conflict1_updated);
        assert_eq!(list2[1], conflict2);

        // 3. delete_file_conflict 删除单条；删除不存在记录返回 Ok 无副作用
        db.delete_file_conflict(att_id, "flib-A", "file_conflict")
            .unwrap();
        let list3 = db.list_file_conflicts("flib-A").unwrap();
        assert_eq!(list3.len(), 1);
        assert_eq!(list3[0], conflict2);

        db.delete_file_conflict(att_id, "flib-A", "non_existent_reason")
            .unwrap();
        let list3_again = db.list_file_conflicts("flib-A").unwrap();
        assert_eq!(list3_again.len(), 1);
    }

    #[test]
    fn pending_download_crud_and_overwrite() {
        let db = create_test_db();
        let att_id = "550e8400-e29b-41d4-a716-446655440000";
        let obj_key = format!("objects/v1/{att_id}");

        let pending = AttachmentPendingDownload {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-A".to_string(),
            object_key: obj_key.clone(),
            remote_version: "v1".to_string(),
            created_at: 100,
        };

        // 4. upsert_pending_download 后 get_pending_download 取回；再次 upsert 覆盖
        db.upsert_pending_download(&pending).unwrap();
        let got = db.get_pending_download(att_id, "flib-A").unwrap();
        assert_eq!(got, Some(pending.clone()));

        let pending_updated = AttachmentPendingDownload {
            remote_version: "v2".to_string(),
            created_at: 200,
            ..pending.clone()
        };
        db.upsert_pending_download(&pending_updated).unwrap();
        let got_updated = db.get_pending_download(att_id, "flib-A").unwrap();
        assert_eq!(got_updated, Some(pending_updated));

        // 5. delete_pending_download 删除后 get_pending_download 返回 None
        db.delete_pending_download(att_id, "flib-A").unwrap();
        let got_none = db.get_pending_download(att_id, "flib-A").unwrap();
        assert_eq!(got_none, None);
    }

    #[test]
    fn list_queries_are_scoped_by_file_library_id() {
        let db = create_test_db();
        let att_id = "550e8400-e29b-41d4-a716-446655440000";
        let obj_key = format!("objects/v1/{att_id}");

        // 6. list_pending_downloads / list_file_conflicts 限定在指定 file_library_id，不跨库返回
        db.upsert_file_conflict(&AttachmentFileConflict {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-A".to_string(),
            object_key: obj_key.clone(),
            remote_version: "v-a".to_string(),
            local_sha256: "hash-a".to_string(),
            reason: "file_conflict".to_string(),
            created_at: 100,
        })
        .unwrap();
        db.upsert_file_conflict(&AttachmentFileConflict {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-B".to_string(),
            object_key: obj_key.clone(),
            remote_version: "v-b".to_string(),
            local_sha256: "hash-b".to_string(),
            reason: "file_conflict".to_string(),
            created_at: 200,
        })
        .unwrap();

        db.upsert_pending_download(&AttachmentPendingDownload {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-A".to_string(),
            object_key: obj_key.clone(),
            remote_version: "v-a".to_string(),
            created_at: 100,
        })
        .unwrap();
        db.upsert_pending_download(&AttachmentPendingDownload {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-B".to_string(),
            object_key: obj_key.clone(),
            remote_version: "v-b".to_string(),
            created_at: 200,
        })
        .unwrap();

        let conflicts_a = db.list_file_conflicts("flib-A").unwrap();
        let conflicts_b = db.list_file_conflicts("flib-B").unwrap();
        assert_eq!(conflicts_a.len(), 1);
        assert_eq!(conflicts_a[0].file_library_id, "flib-A");
        assert_eq!(conflicts_b.len(), 1);
        assert_eq!(conflicts_b[0].file_library_id, "flib-B");

        let pending_a = db.list_pending_downloads("flib-A").unwrap();
        let pending_b = db.list_pending_downloads("flib-B").unwrap();
        assert_eq!(pending_a.len(), 1);
        assert_eq!(pending_a[0].file_library_id, "flib-A");
        assert_eq!(pending_b.len(), 1);
        assert_eq!(pending_b[0].file_library_id, "flib-B");
    }

    #[test]
    fn delete_file_library_state_clears_all_records_and_is_atomic() {
        let db = create_test_db();
        let att_id = "550e8400-e29b-41d4-a716-446655440000";
        let obj_key = format!("objects/v1/{att_id}");

        // 准备 A 库和 B 库的数据
        for lib in ["flib-A", "flib-B"] {
            db.upsert_file_library_binding(&FileLibraryBinding {
                file_library_id: lib.to_string(),
                database_library_id: "dblib-1".to_string(),
                backend_kind: "webdav".to_string(),
                backend_fingerprint: "fp".to_string(),
                protocol_version: 1,
                confirmed_at: 100,
            })
            .unwrap();

            db.upsert_attachment_file_baseline(&AttachmentFileBaseline {
                attachment_id: att_id.to_string(),
                file_library_id: lib.to_string(),
                object_key: obj_key.clone(),
                remote_version: "v1".to_string(),
                local_sha256: "hash".to_string(),
                local_presence: true,
                last_success_at: 100,
            })
            .unwrap();

            db.upsert_file_conflict(&AttachmentFileConflict {
                attachment_id: att_id.to_string(),
                file_library_id: lib.to_string(),
                object_key: obj_key.clone(),
                remote_version: "v1".to_string(),
                local_sha256: "hash".to_string(),
                reason: "file_conflict".to_string(),
                created_at: 100,
            })
            .unwrap();

            db.upsert_pending_download(&AttachmentPendingDownload {
                attachment_id: att_id.to_string(),
                file_library_id: lib.to_string(),
                object_key: obj_key.clone(),
                remote_version: "v1".to_string(),
                created_at: 100,
            })
            .unwrap();
        }

        // 7. delete_file_library_state("flib-A") 同时清空 A 的 conflicts + pending + baselines + binding，但不影响 B 库
        db.delete_file_library_state("flib-A").unwrap();

        assert!(db.get_file_library_binding("flib-A").unwrap().is_none());
        assert!(
            db.get_attachment_file_baseline(att_id, "flib-A")
                .unwrap()
                .is_none()
        );
        assert!(db.list_file_conflicts("flib-A").unwrap().is_empty());
        assert!(db.list_pending_downloads("flib-A").unwrap().is_empty());

        assert!(db.get_file_library_binding("flib-B").unwrap().is_some());
        assert!(
            db.get_attachment_file_baseline(att_id, "flib-B")
                .unwrap()
                .is_some()
        );
        assert_eq!(db.list_file_conflicts("flib-B").unwrap().len(), 1);
        assert_eq!(db.list_pending_downloads("flib-B").unwrap().len(), 1);

        // 注入 Trigger 强制冲突表 DELETE 失败时，整个状态（含 binding、baseline、pending、conflict）保留
        db.with_conn(|conn| {
            conn.execute(
                "CREATE TRIGGER fail_conflicts_delete BEFORE DELETE ON attachment_file_conflicts
                 BEGIN
                     SELECT RAISE(ABORT, 'injected conflict delete failure');
                 END;",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let result = db.delete_file_library_state("flib-B");
        assert!(result.is_err());

        assert!(db.get_file_library_binding("flib-B").unwrap().is_some());
        assert!(
            db.get_attachment_file_baseline(att_id, "flib-B")
                .unwrap()
                .is_some()
        );
        assert_eq!(db.list_file_conflicts("flib-B").unwrap().len(), 1);
        assert_eq!(db.list_pending_downloads("flib-B").unwrap().len(), 1);
    }

    #[test]
    fn object_key_mismatch_validation_in_conflict_and_pending() {
        let db = create_test_db();
        let att_id = "550e8400-e29b-41d4-a716-446655440000";
        let wrong_key = "objects/v1/6ba7b810-9dad-11d1-80b4-00c04fd430c8";

        // 8. 校验 object_key_from_attachment_id 不匹配时，upsert_file_conflict / upsert_pending_download 返回 Err
        let conflict = AttachmentFileConflict {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-A".to_string(),
            object_key: wrong_key.to_string(),
            remote_version: "v1".to_string(),
            local_sha256: "hash".to_string(),
            reason: "file_conflict".to_string(),
            created_at: 100,
        };
        assert!(db.upsert_file_conflict(&conflict).is_err());

        let pending = AttachmentPendingDownload {
            attachment_id: att_id.to_string(),
            file_library_id: "flib-A".to_string(),
            object_key: wrong_key.to_string(),
            remote_version: "v1".to_string(),
            created_at: 100,
        };
        assert!(db.upsert_pending_download(&pending).is_err());
    }

    // ---------- 阶段 4：FileSyncSummary 持久化 ----------

    fn sample_summary(run_id: &str, state: &str) -> FileSyncSummary {
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
            reason: Some("backend_unavailable".to_string()),
            run_id: run_id.to_string(),
            file_library_id: Some("flib-A".to_string()),
            updated_at: 1700000000,
        }
    }

    #[test]
    fn file_sync_summary_round_trip_persists_every_counter() {
        let db = create_test_db();
        let summary = sample_summary("run-abc12345", "partial_failure");

        db.set_file_sync_summary(&summary).unwrap();
        let loaded = db.get_file_sync_summary().unwrap().unwrap();

        assert_eq!(loaded, summary);
        assert_eq!(loaded.uploaded, 1);
        assert_eq!(loaded.downloaded, 2);
        assert_eq!(loaded.deleted, 3);
        assert_eq!(loaded.skipped, 4);
        assert_eq!(loaded.waiting, 5);
        assert_eq!(loaded.pending_download, 6);
        assert_eq!(loaded.unrecoverable_missing, 7);
        assert_eq!(loaded.conflicts, 8);
        assert_eq!(loaded.unknown_divergence, 9);
        assert_eq!(loaded.failures, 10);
        assert_eq!(loaded.state, "partial_failure");
        assert_eq!(loaded.reason.as_deref(), Some("backend_unavailable"));
        assert_eq!(loaded.run_id, "run-abc12345");
        assert_eq!(loaded.file_library_id.as_deref(), Some("flib-A"));
        assert_eq!(loaded.updated_at, 1700000000);
    }

    #[test]
    fn file_sync_summary_absent_returns_none() {
        let db = create_test_db();
        assert!(db.get_file_sync_summary().unwrap().is_none());
    }

    #[test]
    fn file_sync_summary_corrupt_json_returns_error_not_none() {
        let db = create_test_db();
        db.set_sync_meta(FILE_SYNC_LAST_SUMMARY_KEY, "{not-valid-json")
            .unwrap();

        // 损坏必须报错，不能被当作“没有历史结果”
        let result = db.get_file_sync_summary();
        assert!(result.is_err(), "JSON 损坏必须返回 Error，不能当作 None");
    }

    #[test]
    fn file_sync_summary_overwrite_replaces_previous_result() {
        let db = create_test_db();
        db.set_file_sync_summary(&sample_summary("run-1", "complete"))
            .unwrap();
        db.set_file_sync_summary(&sample_summary("run-2", "error"))
            .unwrap();

        let loaded = db.get_file_sync_summary().unwrap().unwrap();
        assert_eq!(loaded.run_id, "run-2");
        assert_eq!(loaded.state, "error");
    }

    // ---------- 阶段 4：确认状态快照 ----------

    fn insert_attachment_with_sync_state(
        db: &Database,
        id: &str,
        version: i64,
        synced_version: i64,
        is_dirty: i64,
        is_deleted: i64,
    ) {
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO attachments (
                    id, literature_id, file_name, file_path, file_size,
                    mime_type, hash, is_dirty, version, is_deleted,
                    synced_version, created_at, updated_at
                 ) VALUES (?1, 'lit-1', 'paper.pdf', '/dir/paper.pdf', 1024,
                    'application/pdf', 'h', ?2, ?3, ?4, ?5, 100, 100)",
                params![id, is_dirty, version, is_deleted, synced_version],
            )?;
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn attachment_sync_snapshots_expose_confirmation_fields() {
        let db = create_test_db();
        let confirmed = "550e8400-e29b-41d4-a716-446655440000";
        let unconfirmed = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        // synced_version >= version 且 is_dirty=0 → 可确认
        insert_attachment_with_sync_state(&db, confirmed, 3, 3, 0, 0);
        // synced_version 落后 → 不得确认
        insert_attachment_with_sync_state(&db, unconfirmed, 5, 2, 0, 0);

        let snapshots = db.attachment_sync_snapshots().unwrap();
        assert_eq!(snapshots.len(), 2);

        let c = snapshots.iter().find(|s| s.id == confirmed).unwrap();
        assert_eq!(c.version, 3);
        assert_eq!(c.synced_version, 3);
        assert!(!c.is_dirty);
        assert!(!c.is_deleted);

        let u = snapshots.iter().find(|s| s.id == unconfirmed).unwrap();
        assert_eq!(u.version, 5);
        assert_eq!(
            u.synced_version, 2,
            "synced_version 落后必须可被 services 识别"
        );
        assert!(!u.is_dirty);
    }

    #[test]
    fn attachment_sync_snapshots_include_dirty_and_tombstone_flags() {
        let db = create_test_db();
        let dirty = "550e8400-e29b-41d4-a716-446655440000";
        let tombstone = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        // is_dirty=1：本地改动未推送
        insert_attachment_with_sync_state(&db, dirty, 2, 2, 1, 0);
        // is_deleted=1 且已确认
        insert_attachment_with_sync_state(&db, tombstone, 4, 4, 0, 1);

        let snapshots = db.attachment_sync_snapshots().unwrap();
        let d = snapshots.iter().find(|s| s.id == dirty).unwrap();
        assert!(d.is_dirty);
        assert!(!d.is_deleted);

        let t = snapshots.iter().find(|s| s.id == tombstone).unwrap();
        assert!(t.is_deleted);
        assert!(!t.is_dirty);
        assert_eq!(t.synced_version, 4);
    }

    // ---------- 阶段 4：已确认删除的原子清理 ----------

    fn seed_library_state(db: &Database, att_id: &str, file_library_id: &str) {
        db.upsert_attachment_file_baseline(&AttachmentFileBaseline {
            attachment_id: att_id.to_string(),
            file_library_id: file_library_id.to_string(),
            object_key: format!("objects/v1/{att_id}"),
            remote_version: "v1".to_string(),
            local_sha256: "hash".to_string(),
            local_presence: true,
            last_success_at: 100,
        })
        .unwrap();
        db.upsert_pending_download(&AttachmentPendingDownload {
            attachment_id: att_id.to_string(),
            file_library_id: file_library_id.to_string(),
            object_key: format!("objects/v1/{att_id}"),
            remote_version: "v1".to_string(),
            created_at: 100,
        })
        .unwrap();
        db.upsert_file_conflict(&AttachmentFileConflict {
            attachment_id: att_id.to_string(),
            file_library_id: file_library_id.to_string(),
            object_key: format!("objects/v1/{att_id}"),
            remote_version: "v1".to_string(),
            local_sha256: "hash".to_string(),
            reason: "unknown_divergence".to_string(),
            created_at: 100,
        })
        .unwrap();
    }

    #[test]
    fn confirmed_remote_delete_clears_baseline_pending_and_conflict_of_current_library_only() {
        let db = create_test_db();
        let att_id = "550e8400-e29b-41d4-a716-446655440000";
        insert_attachment_with_sync_state(&db, att_id, 1, 1, 0, 1);
        seed_library_state(&db, att_id, "flib-A");
        seed_library_state(&db, att_id, "flib-B");

        db.apply_confirmed_remote_delete(att_id, "flib-A").unwrap();

        // A 库：baseline / pending / conflict 全部清理
        assert!(
            db.get_attachment_file_baseline(att_id, "flib-A")
                .unwrap()
                .is_none()
        );
        assert!(db.get_pending_download(att_id, "flib-A").unwrap().is_none());
        assert!(
            db.get_attachment_file_conflicts(att_id, "flib-A")
                .unwrap()
                .is_empty()
        );

        // B 库：不得跨库清理
        assert!(
            db.get_attachment_file_baseline(att_id, "flib-B")
                .unwrap()
                .is_some()
        );
        assert!(db.get_pending_download(att_id, "flib-B").unwrap().is_some());
        assert_eq!(
            db.get_attachment_file_conflicts(att_id, "flib-B")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn confirmed_remote_delete_rolls_back_when_conflict_delete_fails() {
        let db = create_test_db();
        let att_id = "550e8400-e29b-41d4-a716-446655440000";
        insert_attachment_with_sync_state(&db, att_id, 1, 1, 0, 1);
        seed_library_state(&db, att_id, "flib-A");

        // 注入 Trigger：删除 conflict 时失败（事务最后一步）
        db.with_conn(|conn| {
            conn.execute(
                "CREATE TRIGGER fail_conflict_delete BEFORE DELETE ON attachment_file_conflicts
                 BEGIN
                     SELECT RAISE(ABORT, 'injected conflict delete failure');
                 END;",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let result = db.apply_confirmed_remote_delete(att_id, "flib-A");
        assert!(result.is_err(), "conflict 删除失败必须返回错误");

        // 原子回滚：baseline 与 pending 必须完好保留
        assert!(
            db.get_attachment_file_baseline(att_id, "flib-A")
                .unwrap()
                .is_some()
        );
        assert!(db.get_pending_download(att_id, "flib-A").unwrap().is_some());
    }
}
