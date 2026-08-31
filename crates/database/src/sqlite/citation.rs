use super::Database;
use chrono::Local;
use log::{debug, info};
use models::Citation;
use rusqlite::{Result, params};

impl Database {
    /// 添加引用关联
    pub fn add_citation(&self, source_id: &str, target_id: &str) -> Result<()> {
        debug!("数据库: 添加引用关联 ({source_id} -> {target_id})");
        let now = Local::now().timestamp();
        self.with_conn(|conn| {
            // 检查是否存在
            let exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM literature_citations WHERE source_id = ?1 AND target_id = ?2)",
                [source_id, target_id],
                |row| row.get(0),
            )?;

            if exists {
                // 如果存在，更新状态为未删除，并更新版本和时间
                conn.execute(
                    "UPDATE literature_citations
                     SET is_deleted = 0, version = version + 1, updated_at = ?3, is_dirty = 1
                     WHERE source_id = ?1 AND target_id = ?2",
                    params![source_id, target_id, now],
                )?;
            } else {
                // 插入新记录
                conn.execute(
                    "INSERT INTO literature_citations (source_id, target_id, is_deleted, version, updated_at, is_dirty)
                     VALUES (?1, ?2, 0, 1, ?3, 1)",
                    params![source_id, target_id, now],
                )?;
            }

            // 更新 source 和 target 文献的 version 和 updated_at，以触发 UI 刷新
            conn.execute(
                "UPDATE literatures SET version = version + 1, updated_at = ?2 WHERE id = ?1",
                params![source_id, now],
            )?;
            conn.execute(
                "UPDATE literatures SET version = version + 1, updated_at = ?2 WHERE id = ?1",
                params![target_id, now],
            )?;

            Ok(())
        })
    }

    /// 移除引用关联 (软删除)
    pub fn remove_citation(&self, source_id: &str, target_id: &str) -> Result<()> {
        debug!("数据库: 移除引用关联 ({source_id} -> {target_id})");
        let now = Local::now().timestamp();
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE literature_citations
                 SET is_deleted = 1, version = version + 1, updated_at = ?3, is_dirty = 1
                 WHERE source_id = ?1 AND target_id = ?2",
                params![source_id, target_id, now],
            )?;

            // 更新 source 和 target 文献的 version 和 updated_at，以触发 UI 刷新
            conn.execute(
                "UPDATE literatures SET version = version + 1, updated_at = ?2 WHERE id = ?1",
                params![source_id, now],
            )?;
            conn.execute(
                "UPDATE literatures SET version = version + 1, updated_at = ?2 WHERE id = ?1",
                params![target_id, now],
            )?;

            Ok(())
        })
    }

    /// 获取某篇文献的“引用了哪些文献” (References)
    pub fn get_references(&self, source_id: &str) -> Result<Vec<Citation>> {
        self.get_citations(source_id, true)
    }

    /// 获取某篇文献“被哪些文献引用” (Cited By)
    pub fn get_cited_by(&self, target_id: &str) -> Result<Vec<Citation>> {
        self.get_citations(target_id, false)
    }

    /// 内部获取关联方法
    fn get_citations(&self, id: &str, is_source: bool) -> Result<Vec<Citation>> {
        self.with_conn(|conn| {
            let sql = if is_source {
                "SELECT source_id, target_id, is_deleted, version, updated_at
                 FROM literature_citations
                 WHERE source_id = ?1 AND is_deleted = 0"
            } else {
                "SELECT source_id, target_id, is_deleted, version, updated_at
                 FROM literature_citations
                 WHERE target_id = ?1 AND is_deleted = 0"
            };

            let mut stmt = conn.prepare(sql)?;
            let citation_iter = stmt.query_map([id], |row| {
                Ok(Citation {
                    source_id: row.get(0)?,
                    target_id: row.get(1)?,
                    is_deleted: row.get(2)?,
                    version: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })?;

            let mut citations = Vec::new();
            for citation in citation_iter {
                citations.push(citation?);
            }
            Ok(citations)
        })
    }

    // --- Sync Methods ---

    /// 获取所有脏引用记录 (用于同步)
    pub fn get_dirty_citations(&self) -> Result<Vec<Citation>> {
        debug!("数据库: 正在获取待同步引用记录");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT source_id, target_id, is_deleted, version, updated_at
                 FROM literature_citations
                 WHERE is_dirty = 1",
            )?;
            let iter = stmt.query_map([], |row| {
                Ok(Citation {
                    source_id: row.get(0)?,
                    target_id: row.get(1)?,
                    is_deleted: row.get(2)?,
                    version: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })?;

            let mut res = Vec::new();
            for c in iter {
                res.push(c?);
            }
            debug!("数据库: 获取到 {} 条待同步引用记录", res.len());
            Ok(res)
        })
    }

    /// 标记引用记录已同步
    pub fn mark_citation_synced(&self, source_id: &str, target_id: &str) -> Result<()> {
        debug!("数据库: 标记引用已同步 ({source_id} -> {target_id})");
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE literature_citations SET is_dirty = 0 WHERE source_id = ?1 AND target_id = ?2",
                [source_id, target_id],
            )?;
            Ok(())
        })
    }

    /// 原子原语：把远程引用盲目 upsert 到本地（覆盖写或插入）。
    pub fn apply_remote_citation(&self, remote: &Citation) -> Result<()> {
        self.with_conn(|conn| {
            let n = conn.execute(
                "UPDATE literature_citations SET is_deleted = ?3, version = ?4, updated_at = ?5, is_dirty = 0 WHERE source_id = ?1 AND target_id = ?2",
                params![&remote.source_id, &remote.target_id, remote.is_deleted, remote.version, &remote.updated_at],
            )?;
            if n == 0 {
                conn.execute(
                    "INSERT INTO literature_citations (source_id, target_id, is_deleted, version, updated_at, is_dirty) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
                    params![&remote.source_id, &remote.target_id, remote.is_deleted, remote.version, &remote.updated_at],
                )?;
            }
            Ok(())
        })
    }

    /// 合并引用关系：将源文献的引用关系迁移到目标文献
    pub fn merge_citations(&self, source_id: &str, target_id: &str) -> Result<()> {
        info!("数据库: 正在合并引用关系 ({source_id} -> {target_id})");
        self.with_conn(|conn| {
            let now = Local::now().timestamp();

            // 1. 处理 References: S -> X  ==> T -> X
            let mut stmt = conn.prepare("SELECT target_id FROM literature_citations WHERE source_id = ?1 AND is_deleted = 0")?;
            let references: Vec<String> = stmt.query_map([source_id], |row| row.get(0))?
                .collect::<Result<Vec<String>>>()?;

            for ref_id in references {
                if ref_id == target_id { continue; } // 避免自引用

                // 检查 T -> ref_id 是否存在
                let exists: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM literature_citations WHERE source_id = ?1 AND target_id = ?2)",
                    [target_id, &ref_id],
                    |row| row.get(0)
                )?;

                if !exists {
                    conn.execute(
                        "INSERT INTO literature_citations (source_id, target_id, is_deleted, version, updated_at, is_dirty) VALUES (?1, ?2, 0, 1, ?3, 1)",
                        params![target_id, ref_id, now]
                    )?;
                }

                // 删除旧引用 S -> ref_id
                conn.execute(
                    "UPDATE literature_citations SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE source_id = ?2 AND target_id = ?3",
                    params![now, source_id, ref_id]
                )?;
            }

            // 2. 处理 Cited By: Y -> S ==> Y -> T
            let mut stmt = conn.prepare("SELECT source_id FROM literature_citations WHERE target_id = ?1 AND is_deleted = 0")?;
            let cited_by: Vec<String> = stmt.query_map([source_id], |row| row.get(0))?
                .collect::<Result<Vec<String>>>()?;

            for citing_id in cited_by {
                if citing_id == target_id { continue; } // 避免自引用

                // 检查 Y -> T 是否存在
                let exists: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM literature_citations WHERE source_id = ?1 AND target_id = ?2)",
                    [&citing_id, target_id],
                    |row| row.get(0)
                )?;

                if !exists {
                    conn.execute(
                        "INSERT INTO literature_citations (source_id, target_id, is_deleted, version, updated_at, is_dirty) VALUES (?1, ?2, 0, 1, ?3, 1)",
                        params![citing_id, target_id, now]
                    )?;
                }

                // 删除旧引用 Y -> S
                conn.execute(
                    "UPDATE literature_citations SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE source_id = ?2 AND target_id = ?3",
                    params![now, citing_id, source_id]
                )?;
            }

            Ok(())
        })
    }

    pub fn purge_synced_citations(&self) -> Result<usize> {
        info!("数据库: 正在清理已同步的删除引用记录");
        self.with_conn(|conn| {
            let count = conn.execute(
                "DELETE FROM literature_citations WHERE is_deleted = 1 AND is_dirty = 0",
                [],
            )?;
            info!("数据库: 已清理 {count} 条已同步删除的引用");
            Ok(count)
        })
    }
}
