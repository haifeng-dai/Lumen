use super::Database;
use log::{debug, info, warn};
use models::Attachment;
use rusqlite::{Connection, OptionalExtension, Result, params};
use unicode_normalization::UnicodeNormalization;

impl Database {
    // --- Internal conn-based helpers ---

    /// 在事务内插入新附件（用于 set_attachments 等内部上下文）
    pub fn insert_attachment_conn(
        conn: &Connection,
        att: &Attachment,
        literature_id: &str,
        now: i64,
    ) -> Result<()> {
        conn.execute(
            "INSERT INTO attachments (id, literature_id, file_path, file_name, file_size, mime_type, etag, is_main, is_dirty, is_deleted, version, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, 0, 1, ?9, ?10)",
            params![att.id, literature_id, att.file_path, att.file_name, att.file_size as i64, att.mime_type, att.etag, att.is_main, now, now],
        )?;
        Ok(())
    }

    /// 在事务内更新附件的全部可变字段
    pub fn update_attachment_conn(
        conn: &Connection,
        att: &Attachment,
        is_dirty: bool,
        version: i32,
        now: i64,
    ) -> Result<()> {
        conn.execute(
            "UPDATE attachments SET file_path = ?1, file_name = ?2, file_size = ?3, mime_type = ?4, etag = ?5, is_main = ?6, is_deleted = 0, is_dirty = ?7, version = ?8, updated_at = ?9 WHERE id = ?10",
            params![att.file_path, att.file_name, att.file_size as i64, att.mime_type, att.etag, att.is_main, is_dirty, version, now, att.id],
        )?;
        Ok(())
    }

    /// 在事务内软删除附件
    pub fn soft_delete_attachment_conn(
        conn: &Connection,
        id: &str,
        version: i32,
        now: i64,
    ) -> Result<()> {
        conn.execute(
            "UPDATE attachments SET is_deleted = 1, is_dirty = 1, version = ?1, updated_at = ?2 WHERE id = ?3",
            params![version, now, id],
        )?;
        Ok(())
    }

    /// 插入 or 全量更新附件信息
    pub fn insert_attachment(&self, att: &Attachment) -> Result<()> {
        info!(
            "数据库: 准备写入附件 '{}' (ID: {}, LiteratureID: {})",
            att.file_name, att.id, att.literature_id
        );
        self.with_conn(|conn| {
            // 如果设置为主要附件，先将该文献下的其他附件设为非主要
            if att.is_main {
                debug!("数据库: 附件 {} 设置为主附件，正在更新文献 {} 的其他附件状态", att.id, att.literature_id);
                conn.execute(
                    "UPDATE attachments SET is_main = 0, is_dirty = 1, version = version + 1 WHERE literature_id = ?1",
                    [&att.literature_id],
                )?;
            }

            conn.execute(
                "INSERT OR REPLACE INTO attachments (id, literature_id, file_path, file_name, file_size, mime_type, etag, is_main, is_dirty, is_deleted, version, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![att.id, att.literature_id, att.file_path, att.file_name, att.file_size as i64, att.mime_type, att.etag, att.is_main, att.is_dirty, att.is_deleted, att.version, att.created_at, att.updated_at],
            )?;
            Ok(())
        })
    }

