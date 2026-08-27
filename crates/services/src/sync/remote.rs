//! MySQL 同步编排层 (`services::sync::remote`)
//!
//! 原 `database::mysql::sync` 的同步编排逻辑已整体上移至此：
//! - `sync_metadata`：远程开关检测 + 标签同步 + 推送脏记录 + 拉取远程变更，
//!   并收集文献冲突返回给上层。
//! - `push_dirty_records` / `pull_remote_changes`：13 张表的双向同步。
//! - 拉取阶段的**冲突决策**委托给 `crate::sync::conflict`，数据库只提供盲写原语。
//!
//! 数据库 crate 现在只持有 `MySqlManager`（连接池）与各类 `apply_remote_*` /
//! `mark_*_up_to_date` / `get_*_sync_state` 原子原语，不再包含任何同步编排。

use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use chrono::{DateTime, NaiveDate, NaiveDateTime};
use log::{debug, error, info};
use models::{Literature, Tag};

use database::mysql::{MySqlSyncReader, MySqlSyncWriter};
use database::{Database, MySqlManager};

use crate::sync::conflict;

fn author_full_name(a: &models::Author) -> String {
    if let Some(ref middle) = a.middle_name {
        format!("{} {} {}", a.first_name, middle, a.last_name)
    } else {
        format!("{} {}", a.first_name, a.last_name)
    }
}

pub async fn sync_metadata(
    manager: &MySqlManager,
    db: Arc<Database>,
    base_path: &Path,
    allowed_attachment_ids: Option<&[String]>,
) -> Result<Vec<Literature>> {
    let c = manager.get_config();
    let (use_remote, host) = (c.use_remote, c.host.clone());
    if !use_remote {
        return Ok(Vec::new());
    }
    info!("MySQL: 开始元数据同步 (远程主机: {host})");

    let base_path_buf = base_path.to_path_buf();
    let allowed_ids = allowed_attachment_ids.map(<[String]>::to_vec);

    let db_clone = db.clone();
    let manager_config = manager.get_config();

    let sync_task = async move {
        info!("MySQL: 获取连接池成功");
        let mut writer = manager.open_sync_writer().await?;
        let mut reader = manager.open_sync_reader().await?;
        info!("MySQL: 建立数据库连接成功");

        manager.ensure_remote_tables().await?;

        // 远程切换检测
        let current_remote_id = format!(
            "{}:{}/{}",
            manager_config.host, manager_config.port, manager_config.database
        );
        let stored_remote_id = db_clone.get_sync_meta("remote_id")?.unwrap_or_default();
        if stored_remote_id != current_remote_id {
            info!("MySQL: 检测到远程数据库切换 ({stored_remote_id} -> {current_remote_id})");
            if reader.has_literatures().await? {
                info!("MySQL: 远程有文献数据，判定为数据库迁移，执行全量拉取");
            } else {
                info!("MySQL: 远程为空，判定为新云，标记本地全量推送");
                db_clone.mark_all_dirty_for_sync()?;
            }
            db_clone.clear_sync_timestamps()?;
            db_clone.clear_attachment_etags()?;
            db_clone.set_sync_meta("remote_id", &current_remote_id)?;
        }

        info!("MySQL: 正在同步标签...");
        if let Err(e) = perform_sync_tags(
            &mut writer,
            &mut reader,
            manager_config.use_remote,
            &manager_config.host,
            db_clone.clone(),
        )
        .await
        {
            error!("MySQL: 标签同步失败: {e}");
        }

        info!("MySQL: 正在推送本地变更...");
        push_dirty_records(
            &mut writer,
            db_clone.clone(),
            &base_path_buf,
            allowed_ids.as_deref(),
        )
        .await?;

        info!("MySQL: 正在拉取远程全部数据...");
        let conflicts = pull_remote_changes(&mut reader, db_clone.clone(), &base_path_buf).await?;

        info!(
            "MySQL: 元数据同步任务圆满完成，发现 {} 个冲突",
            conflicts.len()
        );
        Ok(conflicts)
    };

    if let Ok(result) = tokio::time::timeout(std::time::Duration::from_secs(300), sync_task).await {
        result
    } else {
        let msg = "MySQL 同步超时 (300秒)";
        error!("{msg}");
        Err(anyhow!(msg))
    }
}

