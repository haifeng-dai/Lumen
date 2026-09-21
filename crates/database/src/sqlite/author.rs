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
        let existing: Option<(String, String, Option<String>, bool, bool, i64, i64, i64)> = conn
            .query_row("SELECT first_name,last_name,middle_name,is_dirty,is_deleted,version,created_at,updated_at FROM authors WHERE id=?1", [&author.id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?)))
            .optional()?;
        if let Some((first, last, middle, dirty, deleted, version, created_at, _updated_at)) =
            existing
        {
            let changed = first != author.first_name
                || last != author.last_name
                || middle != author.middle_name
                || deleted != author.is_deleted;
            conn.execute("UPDATE authors SET first_name=?1,last_name=?2,middle_name=?3,is_deleted=?4,is_dirty=?5,version=?6,created_at=?7,updated_at=?8 WHERE id=?9", params![author.first_name,author.last_name,author.middle_name,author.is_deleted, dirty || changed, if changed { version + 1 } else { version }, created_at, if changed { author.updated_at } else { _updated_at }, author.id])?;
        } else {
            conn.execute("INSERT INTO authors (id, first_name, last_name, middle_name, is_dirty, is_deleted, version, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)", params![author.id, author.first_name, author.last_name, author.middle_name, author.is_dirty, author.is_deleted, author.version.max(1), author.created_at, author.updated_at])?;
        }
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
        let existing = conn
            .query_row(
                "SELECT created_at FROM authors WHERE id=?1",
                [&author.id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if let Some(created_at) = existing {
            conn.execute("UPDATE authors SET first_name=?1,last_name=?2,middle_name=?3,is_dirty=0,is_deleted=?4,version=?5,created_at=?6,updated_at=?7 WHERE id=?8", params![author.first_name,author.last_name,author.middle_name,author.is_deleted,author.version,created_at,author.updated_at,author.id])?;
        } else {
            conn.execute("INSERT INTO authors (id, first_name, last_name, middle_name, is_dirty, is_deleted, version, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8)", params![author.id, author.first_name, author.last_name, author.middle_name, author.is_deleted, author.version.max(1), author.created_at, author.updated_at])?;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_author_preserves_sync_state_and_advances_only_on_change() {
        let db = Database::new(":memory:").unwrap();
        let base = Author {
            id: "a".into(),
            first_name: "A".into(),
            last_name: "B".into(),
            middle_name: None,
            is_dirty: true,
            is_deleted: false,
            version: 1,
            created_at: 1,
            updated_at: 1,
        };
        db.with_conn(|c| {
            Database::upsert_author(c, &base)?;
            c.execute("UPDATE authors SET synced_version=7 WHERE id='a'", [])?;
            Ok(())
        })
        .unwrap();
        let same = Author {
            version: 1,
            ..base.clone()
        };
        db.with_conn(|c| Database::upsert_author(c, &same)).unwrap();
        let changed = Author {
            first_name: "Changed".into(),
            version: 1,
            ..base
        };
        db.with_conn(|c| Database::upsert_author(c, &changed))
            .unwrap();
        db.with_conn(|c| { let row: (String, i32, bool, i64, i64) = c.query_row("SELECT first_name,version,is_dirty,synced_version,created_at FROM authors WHERE id='a'", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?; assert_eq!(row, ("Changed".into(), 2, true, 7, 1)); Ok(()) }).unwrap();
    }
}
