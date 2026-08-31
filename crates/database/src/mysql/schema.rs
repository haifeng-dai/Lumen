use super::MySqlManager;
use anyhow::{Result, anyhow};
use log::{error, info};
use mysql_async::prelude::*;

pub async fn ensure_remote_tables(conn: &mut mysql_async::Conn) -> Result<()> {
    let create_tables = [
        "CREATE TABLE IF NOT EXISTS library_info (singleton TINYINT NOT NULL DEFAULT 1, library_id VARCHAR(64) NOT NULL, schema_version INT NOT NULL DEFAULT 1, created_at BIGINT NOT NULL, PRIMARY KEY (singleton), UNIQUE KEY uq_library_info_id (library_id)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS sync_changes (sequence BIGINT UNSIGNED NOT NULL AUTO_INCREMENT, entity_type VARCHAR(64) NOT NULL, entity_id VARCHAR(255) NOT NULL, version INT NOT NULL, PRIMARY KEY (sequence), INDEX idx_sync_changes_entity (entity_type, entity_id), INDEX idx_sync_changes_sequence (sequence)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS literatures (id VARCHAR(64) PRIMARY KEY, title TEXT NOT NULL, year INT, month INT, day INT, type TEXT NOT NULL, publication_id VARCHAR(64), volume TEXT, issue TEXT, pages TEXT, abstract_text MEDIUMTEXT, doi TEXT, arxiv_id TEXT, url TEXT, rating INT DEFAULT 0, reading_status TEXT, is_deleted BOOLEAN DEFAULT 0, version INT DEFAULT 1, created_at BIGINT NOT NULL DEFAULT 0, updated_at BIGINT NOT NULL DEFAULT 0) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS publications (id VARCHAR(64) PRIMARY KEY, name TEXT NOT NULL, publication_type TEXT NOT NULL, abbreviation TEXT, publisher TEXT, ccf_rank TEXT, jcr_rank TEXT, cas_rank TEXT, is_deleted BOOLEAN DEFAULT 0, version INT DEFAULT 1, created_at BIGINT NOT NULL DEFAULT 0, updated_at BIGINT NOT NULL DEFAULT 0) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS authors (id VARCHAR(64) PRIMARY KEY, first_name TEXT NOT NULL, last_name TEXT NOT NULL, middle_name TEXT, is_deleted BOOLEAN DEFAULT 0, version INT DEFAULT 1, created_at BIGINT NOT NULL DEFAULT 0, updated_at BIGINT NOT NULL DEFAULT 0) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS literature_authors (literature_id VARCHAR(64) NOT NULL, author_id VARCHAR(64) NOT NULL, sort_order INT DEFAULT 0, is_deleted BOOLEAN DEFAULT 0, version INT DEFAULT 1, updated_at BIGINT NOT NULL DEFAULT 0, PRIMARY KEY (literature_id, author_id)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS folders (id VARCHAR(64) PRIMARY KEY, name TEXT NOT NULL, folder_type TEXT NOT NULL, parent_id VARCHAR(64), is_deleted BOOLEAN DEFAULT 0, version INT DEFAULT 1, created_at BIGINT NOT NULL DEFAULT 0, updated_at BIGINT NOT NULL DEFAULT 0) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS literature_folders (literature_id VARCHAR(64) NOT NULL, folder_id VARCHAR(64) NOT NULL, is_deleted BOOLEAN DEFAULT 0, version INT DEFAULT 1, updated_at BIGINT NOT NULL DEFAULT 0, PRIMARY KEY (literature_id, folder_id)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS tags (id VARCHAR(64) PRIMARY KEY, name TEXT NOT NULL, color TEXT, is_deleted BOOLEAN DEFAULT 0, version INT DEFAULT 1, created_at BIGINT NOT NULL DEFAULT 0, updated_at BIGINT NOT NULL DEFAULT 0) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS literature_tags (literature_id VARCHAR(64) NOT NULL, tag_id VARCHAR(64) NOT NULL, is_deleted BOOLEAN DEFAULT 0, version INT DEFAULT 1, updated_at BIGINT NOT NULL DEFAULT 0, PRIMARY KEY (literature_id, tag_id)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS attachments (id VARCHAR(64) PRIMARY KEY, literature_id VARCHAR(64) NOT NULL, file_path TEXT NOT NULL, file_name TEXT NOT NULL, file_size BIGINT UNSIGNED NOT NULL, mime_type TEXT, etag TEXT, hash TEXT, is_main BOOLEAN DEFAULT 0, is_deleted BOOLEAN DEFAULT 0, version INT DEFAULT 1, created_at BIGINT NOT NULL DEFAULT 0, updated_at BIGINT NOT NULL DEFAULT 0) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS feeds (id VARCHAR(64) PRIMARY KEY, name TEXT NOT NULL, title TEXT, feed_type TEXT NOT NULL, url TEXT, last_updated_at TEXT, update_interval INT DEFAULT 24, is_deleted BOOLEAN DEFAULT 0, version INT DEFAULT 1, created_at BIGINT NOT NULL DEFAULT 0, updated_at BIGINT NOT NULL DEFAULT 0) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS feed_items (id VARCHAR(64) PRIMARY KEY, title TEXT NOT NULL, feed_id VARCHAR(64) NOT NULL, is_read BOOLEAN DEFAULT 0, is_added_to_library BOOLEAN DEFAULT 0, added_at TEXT NOT NULL, authors TEXT, year INT, type TEXT, journal TEXT, publisher TEXT, abstract_text MEDIUMTEXT, doi TEXT, url TEXT, volume TEXT, issue TEXT, pages TEXT, published_at TEXT, is_deleted BOOLEAN DEFAULT 0, version INT DEFAULT 1, updated_at BIGINT NOT NULL DEFAULT 0) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS literature_citations (source_id VARCHAR(64) NOT NULL, target_id VARCHAR(64) NOT NULL, is_deleted BOOLEAN DEFAULT 0, version INT DEFAULT 1, updated_at BIGINT NOT NULL DEFAULT 0, PRIMARY KEY (source_id, target_id)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS annotations (id VARCHAR(64) PRIMARY KEY, document_id TEXT NOT NULL, page INT NOT NULL, kind TEXT NOT NULL, color TEXT NOT NULL, `range` TEXT, note TEXT, rect_x FLOAT, rect_y FLOAT, rect_w FLOAT, rect_h FLOAT, is_deleted BOOLEAN DEFAULT 0, version INT DEFAULT 1, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        "CREATE TABLE IF NOT EXISTS literature_notes (id VARCHAR(64) PRIMARY KEY, literature_id VARCHAR(64) NOT NULL, title TEXT NOT NULL, content TEXT NOT NULL, sort_order INT NOT NULL DEFAULT 0, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL DEFAULT 0, is_deleted BOOLEAN DEFAULT 0, version INT DEFAULT 1) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
    ];
    for sql in create_tables {
        conn.query_drop(sql).await?;
    }

    let column: Option<(String, Option<u64>)> = conn
        .exec_first(
            "SELECT data_type, character_maximum_length FROM information_schema.columns
             WHERE table_schema = DATABASE() AND table_name = 'sync_changes' AND column_name = 'entity_id'",
            (),
        )
        .await?;
    let (data_type, length) = column.ok_or_else(|| anyhow!("sync_changes.entity_id is missing"))?;
    if !data_type.eq_ignore_ascii_case("varchar") || length.unwrap_or(0) < 255 {
        conn.query_drop("ALTER TABLE sync_changes MODIFY COLUMN entity_id VARCHAR(255) NOT NULL")
            .await?;
    }

    let indexes = [
        "CREATE INDEX idx_annotations_doc_page ON annotations(document_id(64), page)",
        "CREATE INDEX idx_tags_name ON tags(name(64))",
        "CREATE INDEX idx_literatures_updated ON literatures(updated_at)",
        "CREATE INDEX idx_authors_updated ON authors(updated_at)",
        "CREATE INDEX idx_folders_updated ON folders(updated_at)",
        "CREATE INDEX idx_publications_updated ON publications(updated_at)",
        "CREATE INDEX idx_attachments_updated ON attachments(updated_at)",
        "CREATE INDEX idx_feeds_updated ON feeds(updated_at)",
        "CREATE INDEX idx_feed_items_updated ON feed_items(updated_at)",
        "CREATE INDEX idx_lit_authors_updated ON literature_authors(updated_at)",
        "CREATE INDEX idx_lit_folders_updated ON literature_folders(updated_at)",
        "CREATE INDEX idx_lit_tags_updated ON literature_tags(updated_at)",
        "CREATE INDEX idx_lit_citations_updated ON literature_citations(updated_at)",
        "CREATE INDEX idx_annotations_updated ON annotations(updated_at)",
        "CREATE INDEX idx_lit_notes_updated ON literature_notes(updated_at)",
    ];
    for sql in indexes {
        if let Err(e) = conn.query_drop(sql).await {
            info!("MySQL: 索引创建跳过 (可能已存在): {e}");
        }
    }

    Ok(())
}

