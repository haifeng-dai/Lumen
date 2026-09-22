//! 全库导入写入 + 词表查找
//!
//! services 规划并重写外键后，调用 `import_prepared_library` 盲写。
//! 词表匹配键：作者 last+first、出版源 name、文件夹 name、标签 name。

use std::collections::{HashMap, HashSet};

use log::{debug, info};
use models::library_export::{ExportManifest, ImportLookupMaps, PreparedLibraryImport};
use models::{AnnotationColor, AnnotationKind, Attachment, ImportCounts};
use rusqlite::{OptionalExtension, Result, params};

use super::Database;

fn color_str(c: &AnnotationColor) -> &'static str {
    match c {
        AnnotationColor::Yellow => "Yellow",
        AnnotationColor::Red => "Red",
        AnnotationColor::Green => "Green",
        AnnotationColor::Blue => "Blue",
        AnnotationColor::Purple => "Purple",
        AnnotationColor::Magenta => "Magenta",
        AnnotationColor::Orange => "Orange",
        AnnotationColor::Gray => "Gray",
    }
}

fn annotation_kind_cols(
    kind: &AnnotationKind,
) -> (
    &'static str,
    Option<f32>,
    Option<f32>,
    Option<f32>,
    Option<f32>,
) {
    match kind {
        AnnotationKind::Highlight => ("Highlight", None, None, None, None),
        AnnotationKind::Underline => ("Underline", None, None, None, None),
        AnnotationKind::Rectangle { x, y, w, h } => {
            ("Rectangle", Some(*x), Some(*y), Some(*w), Some(*h))
        }
    }
}

