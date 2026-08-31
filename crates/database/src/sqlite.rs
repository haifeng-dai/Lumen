use log::info;
use rusqlite::{Connection, Result, Transaction};
use std::{fmt, path::Path, sync::Mutex};

mod annotation;
mod attachment;
mod author;

mod citation;
mod feed;
mod feed_item;
mod folder;
mod literature;
mod literature_notes;
mod publication;
mod tag;

mod meta;
mod schema;
mod sync_state;

pub use sync_state::{
    DatabaseSyncSummary, LocalSyncState, SyncConflict, SyncEntityKey, SyncEntityType,
    canonical_key, decode_relation_key,
};
/// 数据库管理器
pub struct Database {
    /// 使用 Mutex 确保 Connection 在多线程环境下是 Sync 的
    conn: Mutex<Connection>,
}

impl fmt::Debug for Database {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Database").finish()
    }
}

impl Database {
    /// 创建并初始化数据库
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        info!("正在打开本地数据库: {db_path:?}");
        let conn = Connection::open(&db_path)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_tables()?;
        db.init_default_data()?;
        Ok(db)
    }

    /// 内部辅助方法：获取连接锁并执行数据库操作
    pub(crate) fn with_conn<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R>,
    {
        let conn = self
            .conn
            .lock()
            .map_err(|_| rusqlite::Error::ExecuteReturnedResults)?;
        f(&conn)
    }

    /// 内部辅助方法：获取连接锁并在显式事务中执行数据库操作
    pub(crate) fn with_transaction<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Transaction) -> Result<R>,
    {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| rusqlite::Error::ExecuteReturnedResults)?;
        let tx = conn.transaction()?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }

    /// Physically removes every locally soft-deleted row.
    ///
    /// This is intentionally separate from the normal synced-tombstone purge:
    /// callers must first finish remote cleanup before invoking it.
    pub fn purge_all_deleted(&self) -> Result<(usize, Vec<String>)> {
        self.with_transaction(|tx| {
            let mut attachment_paths = Vec::new();
            let mut stmt = tx.prepare("SELECT file_path FROM attachments WHERE is_deleted = 1")?;
            for path in stmt.query_map([], |row| row.get::<_, String>(0))? {
                attachment_paths.push(path?);
            }

            let tables = [
                "literature_notes",
                "literature_authors",
                "literature_folders",
                "literature_tags",
                "literature_citations",
                "attachments",
                "annotations",
                "literatures",
                "folders",
                "tags",
                "feeds",
                "feed_items",
                "authors",
                "publications",
            ];
            let mut total = 0;
            for table in tables {
                total += tx.execute(&format!("DELETE FROM {table} WHERE is_deleted = 1"), [])?;
            }

            Ok((total, attachment_paths))
        })
    }
}