/// 迁移历史远程库中仍为文本的 `created_at`/`updated_at` 列 -> BIGINT（Unix 秒）。
///
/// 关键设计（与本地迁移一致）：**逐列检查 information_schema 的真实列类型**，
pub async fn clear_all_data(manager: &MySqlManager) -> Result<()> {
    let (use_remote, host, db_name) = {
        let c = manager.config.read().unwrap();
        (c.use_remote, c.host.clone(), c.database.clone())
    };
    if !use_remote {
        info!("MySQL: 远程同步未启用，跳过清空操作");
        return Ok(());
    }
    info!("MySQL: 开始彻底清空远程数据库: {host} (库名: {db_name})");
    let pool = manager.get_pool().await?;
    let mut conn = pool.get_conn().await?;

    let tables: Vec<String> = conn
        .exec(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = :db",
            params! { "db" => &db_name },
        )
        .await?;
    info!(
        "MySQL: [清理阶段] 发现当前数据库中共有 {} 个表",
        tables.len()
    );

    info!("MySQL: [清理阶段] 正在禁用外键约束检查...");
    conn.query_drop("SET FOREIGN_KEY_CHECKS = 0").await?;

    for table in &tables {
        info!("MySQL: [清理阶段] 正在处理表 '{table}'...");
        let drop_sql = format!("DROP TABLE IF EXISTS `{table}`");
        if let Err(e) = conn.query_drop(&drop_sql).await {
            error!("MySQL: [错误] 删除表 '{table}' 失败: {e}");
        } else {
            info!("MySQL: [成功] 表 '{table}' 已删除");
        }
    }

    info!("MySQL: [清理阶段] 正在重新启用外键约束检查...");
    conn.query_drop("SET FOREIGN_KEY_CHECKS = 1").await?;

    info!("MySQL: [清理阶段] 正在重新验证/初始化核心表结构...");
    ensure_remote_tables(&mut conn).await?;

    info!("MySQL: 远程数据库彻底清空并重置完成");
    Ok(())
}

pub async fn purge_deleted_data(manager: &MySqlManager) -> Result<usize> {
    let use_remote = manager.config.read().unwrap().use_remote;
    if !use_remote {
        info!("MySQL: 远程同步未启用，跳过已删除数据清理");
        return Ok(0);
    }

    let pool = manager.get_pool().await?;
    let mut conn = pool.get_conn().await?;
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
        total += conn
            .exec_drop(format!("DELETE FROM `{table}` WHERE is_deleted = 1"), ())
            .await
            .map(|_| conn.affected_rows() as usize)?;
    }
    info!("MySQL: 已物理清理 {total} 条软删除记录");
    Ok(total)
}
