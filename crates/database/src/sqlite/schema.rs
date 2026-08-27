use log::debug;
use rusqlite::{Result, params};

use super::Database;

impl Database {
    pub(super) fn init_tables(&self) -> Result<()> {
        debug!("正在初始化数据库表结构...");
        self.with_conn(|conn| {
            // 1. 文献主表
            conn.execute(
                "CREATE TABLE IF NOT EXISTS literatures (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    year INTEGER,
                    month INTEGER,
                    day INTEGER,
                    type TEXT NOT NULL,
                    publication_id TEXT,
                    volume TEXT,
                    issue TEXT,
                    pages TEXT,
                    abstract_text TEXT,
                    doi TEXT,
                    arxiv_id TEXT,
                    url TEXT,
                    rating INTEGER DEFAULT 0,
                    reading_status TEXT DEFAULT 'Unread',
                    is_dirty BOOLEAN DEFAULT 1,
                    is_deleted BOOLEAN DEFAULT 0,
                    version INTEGER DEFAULT 1,
                    created_at INTEGER NOT NULL DEFAULT 0,
                    updated_at INTEGER NOT NULL DEFAULT 0
                )",
                [],
            )?;

            // 出版源表 (期刊/会议/书籍)
            conn.execute(
                "CREATE TABLE IF NOT EXISTS publications (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    publication_type TEXT NOT NULL,
                    abbreviation TEXT,
                    publisher TEXT,
                    ccf_rank TEXT,
                    jcr_rank TEXT,
                    cas_rank TEXT,
                    is_dirty BOOLEAN DEFAULT 1,
                    is_deleted BOOLEAN DEFAULT 0,
                    version INTEGER DEFAULT 1,
                    created_at INTEGER NOT NULL DEFAULT 0,
                    updated_at INTEGER NOT NULL DEFAULT 0
                )",
                [],
            )?;

            // 2. 作者表
            conn.execute(
                "CREATE TABLE IF NOT EXISTS authors (
                    id TEXT PRIMARY KEY,
                    first_name TEXT NOT NULL,
                    last_name TEXT NOT NULL,
                    middle_name TEXT,
                    is_dirty BOOLEAN DEFAULT 1,
                    is_deleted BOOLEAN DEFAULT 0,
                    version INTEGER DEFAULT 1,
                    created_at INTEGER NOT NULL DEFAULT 0,
                    updated_at INTEGER NOT NULL DEFAULT 0
                )",
                [],
            )?;

