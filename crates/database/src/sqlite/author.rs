use super::Database;
use log::{debug, info};
use models::Author;
use rusqlite::{Connection, OptionalExtension, Result, params};

impl Database {
    /// 在事务内根据姓名查找作者 ID
    pub fn find_author_id_by_name(
        conn: &Connection,
        first: &str,
        last: &str,
        middle: Option<&str>,
    ) -> Result<Option<String>> {
        let middle = middle.unwrap_or("");
        conn.query_row(
            "SELECT id FROM authors WHERE first_name = ?1 AND last_name = ?2 AND COALESCE(middle_name, '') = ?3 AND is_deleted = 0 LIMIT 1",
            params![first, last, middle],
            |row| row.get(0),
        )
        .optional()
    }

    /// 在事务内插入或替换作者行
    pub fn upsert_author(conn: &Connection, author: &Author) -> Result<()> {
        conn.execute(
            "INSERT OR REPLACE INTO authors (id, first_name, last_name, middle_name, is_dirty, is_deleted, version, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![author.id, author.first_name, author.last_name, author.middle_name, author.is_dirty, author.is_deleted, author.version, author.created_at, author.updated_at],
        )?;
        Ok(())
    }

    // --- 同步支持方法 ---

    pub fn get_dirty_authors(&self) -> Result<Vec<Author>> {
        debug!("数据库: 正在获取待同步作者记录");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT id, first_name, last_name, middle_name, is_dirty, is_deleted, version, created_at, updated_at FROM authors WHERE is_dirty = 1")?;
            let iter = stmt.query_map([], |row| Ok(Author { id: row.get(0)?, first_name: row.get(1)?, last_name: row.get(2)?, middle_name: row.get(3)?, is_dirty: row.get(4)?, is_deleted: row.get(5)?, version: row.get(6)?, created_at: row.get(7)?, updated_at: row.get(8)? }))?;
            let mut authors = Vec::new();
            for a in iter { authors.push(a?); }
            debug!("数据库: 获取到 {} 个待同步作者", authors.len());
            Ok(authors)
        })
    }

    pub fn mark_author_synced(&self, id: &str) -> Result<()> {
        debug!("数据库: 标记作者为已同步 (ID: {id})");
        self.with_conn(|conn| {
            conn.execute("UPDATE authors SET is_dirty = 0 WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// 原子原语：把远程作者盲目 upsert 到本地（覆盖写或插入）。
    pub fn apply_remote_author(&self, remote: &Author) -> Result<()> {
        self.with_conn(|conn| Self::insert_author_internal(conn, remote))
    }

    /// 原子原语：版本一致且本地无修改时，仅刷新时间戳并清脏标记。
    pub fn mark_author_up_to_date(&self, remote: &Author) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE authors SET updated_at = ?1, is_dirty = 0 WHERE id = ?2",
                params![remote.updated_at, remote.id],
            )?;
            Ok(())
        })
    }

    fn insert_author_internal(conn: &Connection, author: &Author) -> Result<()> {
        conn.execute(
            "INSERT OR REPLACE INTO authors (id, first_name, last_name, middle_name, is_dirty, is_deleted, version, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![author.id, author.first_name, author.last_name, author.middle_name, 0, author.is_deleted, author.version, author.created_at, author.updated_at],
        )?;
        Ok(())
    }

    pub fn purge_synced_authors(&self) -> Result<usize> {
        info!("数据库: 正在清理已同步的删除作者记录");
        self.with_conn(|conn| {
            let count = conn.execute(
                "DELETE FROM authors WHERE is_deleted = 1 AND is_dirty = 0",
                [],
            )?;
            info!("数据库: 已清理 {count} 个已同步删除的作者");
            Ok(count)
        })
    }
}
