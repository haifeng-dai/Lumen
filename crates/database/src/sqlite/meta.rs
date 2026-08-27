use log::{debug, info};
use rusqlite::Result;

use super::Database;

impl Database {
    fn drop_all_tables(&self) -> Result<()> {
        info!("警告: 正在删除数据库所有表!");
        let tables = self.get_table_names();
        self.with_conn(|conn| {
            for table in tables {
                debug!("正在删除表: {table}");
                conn.execute(&format!("DROP TABLE IF EXISTS {table}"), [])?;
            }
            Ok(())
        })
    }
    pub fn rebuild_schema(&self) -> Result<()> {
        info!("正在重建数据库结构...");
        self.drop_all_tables()?;

        self.init_tables()?;

        Ok(())
    }

    pub fn get_sync_meta(&self, key: &str) -> Result<Option<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT value FROM sync_meta WHERE key = ?1")?;

            let mut rows = stmt.query([key])?;

            if let Some(row) = rows.next()? {
                Ok(Some(row.get(0)?))
            } else {
                Ok(None)
            }
        })
    }
    pub fn set_sync_meta(&self, key: &str, value: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO sync_meta (key, value) VALUES (?1, ?2)",
                [key, value],
            )?;

            Ok(())
        })
    }
    pub fn get_last_sync_time(&self, table: &str) -> Result<Option<String>> {
        self.get_sync_meta(&format!("last_sync_{table}"))
    }
    pub fn set_last_sync_time(&self, table: &str, time: &str) -> Result<()> {
        self.set_sync_meta(&format!("last_sync_{table}"), time)
    }
    pub fn mark_all_dirty_for_sync(&self) -> Result<()> {
        self.with_conn(|conn| {
            let tables = [
                "literatures",
                "authors",
                "folders",
                "tags",
                "literature_authors",
                "literature_folders",
                "literature_tags",
                "attachments",
                "feeds",
                "feed_items",
                "literature_citations",
                "annotations",
                "literature_notes",
            ];
            for table in tables {
                conn.execute(&format!("UPDATE {table} SET is_dirty = 1"), [])?;
            }
            Ok(())
        })
    }
    pub fn clear_sync_timestamps(&self) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM sync_meta WHERE key LIKE 'last_sync_%'", [])?;
            Ok(())
        })
    }
    pub fn clear_attachment_etags(&self) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute("UPDATE attachments SET etag = NULL", [])?;
            Ok(())
        })
    }
}
