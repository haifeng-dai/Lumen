use super::Database;
use log::{debug, info, warn};
use models::constructors::*;
use models::{Feed, FeedType};
use rusqlite::{Result, params};
use serde_json;

impl Database {
    // --- Feed Operations (Main DB) ---

    pub fn insert_feed(&self, feed: &Feed) -> Result<()> {
        info!("数据库: 准备写入订阅源 '{}' (ID: {})", feed.name, feed.id);
        let type_str = serde_json::to_string(&feed.feed_type)
            .unwrap_or_else(|_| "\"rss\"".to_string())
            .trim_matches('"')
            .to_string();

        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO feeds (id, name, title, feed_type, url, last_updated_at, update_interval, is_dirty, is_deleted, version, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![feed.id, feed.name, feed.title, type_str, feed.url, feed.last_updated_at, feed.update_interval, feed.is_dirty, feed.is_deleted, feed.version, feed.created_at, feed.updated_at],
            )?;
            Ok(())
        })
    }

    pub fn get_all_feeds(&self) -> Result<Vec<Feed>> {
        debug!("数据库: 正在获取所有订阅源列表");
        self.with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT id, name, title, feed_type, url, last_updated_at, update_interval, is_dirty, is_deleted, version, created_at, updated_at FROM feeds WHERE is_deleted = 0")?;
            let feed_iter = stmt.query_map([], |row| {
                let type_str: String = row.get(3)?;
                let feed_type: FeedType =
                    serde_json::from_str(&format!("\"{type_str}\"")).unwrap_or(FeedType::Rss);

                let mut feed = create_feed(
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    feed_type,
                );
                feed.title = row.get(2)?;
                feed.url = row.get(4)?;
                feed.last_updated_at = row.get(5)?;
                feed.update_interval = row.get(6)?;
                feed.is_dirty = row.get(7)?;
                feed.is_deleted = row.get(8)?;
                feed.version = row.get(9)?;
                feed.created_at = row.get(10)?;
                feed.updated_at = row.get(11)?;
                Ok(feed)
            })?;

            let mut feeds = Vec::new();
            for f in feed_iter {
                feeds.push(f?);
            }
            debug!("数据库: 成功获取 {} 个订阅源", feeds.len());
            Ok(feeds)
        })
    }

    pub fn get_feed(&self, id: &str) -> Result<Option<Feed>> {
        debug!("数据库: 正在获取订阅源 (ID: {id})");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, title, feed_type, url, last_updated_at, update_interval, is_dirty, is_deleted, version, created_at, updated_at FROM feeds WHERE id = ?1 AND is_deleted = 0",
            )?;
            let mut rows = stmt.query([id])?;
            if let Some(row) = rows.next()? {
                let type_str: String = row.get(3)?;
                let feed_type: FeedType =
                    serde_json::from_str(&format!("\"{type_str}\"")).unwrap_or(FeedType::Rss);
                let mut feed = create_feed(row.get::<_, String>(0)?, row.get::<_, String>(1)?, feed_type);
                feed.title = row.get(2)?;
                feed.url = row.get(4)?;
                feed.last_updated_at = row.get(5)?;
                feed.update_interval = row.get(6)?;
                feed.is_dirty = row.get(7)?;
                feed.is_deleted = row.get(8)?;
                feed.version = row.get(9)?;
                feed.created_at = row.get(10)?;
                feed.updated_at = row.get(11)?;
                Ok(Some(feed))
            } else {
                Ok(None)
            }
        })
    }

    pub fn update_feed(&self, feed: &Feed) -> Result<()> {
        info!(
            "数据库: 正在更新订阅源信息 '{}' (ID: {})",
            feed.name, feed.id
        );
        let type_str = serde_json::from_str::<serde_json::Value>(
            &serde_json::to_string(&feed.feed_type).unwrap_or_else(|_| "\"rss\"".to_string()),
        )
        .unwrap_or_else(|_| serde_json::json!("rss"))
        .as_str()
        .unwrap_or("rss")
        .to_string();

        self.with_conn(|conn| {
            let rows = conn.execute(
                "UPDATE feeds SET name = ?1, title = ?2, feed_type = ?3, url = ?4, last_updated_at = ?5, update_interval = ?6, is_dirty = 1, version = version + 1, updated_at = ?7 WHERE id = ?8",
                params![feed.name, feed.title, type_str, feed.url, feed.last_updated_at, feed.update_interval, chrono::Local::now().timestamp(), feed.id],
            )?;
            if rows == 0 {
                warn!("数据库: 更新订阅源失败，未找到 ID 为 {} 的记录", feed.id);
            }
            Ok(())
        })
    }

    pub fn delete_feed(&self, id: &str) -> Result<()> {
        info!("数据库: 准备删除订阅源 (ID: {id})");
        self.with_conn(|conn| {
            let rows = conn.execute(
                "UPDATE feeds SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE id = ?2",
                params![chrono::Local::now().timestamp(), id]
            )?;
            if rows > 0 {
                debug!("数据库: 订阅源 (ID: {id}) 已标记为删除");
            } else {
                warn!("数据库: 删除订阅源失败，未找到 ID 为 {id} 的记录");
            }
            Ok(())
        })
    }

    // --- 同步支持方法 ---

    pub fn get_dirty_feeds(&self) -> Result<Vec<Feed>> {
        debug!("数据库: 正在获取待同步订阅源记录");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, title, feed_type, url, last_updated_at, update_interval, is_dirty, is_deleted, version, created_at, updated_at FROM feeds WHERE is_dirty = 1"
            )?;
            let iter = stmt.query_map([], |row| {
                let type_str: String = row.get(3)?;
                let feed_type: FeedType =
                    serde_json::from_str(&format!("\"{type_str}\"")).unwrap_or(FeedType::Rss);

                let mut feed = create_feed(
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    feed_type,
                );
                feed.title = row.get(2)?;
                feed.url = row.get(4)?;
                feed.last_updated_at = row.get(5)?;
                feed.update_interval = row.get(6)?;
                feed.is_dirty = row.get(7)?;
                feed.is_deleted = row.get(8)?;
                feed.version = row.get(9)?;
                feed.created_at = row.get(10)?;
                feed.updated_at = row.get(11)?;
                Ok(feed)
            })?;
            let feeds = iter.collect::<Result<Vec<_>>>()?;
            debug!("数据库: 获取到 {} 个待同步订阅源", feeds.len());
            Ok(feeds)
        })
    }

    pub fn mark_feed_synced(&self, id: &str) -> Result<()> {
        debug!("数据库: 标记订阅源为已同步 (ID: {id})");
        self.with_conn(|conn| {
            let rows = conn.execute("UPDATE feeds SET is_dirty = 0 WHERE id = ?1", [id])?;
            if rows == 0 {
                warn!("数据库: 标记订阅源同步失败，未找到 ID 为 {id} 的记录");
            }
            Ok(())
        })
    }

    /// 原子原语：把远程订阅源盲目 upsert 到本地（覆盖写或插入）。
    pub fn apply_remote_feed(&self, remote: &Feed) -> Result<()> {
        self.with_conn(|conn| self.insert_feed_internal(conn, remote))
    }

    /// 原子原语：版本一致且本地无修改时，仅刷新时间戳并清脏标记。
    pub fn mark_feed_up_to_date(&self, remote: &Feed) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE feeds SET updated_at = ?1, is_dirty = 0 WHERE id = ?2",
                params![remote.updated_at, remote.id],
            )?;
            Ok(())
        })
    }

    fn insert_feed_internal(&self, conn: &rusqlite::Connection, feed: &Feed) -> Result<()> {
        let type_str = serde_json::to_string(&feed.feed_type)
            .unwrap_or_else(|_| "\"rss\"".to_string())
            .trim_matches('"')
            .to_string();

        conn.execute(
            "INSERT OR REPLACE INTO feeds (id, name, title, feed_type, url, last_updated_at, update_interval, is_dirty, is_deleted, version, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![feed.id, feed.name, feed.title, type_str, feed.url, feed.last_updated_at, feed.update_interval, 0, feed.is_deleted, feed.version, feed.created_at, feed.updated_at],
        )?;
        Ok(())
    }

    pub fn purge_synced_feeds(&self) -> Result<usize> {
        info!("数据库: 正在清理已同步的删除订阅源记录");
        self.with_conn(|conn| {
            let count = conn.execute(
                "DELETE FROM feeds WHERE is_deleted = 1 AND is_dirty = 0",
                [],
            )?;
            info!("数据库: 已清理 {count} 个已同步删除的订阅源");
            Ok(count)
        })
    }
}