async fn perform_sync_tags(
    writer: &mut MySqlSyncWriter,
    reader: &mut MySqlSyncReader,
    use_remote: bool,
    host: &str,
    db: Arc<Database>,
) -> Result<Vec<Tag>> {
    if !use_remote {
        return Ok(Vec::new());
    }
    info!("MySQL: 开始标签同步 (远程主机: {host})");

    let dirty_tags = db.get_dirty_tags()?;
    if !dirty_tags.is_empty() {
        info!("MySQL: 正在推送 {} 个本地变更标签...", dirty_tags.len());
        for tag in dirty_tags {
            if let Err(e) = writer.upsert_tag(&tag).await {
                error!(
                    "MySQL: 推送标签失败 [名称: {}, ID: {}]: {}",
                    tag.name, tag.id, e
                );
            } else {
                debug!("MySQL: 成功推送标签 [名称: {}, ID: {}]", tag.name, tag.id);
                if let Err(e) = db.mark_tag_clean(&tag.id) {
                    error!("MySQL: 更新本地标签同步状态失败 (ID: {}): {}", tag.id, e);
                }
            }
        }
    }

    let last_sync_time = db
        .get_last_sync_time("tags")?
        .unwrap_or_else(|| "0".to_string());

    let rows = reader.fetch_tags_since(&last_sync_time).await?;

    let mut updated_tags = Vec::new();
    if !rows.is_empty() {
        info!("MySQL: 发现 {} 个远程标签更新", rows.len());
        let mut max_ua: i64 = last_sync_time.parse().unwrap_or(0);

        for row in rows {
            if let Some(ua) = row.updated_at
                && ua > max_ua
            {
                max_ua = ua;
            }
            let tag = row.into_model();
            conflict::merge_remote_tag(&db, tag.clone())?;
            updated_tags.push(tag);
        }
        db.set_last_sync_time("tags", &max_ua.to_string())?;
    }

    Ok(updated_tags)
}

