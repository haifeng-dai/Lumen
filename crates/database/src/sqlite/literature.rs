use super::Database;
use anyhow::anyhow;
use chrono::Local;
use log::{debug, error, info, warn};
use models::constructors::*;
use models::{
    Attachment, Author, DEFAULT_TAG_COLOR, Literature, LiteratureType, Publication, PublicationType,
};
use rusqlite::{Connection, OptionalExtension, Result, Row, params};
use uuid::Uuid;

type AuthorRelationTuple = (String, String, Option<i32>, bool, i32);
type CommonRelationTuple = (String, String, bool, i32);

struct RelationUpsertData {
    lit_id: String,
    target_id: String,
    sort_order: Option<i32>,
    is_deleted: bool,
    version: i32,
}

impl Database {
    /// 插入或全量更新文献及其关联关系
    pub fn insert_literature(&self, lit: &Literature) -> Result<()> {
        info!(
            "数据库: 准备写入文献, ID: {}, Title: '{}'",
            lit.id, lit.title
        );
        debug!(
            "数据库: 写入文献详情: year={:?}, type={:?}, doi={:?}",
            lit.year, lit.literature_type, lit.doi
        );
        self.with_transaction(|tx| {
            // 首先处理出版源，获取 publication_id
            let publication_id = Self::set_publication(tx, lit.publication.as_ref())?;

            tx.execute(
                "INSERT OR REPLACE INTO literatures (
                    id, title, year, month, day, type, publication_id, volume, issue, pages,
                    abstract_text, doi, arxiv_id, url,
                    rating, reading_status, is_dirty, is_deleted, version,
                    created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
                params![
                    lit.id, lit.title, lit.year, lit.month, lit.day,
                    serde_json::to_string(&lit.literature_type).unwrap_or_else(|_| "\"article\"".to_string()).trim_matches('"'),
                    publication_id, lit.volume, lit.issue, lit.pages,
                    lit.abstract_text, lit.doi, lit.arxiv_id, lit.url,
                    lit.rating, lit.reading_status.to_string(), lit.is_dirty, lit.is_deleted, lit.version, lit.created_at, lit.updated_at,
                ],
            )?;

            debug!("数据库: 开始更新文献关联关系 (ID: {})", lit.id);
            Self::set_authors(tx, &lit.id, &lit.authors)?;
            Self::set_folders(tx, &lit.id, &lit.folder_ids)?;
            Self::set_tags(tx, &lit.id, &lit.tags)?;
            Self::set_attachments(tx, &lit.id, &lit.attachments)?;

            info!("数据库: 文献 {} 及其关联关系写入成功", lit.id);
            Ok(())
        })
    }

    /// 仅更新 `literatures` 表行 + publication（不碰 authors/tags/folders/attachments 等关联表）。
    /// 要完整保存文献请用 `insert_literature`。
    /// 这是 database 层暴露的底层操作，供 services 层统一包装后由上层调用。
    pub fn update_literature_row(&self, lit: &Literature) -> Result<()> {
        debug!("数据库: 正在更新文献元数据 (ID: {})", lit.id);
        self.with_conn(|conn| {
            // 处理出版源
            let publication_id = Self::set_publication(conn, lit.publication.as_ref())?;

            let rows = conn.execute(
                "UPDATE literatures SET
                    title = ?1, year = ?2, month = ?3, day = ?4, type = ?5, publication_id = ?6,
                    volume = ?7, issue = ?8, pages = ?9,
                    abstract_text = ?10, doi = ?11, arxiv_id = ?12, url = ?13,
                    rating = ?14, updated_at = ?15, is_dirty = 1, version = version + 1
                WHERE id = ?16",
                params![
                    lit.title,
                    lit.year,
                    lit.month,
                    lit.day,
                    serde_json::to_string(&lit.literature_type)
                        .unwrap_or_else(|_| "\"article\"".to_string())
                        .trim_matches('"'),
                    publication_id,
                    lit.volume,
                    lit.issue,
                    lit.pages,
                    lit.abstract_text,
                    lit.doi,
                    lit.arxiv_id,
                    lit.url,
                    lit.rating,
                    Local::now().timestamp(),
                    lit.id,
                ],
            )?;
            if rows == 0 {
                warn!("数据库: 更新文献元数据失败，未找到 ID 为 {} 的记录", lit.id);
            } else {
                debug!("数据库: 文献元数据更新完成 (ID: {})", lit.id);
            }
            Ok(())
        })
    }

    pub fn update_reading_status(&self, id: &str, status: models::ReadingStatus) -> Result<()> {
        debug!("数据库: 正在更新文献阅读状态 (ID: {id}, status: {status:?})");
        self.with_conn(|conn| {
            let rows = conn.execute("UPDATE literatures SET reading_status = ?1, updated_at = ?2, is_dirty = 1, version = version + 1 WHERE id = ?3",
                params![status.to_string(), Local::now().timestamp(), id])?;
            if rows == 0 {
                warn!("数据库: 更新阅读状态失败，未找到 ID 为 {id} 的记录");
            } else {
                debug!("数据库: 文献阅读状态更新成功 (ID: {id})");
            }
            Ok(())
        })
    }