    /// 获取特定附件详情
    pub fn get_attachment(&self, id: &str) -> Result<Option<Attachment>> {
        debug!("数据库: 正在获取附件详情 (ID: {id})");
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, literature_id, file_path, file_name, file_size, mime_type, etag, hash, is_main, is_dirty, is_deleted, version, created_at, updated_at FROM attachments WHERE id = ?1 AND is_deleted = 0",
                [id],
                |row| Ok(Attachment {
                    id: row.get(0)?,
                    literature_id: row.get(1)?,
                    file_path: row.get(2)?,
                    file_name: row.get(3)?,
                    file_size: row.get::<_, i64>(4)? as u64,
                    mime_type: row.get(5)?,
                    etag: row.get(6)?,
                    hash: row.get(7)?,
                    is_main: row.get(8)?,
                    is_dirty: row.get(9)?,
                    is_deleted: row.get(10)?,
                    version: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                })
            ).optional()
        })
    }

    /// 获取所有附件（包含已删除的），用于同步扫描
    pub fn get_all_attachments_include_deleted(&self) -> Result<Vec<Attachment>> {
        debug!("数据库: 正在获取所有附件（含已删除）");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT id, literature_id, file_path, file_name, file_size, mime_type, etag, hash, is_main, is_dirty, is_deleted, version, created_at, updated_at FROM attachments")?;
            let att_iter = stmt.query_map([], |row| {
                Ok(Attachment {
                    id: row.get(0)?,
                    literature_id: row.get(1)?,
                    file_path: row.get(2)?,
                    file_name: row.get(3)?,
                    file_size: row.get::<_, i64>(4)? as u64,
                    mime_type: row.get(5)?,
                    etag: row.get(6)?,
                    hash: row.get(7)?,
                    is_main: row.get(8)?,
                    is_dirty: row.get(9)?,
                    is_deleted: row.get(10)?,
                    version: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                })
            })?;

            let mut attachments = Vec::new();
            for att in att_iter {
                attachments.push(att?);
            }
            debug!("数据库: 成功获取共 {} 条附件记录", attachments.len());
            Ok(attachments)
        })
    }

    /// 删除附件记录 (软删除)
    pub fn delete_attachment(&self, id: &str) -> Result<()> {
        info!("数据库: 准备删除附件 (ID: {id})");
        self.with_conn(|conn| {
            let rows = conn.execute(
                "UPDATE attachments SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE id = ?2",
                params![chrono::Local::now().timestamp(), id],
            )?;
            if rows > 0 {
                debug!("数据库: 附件 (ID: {id}) 已标记为删除");
            } else {
                warn!("数据库: 删除附件失败，未找到 ID 为 {id} 的记录");
            }
            Ok(())
        })
    }

    // --- 同步支持方法 ---

    pub fn get_dirty_attachments(&self) -> Result<Vec<Attachment>> {
        debug!("数据库: 正在获取待同步的附件");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, literature_id, file_path, file_name, file_size, mime_type, etag, hash, is_main, is_dirty, is_deleted, version, created_at, updated_at FROM attachments WHERE is_dirty = 1"
            )?;
            let iter = stmt.query_map([], |row| {
                Ok(Attachment {
                    id: row.get(0)?,
                    literature_id: row.get(1)?,
                    file_path: row.get(2)?,
                    file_name: row.get(3)?,
                    file_size: row.get::<_, i64>(4)? as u64,
                    mime_type: row.get(5)?,
                    etag: row.get(6)?,
                    hash: row.get(7)?,
                    is_main: row.get(8)?,
                    is_dirty: row.get(9)?,
                    is_deleted: row.get(10)?,
                    version: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                })
            })?;
            let items = iter.collect::<Result<Vec<_>>>()?;
            debug!("数据库: 成功获取 {} 条待同步附件记录", items.len());
            Ok(items)
        })
    }

    pub fn mark_attachment_synced(&self, id: &str) -> Result<()> {
        debug!("数据库: 标记附件为已同步 (ID: {id})");
        self.with_conn(|conn| {
            conn.execute("UPDATE attachments SET is_dirty = 0 WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// 根据文件路径查询附件
    ///
    /// 注意: macOS 文件系统返回的路径是 NFD 格式，而数据库中存储的是 NFC 格式，
    /// 因此需要先将输入路径规范化为 NFC 再进行查询。
    pub fn get_attachment_by_file_path(&self, file_path: &str) -> Result<Option<Attachment>> {
        // 将路径规范化为 NFC 格式（与数据库存储格式一致）
        let normalized_path: String = file_path.nfc().collect();
        debug!("数据库: 根据文件路径查询附件 (path: {normalized_path})");
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, literature_id, file_path, file_name, file_size, mime_type, etag, hash, is_main, is_dirty, is_deleted, version, created_at, updated_at FROM attachments WHERE file_path = ?1 AND is_deleted = 0",
                [&normalized_path],
                |row| Ok(Attachment {
                    id: row.get(0)?,
                    literature_id: row.get(1)?,
                    file_path: row.get(2)?,
                    file_name: row.get(3)?,
                    file_size: row.get::<_, i64>(4)? as u64,
                    mime_type: row.get(5)?,
                    etag: row.get(6)?,
                    hash: row.get(7)?,
                    is_main: row.get(8)?,
                    is_dirty: row.get(9)?,
                    is_deleted: row.get(10)?,
                    version: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                })
            ).optional()
        })
    }

    /// 根据文件名查询附件 (可能返回多个，需进一步确认路径)
    pub fn get_attachments_by_file_name(&self, file_name: &str) -> Result<Vec<Attachment>> {
        // 同样进行 NFC 规范化，虽然文件名匹配可能不需要，但保持一致
        let normalized_name: String = file_name.nfc().collect();
        debug!("数据库: 根据文件名查询附件 (name: {normalized_name})");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, literature_id, file_path, file_name, file_size, mime_type, etag, hash, is_main, is_dirty, is_deleted, version, created_at, updated_at FROM attachments WHERE file_name = ?1 AND is_deleted = 0"
            )?;
            let rows = stmt.query_map([&normalized_name], |row| {
                Ok(Attachment {
                    id: row.get(0)?,
                    literature_id: row.get(1)?,
                    file_path: row.get(2)?,
                    file_name: row.get(3)?,
                    file_size: row.get::<_, i64>(4)? as u64,
                    mime_type: row.get(5)?,
                    etag: row.get(6)?,
                    hash: row.get(7)?,
                    is_main: row.get(8)?,
                    is_dirty: row.get(9)?,
                    is_deleted: row.get(10)?,
                    version: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                })
            })?;

            let mut attachments = Vec::new();
            for att in rows {
                attachments.push(att?);
            }
            Ok(attachments)
        })
    }

    /// 将附件标记为 dirty (需要同步)
    pub fn mark_attachment_dirty(&self, id: &str) -> Result<()> {
        info!("数据库: 标记附件为需要同步 (ID: {id})");
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE attachments SET is_dirty = 1, etag = NULL, version = version + 1, updated_at = ?1 WHERE id = ?2",
                params![chrono::Local::now().timestamp(), id],
            )?;
            Ok(())
        })
    }

    /// 清理已同步的删除记录，并返回这些记录的文件路径以便删除物理文件
    pub fn purge_synced_attachments(&self) -> Result<Vec<String>> {
        info!("数据库: 正在清理已同步的删除附件记录");
        self.with_conn(|conn| {
            let mut paths = Vec::new();
            let mut stmt = conn.prepare(
                "SELECT file_path FROM attachments WHERE is_deleted = 1 AND is_dirty = 0",
            )?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            for path in rows {
                paths.push(path?);
            }

            let count = conn.execute(
                "DELETE FROM attachments WHERE is_deleted = 1 AND is_dirty = 0",
                [],
            )?;
            info!("数据库: 已从数据库中彻底删除 {count} 条附件记录");
            Ok(paths)
        })
    }

    /// 读取附件本地同步状态 `(version, is_dirty)`。
    pub fn get_attachment_sync_state(&self, id: &str) -> Result<Option<(i32, bool)>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT version, is_dirty FROM attachments WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
        })
    }

    /// 原子原语：把远程附件盲目 upsert 到本地（覆盖写）。
    pub fn apply_remote_attachment(&self, remote: &Attachment) -> Result<()> {
        self.with_conn(|conn| self.insert_attachment_internal(conn, remote))
    }

    /// 原子原语：版本一致本地无修改时，仅刷新时间戳/etag 并清脏标记。
    pub fn mark_attachment_up_to_date(&self, remote: &Attachment) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE attachments SET updated_at = ?1, is_dirty = 0, etag = ?2 WHERE id = ?3",
                params![remote.updated_at, remote.etag, remote.id],
            )?;
            Ok(())
        })
    }

    fn insert_attachment_internal(
        &self,
        conn: &rusqlite::Connection,
        att: &Attachment,
    ) -> Result<()> {
        debug!("数据库: 正在执行内部附件插入 (ID: {})", att.id);
        conn.execute(
            "INSERT OR REPLACE INTO attachments (id, literature_id, file_path, file_name, file_size, mime_type, etag, is_main, is_dirty, is_deleted, version, created_at, updated_at, hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![att.id, att.literature_id, att.file_path, att.file_name, att.file_size as i64, att.mime_type, att.etag, att.is_main, 0, att.is_deleted, att.version, att.created_at, att.updated_at, att.hash],
        )?;
        Ok(())
    }

    /// 合并附件：将源文献的附件迁移到目标文献
    pub fn merge_attachments(&self, source_id: &str, target_id: &str) -> Result<()> {
        info!("数据库: 正在合并附件 ({source_id} -> {target_id})");
        self.with_conn(|conn| {
            let now = chrono::Local::now().timestamp();

            // 1. 获取目标文献的所有附件
            let mut stmt = conn.prepare("SELECT file_path, file_name, file_size, is_main FROM attachments WHERE literature_id = ?1 AND is_deleted = 0")?;
            let target_atts_iter = stmt.query_map([target_id], |row| {
                Ok((
                    row.get::<_, String>(0)?.nfc().collect::<String>(), // Normalize path
                    row.get::<_, String>(1)?.nfc().collect::<String>(), // Normalize name
                    row.get::<_, i64>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            })?;

            let mut target_has_main = false;
            let mut target_paths = std::collections::HashSet::new();
            let mut target_files = std::collections::HashSet::new();

            for att in target_atts_iter {
                let (path, name, size, is_main) = att?;
                if is_main { target_has_main = true; }
                target_paths.insert(path);
                target_files.insert((name, size));
            }

            // 2. 获取源文献的所有附件
            let mut stmt = conn.prepare("SELECT id, file_path, file_name, file_size, is_main FROM attachments WHERE literature_id = ?1 AND is_deleted = 0")?;
            let source_atts_iter = stmt.query_map([source_id], |row| {
                 Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?.nfc().collect::<String>(),
                    row.get::<_, String>(2)?.nfc().collect::<String>(),
                    row.get::<_, i64>(3)?,
                    row.get::<_, bool>(4)?,
                ))
            })?;

            let mut migrated_count = 0;

            for att in source_atts_iter {
                let (id, path, name, size, is_main) = att?;

                // 3. 检查重复
                if target_paths.contains(&path) || target_files.contains(&(name.clone(), size)) {
                    debug!("数据库: 附件重复，标记删除源附件 (ID: {id}, Name: {name})");
                    conn.execute(
                        "UPDATE attachments SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE id = ?2",
                        params![now, id],
                    )?;
                    continue;
                }

                // 4. 迁移
                let new_is_main = if target_has_main { false } else { is_main };
                if new_is_main { target_has_main = true; }

                conn.execute(
                    "UPDATE attachments SET literature_id = ?1, is_main = ?2, is_dirty = 1, version = version + 1, updated_at = ?3 WHERE id = ?4",
                    params![target_id, new_is_main, now, id],
                )?;
                migrated_count += 1;
            }

            info!("数据库: 已迁移 {migrated_count} 个附件 ({source_id} -> {target_id})");

            // 5. 迁移注释：将 document_id 从 "source_id::附件ID" 重写为 "target_id::附件ID"
            let old_prefix = format!("{}::", source_id);
            let new_prefix = format!("{}::", target_id);
            let affected = conn.execute(
                "UPDATE annotations SET document_id = REPLACE(document_id, ?1, ?2), is_dirty = 1, version = version + 1, updated_at = ?3 WHERE document_id LIKE ?4",
                params![old_prefix, new_prefix, now, format!("{}%", old_prefix)],
            )?;
            if affected > 0 {
                info!("数据库: 已迁移 {affected} 条注释 ({source_id} -> {target_id})");
            }

            Ok(())
        })
    }
}