async fn push_dirty_records(
    writer: &mut MySqlSyncWriter,
    db: Arc<Database>,
    base_path: &Path,
    allowed_attachment_ids: Option<&[String]>,
) -> Result<()> {
    let authors = db.get_dirty_authors()?;
    if !authors.is_empty() {
        info!("MySQL: 正在推送 {} 个脏作者记录...", authors.len());
        for a in authors {
            debug!("MySQL: 推送作者: {} (ID: {})", author_full_name(&a), a.id);
            if let Err(e) = writer.upsert_author(&a).await {
                error!(
                    "MySQL: 推送作者失败 '{}' (ID: {}): {}",
                    author_full_name(&a),
                    a.id,
                    e
                );
            } else if let Err(e) = db.mark_author_synced(&a.id) {
                error!("MySQL: 更新本地作者同步状态失败 (ID: {}): {}", a.id, e);
            }
        }
    }

    let folders = db.get_dirty_folders()?;
    if !folders.is_empty() {
        info!("MySQL: 正在推送 {} 个脏文件夹记录...", folders.len());
        for f in folders {
            debug!("MySQL: 推送文件夹: {} (ID: {})", f.name, f.id);
            if let Err(e) = writer.upsert_folder(&f).await {
                error!("MySQL: 推送文件夹失败 '{}' (ID: {}): {}", f.name, f.id, e);
            } else if let Err(e) = db.mark_folder_synced(&f.id) {
                error!("MySQL: 更新本地文件夹同步状态失败 (ID: {}): {}", f.id, e);
            }
        }
    }

    let pubs = db.get_dirty_publications()?;
    if !pubs.is_empty() {
        info!("MySQL: 正在推送 {} 个脏出版源记录...", pubs.len());
        for p in pubs {
            debug!("MySQL: 推送出版源: {} (ID: {})", p.name, p.id);
            if let Err(e) = writer.upsert_publication(&p).await {
                error!("MySQL: 推送出版源失败 '{}' (ID: {}): {}", p.name, p.id, e);
            } else if let Err(e) = db.mark_publication_synced(&p.id) {
                error!("MySQL: 更新本地出版源同步状态失败 (ID: {}): {}", p.id, e);
            }
        }
    }

    let lits = db.get_dirty_literatures()?;
    if !lits.is_empty() {
        info!("MySQL: 正在推送 {} 篇文献修改...", lits.len());
        for lit in lits {
            debug!("MySQL: 推送文献: '{}' (ID: {})", lit.title, lit.id);
            if let Err(e) = writer.upsert_literature(&lit).await {
                error!(
                    "MySQL: 推送文献失败 '{}' (ID: {}): {}",
                    lit.title, lit.id, e
                );
            } else if let Err(e) = db.mark_literature_synced(&lit.id) {
                error!("MySQL: 更新本地文献同步状态失败 (ID: {}): {}", lit.id, e);
            }
        }
    }

    let (auth_rels, fold_rels, tag_rels) = db.get_dirty_relations()?;
    if !auth_rels.is_empty() || !fold_rels.is_empty() || !tag_rels.is_empty() {
        info!(
            "MySQL: 正在推送关联关系: 作者关系={}, 文件夹关系={}, 标签关系={}",
            auth_rels.len(),
            fold_rels.len(),
            tag_rels.len()
        );
        for r in auth_rels {
            debug!("MySQL: 推送作者关联: 文献ID={} <-> 作者ID={}", r.0, r.1);
            if let Err(e) = writer
                .upsert_author_relation(&r.0, &r.1, r.2.unwrap_or(0), r.3, r.4)
                .await
            {
                error!(
                    "MySQL: 推送作者关联失败 (文献: {}, 作者: {}): {}",
                    r.0, r.1, e
                );
            } else if let Err(e) = db.mark_relation_synced("literature_authors", &r.0, &r.1) {
                error!(
                    "MySQL: 更新本地作者关联同步状态失败 (文献: {}, 作者: {}): {}",
                    r.0, r.1, e
                );
            }
        }
        for r in fold_rels {
            debug!("MySQL: 推送文件夹关联: 文献ID={} <-> 文件夹ID={}", r.0, r.1);
            if let Err(e) = writer.upsert_folder_relation(&r.0, &r.1, r.2, r.3).await {
                error!(
                    "MySQL: 推送文件夹关联失败 (文献: {}, 文件夹: {}): {}",
                    r.0, r.1, e
                );
            } else if let Err(e) = db.mark_relation_synced("literature_folders", &r.0, &r.1) {
                error!(
                    "MySQL: 更新本地文件夹关联同步状态失败 (文献: {}, 文件夹: {}): {}",
                    r.0, r.1, e
                );
            }
        }
        for r in tag_rels {
            debug!("MySQL: 推送标签关联: 文献ID={} <-> 标签ID={}", r.0, r.1);
            if let Err(e) = writer.upsert_tag_relation(&r.0, &r.1, r.2, r.3).await {
                error!(
                    "MySQL: 推送标签关联失败 (文献: {}, 标签: {}): {}",
                    r.0, r.1, e
                );
            } else if let Err(e) = db.mark_relation_synced("literature_tags", &r.0, &r.1) {
                error!(
                    "MySQL: 更新本地标签关联同步状态失败 (文献: {}, 标签: {}): {}",
                    r.0, r.1, e
                );
            }
        }
    }

    let atts = db.get_dirty_attachments()?;
    if !atts.is_empty() {
        let total_dirty = atts.len();
        let filtered_atts: Vec<_> = if let Some(allowed_ids) = allowed_attachment_ids {
            atts.into_iter()
                .filter(|a| allowed_ids.contains(&a.id))
                .collect()
        } else {
            atts
        };

        if filtered_atts.len() < total_dirty {
            info!(
                "MySQL: 发现 {} 个脏附件记录，但只推送 {} 个成功上传到 WebDAV 的附件",
                total_dirty,
                filtered_atts.len()
            );
        } else {
            info!("MySQL: 正在推送 {} 个脏附件记录...", filtered_atts.len());
        }

        for a in filtered_atts {
            debug!("MySQL: 推送附件: {} (ID: {})", a.file_name, a.id);
            let abs_path = Path::new(&a.file_path);
            let rel_path_str = if let Ok(rel) = abs_path.strip_prefix(base_path) {
                rel.to_string_lossy().replace('\\', "/")
            } else {
                a.file_name.clone()
            };

            if let Err(e) = writer.upsert_attachment(&a, &rel_path_str).await {
                error!(
                    "MySQL: 推送附件失败 '{}' (ID: {}): {}",
                    a.file_name, a.id, e
                );
            } else if let Err(e) = db.mark_attachment_synced(&a.id) {
                error!("MySQL: 更新本地附件同步状态失败 (ID: {}): {}", a.id, e);
            }
        }
    }

    let feeds = db.get_dirty_feeds()?;
    if !feeds.is_empty() {
        info!("MySQL: 正在推送 {} 个脏订阅源记录...", feeds.len());
        for f in feeds {
            debug!("MySQL: 推送订阅源: {} (ID: {})", f.name, f.id);
            let normalized_last_up = f.last_updated_at.as_ref().map(|s| normalize_time_string(s));

            if let Err(e) = writer.upsert_feed(&f, normalized_last_up.as_deref()).await {
                error!("MySQL: 推送订阅源失败 '{}' (ID: {}): {}", f.name, f.id, e);
            } else if let Err(e) = db.mark_feed_synced(&f.id) {
                error!("MySQL: 更新本地订阅源同步状态失败 (ID: {}): {}", f.id, e);
            }
        }
    }

    let items = db.get_dirty_feed_items()?;
    if !items.is_empty() {
        info!("MySQL: 正在推送 {} 个脏订阅条目记录...", items.len());
        for i in items {
            debug!("MySQL: 推送订阅条目: {} (ID: {})", i.title, i.id);
            let normalized_pub_at = i.published_at.as_ref().map(|s| normalize_time_string(s));

            if let Err(e) = writer
                .upsert_feed_item(&i, normalized_pub_at.as_deref())
                .await
            {
                error!(
                    "MySQL: 推送订阅条目失败 '{}' (ID: {}): {}",
                    i.title, i.id, e
                );
            } else if let Err(e) = db.mark_feed_item_synced(&i.id) {
                error!("MySQL: 更新本地订阅条目同步状态失败 (ID: {}): {}", i.id, e);
            }
        }
    }
    let citations = db.get_dirty_citations()?;
    if !citations.is_empty() {
        info!("MySQL: 正在推送 {} 个脏引用记录...", citations.len());
        for c in citations {
            debug!("MySQL: 推送引用: {} -> {}", c.source_id, c.target_id);
            if let Err(e) = writer.upsert_citation(&c).await {
                error!(
                    "MySQL: 推送引用失败 '{} -> {}': {}",
                    c.source_id, c.target_id, e
                );
            } else if let Err(e) = db.mark_citation_synced(&c.source_id, &c.target_id) {
                error!(
                    "MySQL: 更新本地引用同步状态失败 ({} -> {}): {}",
                    c.source_id, c.target_id, e
                );
            }
        }
    }
    let annotations = db.get_dirty_annotations()?;
    if !annotations.is_empty() {
        info!("MySQL: 正在推送 {} 个脏注释记录...", annotations.len());
        for ann in annotations {
            debug!("MySQL: 推送注释: {}", ann.id);
            if let Err(e) = writer.upsert_annotation(&ann).await {
                error!("MySQL: 推送注释失败 '{}': {}", ann.id, e);
            } else if let Err(e) = db.mark_annotation_synced(&ann.id) {
                error!("MySQL: 更新本地注释同步状态失败 ({}): {}", ann.id, e);
            }
        }
    }

    let notes = db.get_dirty_notes()?;
    if !notes.is_empty() {
        info!("MySQL: 正在推送 {} 个脏笔记记录...", notes.len());
        for note in notes {
            debug!("MySQL: 推送笔记: {} (ID: {})", note.title, note.id);
            if let Err(e) = writer.upsert_note(&note).await {
                error!("MySQL: 推送笔记失败 '{}': {}", note.id, e);
            } else if let Err(e) = db.mark_note_synced(&note.id) {
                error!("MySQL: 更新本地笔记同步状态失败 ({}): {}", note.id, e);
            }
        }
    }

    Ok(())
}