            // 3. 关联表：文献-作者 (多对多)
            conn.execute(
                "CREATE TABLE IF NOT EXISTS literature_authors (
                    literature_id TEXT NOT NULL,
                    author_id TEXT NOT NULL,
                    sort_order INTEGER DEFAULT 0,
                    is_dirty BOOLEAN DEFAULT 1,
                    is_deleted BOOLEAN DEFAULT 0,
                    version INTEGER DEFAULT 1,
                    updated_at INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (literature_id, author_id)
                )",
                [],
            )?;

            // 4. 文件夹/分类表
            conn.execute(
                "CREATE TABLE IF NOT EXISTS folders (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    folder_type TEXT NOT NULL,
                    parent_id TEXT,
                    is_dirty BOOLEAN DEFAULT 1,
                    is_deleted BOOLEAN DEFAULT 0,
                    version INTEGER DEFAULT 1,
                    created_at INTEGER NOT NULL DEFAULT 0,
                    updated_at INTEGER NOT NULL DEFAULT 0
                )",
                [],
            )?;

            // 5. 关联表：文献-文件夹
            conn.execute(
                "CREATE TABLE IF NOT EXISTS literature_folders (
                    literature_id TEXT NOT NULL,
                    folder_id TEXT NOT NULL,
                    is_dirty BOOLEAN DEFAULT 1,
                    is_deleted BOOLEAN DEFAULT 0,
                    version INTEGER DEFAULT 1,
                    updated_at INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (literature_id, folder_id)
                )",
                [],
            )?;

            // 6. 标签表 (升级为独立实体)
            conn.execute(
                "CREATE TABLE IF NOT EXISTS tags (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    color TEXT DEFAULT '#808080',
                    is_dirty BOOLEAN DEFAULT 1,
                    is_deleted BOOLEAN DEFAULT 0,
                    version INTEGER DEFAULT 1,
                    created_at INTEGER NOT NULL DEFAULT 0,
                    updated_at INTEGER NOT NULL DEFAULT 0
                )",
                [],
            )?;

            // 确保未删除的标签名称唯一 (应用层辅助，但数据库层也加上唯一索引以防万一)
            // 注意：这里使用部分索引 (Partial Index) 来允许软删除的同名标签存在
            conn.execute(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_tags_name_active ON tags(name) WHERE is_deleted = 0",
                [],
            )?;

            // 7. 关联表：文献-标签
            conn.execute(
                "CREATE TABLE IF NOT EXISTS literature_tags (
                    literature_id TEXT NOT NULL,
                    tag_id TEXT NOT NULL,
                    is_dirty BOOLEAN DEFAULT 1,
                    is_deleted BOOLEAN DEFAULT 0,
                    version INTEGER DEFAULT 1,
                    updated_at INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (literature_id, tag_id)
                )",
                [],
            )?;

            // 8. 附件表
            conn.execute(
                "CREATE TABLE IF NOT EXISTS attachments (
                    id TEXT PRIMARY KEY,
                    literature_id TEXT NOT NULL,
                    file_path TEXT NOT NULL,
                    file_name TEXT NOT NULL,
                    file_size INTEGER NOT NULL,
                    mime_type TEXT,
                    etag TEXT,
                    hash TEXT,
                    is_main BOOLEAN DEFAULT 0,
                    is_dirty BOOLEAN DEFAULT 1,
                    is_deleted BOOLEAN DEFAULT 0,
                    version INTEGER DEFAULT 1,
                    created_at INTEGER NOT NULL DEFAULT 0,
                    updated_at INTEGER NOT NULL DEFAULT 0
                )",
                [],
            )?;

            // 9. 订阅 Feeds 表
            conn.execute(
                "CREATE TABLE IF NOT EXISTS feeds (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    title TEXT,
                    feed_type TEXT NOT NULL,
                    url TEXT,
                    last_updated_at TEXT,
                    update_interval INTEGER DEFAULT 24,
                    is_dirty BOOLEAN DEFAULT 1,
                    is_deleted BOOLEAN DEFAULT 0,
                    version INTEGER DEFAULT 1,
                    created_at INTEGER NOT NULL DEFAULT 0,
                    updated_at INTEGER NOT NULL DEFAULT 0
                )",
                [],
            )?;
            // 兼容旧库：已存在的 feeds 表可能缺少 title 列
            let has_title: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('feeds') WHERE name='title'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map(|c| c > 0)
                .unwrap_or(false);
            if !has_title {
                conn.execute("ALTER TABLE feeds ADD COLUMN title TEXT", [])?;
            }

            // 10. 订阅条目表
            conn.execute(
                "CREATE TABLE IF NOT EXISTS feed_items (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    feed_id TEXT NOT NULL,
                    is_read BOOLEAN DEFAULT 0,
                    is_added_to_library BOOLEAN DEFAULT 0,
                    added_at TEXT NOT NULL,
                    authors TEXT,
                    year INTEGER,
                    type TEXT,
                    journal TEXT,
                    publisher TEXT,
                    abstract_text TEXT,
                    doi TEXT,
                    url TEXT,
                    volume TEXT,
                    issue TEXT,
                    pages TEXT,
                    published_at TEXT,
                    is_dirty BOOLEAN DEFAULT 1,
                    is_deleted BOOLEAN DEFAULT 0,
                    version INTEGER DEFAULT 1,
                    updated_at INTEGER NOT NULL DEFAULT 0
                )",
                [],
            )?;

            // 11. 引用关系表 (文献之间的引用关系)
            conn.execute(
                "CREATE TABLE IF NOT EXISTS literature_citations (
                    source_id TEXT NOT NULL,
                    target_id TEXT NOT NULL,
                    is_dirty BOOLEAN DEFAULT 1,
                    is_deleted BOOLEAN DEFAULT 0,
                    version INTEGER DEFAULT 1,
                    updated_at INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (source_id, target_id)
                )",
                [],
            )?;

            // 12. 同步元数据表 (记录上次同步时间等)
            conn.execute(
                "CREATE TABLE IF NOT EXISTS sync_meta (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                )",
                [],
            )?;

            // 13. PDF 注释表
            conn.execute(
                "CREATE TABLE IF NOT EXISTS annotations (
                    id TEXT PRIMARY KEY,
                    document_id TEXT NOT NULL,
                    page INTEGER NOT NULL,
                    kind TEXT NOT NULL,
                    color TEXT NOT NULL,
                    range TEXT, -- JSON
                    note TEXT,
                    rect_x REAL,
                    rect_y REAL,
                    rect_w REAL,
                    rect_h REAL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    version INTEGER DEFAULT 1,
                    is_deleted INTEGER DEFAULT 0,
                    is_dirty INTEGER DEFAULT 0
                )",
                [],
            )?;

            // 为文档 ID 和页码创建索引
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_annotations_doc_page ON annotations(document_id, page)",
                [],
            )?;

            // 14. 文献笔记独立表（从 literatures.notes 拆分，1:N 多笔记）
            conn.execute(
                "CREATE TABLE IF NOT EXISTS literature_notes (
                    id              TEXT PRIMARY KEY,
                    literature_id   TEXT NOT NULL,
                    title           TEXT NOT NULL DEFAULT '',
                    content         TEXT NOT NULL DEFAULT '',
                    sort_order      INTEGER NOT NULL DEFAULT 0,
                    created_at      INTEGER NOT NULL,
                    updated_at      INTEGER NOT NULL DEFAULT 0,
                    is_deleted      INTEGER DEFAULT 0,
                    is_dirty        INTEGER DEFAULT 0,
                    version         INTEGER DEFAULT 1
                )",
                [],
            )?;

            Ok(())
        })
    }

    /// 把历史库中 `created_at`/`updated_at` 文本列转换为 INTEGER（Unix 秒）。
    ///
    /// 关键修正（上一版踩的坑）：
    pub(super) fn init_default_data(&self) -> Result<()> {
        debug!("初始化默认数据（内置文件夹和订阅源）");
        self.with_conn(|conn| {
            let now = chrono::Local::now().timestamp();

            // 内置文件夹（INSERT OR IGNORE 确保仅首次创建）
            conn.execute(
                "INSERT OR IGNORE INTO folders (id, name, folder_type, parent_id, is_dirty, is_deleted, version, created_at, updated_at)
                 VALUES (?1, ?2, ?3, NULL, 1, 0, 1, ?4, ?4)",
                params!["all", "All Literature", "all", now],
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO folders (id, name, folder_type, parent_id, is_dirty, is_deleted, version, created_at, updated_at)
                 VALUES (?1, ?2, ?3, NULL, 1, 0, 1, ?4, ?4)",
                params!["uncategorized", "Uncategorized", "uncategorized", now],
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO folders (id, name, folder_type, parent_id, is_dirty, is_deleted, version, created_at, updated_at)
                 VALUES (?1, ?2, ?3, NULL, 1, 0, 1, ?4, ?4)",
                params!["trash", "Trash", "trash", now],
            )?;

            // 内置订阅源
            conn.execute(
                "INSERT OR IGNORE INTO feeds (id, name, feed_type, url, update_interval, is_dirty, is_deleted, version, created_at, updated_at)
                 VALUES (?1, ?2, ?3, NULL, 24, 1, 0, 1, ?4, ?4)",
                params!["all_subs", "All Subscriptions", "rss", now],
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO feeds (id, name, feed_type, url, update_interval, is_dirty, is_deleted, version, created_at, updated_at)
                 VALUES (?1, ?2, ?3, NULL, 24, 1, 0, 1, ?4, ?4)",
                params!["unread", "Unread", "rss", now],
            )?;

            debug!("默认数据初始化完成");
            Ok(())
        })
    }
    pub(super) fn get_table_names(&self) -> Vec<&'static str> {
        vec![
            "literatures",
            "publications",
            "authors",
            "literature_authors",
            "folders",
            "literature_folders",
            "tags",
            "literature_tags",
            "attachments",
            "feeds",
            "feed_items",
            "sync_meta",
            "literature_citations",
            "annotations",
            "literature_notes",
        ]
    }
}