impl Database {
    /// 导入规划用：一次读出词表与已有主键集合
    pub fn import_lookup_maps(&self) -> Result<ImportLookupMaps> {
        self.with_conn(|conn| {
            let mut maps = ImportLookupMaps::default();

            let mut stmt =
                conn.prepare("SELECT id, first_name, last_name FROM authors WHERE is_deleted = 0")?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (id, first, last) = row?;
                maps.authors_by_name.insert((first, last), id);
            }

            let mut stmt =
                conn.prepare("SELECT id, name FROM publications WHERE is_deleted = 0")?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows {
                let (id, name) = row?;
                maps.publications_by_name.insert(name, id);
            }

            let mut stmt = conn.prepare("SELECT id, name FROM folders WHERE is_deleted = 0")?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows {
                let (id, name) = row?;
                maps.folders_by_name.insert(name, id.clone());
                // 系统夹也可按固定 id 命中
                maps.folders_by_name.entry(id.clone()).or_insert(id);
            }

            let mut stmt = conn.prepare("SELECT id, name FROM tags WHERE is_deleted = 0")?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows {
                let (id, name) = row?;
                maps.tags_by_name.insert(name, id);
            }

            fn load_ids(
                conn: &rusqlite::Connection,
                table: &str,
                out: &mut HashSet<String>,
            ) -> Result<()> {
                let mut stmt = conn.prepare(&format!("SELECT id FROM {table}"))?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                for id in rows {
                    out.insert(id?);
                }
                Ok(())
            }

            load_ids(conn, "literatures", &mut maps.existing_literature_ids)?;
            load_ids(conn, "attachments", &mut maps.existing_attachment_ids)?;
            load_ids(conn, "annotations", &mut maps.existing_annotation_ids)?;
            load_ids(conn, "literature_notes", &mut maps.existing_note_ids)?;
            load_ids(conn, "feeds", &mut maps.existing_feed_ids)?;
            load_ids(conn, "feed_items", &mut maps.existing_feed_item_ids)?;

            Ok(maps)
        })
    }

    /// 写入已规划、外键已重写的导入数据。
    pub fn import_prepared_library(&self, plan: &PreparedLibraryImport) -> Result<ImportCounts> {
        info!(
            "数据库: 写入导入计划 lits={} atts={}",
            plan.literatures.len(),
            plan.attachments.len()
        );
        self.with_transaction(|tx| {
            let now = chrono::Utc::now().timestamp();
            let mut counts = ImportCounts::default();

            for p in &plan.publications {
                tx.execute(
                    "INSERT OR IGNORE INTO publications (id, name, publication_type, abbreviation, publisher, ccf_rank, jcr_rank, cas_rank, is_dirty, is_deleted, version, created_at, updated_at, synced_version)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1,0,?9,?10,?11,0)",
                    params![
                        p.id,
                        p.name,
                        p.publication_type.to_string(),
                        p.abbreviation,
                        p.publisher,
                        p.ccf_rank,
                        p.jcr_rank,
                        p.cas_rank,
                        p.version,
                        p.created_at,
                        p.updated_at
                    ],
                )?;
                counts.publications_inserted += 1;
            }

            for a in &plan.authors {
                tx.execute(
                    "INSERT OR IGNORE INTO authors (id, first_name, last_name, middle_name, is_dirty, is_deleted, version, created_at, updated_at, synced_version)
                     VALUES (?1,?2,?3,?4,1,0,?5,?6,?7,0)",
                    params![
                        a.id,
                        a.first_name,
                        a.last_name,
                        a.middle_name,
                        a.version,
                        a.created_at,
                        a.updated_at
                    ],
                )?;
                counts.authors_inserted += 1;
            }

            for f in &plan.folders {
                tx.execute(
                    "INSERT OR IGNORE INTO folders (id, name, folder_type, parent_id, is_dirty, is_deleted, version, created_at, updated_at, synced_version)
                     VALUES (?1,?2,?3,?4,1,0,?5,?6,?7,0)",
                    params![
                        f.id,
                        f.name,
                        f.folder_type,
                        f.parent_id,
                        f.version,
                        f.created_at,
                        f.updated_at
                    ],
                )?;
                counts.folders_inserted += 1;
            }

            for t in &plan.tags {
                tx.execute(
                    "INSERT OR IGNORE INTO tags (id, name, color, is_dirty, is_deleted, version, created_at, updated_at, synced_version)
                     VALUES (?1,?2,?3,1,0,?4,?5,?6,0)",
                    params![t.id, t.name, t.color, t.version, t.created_at, t.updated_at],
                )?;
                counts.tags_inserted += 1;
            }

            for f in &plan.feeds {
                let type_str = match f.feed_type {
                    models::FeedType::Journal => "Journal",
                    models::FeedType::Conference => "Conference",
                    models::FeedType::Rss => "Rss",
                };
                tx.execute(
                    "INSERT OR IGNORE INTO feeds (id, name, title, feed_type, url, last_updated_at, update_interval, is_dirty, is_deleted, version, created_at, updated_at, synced_version)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,1,0,?8,?9,?10,0)",
                    params![
                        f.id,
                        f.name,
                        f.title,
                        type_str,
                        f.url,
                        f.last_updated_at,
                        f.update_interval,
                        f.version,
                        f.created_at,
                        f.updated_at
                    ],
                )?;
                counts.feeds_inserted += 1;
            }

            for item in &plan.feed_items {
                let authors: Vec<models::Author> = item
                    .authors
                    .iter()
                    .enumerate()
                    .map(|(i, a)| models::Author {
                        id: format!("{}-a{}", item.id, i),
                        first_name: a.first_name.clone(),
                        last_name: a.last_name.clone(),
                        middle_name: a.middle_name.clone(),
                        is_dirty: true,
                        is_deleted: false,
                        version: 1,
                        created_at: 0,
                        updated_at: 0,
                    })
                    .collect();
                let authors_json = serde_json::to_string(&authors).unwrap_or_else(|_| "[]".into());
                let type_json = serde_json::to_string(&item.literature_type)
                    .unwrap_or_else(|_| "\"article\"".into());
                tx.execute(
                    "INSERT OR IGNORE INTO feed_items (id, title, feed_id, is_read, is_added_to_library, added_at, authors, year, type, journal, publisher, abstract_text, doi, url, volume, issue, pages, published_at, is_dirty, is_deleted, version, updated_at, synced_version)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,1,0,?19,?20,0)",
                    params![
                        item.id,
                        item.title,
                        item.feed_id,
                        item.is_read,
                        item.is_added_to_library,
                        item.added_at,
                        authors_json,
                        item.year,
                        type_json,
                        item.journal,
                        item.publisher,
                        item.abstract_text,
                        item.doi,
                        item.url,
                        item.volume,
                        item.issue,
                        item.pages,
                        item.published_at,
                        item.version,
                        item.updated_at
                    ],
                )?;
                counts.feed_items_inserted += 1;
            }

            for lit in &plan.literatures {
                tx.execute(
                    "INSERT OR REPLACE INTO literatures (id, title, year, month, day, type, publication_id, volume, issue, pages, abstract_text, doi, arxiv_id, url, rating, reading_status, is_dirty, is_deleted, version, created_at, updated_at, synced_version)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,1,0,?17,?18,?19,0)",
                    params![
                        lit.id,
                        lit.title,
                        lit.year,
                        lit.month,
                        lit.day,
                        lit.literature_type.as_str(),
                        lit.publication_id,
                        lit.volume,
                        lit.issue,
                        lit.pages,
                        lit.abstract_text,
                        lit.doi,
                        lit.arxiv_id,
                        lit.url,
                        lit.rating,
                        lit.reading_status.to_string(),
                        lit.version,
                        lit.created_at,
                        lit.updated_at
                    ],
                )?;
            }

            for r in &plan.literature_authors {
                tx.execute(
                    "INSERT OR REPLACE INTO literature_authors (literature_id, author_id, sort_order, is_dirty, is_deleted, version, updated_at, synced_version)
                     VALUES (?1,?2,?3,1,0,?4,?5,0)",
                    params![
                        r.literature_id,
                        r.author_id,
                        r.sort_order,
                        r.version,
                        r.updated_at
                    ],
                )?;
            }
            for r in &plan.literature_folders {
                tx.execute(
                    "INSERT OR REPLACE INTO literature_folders (literature_id, folder_id, is_dirty, is_deleted, version, updated_at, synced_version)
                     VALUES (?1,?2,1,0,?3,?4,0)",
                    params![r.literature_id, r.folder_id, r.version, r.updated_at],
                )?;
            }
            for r in &plan.literature_tags {
                tx.execute(
                    "INSERT OR REPLACE INTO literature_tags (literature_id, tag_id, is_dirty, is_deleted, version, updated_at, synced_version)
                     VALUES (?1,?2,1,0,?3,?4,0)",
                    params![r.literature_id, r.tag_id, r.version, r.updated_at],
                )?;
            }

            let mut att_ids = HashSet::new();
            for att in &plan.attachments {
                let a = Attachment {
                    id: att.id.clone(),
                    literature_id: att.literature_id.clone(),
                    file_path: att.file_path.clone(),
                    file_name: att.file_name.clone(),
                    file_size: att.file_size,
                    mime_type: att.mime_type.clone(),
                    etag: att.etag.clone(),
                    hash: att.hash.clone(),
                    is_main: att.is_main,
                    is_dirty: true,
                    is_deleted: false,
                    version: att.version,
                    created_at: att.created_at,
                    updated_at: att.updated_at,
                };
                Self::insert_attachment_conn(tx, &a, &att.literature_id, now)?;
                att_ids.insert(att.id.clone());
                // 阅读器键 literature_id::attachment_id 与纯 att_id 都算已导入
                att_ids.insert(format!("{}::{}", att.literature_id, att.id));
                counts.attachments_inserted += 1;
            }

            for n in &plan.literature_notes {
                tx.execute(
                    "INSERT OR REPLACE INTO literature_notes (id, literature_id, title, content, sort_order, created_at, updated_at, is_deleted, is_dirty, version, synced_version)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,0,1,?8,0)",
                    params![
                        n.id,
                        n.literature_id,
                        n.title,
                        n.content,
                        n.sort_order,
                        n.created_at,
                        n.updated_at,
                        n.version
                    ],
                )?;
                counts.literature_notes_inserted += 1;
            }

            for ann in &plan.annotations {
                // document_id 可能是 att_id 或 literature_id::att_id
                let ok = att_ids.contains(&ann.document_id)
                    || ann
                        .document_id
                        .rsplit("::")
                        .next()
                        .is_some_and(|att| att_ids.contains(att));
                if !ok {
                    counts.annotations_skipped += 1;
                    continue;
                }
                let (kind_str, rx, ry, rw, rh) = annotation_kind_cols(&ann.kind);
                let range_json = ann
                    .range
                    .as_ref()
                    .and_then(|r| serde_json::to_string(r).ok());
                tx.execute(
                    "INSERT OR REPLACE INTO annotations (id, document_id, page, kind, color, range, note, rect_x, rect_y, rect_w, rect_h, created_at, updated_at, version, is_deleted, is_dirty, synced_version)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,0,1,0)",
                    params![
                        ann.id,
                        ann.document_id,
                        ann.page as i64,
                        kind_str,
                        color_str(&ann.color),
                        range_json,
                        ann.note,
                        rx,
                        ry,
                        rw,
                        rh,
                        ann.created_at,
                        ann.updated_at,
                        ann.version
                    ],
                )?;
                counts.annotations_inserted += 1;
            }

            for c in &plan.citations {
                tx.execute(
                    "INSERT OR REPLACE INTO literature_citations (source_id, target_id, is_dirty, is_deleted, version, updated_at, synced_version)
                     VALUES (?1,?2,1,0,?3,?4,0)",
                    params![c.source_id, c.target_id, c.version, c.updated_at],
                )?;
                counts.citations_inserted += 1;
            }

            debug!("数据库: 导入写入完成 {:?}", counts);
            let _ = HashMap::<String, String>::new();
            Ok(counts)
        })
    }

    pub fn validate_import_manifest(manifest: &ExportManifest) -> std::result::Result<(), String> {
        if manifest.format != models::LIBRARY_EXPORT_FORMAT {
            return Err(format!("unsupported format: {}", manifest.format));
        }
        if manifest.format_version > models::LIBRARY_EXPORT_FORMAT_VERSION {
            return Err(format!(
                "format_version {} is newer than supported {}",
                manifest.format_version,
                models::LIBRARY_EXPORT_FORMAT_VERSION
            ));
        }
        Ok(())
    }

    /// 查找 active 作者（first + last，不比较 middle）
    pub fn find_author_id_by_full_name(&self, first: &str, last: &str) -> Result<Option<String>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id FROM authors WHERE first_name = ?1 AND last_name = ?2 AND is_deleted = 0 LIMIT 1",
                params![first, last],
                |r| r.get(0),
            )
            .optional()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::constructors::create_literature;
    use models::{LiteratureType, ReadingStatus};

    #[test]
    fn import_lookup_maps_loads_authors_and_ids() {
        let db = Database::new(":memory:").unwrap();
        let lit = create_literature("lit-x", "T", LiteratureType::Article);
        db.insert_literature(&lit).unwrap();
        let maps = db.import_lookup_maps().unwrap();
        assert!(maps.existing_literature_ids.contains("lit-x"));
        // 系统文件夹
        assert!(
            maps.folders_by_name.contains_key("all")
                || maps.folders_by_name.contains_key("All Literature")
        );
    }

    #[test]
    fn import_prepared_writes_literature_and_attachment() {
        let db = Database::new(":memory:").unwrap();
        let plan = PreparedLibraryImport {
            literatures: vec![models::LiteratureExportRow {
                id: "L9".into(),
                title: "P".into(),
                year: Some(2024),
                month: None,
                day: None,
                literature_type: LiteratureType::Article,
                publication_id: None,
                volume: None,
                issue: None,
                pages: None,
                abstract_text: None,
                doi: None,
                arxiv_id: None,
                url: None,
                rating: 0,
                reading_status: ReadingStatus::Unread,
                is_dirty: true,
                version: 1,
                created_at: 1,
                updated_at: 1,
            }],
            attachments: vec![models::PreparedAttachmentInsert {
                source_package_id: "att-pkg".into(),
                id: "A9".into(),
                literature_id: "L9".into(),
                file_path: "/tmp/a9.pdf".into(),
                file_name: "a9.pdf".into(),
                file_size: 1,
                mime_type: None,
                etag: None,
                hash: None,
                is_main: true,
                version: 1,
                created_at: 1,
                updated_at: 1,
            }],
            imported_literature_ids: vec!["L9".into()],
            ..Default::default()
        };
        let counts = db.import_prepared_library(&plan).unwrap();
        assert_eq!(counts.attachments_inserted, 1);
        assert!(db.get_literature("L9").unwrap().is_some());
        let att = db.get_attachment("A9").unwrap().unwrap();
        assert_eq!(att.literature_id, "L9");
    }

    #[test]
    fn validate_manifest_rejects_bad_format() {
        let mut m = ExportManifest::new(0, "0", "pkg", models::ExportCounts::default());
        assert!(Database::validate_import_manifest(&m).is_ok());
        m.format = "nope".into();
        assert!(Database::validate_import_manifest(&m).is_err());
    }
}
