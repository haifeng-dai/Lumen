use super::Database;
use log::{debug, info, warn};
use models::FeedItem;
use models::constructors::*;
use rusqlite::{Result, params};
use serde_json;

impl Database {
    // --- Feed Item Operations (Subscription DB) ---

    pub fn insert_feed_item(&self, item: &FeedItem) -> Result<()> {
        info!(
            "数据库: 准备写入订阅条目 '{}' (ID: {}, DOI: {:?})",
            item.title, item.id, item.doi
        );
        let authors_json =
            serde_json::to_string(&item.authors).unwrap_or_else(|_| "[]".to_string());

        let type_str = serde_json::to_string(&item.literature_type)
            .unwrap_or_else(|_| "\"Article\"".to_string());

        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO feed_items (
                    id, title, feed_id, is_read, is_added_to_library, added_at,
                    authors, year, type, journal, publisher, abstract_text, doi, url,
                    volume, issue, pages, published_at, is_dirty, is_deleted, version, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
                params![
                    item.id, item.title, item.feed_id, item.is_read, item.is_added_to_library, item.added_at,
                    authors_json, item.year, type_str, item.journal, item.publisher, item.abstract_text, item.doi, item.url,
                    item.volume, item.issue, item.pages, item.published_at, item.is_dirty, item.is_deleted, item.version, item.updated_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_feed_items_by_feed(&self, feed_id: &str) -> Result<Vec<FeedItem>> {
        debug!("数据库: 正在获取订阅源 (ID: {feed_id}) 的所有条目");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT
                id, title, feed_id, is_read, is_added_to_library, added_at,
                authors, year, type, journal, publisher, abstract_text, doi, url,

                volume, issue, pages, published_at, is_dirty, is_deleted, version, updated_at

                FROM feed_items WHERE feed_id = ?1 AND is_deleted = 0 ORDER BY added_at DESC",
            )?;

            let item_iter = stmt.query_map([feed_id], |row| {
                let id: String = row.get(0)?;

                let title: String = row.get(1)?;

                let fid: String = row.get(2)?;

                let mut item = create_feed_item(id, title, fid);

                item.is_read = row.get(3)?;

                item.is_added_to_library = row.get(4)?;

                item.added_at = row.get(5)?;

                let authors_json: Option<String> = row.get(6)?;

                if let Some(json) = authors_json {
                    item.authors = serde_json::from_str(&json).unwrap_or_default();
                }

                item.year = row.get(7)?;

                let type_json: Option<String> = row.get(8)?;

                if let Some(json) = type_json {
                    item.literature_type =
                        serde_json::from_str(&json).unwrap_or(models::LiteratureType::Article);
                }

                item.journal = row.get(9)?;

                item.publisher = row.get(10)?;

                item.abstract_text = row.get(11)?;

                item.doi = row.get(12)?;

                item.url = row.get(13)?;

                item.volume = row.get(14)?;

                item.issue = row.get(15)?;

                item.pages = row.get(16)?;

                item.published_at = row.get(17)?;

                item.is_dirty = row.get(18)?;

                item.is_deleted = row.get(19)?;

                item.version = row.get(20)?;

                item.updated_at = row.get(21)?;

                Ok(item)
            })?;

            let mut items = Vec::new();

            for i in item_iter {
                items.push(i?);
            }

            debug!(
                "数据库: 成功获取订阅源 {} 的 {} 个条目",
                feed_id,
                items.len()
            );
            Ok(items)
        })
    }

