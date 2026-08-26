use super::MySqlManager;
use anyhow::Result;
use log::{error, info};
use mysql_async::prelude::*;

pub async fn ensure_remote_tables(conn: &mut mysql_async::Conn) -> Result<()> {
    let create_tables = [
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

    // 一次性迁移：把历史库中 created_at 文本列转换为 BIGINT（Unix 秒）。
    // 顺序关键——先转数据再改列类型，否则会被截断损坏。
    run_remote_migrations(conn).await?;

    Ok(())
}

/// 迁移历史远程库中仍为文本的 `created_at`/`updated_at` 列 -> BIGINT（Unix 秒）。
///
/// 关键设计（与本地迁移一致）：**逐列检查 information_schema 的真实列类型**，
/// 只对仍为文本的列做转换，绝不依赖 `_migrations` 版本号。历史上曾写过
/// `version=1` 但列仍是 TEXT（漏改/被跳过），用版本号判断会"假成功跳过"，
/// 留下 `publications.created_at` 这类遗漏列。
async fn run_remote_migrations(conn: &mut mysql_async::Conn) -> Result<()> {
    // 覆盖所有含 created_at/updated_at 的表（含 publications）。已是 BIGINT 的列
    // 经下面真实类型检查会被跳过，故这里多列无害，反而能兜住任何历史 TEXT 列。
    let pairs: &[(&str, &str)] = &[
        ("literatures", "created_at"),
        ("literatures", "updated_at"),
        ("publications", "created_at"),
        ("publications", "updated_at"),
        ("authors", "created_at"),
        ("authors", "updated_at"),
        ("folders", "created_at"),
        ("folders", "updated_at"),
        ("tags", "created_at"),
        ("tags", "updated_at"),
        ("attachments", "created_at"),
        ("attachments", "updated_at"),
        ("feeds", "created_at"),
        ("feeds", "updated_at"),
        ("feed_items", "updated_at"),
        ("annotations", "created_at"),
        ("annotations", "updated_at"),
        ("literature_notes", "created_at"),
        ("literature_notes", "updated_at"),
        ("literature_authors", "updated_at"),
        ("literature_folders", "updated_at"),
        ("literature_tags", "updated_at"),
        ("literature_citations", "updated_at"),
    ];

    let mut converted = 0u32;
    for (table, col) in pairs {
        // 查询真实列类型；列不存在时 DATA_TYPE 为 NULL，跳过。
        let data_type: Option<String> = conn
            .exec_first(
                "SELECT DATA_TYPE FROM information_schema.columns \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND COLUMN_NAME = ?",
                (table, col),
            )
            .await?;
        let is_text = match data_type {
            Some(d) => {
                let d = d.to_ascii_lowercase();
                d == "text" || d == "varchar" || d == "char" || d == "datetime" || d == "timestamp"
            }
            None => false,
        };
        if !is_text {
            continue; // 已是 BIGINT 或列不存在，无需处理
        }

        // 转数据：datetime 串 -> UNIX_TIMESTAMP；纯数字串 -> 其值；NULL/空/其余 -> 0。
        // 不设 WHERE，确保 NULL 与 '' 也落入 CASE 的 ELSE 分支置 0，避免后续 MODIFY NOT NULL 失败。
        let upd = format!(
            "UPDATE {table} SET {col} = CASE \
                WHEN {col} REGEXP '^[0-9]{{4}}-' THEN UNIX_TIMESTAMP({col}) \
                WHEN {col} REGEXP '^[0-9]+$' THEN CAST({col} AS UNSIGNED) \
                ELSE 0 END"
        );
        if let Err(e) = conn.query_drop(&upd).await {
            error!("MySQL 迁移数据转换失败(表 {table}.{col}): {e}");
        }
        // 再改列类型为 BIGINT
        let alt = format!("ALTER TABLE {table} MODIFY {col} BIGINT NOT NULL DEFAULT 0");
        if let Err(e) = conn.query_drop(&alt).await {
            error!("MySQL 迁移列类型修改失败(表 {table}.{col}): {e}");
        } else {
            converted += 1;
            info!("MySQL: 已迁移 {table}.{col} -> BIGINT");
        }
    }

    // 仅作已运行标记；不再作为是否执行的依据（避免脏标记导致跳过）。
    let _ = conn
        .query_drop(
            "CREATE TABLE IF NOT EXISTS _migrations (version INT PRIMARY KEY, applied_at DATETIME)",
        )
        .await;
    let _ = conn
        .exec_drop(
            "INSERT INTO _migrations (version, applied_at) VALUES (1, NOW()) ON DUPLICATE KEY UPDATE applied_at = NOW()",
            (),
        )
        .await;
    info!("MySQL: 时间戳列迁移完成（按真实列类型逐列检查，本次转换 {converted} 列）");
    Ok(())
}

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