    pub fn get_folders_for_literature(&self, literature_id: &str) -> anyhow::Result<Vec<String>> {
        debug!("数据库: 正在获取文献所属文件夹 (ID: {literature_id})");
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT folder_id FROM literature_folders WHERE literature_id = ? AND is_deleted = 0",
        )?;
        let rows = stmt.query_map([literature_id], |row: &Row| row.get::<_, String>(0))?;
        let mut folders = Vec::new();
        for folder in rows {
            folders.push(folder?);
        }
        debug!(
            "数据库: 成功获取 {} 个所属文件夹 (ID: {})",
            folders.len(),
            literature_id
        );
        Ok(folders)
    }

    pub fn set_literature_authors(&self, literature_id: &str, authors: &[Author]) -> Result<()> {
        info!(
            "数据库: 正在设置文献作者 (ID: {}, 作者数量: {})",
            literature_id,
            authors.len()
        );
        self.with_transaction(|tx| {
            Self::set_authors(tx, literature_id, authors)?;
            tx.execute("UPDATE literatures SET updated_at = ?1, is_dirty = 1, version = version + 1 WHERE id = ?2",
                params![Local::now().timestamp(), literature_id])?;
            Ok(())
        })
    }

    pub fn set_literature_folders(&self, literature_id: &str, folder_ids: &[String]) -> Result<()> {
        info!(
            "数据库: 正在设置文献文件夹关联 (ID: {}, 文件夹数量: {})",
            literature_id,
            folder_ids.len()
        );
        self.with_transaction(|tx| {
            Self::set_folders(tx, literature_id, folder_ids)?;
            tx.execute("UPDATE literatures SET updated_at = ?1, is_dirty = 1, version = version + 1 WHERE id = ?2",
                params![Local::now().timestamp(), literature_id])?;
            Ok(())
        })
    }

    pub fn set_literature_tags(&self, literature_id: &str, tags: &[String]) -> Result<()> {
        info!(
            "数据库: 正在设置文献标签 (ID: {}, 标签数量: {})",
            literature_id,
            tags.len()
        );
        self.with_transaction(|tx| {
            Self::set_tags(tx, literature_id, tags)?;
            tx.execute("UPDATE literatures SET updated_at = ?1, is_dirty = 1, version = version + 1 WHERE id = ?2",
                params![Local::now().timestamp(), literature_id])?;
            Ok(())
        })
    }

    /// 为文献添加单个标签（增量操作）
    pub fn add_tag_to_literature(&self, literature_id: &str, tag_name: &str) -> Result<()> {
        info!("数据库: 为文献添加标签 (ID: {literature_id}, 标签: {tag_name})");
        let now = Local::now().timestamp();

        self.with_transaction(|tx| {
            // 1. 确保标签存在（使用 create_tag 逻辑）
            let tag_id: Option<String> = tx.query_row(
                "SELECT id FROM tags WHERE name = ?1 AND is_deleted = 0",
                [tag_name],
                |row| row.get(0)
            ).optional()?;

            let tag_id = if let Some(id) = tag_id {
                id
            } else {
                // 标签不存在，创建新标签
                let new_id = Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT INTO tags (id, name, color, is_dirty, is_deleted, version, created_at, updated_at)
                     VALUES (?1, ?2, '#4A90E2', 1, 0, 1, ?3, ?3)",
                    params![new_id, tag_name, now]
                )?;
                debug!("数据库: 创建新标签 '{tag_name}' (ID: {new_id})");
                new_id
            };

            // 2. 检查文献-标签关联是否存在
            let existing: Option<(bool, i32)> = tx.query_row(
                "SELECT is_deleted, version FROM literature_tags WHERE literature_id = ?1 AND tag_id = ?2",
                params![literature_id, tag_id],
                |row| Ok((row.get(0)?, row.get(1)?))
            ).optional()?;

            if let Some((is_deleted, version)) = existing {
                if is_deleted {
                    // 恢复已删除的关联
                    tx.execute(
                        "UPDATE literature_tags SET is_deleted = 0, is_dirty = 1, version = ?1, updated_at = ?2
                         WHERE literature_id = ?3 AND tag_id = ?4",
                        params![version + 1, now, literature_id, tag_id]
                    )?;
                    debug!("数据库: 恢复文献-标签关联 (LiteratureID: {literature_id}, Tag: {tag_name})");
                } else {
                    debug!("数据库: 文献已有该标签，跳过 (LiteratureID: {literature_id}, Tag: {tag_name})");
                }
            } else {
                // 创建新关联
                tx.execute(
                    "INSERT INTO literature_tags (literature_id, tag_id, is_dirty, is_deleted, version, updated_at)
                     VALUES (?1, ?2, 1, 0, 1, ?3)",
                    params![literature_id, tag_id, now]
                )?;
                debug!("数据库: 创建新文献-标签关联 (LiteratureID: {literature_id}, Tag: {tag_name})");
            }

            // 3. 更新文献的 updated_at 和 version
            tx.execute(
                "UPDATE literatures SET updated_at = ?1, is_dirty = 1, version = version + 1 WHERE id = ?2",
                params![now, literature_id]
            )?;

            Ok(())
        })
    }

    /// 从文献移除单个标签（软删除）
    pub fn remove_tag_from_literature(&self, literature_id: &str, tag_name: &str) -> Result<()> {
        info!("数据库: 从文献移除标签 (ID: {literature_id}, 标签: {tag_name})");
        let now = Local::now().timestamp();

        self.with_transaction(|tx| {
            // 1. 获取标签 ID
            let tag_id: Option<String> = tx.query_row(
                "SELECT id FROM tags WHERE name = ?1",
                [tag_name],
                |row| row.get(0)
            ).optional()?;

            if let Some(tag_id) = tag_id {
                // 2. 软删除关联
                let rows = tx.execute(
                    "UPDATE literature_tags SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1
                     WHERE literature_id = ?2 AND tag_id = ?3 AND is_deleted = 0",
                    params![now, literature_id, tag_id]
                )?;

                if rows > 0 {
                    debug!("数据库: 成功移除文献-标签关联 (LiteratureID: {literature_id}, Tag: {tag_name})");

                    // 3. 更新文献的 updated_at 和 version
                    tx.execute(
                        "UPDATE literatures SET updated_at = ?1, is_dirty = 1, version = version + 1 WHERE id = ?2",
                        params![now, literature_id]
                    )?;
                } else {
                    warn!("数据库: 未找到需要删除的文献-标签关联 (LiteratureID: {literature_id}, Tag: {tag_name})");
                }
            } else {
                warn!("数据库: 标签不存在，无法移除 (Tag: {tag_name})");
            }

            Ok(())
        })
    }

    pub fn get_literature(&self, id: &str) -> Result<Option<Literature>> {
        debug!("数据库: 正在获取文献详情 (ID: {id})");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT id, title, year, month, day, type, volume, issue, pages, abstract_text, doi, arxiv_id, url, rating, reading_status, is_dirty, is_deleted, version, created_at, updated_at, publication_id FROM literatures WHERE id = ?1")?;
            let mut rows = stmt.query([id])?;
            if let Some(row) = rows.next()? {
                let lit = Self::map_literature_row(conn, row)?;
                debug!("数据库: 成功获取文献详情: {}", lit.title);
                Ok(Some(lit))
            } else {
                debug!("数据库: 未找到 ID 为 {id} 的文献记录");
                Ok(None)
            }
        })
    }

    pub fn get_all_literatures(&self) -> Result<Vec<Literature>> {
        debug!("数据库: 正在获取所有未删除的文献");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT id, title, year, month, day, type, volume, issue, pages, abstract_text, doi, arxiv_id, url, rating, reading_status, is_dirty, is_deleted, version, created_at, updated_at, publication_id FROM literatures WHERE is_deleted = 0 ORDER BY created_at DESC")?;
            let lit_iter = stmt.query_map([], |row| Self::map_literature_row(conn, row))?;
            let mut literatures = Vec::new();
            for lit in lit_iter { literatures.push(lit?); }
            debug!("数据库: 成功加载 {} 条文献记录", literatures.len());
            Ok(literatures)
        })
    }

    pub fn get_literatures_by_folder(&self, folder_id: &str) -> Result<Vec<Literature>> {
        debug!("数据库: 正在获取文件夹中的文献 (FolderID: {folder_id})");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT l.id, l.title, l.year, l.month, l.day, l.type, l.volume, l.issue, l.pages, l.abstract_text, l.doi, l.arxiv_id, l.url, l.rating, l.reading_status, l.is_dirty, l.is_deleted, l.version, l.created_at, l.updated_at, l.publication_id FROM literatures l JOIN literature_folders lf ON l.id = lf.literature_id WHERE lf.folder_id = ?1 AND l.is_deleted = 0 AND lf.is_deleted = 0 ORDER BY l.created_at DESC")?;
            let lit_iter = stmt.query_map([folder_id], |row| Self::map_literature_row(conn, row))?;
            let mut literatures = Vec::new();
            for lit in lit_iter { literatures.push(lit?); }
            debug!("数据库: 文件夹 {} 中共有 {} 条文献记录", folder_id, literatures.len());
            Ok(literatures)
        })
    }

    pub fn delete_literature(&self, id: &str) -> Result<()> {
        info!("数据库: 准备软删除文献记录: {id}");
        self.with_transaction(|tx| {
            let now = Local::now().timestamp();

            // 记录关联的作者 ID 和出版源 ID，用于后续孤立检查
            let author_ids: Vec<String> = tx
                .prepare(
                    "SELECT author_id FROM literature_authors WHERE literature_id = ?1 AND is_deleted = 0",
                )?
                .query_map([id], |row| row.get(0))?
                .collect::<Result<Vec<_>>>()?;
            let pub_id: Option<String> = tx
                .query_row(
                    "SELECT publication_id FROM literatures WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();

            let rows = tx.execute(
                "UPDATE literatures SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            if rows > 0 {
                info!("数据库: 文献 {id} 已成功标记为软删除");
            } else {
                warn!("数据库: 软删除文献失败，找不到 ID 为 {id} 的记录");
            }

            // 一并软删除关联表记录，保持数据一致性
            tx.execute(
                "UPDATE literature_folders SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE literature_id = ?2",
                params![now, id],
            )?;
            tx.execute(
                "UPDATE literature_authors SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE literature_id = ?2",
                params![now, id],
            )?;
            tx.execute(
                "UPDATE literature_tags SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE literature_id = ?2",
                params![now, id],
            )?;

            // 补全遗漏的关联表软删除
            tx.execute(
                "UPDATE attachments SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE literature_id = ?2 AND is_deleted = 0",
                params![now, id],
            )?;
            tx.execute(
                "UPDATE literature_citations SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE (source_id = ?2 OR target_id = ?2) AND is_deleted = 0",
                params![now, id],
            )?;
            tx.execute(
                "UPDATE annotations SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE (document_id = ?2 OR document_id LIKE ?3) AND is_deleted = 0",
                params![now, id, format!("{id}::%")],
            )?;
            tx.execute(
                "UPDATE literature_notes SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE literature_id = ?2 AND is_deleted = 0",
                params![now, id],
            )?;

            // 孤立作者检查：该作者不再被任何未删除文献引用
            for aid in &author_ids {
                let remaining: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM literature_authors WHERE author_id = ?1 AND is_deleted = 0",
                    params![aid],
                    |row| row.get(0),
                )?;
                if remaining == 0 {
                    debug!("数据库: 作者 {aid} 已孤立，执行软删除");
                    tx.execute(
                        "UPDATE authors SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE id = ?2",
                        params![now, aid],
                    )?;
                }
            }

            // 孤立出版源检查：不再被任何未删除文献引用
            if let Some(ref pid) = pub_id {
                let remaining: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM literatures WHERE publication_id = ?1 AND is_deleted = 0",
                    params![pid],
                    |row| row.get(0),
                )?;
                if remaining == 0 {
                    debug!("数据库: 出版源 {pid} 已孤立，执行软删除");
                    tx.execute(
                        "UPDATE publications SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE id = ?2",
                        params![now, pid],
                    )?;
                }
            }

            Ok(())
        })
    }

    pub fn get_dirty_literatures(&self) -> Result<Vec<Literature>> {
        debug!("数据库: 正在获取所有标记为脏数据的文献");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT id, title, year, month, day, type, volume, issue, pages, abstract_text, doi, arxiv_id, url, rating, reading_status, is_dirty, is_deleted, version, created_at, updated_at, publication_id FROM literatures WHERE is_dirty = 1")?;
            let lit_iter = stmt.query_map([], |row| Self::map_literature_row(conn, row))?;
            let mut literatures = Vec::new();
            for lit in lit_iter { literatures.push(lit?); }
            debug!("数据库: 成功加载 {} 条脏数据文献记录", literatures.len());
            Ok(literatures)
        })
    }

    pub fn mark_literature_synced(&self, id: &str) -> Result<()> {
        debug!("数据库: 正在标记文献已同步 (ID: {id})");
        self.with_conn(|conn| {
            let rows = conn.execute("UPDATE literatures SET is_dirty = 0 WHERE id = ?1", [id])?;
            if rows == 0 {
                warn!("数据库: 标记同步失败，未找到 ID 为 {id} 的记录");
            } else {
                debug!("数据库: 文献 {id} 已标记为同步状态");
            }
            Ok(())
        })
    }

    pub fn purge_synced_deletions(&self) -> Result<usize> {
        info!("数据库: 正在清理已同步的删除记录");
        self.with_transaction(|tx| {
            let mut total = 0;
            total += tx.execute(
                "DELETE FROM literatures WHERE is_deleted = 1 AND is_dirty = 0",
                [],
            )?;
            total += tx.execute(
                "DELETE FROM literature_authors WHERE is_deleted = 1 AND is_dirty = 0",
                [],
            )?;
            total += tx.execute(
                "DELETE FROM literature_folders WHERE is_deleted = 1 AND is_dirty = 0",
                [],
            )?;
            total += tx.execute(
                "DELETE FROM literature_tags WHERE is_deleted = 1 AND is_dirty = 0",
                [],
            )?;
            total += tx.execute(
                "DELETE FROM literature_notes WHERE is_deleted = 1 AND is_dirty = 0",
                [],
            )?;
            info!("数据库: 清理完成，共删除 {total} 条已同步的物理记录");
            Ok(total)
        })
    }

    /// 读取文献本地同步状态 `(version, is_dirty)`。
    pub fn get_literature_sync_state(&self, id: &str) -> Result<Option<(i32, bool)>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT version, is_dirty FROM literatures WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
        })
    }

    /// 原子原语：把远程文献盲目 upsert 到本地（覆盖写）。
    pub fn apply_remote_literature(&self, remote: &Literature) -> Result<()> {
        self.with_conn(|conn| self.insert_literature_internal(conn, remote))
    }

    /// 原子原语：版本一致本地无修改时，仅刷新时间戳并清脏标记。
    pub fn mark_literature_up_to_date(&self, remote: &Literature) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE literatures SET updated_at = ?1, is_dirty = 0 WHERE id = ?2",
                params![remote.updated_at, remote.id],
            )?;
            Ok(())
        })
    }

    fn insert_literature_internal(&self, conn: &Connection, lit: &Literature) -> Result<()> {
        debug!("数据库: 正在执行内部文献插入/更新 (ID: {})", lit.id);
        // 处理出版源
        let publication_id = Self::set_publication(conn, lit.publication.as_ref())?;

        conn.execute(
            "INSERT OR REPLACE INTO literatures (id, title, year, month, day, type, publication_id, volume, issue, pages, abstract_text, doi, arxiv_id, url, rating, reading_status, is_dirty, is_deleted, version, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 0, ?17, ?18, ?19, ?20)",
            params![lit.id, lit.title, lit.year, lit.month, lit.day, serde_json::to_string(&lit.literature_type).unwrap_or_default().trim_matches('"'), publication_id, lit.volume, lit.issue, lit.pages, lit.abstract_text, lit.doi, lit.arxiv_id, lit.url, lit.rating, lit.reading_status.to_string(), lit.is_deleted, lit.version, lit.created_at, lit.updated_at],
        )?;
        Ok(())
    }

    fn map_literature_row(conn: &Connection, row: &Row) -> Result<Literature> {
        let id: String = row.get(0)?;
        let title: String = row.get(1)?;
        debug!("数据库: 正在从数据行映射文献对象 (ID: {id}, Title: {title})");
        let lit_type_str: String = row.get(5)?;
        let lit_type: LiteratureType =
            serde_json::from_str(&format!("\"{lit_type_str}\"")).unwrap_or(LiteratureType::Article);
        let mut lit = create_literature(id.clone(), title, lit_type);
        lit.year = row.get(2)?;
        lit.month = row.get(3)?;
        lit.day = row.get(4)?;

        lit.volume = row.get(6)?;
        lit.issue = row.get(7)?;
        lit.pages = row.get(8)?;
        lit.abstract_text = row.get(9)?;
        lit.doi = row.get(10)?;
        lit.arxiv_id = row.get(11)?;
        lit.url = row.get(12)?;
        lit.rating = row.get(13)?;
        lit.reading_status = match row.get::<_, String>(14) {
            Ok(s) => match s.as_str() {
                "ToRead" => models::ReadingStatus::ToRead,
                "Reading" => models::ReadingStatus::Reading,
                "Read" => models::ReadingStatus::Read,
                _ => models::ReadingStatus::Unread,
            },
            Err(_) => models::ReadingStatus::Unread,
        };
        lit.is_dirty = row.get(15)?;
        lit.is_deleted = row.get(16)?;
        lit.version = row.get(17)?;
        lit.created_at = row.get(18)?;
        lit.updated_at = row.get(19)?;

        // 加载出版源信息 (在 SELECT 列表 the last)
        let publication_id: Option<String> = row.get(20).unwrap_or(None);
        if let Some(ref pub_id) = publication_id {
            lit.publication = Self::get_publication(conn, pub_id)?;
        }

        debug!("数据库: 正在加载文献关联信息 (ID: {id})");
        let mut auth_stmt = conn.prepare("SELECT a.* FROM authors a JOIN literature_authors la ON a.id = la.author_id WHERE la.literature_id = ?1 AND la.is_deleted = 0 AND a.is_deleted = 0 ORDER BY la.sort_order")?;
        lit.authors = auth_stmt
            .query_map([id.clone()], |a_row| {
                Ok(Author {
                    id: a_row.get(0)?,
                    first_name: a_row.get(1)?,
                    last_name: a_row.get(2)?,
                    middle_name: a_row.get(3)?,
                    is_dirty: a_row.get(4)?,
                    is_deleted: a_row.get(5)?,
                    version: a_row.get(6)?,
                    created_at: a_row.get(7)?,
                    updated_at: a_row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>>>()?;
        debug!("数据库: 加载了 {} 位作者 (ID: {})", lit.authors.len(), id);

        let mut folder_stmt = conn.prepare(
            "SELECT folder_id FROM literature_folders WHERE literature_id = ?1 AND is_deleted = 0",
        )?;
        lit.folder_ids = folder_stmt
            .query_map([id.clone()], |f_row| f_row.get(0))?
            .collect::<Result<Vec<String>>>()?;
        debug!(
            "数据库: 加载了 {} 个文件夹关联 (ID: {})",
            lit.folder_ids.len(),
            id
        );

        let mut tag_stmt = conn.prepare("SELECT t.name FROM tags t JOIN literature_tags lt ON t.id = lt.tag_id WHERE lt.literature_id = ?1 AND lt.is_deleted = 0 AND t.is_deleted = 0")?;
        lit.tags = tag_stmt
            .query_map([id.clone()], |t_row| t_row.get(0))?
            .collect::<Result<Vec<String>>>()?;
        debug!("数据库: 加载了 {} 个标签 (ID: {})", lit.tags.len(), id);

        let mut att_stmt = conn.prepare("SELECT id, literature_id, file_path, file_name, file_size, mime_type, etag, hash, is_main, is_dirty, is_deleted, version, created_at, updated_at FROM attachments WHERE literature_id = ?1 AND is_deleted = 0")?;
        lit.attachments = att_stmt
            .query_map([id.clone()], |row| {
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
            })?
            .collect::<Result<Vec<Attachment>>>()?;
        info!(
            "数据库: 文献 {} 加载了 {} 个附件",
            id,
            lit.attachments.len()
        );

        Ok(lit)
    }

    /// 处理出版源：查找已存在的或创建新的
    /// 返回出版源 ID（如果有）
    fn set_publication(
        conn: &Connection,
        publication: Option<&Publication>,
    ) -> Result<Option<String>> {
        let Some(pub_data) = publication else {
            return Ok(None);
        };

        let now = Local::now().timestamp();
        let pub_type_str = pub_data.publication_type.to_string();

        // 首先查找是否已存在同名出版源（忽略 type 差异，避免重复）
        let existing_id: Option<String> = conn
            .query_row(
                "SELECT id FROM publications
                 WHERE LOWER(name) = LOWER(?1)
                 ORDER BY is_deleted ASC, version DESC
                 LIMIT 1",
                params![pub_data.name],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(existing_id) = existing_id {
            // 合并：更新现有记录的元数据（如 rank 信息）
            info!(
                "数据库: 找到已存在的出版源 '{}' (ID: {}), 执行合并",
                pub_data.name, existing_id
            );

            // 仅更新非空元数据字段
            if pub_data.ccf_rank.is_some()
                || pub_data.jcr_rank.is_some()
                || pub_data.cas_rank.is_some()
                || pub_data.publisher.is_some()
                || pub_data.abbreviation.is_some()
            {
                conn.execute(
                    "UPDATE publications SET
                        ccf_rank = COALESCE(?1, ccf_rank),
                        jcr_rank = COALESCE(?2, jcr_rank),
                        cas_rank = COALESCE(?3, cas_rank),
                        abbreviation = COALESCE(?4, abbreviation),
                        publisher = COALESCE(?5, publisher),
                        updated_at = ?6,
                        is_dirty = 1,
                        version = version + 1
                    WHERE id = ?7",
                    params![
                        pub_data.ccf_rank,
                        pub_data.jcr_rank,
                        pub_data.cas_rank,
                        pub_data.abbreviation,
                        pub_data.publisher,
                        now,
                        existing_id
                    ],
                )?;
            }

            Ok(Some(existing_id))
        } else {
            // 创建新的出版源
            let new_id = Uuid::new_v4().to_string();
            info!(
                "数据库: 创建新的出版源 '{}' (ID: {})",
                pub_data.name, new_id
            );

            conn.execute(
                "INSERT INTO publications (id, name, publication_type, abbreviation, publisher, ccf_rank, jcr_rank, cas_rank, is_dirty, is_deleted, version, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, 0, 1, ?9, ?10)",
                params![
                    new_id, pub_data.name, pub_type_str,
                    pub_data.abbreviation, pub_data.publisher,
                    pub_data.ccf_rank, pub_data.jcr_rank, pub_data.cas_rank,
                    now, now
                ],
            )?;

            Ok(Some(new_id))
        }
    }

    /// 根据 ID 获取出版源信息
    fn get_publication(conn: &Connection, id: &str) -> Result<Option<Publication>> {
        let pub_opt: Option<Publication> = conn
            .query_row(
                "SELECT id, name, publication_type, abbreviation, publisher, ccf_rank, jcr_rank, cas_rank, is_dirty, is_deleted, version, created_at, updated_at
                 FROM publications WHERE id = ?1 AND is_deleted = 0",
                params![id],
                |row| {
                    let pub_type_str: String = row.get(2)?;
                    let pub_type = match pub_type_str.as_str() {
                        "Conference" => PublicationType::Conference,
                        "Book" => PublicationType::Book,
                        _ => PublicationType::Journal,
                    };
                    Ok(Publication {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        publication_type: pub_type,
                        abbreviation: row.get(3)?,
                        publisher: row.get(4)?,
                        ccf_rank: row.get(5)?,
                        jcr_rank: row.get(6)?,
                        cas_rank: row.get(7)?,
                        is_dirty: row.get(8)?,
                        is_deleted: row.get(9)?,
                        version: row.get(10)?,
                        created_at: row.get(11)?,
                        updated_at: row.get(12)?,
                    })
                },
            )
            .optional()?;

        Ok(pub_opt)
    }

    fn set_authors(conn: &Connection, literature_id: &str, authors: &[Author]) -> Result<()> {
        debug!(
            "数据库: 正在更新文献作者关联关系 (ID: {}, 作者数: {})",
            literature_id,
            authors.len()
        );
        let now = Local::now().timestamp();
        let mut stmt = conn.prepare("SELECT author_id, is_deleted, version FROM literature_authors WHERE literature_id = ?1")?;
        let current_relations: Vec<(String, bool, i32)> = stmt
            .query_map([literature_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<Result<Vec<_>>>()?;
        let mut canonical_authors = Vec::new();
        for author in authors {
            let existing_id = Database::find_author_id_by_name(
                conn,
                &author.first_name,
                &author.last_name,
                author.middle_name.as_deref(),
            )?;

            if let Some(id) = existing_id {
                let mut a = author.clone();
                if a.id != id {
                    info!(
                        "数据库: 发现同名作者 '{} {}', 执行合并: 将临时 ID {} 修正为现有 ID {}",
                        author.first_name, author.last_name, a.id, id
                    );
                    a.id = id;
                }
                canonical_authors.push(a);
            } else {
                info!(
                    "数据库: 未发现同名作者, 使用新解析的 ID: {} (姓名: {} {})",
                    author.id, author.first_name, author.last_name
                );
                canonical_authors.push(author.clone());
            }
        }

        let target_ids: Vec<String> = canonical_authors.iter().map(|a| a.id.clone()).collect();
        let mut deleted_count = 0;
        for (auth_id, is_deleted, version) in &current_relations {
            if !target_ids.contains(auth_id) && !is_deleted {
                conn.execute("UPDATE literature_authors SET is_deleted = 1, is_dirty = 1, version = ?1, updated_at = ?2 WHERE literature_id = ?3 AND author_id = ?4", params![version + 1, now, literature_id, auth_id])?;
                deleted_count += 1;
            }
        }
        if deleted_count > 0 {
            debug!("数据库: 标记删除了 {deleted_count} 条过时的作者关联 (ID: {literature_id})");
        }

        for (i, author) in canonical_authors.iter().enumerate() {
            info!(
                "数据库: 正在关联作者, 文献ID: {}, 作者ID: {}, 姓名: {} {}, 排序: {}",
                literature_id, author.id, author.first_name, author.last_name, i
            );
            Database::upsert_author(conn, author)?;
            let existing = current_relations.iter().find(|(id, _, _)| id == &author.id);
            match existing {
                Some((_, is_deleted, version)) => {
                    conn.execute("UPDATE literature_authors SET sort_order = ?1, is_deleted = 0, is_dirty = 1, version = ?2, updated_at = ?3 WHERE literature_id = ?4 AND author_id = ?5", params![i as i32, if *is_deleted { version + 1 } else { *version }, now, literature_id, author.id])?;
                }
                None => {
                    conn.execute("INSERT INTO literature_authors (literature_id, author_id, sort_order, is_dirty, is_deleted, version, updated_at) VALUES (?1, ?2, ?3, 1, 0, 1, ?4)", params![literature_id, author.id, i as i32, now])?;
                }
            }
        }
        Ok(())
    }

    fn set_folders(conn: &Connection, literature_id: &str, folder_ids: &[String]) -> Result<()> {
        debug!(
            "数据库: 正在更新文献文件夹关联 (ID: {}, 文件夹数: {})",
            literature_id,
            folder_ids.len()
        );
        let now = Local::now().timestamp();
        let mut stmt = conn.prepare("SELECT folder_id, is_deleted, version FROM literature_folders WHERE literature_id = ?1")?;
        let current_relations: Vec<(String, bool, i32)> = stmt
            .query_map([literature_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<Result<Vec<_>>>()?;
        let mut deleted_count = 0;
        for (f_id, is_deleted, version) in &current_relations {
            if !folder_ids.contains(f_id) && !is_deleted {
                conn.execute("UPDATE literature_folders SET is_deleted = 1, is_dirty = 1, version = ?1, updated_at = ?2 WHERE literature_id = ?3 AND folder_id = ?4", params![version + 1, now, literature_id, f_id])?;
                deleted_count += 1;
            }
        }
        if deleted_count > 0 {
            debug!("数据库: 标记删除了 {deleted_count} 条过时的文件夹关联 (ID: {literature_id})");
        }

        let mut seen = std::collections::HashSet::new();
        for fid in folder_ids.iter().filter(|fid| seen.insert(*fid)) {
            let existing = current_relations.iter().find(|(id, _, _)| id == fid);
            if let Some((_, is_deleted, version)) = existing {
                if *is_deleted {
                    debug!(
                        "数据库: 恢复文件夹关联 (LiteratureID: {literature_id}, FolderID: {fid})"
                    );
                    conn.execute("UPDATE literature_folders SET is_deleted = 0, is_dirty = 1, version = ?1, updated_at = ?2 WHERE literature_id = ?3 AND folder_id = ?4", params![version + 1, now, literature_id, fid])?;
                }
            } else {
                debug!("数据库: 创建新文件夹关联 (LiteratureID: {literature_id}, FolderID: {fid})");
                conn.execute("INSERT INTO literature_folders (literature_id, folder_id, is_dirty, is_deleted, version, updated_at) VALUES (?1, ?2, 1, 0, 1, ?3)", params![literature_id, fid, now])?;
            }
        }
        Ok(())
    }

    fn set_tags(conn: &Connection, literature_id: &str, tags: &[String]) -> Result<()> {
        debug!(
            "数据库: 正在更新文献标签关联 (ID: {}, 标签数: {})",
            literature_id,
            tags.len()
        );
        let now = Local::now().timestamp();
        let mut stmt = conn.prepare("SELECT t.name, lt.is_deleted, lt.version FROM tags t JOIN literature_tags lt ON t.id = lt.tag_id WHERE lt.literature_id = ?1")?;
        let current_relations: Vec<(String, bool, i32)> = stmt
            .query_map([literature_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<Result<Vec<_>>>()?;
        let mut deleted_count = 0;
        for (tag_name, is_deleted, version) in &current_relations {
            if !tags.contains(tag_name) && !is_deleted {
                let tag_id: String =
                    conn.query_row("SELECT id FROM tags WHERE name = ?1", [tag_name], |row| {
                        row.get(0)
                    })?;
                conn.execute("UPDATE literature_tags SET is_deleted = 1, is_dirty = 1, version = ?1, updated_at = ?2 WHERE literature_id = ?3 AND tag_id = ?4", params![version + 1, now, literature_id, tag_id])?;
                deleted_count += 1;
            }
        }
        if deleted_count > 0 {
            debug!("数据库: 标记删除了 {deleted_count} 条过时的标签关联 (ID: {literature_id})");
        }

        for tag_name in tags {
            let tag_id: String = match Database::find_tag_id_by_name(conn, tag_name)? {
                Some(id) => id,
                None => {
                    let new_id = Uuid::new_v4().to_string();
                    Database::upsert_tag(conn, &new_id, tag_name, DEFAULT_TAG_COLOR, now)?;
                    new_id
                }
            };
            let existing = current_relations
                .iter()
                .find(|(name, _, _)| name == tag_name);
            if let Some((_, is_deleted, version)) = existing {
                if *is_deleted {
                    debug!("数据库: 恢复标签关联 (LiteratureID: {literature_id}, Tag: {tag_name})");
                    conn.execute("UPDATE literature_tags SET is_deleted = 0, is_dirty = 1, version = ?1, updated_at = ?2 WHERE literature_id = ?3 AND tag_id = ?4", params![version + 1, now, literature_id, tag_id])?;
                }
            } else {
                debug!("数据库: 创建新标签关联 (LiteratureID: {literature_id}, Tag: {tag_name})");
                conn.execute("INSERT INTO literature_tags (literature_id, tag_id, is_dirty, is_deleted, version, updated_at) VALUES (?1, ?2, 1, 0, 1, ?3)", params![literature_id, tag_id, now])?;
            }
        }
        Ok(())
    }

    fn set_attachments(
        conn: &Connection,
        literature_id: &str,
        attachments: &[Attachment],
    ) -> Result<()> {
        debug!(
            "数据库: 正在更新文献附件关联 (ID: {}, 附件数: {})",
            literature_id,
            attachments.len()
        );
        let now = Local::now().timestamp();
        let mut stmt = conn
            .prepare("SELECT id, is_deleted, version FROM attachments WHERE literature_id = ?1 AND is_deleted = 0")?;
        let current_relations: Vec<(String, bool, i32)> = stmt
            .query_map([literature_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<Result<Vec<_>>>()?;
        let mut deleted_count = 0;
        for (att_id, is_deleted, version) in &current_relations {
            if !attachments.iter().any(|a| &a.id == att_id) && !is_deleted {
                Database::soft_delete_attachment_conn(conn, att_id, version + 1, now)?;
                deleted_count += 1;
            }
        }
        if deleted_count > 0 {
            debug!("数据库: 标记删除了 {deleted_count} 条过时的附件关联 (ID: {literature_id})");
        }

        for att in attachments {
            let existing = current_relations.iter().find(|(id, _, _)| id == &att.id);
            if let Some((_, _, version)) = existing {
                let changed = {
                    let old = conn.query_row(
                        "SELECT file_path, file_name, file_size, mime_type, etag, is_main FROM attachments WHERE id = ?1",
                        params![att.id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, i64>(2)?,
                                row.get::<_, Option<String>>(3)?,
                                row.get::<_, Option<String>>(4)?,
                                row.get::<_, bool>(5)?,
                            ))
                        },
                    ).optional()?;
                    match old {
                        Some((old_path, old_name, old_size, old_mime, old_etag, old_main)) => {
                            old_path != att.file_path
                                || old_name != att.file_name
                                || old_size != att.file_size as i64
                                || old_mime != att.mime_type
                                || old_etag != att.etag
                                || old_main != att.is_main
                        }
                        None => true,
                    }
                };
                debug!(
                    "数据库: 更新现有附件 (ID: {}, FileName: {}, changed: {changed})",
                    att.id, att.file_name
                );
                Database::update_attachment_conn(conn, att, changed, version + 1, now)?;
            } else {
                debug!(
                    "数据库: 创建新附件关联 (ID: {}, FileName: {})",
                    att.id, att.file_name
                );
                Database::insert_attachment_conn(conn, att, literature_id, now)?;
                info!(
                    "数据库: 成功插入附件 {} 到文献 {}",
                    att.file_name, literature_id
                );
            }
        }
        Ok(())
    }

    pub fn get_dirty_relations(
        &self,
    ) -> Result<(
        Vec<AuthorRelationTuple>,
        Vec<CommonRelationTuple>,
        Vec<CommonRelationTuple>,
    )> {
        debug!("数据库: 正在获取所有标记为脏数据的文献关联关系");
        self.with_conn(|conn| {
            let mut authors = Vec::new(); let mut folders = Vec::new(); let mut tags = Vec::new();
            let mut stmt = conn.prepare("SELECT literature_id, author_id, sort_order, is_deleted, version FROM literature_authors WHERE is_dirty = 1")?;
            let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, Some(row.get::<_, i32>(2)?), row.get::<_, bool>(3)?, row.get::<_, i32>(4)?)))?;
            for r in rows { authors.push(r?); }

            let mut stmt = conn.prepare("SELECT literature_id, folder_id, is_deleted, version FROM literature_folders WHERE is_dirty = 1")?;
            let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?, row.get::<_, i32>(3)?)))?;
            for r in rows { folders.push(r?); }

            let mut stmt = conn.prepare("SELECT literature_id, tag_id, is_deleted, version FROM literature_tags WHERE is_dirty = 1")?;
            let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?, row.get::<_, i32>(3)?)))?;
            for r in rows { tags.push(r?); }

            debug!("数据库: 获取到脏数据关联: authors={}, folders={}, tags={}", authors.len(), folders.len(), tags.len());
            Ok((authors, folders, tags))
        })
    }

    pub fn mark_relation_synced(&self, table: &str, lit_id: &str, target_id: &str) -> Result<()> {
        debug!(
            "数据库: 正在标记关联关系已同步 (Table: {table}, LiteratureID: {lit_id}, TargetID: {target_id})"
        );
        let id_col = match table {
            "literature_authors" => "author_id",
            "literature_folders" => "folder_id",
            "literature_tags" => "tag_id",
            _ => return Err(rusqlite::Error::InvalidQuery),
        };
        let sql =
            format!("UPDATE {table} SET is_dirty = 0 WHERE literature_id = ?1 AND {id_col} = ?2");
        self.with_conn(|conn| {
            let rows = conn.execute(&sql, [lit_id, target_id])?;
            if rows == 0 {
                warn!("数据库: 标记关联同步失败，未找到匹配记录 (Table: {table}, LiteratureID: {lit_id}, TargetID: {target_id})");
            } else {
                debug!("数据库: 关联关系标记同步成功");
            }
            Ok(())
        })
    }

    /// 读取文献关联本地同步状态 `(version, is_dirty)`。
    pub fn get_relation_sync_state(
        &self,
        table: &str,
        lit_id: &str,
        target_id: &str,
    ) -> Result<Option<(i32, bool)>> {
        let id_col = match table {
            "literature_authors" => "author_id",
            "literature_folders" => "folder_id",
            "literature_tags" => "tag_id",
            _ => return Err(rusqlite::Error::InvalidQuery),
        };
        self.with_conn(|conn| {
            let sql = format!(
                "SELECT version, is_dirty FROM {table} WHERE literature_id = ?1 AND {id_col} = ?2"
            );
            conn.query_row(&sql, [lit_id, target_id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()
        })
    }

    /// 原子原语：把远程关联盲目 upsert 到本地（覆盖写或插入）。
    pub fn apply_remote_relation(
        &self,
        table: &str,
        lit_id: &str,
        target_id: &str,
        sort_order: Option<i32>,
        is_deleted: bool,
        version: i32,
    ) -> Result<()> {
        self.with_conn(|conn| {
            self.upsert_relation_internal(
                conn,
                table,
                RelationUpsertData {
                    lit_id: lit_id.to_string(),
                    target_id: target_id.to_string(),
                    sort_order,
                    is_deleted,
                    version,
                },
            )
        })
    }

    /// 原子原语：关联版本一致且本地无修改时，仅清除 dirty 标记。
    pub fn mark_relation_up_to_date(
        &self,
        table: &str,
        lit_id: &str,
        target_id: &str,
    ) -> Result<()> {
        let id_col = match table {
            "literature_authors" => "author_id",
            "literature_folders" => "folder_id",
            "literature_tags" => "tag_id",
            _ => return Err(rusqlite::Error::InvalidQuery),
        };
        self.with_conn(|conn| {
            let sql = format!(
                "UPDATE {table} SET is_dirty = 0 WHERE literature_id = ?1 AND {id_col} = ?2"
            );
            conn.execute(&sql, [lit_id, target_id])?;
            Ok(())
        })
    }

    /// 合并文献关联关系（将源文献的关联迁移到目标文献）
    pub fn merge_literature_relations(&self, source_id: &str, target_id: &str) -> Result<()> {
        info!("数据库: 开始合并文献关联关系 ({source_id} -> {target_id})");

        // 1. 合并标签
        if let Err(e) = self.merge_literature_tags(source_id, target_id) {
            error!("合并标签失败: {e}");
        }

        // 2. 合并文件夹
        if let Err(e) = self.merge_literature_folders(source_id, target_id) {
            error!("合并文件夹失败: {e}");
        }

        // 3. 合并附件
        if let Err(e) = self.merge_attachments(source_id, target_id) {
            error!("合并附件失败: {e}");
        }

        // 4. 合并引用关系
        if let Err(e) = self.merge_citations(source_id, target_id) {
            error!("合并引用关系失败: {e}");
        }

        // 5. 合并元数据 (Rating, ReadingStatus)
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT rating, reading_status FROM literatures WHERE id = ?1")?;

            let source_data = stmt.query_row([source_id], |row| {
                Ok((
                    row.get::<_, i32>(0)?,
                    row.get::<_, String>(1)?,
                ))
            }).optional()?;

            let target_data = stmt.query_row([target_id], |row| {
                Ok((
                    row.get::<_, i32>(0)?,
                    row.get::<_, String>(1)?,
                ))
            }).optional()?;

            if let (Some((s_rating, s_status)), Some((t_rating, t_status))) = (source_data, target_data) {
                let now = Local::now().timestamp();

                let new_rating = std::cmp::max(s_rating, t_rating);

                fn status_score(s: &str) -> i32 {
                    match s {
                        "Read" => 3,
                        "Reading" => 2,
                        _ => 1,
                    }
                }
                let new_status = if status_score(&s_status) > status_score(&t_status) { s_status } else { t_status };

                conn.execute(
                    "UPDATE literatures SET rating = ?1, reading_status = ?2, is_dirty = 1, version = version + 1, updated_at = ?3 WHERE id = ?4",
                    params![new_rating, new_status, now, target_id]
                )?;
            }

            // 6. 合并笔记：将源文献的所有笔记改挂到目标文献
            conn.execute(
                "UPDATE literature_notes SET literature_id = ?1 WHERE literature_id = ?2",
                params![target_id, source_id],
            )?;

            Ok(())
        })
    }

    fn upsert_relation_internal(
        &self,
        conn: &Connection,
        table: &str,
        data: RelationUpsertData,
    ) -> Result<()> {
        debug!(
            "数据库: 正在执行内部关联 Upsert (Table: {}, LiteratureID: {}, TargetID: {})",
            table, data.lit_id, data.target_id
        );
        let now = Local::now().timestamp();
        match table {
            "literature_authors" => {
                conn.execute("INSERT OR REPLACE INTO literature_authors (literature_id, author_id, sort_order, is_dirty, is_deleted, version, updated_at) VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6)", params![data.lit_id, data.target_id, data.sort_order.unwrap_or(0), data.is_deleted, data.version, now])?;
            }
            "literature_folders" => {
                conn.execute("INSERT OR REPLACE INTO literature_folders (literature_id, folder_id, is_dirty, is_deleted, version, updated_at) VALUES (?1, ?2, 0, ?3, ?4, ?5)", params![data.lit_id, data.target_id, data.is_deleted, data.version, now])?;
            }
            "literature_tags" => {
                conn.execute("INSERT OR REPLACE INTO literature_tags (literature_id, tag_id, is_dirty, is_deleted, version, updated_at) VALUES (?1, ?2, 0, ?3, ?4, ?5)", params![data.lit_id, data.target_id, data.is_deleted, data.version, now])?;
            }
            _ => {}
        }
        Ok(())
    }

    /// 内部方法：合并文献标签关联
    fn merge_literature_tags(&self, source_id: &str, target_id: &str) -> Result<()> {
        self.with_conn(|conn| {
            let now = Local::now().timestamp();
            // 找出源文献的所有标签
            let mut stmt = conn.prepare("SELECT tag_id, version FROM literature_tags WHERE literature_id = ?1 AND is_deleted = 0")?;
            let tag_relations: Vec<(String, i32)> = stmt.query_map([source_id], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<Vec<_>>>()?;

            for (tag_id, version) in tag_relations {
                // 检查目标文献是否已有关联
                let exists: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM literature_tags WHERE literature_id = ?1 AND tag_id = ?2)",
                    [target_id, &tag_id],
                    |row| row.get(0)
                )?;

                if exists {
                    // 如果已存在关联，确保它是未删除状态
                    conn.execute(
                        "UPDATE literature_tags SET is_deleted = 0, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE literature_id = ?2 AND tag_id = ?3",
                        params![now, target_id, tag_id]
                    )?;
                } else {
                    // 插入新关联
                    conn.execute(
                        "INSERT INTO literature_tags (literature_id, tag_id, is_dirty, is_deleted, version, updated_at) VALUES (?1, ?2, 1, 0, 1, ?3)",
                        params![target_id, tag_id, now]
                    )?;
                }
                // 软删除源文献关联
                conn.execute(
                    "UPDATE literature_tags SET is_deleted = 1, is_dirty = 1, version = ?1, updated_at = ?2 WHERE literature_id = ?3 AND tag_id = ?4",
                    params![version + 1, now, source_id, tag_id]
                )?;
            }
            Ok(())
        })
    }

    /// 内部方法：合并文献文件夹关联
    fn merge_literature_folders(&self, source_id: &str, target_id: &str) -> Result<()> {
        self.with_conn(|conn| {
            let now = Local::now().timestamp();
            let mut stmt = conn.prepare("SELECT folder_id, version FROM literature_folders WHERE literature_id = ?1 AND is_deleted = 0")?;
            let folder_relations: Vec<(String, i32)> = stmt.query_map([source_id], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<Vec<_>>>()?;

            for (folder_id, version) in folder_relations {
                let exists: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM literature_folders WHERE literature_id = ?1 AND folder_id = ?2)",
                    [target_id, &folder_id],
                    |row| row.get(0)
                )?;

                if exists {
                    conn.execute(
                        "UPDATE literature_folders SET is_deleted = 0, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE literature_id = ?2 AND folder_id = ?3",
                        params![now, target_id, folder_id]
                    )?;
                } else {
                    conn.execute(
                        "INSERT INTO literature_folders (literature_id, folder_id, is_dirty, is_deleted, version, updated_at) VALUES (?1, ?2, 1, 0, 1, ?3)",
                        params![target_id, folder_id, now]
                    )?;
                }
                conn.execute(
                    "UPDATE literature_folders SET is_deleted = 1, is_dirty = 1, version = ?1, updated_at = ?2 WHERE literature_id = ?3 AND folder_id = ?4",
                    params![version + 1, now, source_id, folder_id]
                )?;
            }
            Ok(())
        })
    }
}