    pub fn get_feed_item(&self, id: &str) -> Result<Option<FeedItem>> {
        debug!("数据库: 正在获取订阅条目 (ID: {id})");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, title, feed_id, is_read, is_added_to_library, added_at, authors, year, type, journal, publisher, abstract_text, doi, url, volume, issue, pages, published_at, is_dirty, is_deleted, version, updated_at FROM feed_items WHERE id = ?1 AND is_deleted = 0",
            )?;
            let mut rows = stmt.query([id])?;
            if let Some(row) = rows.next()? {
                let item_id: String = row.get(0)?;
                let title: String = row.get(1)?;
                let fid: String = row.get(2)?;
                let mut item = models::constructors::create_feed_item(item_id, title, fid);
                item.is_read = row.get(3)?;
                item.is_added_to_library = row.get(4)?;
                item.added_at = row.get(5)?;
                let authors_json: Option<String> = row.get(6)?;
                if let Some(json) = authors_json {
                    item.authors = serde_json::from_str(&json).unwrap_or_default();
                }
                item.year = row.get(7)?;
                let type_json: Option<String> = row.get(8)?;
                item.literature_type = type_json
                    .and_then(|j| serde_json::from_str(&j).ok())
                    .unwrap_or(models::LiteratureType::Article);
                item.journal = row.get(9)?;
                item.publisher = row.get(10)?;
                item.abstract_text = row.get(11)?;
                item.doi = row.get(12)?;
                item.url = row.get(13)?;
                item.volume = row.get(14)?;
                item.issue = row.get(15)?;
                item.pages = row.get(16)?;
                item.published_at = row.get(17)?;
                item.is_dirty = row.get(18)?;
                item.is_deleted = row.get(19)?;
                item.version = row.get(20)?;
                item.updated_at = row.get(21)?;
                Ok(Some(item))
            } else {
                Ok(None)
            }
        })
    }

    pub fn update_feed_item_read_status(&self, id: &str, is_read: bool) -> Result<()> {
        debug!("数据库: 更新订阅条目已读状态 (ID: {id}, is_read: {is_read})");
        self.with_conn(|conn| {
            let rows = conn.execute(
                "UPDATE feed_items SET is_read = ?1, is_dirty = 1, version = version + 1, updated_at = ?2 WHERE id = ?3",
                params![is_read, chrono::Local::now().timestamp(), id],
            )?;
            if rows == 0 {
                warn!("数据库: 更新订阅条目已读状态失败，未找到 ID 为 {id} 的记录");
            }
            Ok(())
        })
    }

    pub fn update_feed_item_added_status(&self, id: &str, is_added: bool) -> Result<()> {
        debug!("数据库: 更新订阅条目库状态 (ID: {id}, is_added: {is_added})");
        self.with_conn(|conn| {
            let rows = conn.execute(
                "UPDATE feed_items SET is_added_to_library = ?1, is_dirty = 1, version = version + 1, updated_at = ?2 WHERE id = ?3",
                params![is_added, chrono::Local::now().timestamp(), id],
            )?;
            if rows == 0 {
                warn!("数据库: 更新订阅条目入库状态失败，未找到 ID 为 {id} 的记录");
            }
            Ok(())
        })
    }

    pub fn delete_feed_item(&self, id: &str) -> Result<()> {
        info!("数据库: 准备删除订阅条目 (ID: {id})");
        self.with_conn(|conn| {
            let rows = conn.execute(
                "UPDATE feed_items SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE id = ?2",
                params![chrono::Local::now().timestamp(), id],
            )?;
            if rows > 0 {
                debug!("数据库: 订阅条目 (ID: {id}) 已标记为删除");
            } else {
                warn!("数据库: 删除订阅条目失败，未找到 ID 为 {id} 的记录");
            }
            Ok(())
        })
    }

    pub fn delete_items_by_feed(&self, feed_id: &str) -> Result<()> {
        info!("数据库: 准备删除订阅源的所有条目 (FeedID: {feed_id})");
        self.with_conn(|conn| {
            let rows = conn.execute(
                "UPDATE feed_items SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE feed_id = ?2",
                params![chrono::Local::now().timestamp(), feed_id],
            )?;
            info!("数据库: 已将订阅源 {feed_id} 的 {rows} 个条目标记为删除");
            Ok(())
        })
    }

    // --- 同步支持方法 ---

    pub fn get_dirty_feed_items(&self) -> Result<Vec<FeedItem>> {
        debug!("数据库: 正在获取待同步订阅条目记录");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT

                id, title, feed_id, is_read, is_added_to_library, added_at,

                authors, year, type, journal, publisher, abstract_text, doi, url,

                volume, issue, pages, published_at, is_dirty, is_deleted, version, updated_at

                FROM feed_items WHERE is_dirty = 1",
            )?;

            let item_iter = stmt.query_map([], |row| {
                let id: String = row.get(0)?;

                let title: String = row.get(1)?;

                let fid: String = row.get(2)?;

                let mut item = create_feed_item(id, title, fid);

                item.is_read = row.get(3)?;

                item.is_added_to_library = row.get(4)?;

                item.added_at = row.get(5)?;

                let authors_json: Option<String> = row.get(6)?;

                if let Some(json) = authors_json {
                    item.authors = serde_json::from_str(&json).unwrap_or_default();
                }

                item.year = row.get(7)?;

                let type_json: Option<String> = row.get(8)?;

                if let Some(json) = type_json {
                    item.literature_type =
                        serde_json::from_str(&json).unwrap_or(models::LiteratureType::Article);
                }

                item.journal = row.get(9)?;

                item.publisher = row.get(10)?;

                item.abstract_text = row.get(11)?;

                item.doi = row.get(12)?;

                item.url = row.get(13)?;

                item.volume = row.get(14)?;

                item.issue = row.get(15)?;

                item.pages = row.get(16)?;

                item.published_at = row.get(17)?;

                item.is_dirty = row.get(18)?;

                item.is_deleted = row.get(19)?;

                item.version = row.get(20)?;

                item.updated_at = row.get(21)?;

                Ok(item)
            })?;

            let items = item_iter.collect::<Result<Vec<_>>>()?;
            debug!("数据库: 获取到 {} 个待同步订阅条目", items.len());
            Ok(items)
        })
    }

    pub fn mark_feed_item_synced(&self, id: &str) -> Result<()> {
        debug!("数据库: 标记订阅条目为已同步 (ID: {id})");
        self.with_conn(|conn| {
            let rows = conn.execute("UPDATE feed_items SET is_dirty = 0 WHERE id = ?1", [id])?;
            if rows == 0 {
                warn!("数据库: 标记订阅条目同步失败，未找到 ID 为 {id} 的记录");
            }
            Ok(())
        })
    }

    /// 原子原语：把远程订阅条目盲目 upsert 到本地（覆盖写或插入）。
    pub fn apply_remote_feed_item(&self, remote: &FeedItem) -> Result<()> {
        self.with_conn(|conn| self.insert_feed_item_internal(conn, remote))
    }

    /// 原子原语：版本一致且本地无修改时，仅刷新时间戳并清脏标记。
    pub fn mark_feed_item_up_to_date(&self, remote: &FeedItem) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE feed_items SET updated_at = ?1, is_dirty = 0 WHERE id = ?2",
                params![remote.updated_at, remote.id],
            )?;
            Ok(())
        })
    }

    fn insert_feed_item_internal(
        &self,
        conn: &rusqlite::Connection,
        item: &FeedItem,
    ) -> Result<()> {
        let authors_json =
            serde_json::to_string(&item.authors).unwrap_or_else(|_| "[]".to_string());

        let type_str = serde_json::to_string(&item.literature_type)
            .unwrap_or_else(|_| "\"Article\"".to_string());

        conn.execute(

            "INSERT OR REPLACE INTO feed_items (

                id, title, feed_id, is_read, is_added_to_library, added_at,

                authors, year, type, journal, publisher, abstract_text, doi, url,

                volume, issue, pages, published_at, is_dirty, is_deleted, version, updated_at

            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",

            params![

                item.id, item.title, item.feed_id, item.is_read, item.is_added_to_library, item.added_at,

                authors_json, item.year, type_str, item.journal, item.publisher, item.abstract_text, item.doi, item.url,

                item.volume, item.issue, item.pages, item.published_at, 0, item.is_deleted, item.version, item.updated_at

            ],

        )?;

        Ok(())
    }

    pub fn purge_synced_feed_items(&self) -> Result<usize> {
        info!("数据库: 正在清理已同步的删除订阅条目记录");
        self.with_conn(|conn| {
            let count = conn.execute(
                "DELETE FROM feed_items WHERE is_deleted = 1 AND is_dirty = 0",
                [],
            )?;
            info!("数据库: 已清理 {count} 个已同步删除的订阅条目");
            Ok(count)
        })
    }
}