async fn pull_remote_changes(
    reader: &mut MySqlSyncReader,
    db: Arc<Database>,
    base_path: &Path,
) -> Result<Vec<Literature>> {
    info!("MySQL: 正在增量拉取远程变更...");

    let mut conflicts = Vec::new();

    // ── authors ──
    debug!("MySQL: [pull] 拉取表 authors");
    let last_sync = db
        .get_last_sync_time("authors")?
        .unwrap_or_else(|| "0".to_string());
    let rows = reader.fetch_authors_since(&last_sync).await?;
    if !rows.is_empty() {
        info!("MySQL: 发现 {} 条远程作者更新", rows.len());
        let mut max_ua: i64 = last_sync.parse().unwrap_or(0);
        for r in rows {
            let row = r;
            if let Some(ua) = row.updated_at
                && ua > max_ua
            {
                max_ua = ua;
            }
            conflict::merge_remote_author(&db, row.into_model())?;
        }
        db.set_last_sync_time("authors", &max_ua.to_string())?;
    }

    // ── folders ──
    debug!("MySQL: [pull] 拉取表 folders");
    let last_sync = db
        .get_last_sync_time("folders")?
        .unwrap_or_else(|| "0".to_string());
    let rows = reader.fetch_folders_since(&last_sync).await?;
    if !rows.is_empty() {
        info!("MySQL: 发现 {} 条远程文件夹更新", rows.len());
        let mut max_ua: i64 = last_sync.parse().unwrap_or(0);
        for r in rows {
            let row = r;
            if let Some(ua) = row.updated_at
                && ua > max_ua
            {
                max_ua = ua;
            }
            conflict::merge_remote_folder(&db, row.into_model())?;
        }
        db.set_last_sync_time("folders", &max_ua.to_string())?;
    }

    // ── publications ──
    debug!("MySQL: [pull] 拉取表 publications");
    let last_sync = db
        .get_last_sync_time("publications")?
        .unwrap_or_else(|| "0".to_string());
    let rows = reader.fetch_publications_since(&last_sync).await?;
    if !rows.is_empty() {
        info!("MySQL: 发现 {} 条远程出版源更新", rows.len());
        let mut max_ua: i64 = last_sync.parse().unwrap_or(0);
        for r in rows {
            let row = r;
            if let Some(ua) = row.updated_at
                && ua > max_ua
            {
                max_ua = ua;
            }
            conflict::merge_remote_publication(&db, row.into_model())?;
        }
        db.set_last_sync_time("publications", &max_ua.to_string())?;
    }

    // ── literatures ──
    debug!("MySQL: [pull] 拉取表 literatures");
    let last_sync = db
        .get_last_sync_time("literatures")?
        .unwrap_or_else(|| "0".to_string());
    let rows = reader.fetch_literatures_since(&last_sync).await?;
    if !rows.is_empty() {
        info!("MySQL: 发现 {} 条远程文献更新", rows.len());
        let mut max_ua: i64 = last_sync.parse().unwrap_or(0);
        for r in rows {
            let row = r;
            if let Some(ua) = row.updated_at
                && ua > max_ua
            {
                max_ua = ua;
            }
            let lit = row.into_literature();
            if let Some(conflict) = conflict::merge_remote_literature(&db, lit)? {
                conflicts.push(conflict);
            }
        }
        db.set_last_sync_time("literatures", &max_ua.to_string())?;
    }

    // ── literature_authors ──
    debug!("MySQL: [pull] 拉取表 literature_authors");
    let last_sync = db
        .get_last_sync_time("literature_authors")?
        .unwrap_or_else(|| "0".to_string());
    let rows = reader.fetch_author_relations_since(&last_sync).await?;
    if !rows.is_empty() {
        info!("MySQL: 发现 {} 条远程作者关联更新", rows.len());
        let mut max_ua: i64 = last_sync.parse().unwrap_or(0);
        for r in rows {
            let (lid, aid, sort_order, is_deleted, version, ua) = r;
            if ua > max_ua {
                max_ua = ua;
            }
            conflict::merge_remote_relation(
                &db,
                "literature_authors",
                &lid,
                &aid,
                Some(sort_order),
                is_deleted,
                version,
            )?;
        }
        db.set_last_sync_time("literature_authors", &max_ua.to_string())?;
    }

    // ── literature_folders ──
    debug!("MySQL: [pull] 拉取表 literature_folders");
    let last_sync = db
        .get_last_sync_time("literature_folders")?
        .unwrap_or_else(|| "0".to_string());
    let rows = reader.fetch_folder_relations_since(&last_sync).await?;
    if !rows.is_empty() {
        info!("MySQL: 发现 {} 条远程文件夹关联更新", rows.len());
        let mut max_ua: i64 = last_sync.parse().unwrap_or(0);
        for r in rows {
            let (lid, fid, is_deleted, version, ua) = r;
            if ua > max_ua {
                max_ua = ua;
            }
            conflict::merge_remote_relation(
                &db,
                "literature_folders",
                &lid,
                &fid,
                None,
                is_deleted,
                version,
            )?;
        }
        db.set_last_sync_time("literature_folders", &max_ua.to_string())?;
    }

    // ── literature_tags ──
    debug!("MySQL: [pull] 拉取表 literature_tags");
    let last_sync = db
        .get_last_sync_time("literature_tags")?
        .unwrap_or_else(|| "0".to_string());
    let rows = reader.fetch_tag_relations_since(&last_sync).await?;
    if !rows.is_empty() {
        info!("MySQL: 发现 {} 条远程标签关联更新", rows.len());
        let mut max_ua: i64 = last_sync.parse().unwrap_or(0);
        for r in rows {
            let (lid, tid, is_deleted, version, ua) = r;
            if ua > max_ua {
                max_ua = ua;
            }
            conflict::merge_remote_relation(
                &db,
                "literature_tags",
                &lid,
                &tid,
                None,
                is_deleted,
                version,
            )?;
        }
        db.set_last_sync_time("literature_tags", &max_ua.to_string())?;
    }

    // ── attachments ──
    debug!("MySQL: [pull] 拉取表 attachments");
    let last_sync = db
        .get_last_sync_time("attachments")?
        .unwrap_or_else(|| "0".to_string());
    let rows = reader.fetch_attachments_since(&last_sync).await?;
    if !rows.is_empty() {
        info!("MySQL: 发现 {} 条远程附件更新", rows.len());
        let mut max_ua: i64 = last_sync.parse().unwrap_or(0);
        for r in rows {
            let row = r;
            if let Some(ua) = row.updated_at
                && ua > max_ua
            {
                max_ua = ua;
            }
            conflict::merge_remote_attachment(&db, row.into_model(base_path))?;
        }
        db.set_last_sync_time("attachments", &max_ua.to_string())?;
    }

    // ── feeds ──
    debug!("MySQL: [pull] 拉取表 feeds");
    let last_sync = db
        .get_last_sync_time("feeds")?
        .unwrap_or_else(|| "0".to_string());
    let rows = reader.fetch_feeds_since(&last_sync).await?;
    if !rows.is_empty() {
        info!("MySQL: 发现 {} 条远程订阅源更新", rows.len());
        let mut max_ua: i64 = last_sync.parse().unwrap_or(0);
        for r in rows {
            let row = r;
            if let Some(ua) = row.updated_at
                && ua > max_ua
            {
                max_ua = ua;
            }
            conflict::merge_remote_feed(&db, row.into_model())?;
        }
        db.set_last_sync_time("feeds", &max_ua.to_string())?;
    }

    // ── feed_items ──
    debug!("MySQL: [pull] 拉取表 feed_items");
    let last_sync = db
        .get_last_sync_time("feed_items")?
        .unwrap_or_else(|| "0".to_string());
    let rows = reader.fetch_feed_items_since(&last_sync).await?;
    if !rows.is_empty() {
        info!("MySQL: 发现 {} 条远程订阅条目更新", rows.len());
        let mut max_ua: i64 = last_sync.parse().unwrap_or(0);
        for r in rows {
            let row = r;
            if let Some(ua) = row.updated_at
                && ua > max_ua
            {
                max_ua = ua;
            }
            conflict::merge_remote_feed_item(&db, row.into_model())?;
        }
        db.set_last_sync_time("feed_items", &max_ua.to_string())?;
    }

    // ── literature_citations ──
    debug!("MySQL: [pull] 拉取表 literature_citations");
    let last_sync = db
        .get_last_sync_time("literature_citations")?
        .unwrap_or_else(|| "0".to_string());
    let rows = reader.fetch_citations_since(&last_sync).await?;
    if !rows.is_empty() {
        info!("MySQL: 发现 {} 条远程引用更新", rows.len());
        let mut max_ua: i64 = last_sync.parse().unwrap_or(0);
        for r in rows {
            if r.updated_at > max_ua {
                max_ua = r.updated_at;
            }
            conflict::merge_remote_citation(&db, r)?;
        }
        db.set_last_sync_time("literature_citations", &max_ua.to_string())?;
    }

    // ── annotations (BIGINT updated_at) ──
    debug!("MySQL: [pull] 拉取表 annotations");
    let last_sync = db
        .get_last_sync_time("annotations")?
        .unwrap_or_else(|| "0".to_string());
    let rows = reader.fetch_annotations_since(&last_sync).await?;
    if !rows.is_empty() {
        info!("MySQL: 发现 {} 条远程注释更新", rows.len());
        let mut max_ua: i64 = last_sync.parse().unwrap_or(0);
        for ann in rows {
            if ann.updated_at > max_ua {
                max_ua = ann.updated_at;
            }
            conflict::merge_remote_annotation(&db, ann)?;
        }
        db.set_last_sync_time("annotations", &max_ua.to_string())?;
    }

    // ── literature_notes (BIGINT updated_at) ──
    debug!("MySQL: [pull] 拉取表 literature_notes");
    let last_sync = db
        .get_last_sync_time("literature_notes")?
        .unwrap_or_else(|| "0".to_string());
    let rows = reader.fetch_notes_since(&last_sync).await?;
    if !rows.is_empty() {
        info!("MySQL: 发现 {} 条远程笔记更新", rows.len());
        let mut max_ua: i64 = last_sync.parse().unwrap_or(0);
        for note in rows {
            if note.updated_at > max_ua {
                max_ua = note.updated_at;
            }
            conflict::merge_remote_note(&db, note)?;
        }
        db.set_last_sync_time("literature_notes", &max_ua.to_string())?;
    }

    Ok(conflicts)
}

/// 把远程时间字符串解析为 `NaiveDateTime`，兼容历史数据中的常见格式。
fn parse_time_string(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    if let Ok(dt) = DateTime::parse_from_rfc2822(s) {
        return Some(dt.naive_utc());
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(dt);
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.naive_utc());
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%d %b %Y %H:%M:%S") {
        return Some(dt);
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%d %b %Y") {
        return d.and_hms_opt(0, 0, 0);
    }

    None
}

fn normalize_time_string(s: &str) -> String {
    if let Some(dt) = parse_time_string(s) {
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        s.to_string()
    }
}
