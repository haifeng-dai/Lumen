//! 全库导出只读快照
//!
//! 只做 `is_deleted = 0` 的业务数据读取；不碰磁盘、不做导入。

use log::debug;
use models::library_export::{
    AnnotationExportRaw, AttachmentExportRaw, AuthorExportRow, CitationExportRow, FeedExportRow,
    FeedItemExportRow, FolderExportRow, LibraryExportSnapshot, LiteratureAuthorExportRow,
    LiteratureExportRow, LiteratureFolderExportRow, LiteratureNoteExportRow,
    LiteratureTagExportRow, PublicationExportRow, TagExportRow,
};
use models::{
    AnnotationColor, AnnotationKind, AuthorExportAuthor, FeedType, LiteratureType, ReadingStatus,
    TextRange,
};
use rusqlite::{Connection, Result, Row};

use super::Database;

fn parse_bool(v: i64) -> bool {
    v != 0
}

fn parse_literature_type(s: &str) -> LiteratureType {
    LiteratureType::from_str(s).unwrap_or(LiteratureType::Other)
}

fn parse_reading_status(s: &str) -> ReadingStatus {
    match s {
        "ToRead" => ReadingStatus::ToRead,
        "Reading" => ReadingStatus::Reading,
        "Read" => ReadingStatus::Read,
        _ => ReadingStatus::Unread,
    }
}

fn parse_feed_type(s: &str) -> FeedType {
    match s {
        "Journal" | "journal" => FeedType::Journal,
        "Conference" | "conference" => FeedType::Conference,
        _ => FeedType::Rss,
    }
}

fn parse_annotation_kind(
    kind: &str,
    rect_x: Option<f32>,
    rect_y: Option<f32>,
    rect_w: Option<f32>,
    rect_h: Option<f32>,
) -> AnnotationKind {
    match kind {
        "Underline" => AnnotationKind::Underline,
        "Rectangle" => AnnotationKind::Rectangle {
            x: rect_x.unwrap_or(0.0),
            y: rect_y.unwrap_or(0.0),
            w: rect_w.unwrap_or(0.0),
            h: rect_h.unwrap_or(0.0),
        },
        _ => AnnotationKind::Highlight,
    }
}

fn parse_annotation_color(s: &str) -> AnnotationColor {
    match s {
        "Red" => AnnotationColor::Red,
        "Green" => AnnotationColor::Green,
        "Blue" => AnnotationColor::Blue,
        "Purple" => AnnotationColor::Purple,
        "Magenta" => AnnotationColor::Magenta,
        "Orange" => AnnotationColor::Orange,
        "Gray" => AnnotationColor::Gray,
        _ => AnnotationColor::Yellow,
    }
}

fn map_text_range(raw: Option<String>) -> Option<TextRange> {
    raw.and_then(|s| serde_json::from_str(&s).ok())
}

impl Database {
    /// 读取有效业务数据快照（全部 `is_deleted = 0`）。
    ///
    /// 附件仅返回元数据与绝对 `file_path`；文件是否存在由 services 判定。
    pub fn export_snapshot(&self) -> Result<LibraryExportSnapshot> {
        debug!("数据库: 构建导出快照");
        self.with_conn(|conn| {
            let mut snap = LibraryExportSnapshot::default();
            snap.literatures = read_literatures(conn)?;
            snap.publications = read_publications(conn)?;
            snap.authors = read_authors(conn)?;
            snap.literature_authors = read_literature_authors(conn)?;
            snap.folders = read_folders(conn)?;
            snap.literature_folders = read_literature_folders(conn)?;
            snap.tags = read_tags(conn)?;
            snap.literature_tags = read_literature_tags(conn)?;
            snap.attachments_raw = read_attachments(conn)?;
            snap.annotations_raw = read_annotations(conn)?;
            snap.literature_notes = read_notes(conn)?;
            snap.citations = read_citations(conn)?;
            snap.feeds = read_feeds(conn)?;
            snap.feed_items = read_feed_items(conn)?;
            debug!(
                "数据库: 导出快照完成 lits={} atts={} anns={}",
                snap.literatures.len(),
                snap.attachments_raw.len(),
                snap.annotations_raw.len()
            );
            Ok(snap)
        })
    }
}

fn read_literatures(conn: &Connection) -> Result<Vec<LiteratureExportRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, year, month, day, type, publication_id, volume, issue, pages,
                abstract_text, doi, arxiv_id, url, rating, reading_status, is_dirty, version,
                created_at, updated_at
         FROM literatures WHERE is_deleted = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(LiteratureExportRow {
            id: row.get(0)?,
            title: row.get(1)?,
            year: row.get(2)?,
            month: row.get(3)?,
            day: row.get(4)?,
            literature_type: parse_literature_type(&row.get::<_, String>(5)?),
            publication_id: row.get(6)?,
            volume: row.get(7)?,
            issue: row.get(8)?,
            pages: row.get(9)?,
            abstract_text: row.get(10)?,
            doi: row.get(11)?,
            arxiv_id: row.get(12)?,
            url: row.get(13)?,
            rating: row.get(14)?,
            reading_status: parse_reading_status(&row.get::<_, String>(15)?),
            is_dirty: parse_bool(row.get(16)?),
            version: row.get(17)?,
            created_at: row.get(18)?,
            updated_at: row.get(19)?,
        })
    })?;
    rows.collect()
}

fn read_publications(conn: &Connection) -> Result<Vec<PublicationExportRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, publication_type, abbreviation, publisher, ccf_rank, jcr_rank, cas_rank,
                is_dirty, version, created_at, updated_at
         FROM publications WHERE is_deleted = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        let type_str: String = row.get(2)?;
        let publication_type = match type_str.to_lowercase().as_str() {
            "conference" => models::PublicationType::Conference,
            "book" => models::PublicationType::Book,
            _ => models::PublicationType::Journal,
        };
        Ok(PublicationExportRow {
            id: row.get(0)?,
            name: row.get(1)?,
            publication_type,
            abbreviation: row.get(3)?,
            publisher: row.get(4)?,
            ccf_rank: row.get(5)?,
            jcr_rank: row.get(6)?,
            cas_rank: row.get(7)?,
            is_dirty: parse_bool(row.get(8)?),
            version: row.get(9)?,
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
        })
    })?;
    rows.collect()
}

fn read_authors(conn: &Connection) -> Result<Vec<AuthorExportRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, first_name, last_name, middle_name, is_dirty, version, created_at, updated_at
         FROM authors WHERE is_deleted = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(AuthorExportRow {
            id: row.get(0)?,
            first_name: row.get(1)?,
            last_name: row.get(2)?,
            middle_name: row.get(3)?,
            is_dirty: parse_bool(row.get(4)?),
            version: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;
    rows.collect()
}

fn read_literature_authors(conn: &Connection) -> Result<Vec<LiteratureAuthorExportRow>> {
    let mut stmt = conn.prepare(
        "SELECT literature_id, author_id, sort_order, is_dirty, version, updated_at
         FROM literature_authors WHERE is_deleted = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(LiteratureAuthorExportRow {
            literature_id: row.get(0)?,
            author_id: row.get(1)?,
            sort_order: row.get(2)?,
            is_dirty: parse_bool(row.get(3)?),
            version: row.get(4)?,
            updated_at: row.get(5)?,
        })
    })?;
    rows.collect()
}

fn read_folders(conn: &Connection) -> Result<Vec<FolderExportRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, folder_type, parent_id, is_dirty, version, created_at, updated_at
         FROM folders WHERE is_deleted = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(FolderExportRow {
            id: row.get(0)?,
            name: row.get(1)?,
            folder_type: row.get(2)?,
            parent_id: row.get(3)?,
            is_dirty: parse_bool(row.get(4)?),
            version: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;
    rows.collect()
}

fn read_literature_folders(conn: &Connection) -> Result<Vec<LiteratureFolderExportRow>> {
    let mut stmt = conn.prepare(
        "SELECT literature_id, folder_id, is_dirty, version, updated_at
         FROM literature_folders WHERE is_deleted = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(LiteratureFolderExportRow {
            literature_id: row.get(0)?,
            folder_id: row.get(1)?,
            is_dirty: parse_bool(row.get(2)?),
            version: row.get(3)?,
            updated_at: row.get(4)?,
        })
    })?;
    rows.collect()
}

fn read_tags(conn: &Connection) -> Result<Vec<TagExportRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, color, is_dirty, version, created_at, updated_at
         FROM tags WHERE is_deleted = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(TagExportRow {
            id: row.get(0)?,
            name: row.get(1)?,
            color: row
                .get::<_, Option<String>>(2)?
                .unwrap_or_else(|| "#808080".into()),
            is_dirty: parse_bool(row.get(3)?),
            version: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

fn read_literature_tags(conn: &Connection) -> Result<Vec<LiteratureTagExportRow>> {
    let mut stmt = conn.prepare(
        "SELECT literature_id, tag_id, is_dirty, version, updated_at
         FROM literature_tags WHERE is_deleted = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(LiteratureTagExportRow {
            literature_id: row.get(0)?,
            tag_id: row.get(1)?,
            is_dirty: parse_bool(row.get(2)?),
            version: row.get(3)?,
            updated_at: row.get(4)?,
        })
    })?;
    rows.collect()
}

fn read_attachments(conn: &Connection) -> Result<Vec<AttachmentExportRaw>> {
    let mut stmt = conn.prepare(
        "SELECT id, literature_id, file_path, file_name, file_size, mime_type, etag, hash,
                is_main, is_dirty, version, created_at, updated_at
         FROM attachments WHERE is_deleted = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(AttachmentExportRaw {
            id: row.get(0)?,
            literature_id: row.get(1)?,
            file_path: row.get(2)?,
            file_name: row.get(3)?,
            file_size: row.get::<_, i64>(4)? as u64,
            mime_type: row.get(5)?,
            etag: row.get(6)?,
            hash: row.get(7)?,
            is_main: parse_bool(row.get(8)?),
            is_dirty: parse_bool(row.get(9)?),
            version: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
        })
    })?;
    rows.collect()
}

fn read_annotations(conn: &Connection) -> Result<Vec<AnnotationExportRaw>> {
    let mut stmt = conn.prepare(
        "SELECT id, document_id, page, kind, color, range, note,
                rect_x, rect_y, rect_w, rect_h, created_at, updated_at, version
         FROM annotations WHERE is_deleted = 0",
    )?;
    let rows = stmt.query_map([], |row: &Row<'_>| {
        let kind: String = row.get(3)?;
        let color: String = row.get(4)?;
        Ok(AnnotationExportRaw {
            id: row.get(0)?,
            document_id: row.get(1)?,
            page: row.get::<_, i64>(2)? as u16,
            kind: parse_annotation_kind(&kind, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?),
            color: parse_annotation_color(&color),
            range: map_text_range(row.get(5)?),
            note: row.get(6)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
            version: row.get(13)?,
        })
    })?;
    rows.collect()
}

fn read_notes(conn: &Connection) -> Result<Vec<LiteratureNoteExportRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, literature_id, title, content, sort_order, is_dirty, version,
                created_at, updated_at
         FROM literature_notes WHERE is_deleted = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(LiteratureNoteExportRow {
            id: row.get(0)?,
            literature_id: row.get(1)?,
            title: row.get(2)?,
            content: row.get(3)?,
            sort_order: row.get(4)?,
            is_dirty: parse_bool(row.get(5)?),
            version: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    })?;
    rows.collect()
}

fn read_citations(conn: &Connection) -> Result<Vec<CitationExportRow>> {
    let mut stmt = conn.prepare(
        "SELECT source_id, target_id, is_dirty, version, updated_at
         FROM literature_citations WHERE is_deleted = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(CitationExportRow {
            source_id: row.get(0)?,
            target_id: row.get(1)?,
            is_dirty: parse_bool(row.get(2)?),
            version: row.get(3)?,
            updated_at: row.get(4)?,
        })
    })?;
    rows.collect()
}

fn read_feeds(conn: &Connection) -> Result<Vec<FeedExportRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, title, feed_type, url, last_updated_at, update_interval, is_dirty,
                version, created_at, updated_at
         FROM feeds WHERE is_deleted = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(FeedExportRow {
            id: row.get(0)?,
            name: row.get(1)?,
            title: row.get(2)?,
            feed_type: parse_feed_type(&row.get::<_, String>(3)?),
            url: row.get(4)?,
            last_updated_at: row.get(5)?,
            update_interval: row.get(6)?,
            is_dirty: parse_bool(row.get(7)?),
            version: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    })?;
    rows.collect()
}

fn read_feed_items(conn: &Connection) -> Result<Vec<FeedItemExportRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, feed_id, is_read, is_added_to_library, added_at, authors, year,
                type, journal, publisher, abstract_text, doi, url, volume, issue, pages,
                published_at, is_dirty, version, updated_at
         FROM feed_items WHERE is_deleted = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        let authors_json: Option<String> = row.get(6)?;
        let authors: Vec<AuthorExportAuthor> = authors_json
            .and_then(|j| serde_json::from_str::<Vec<models::Author>>(&j).ok())
            .unwrap_or_default()
            .iter()
            .map(AuthorExportAuthor::from)
            .collect();
        let type_raw: Option<String> = row.get(8)?;
        let literature_type = type_raw
            .as_deref()
            .map(|s| {
                if let Some(t) = LiteratureType::from_str(s) {
                    t
                } else {
                    // 历史库可能存 `"article"` 或 Display 形态
                    let trimmed = s.trim_matches('"');
                    LiteratureType::from_str(trimmed).unwrap_or(LiteratureType::Other)
                }
            })
            .unwrap_or(LiteratureType::Other);
        Ok(FeedItemExportRow {
            id: row.get(0)?,
            title: row.get(1)?,
            feed_id: row.get(2)?,
            is_read: parse_bool(row.get(3)?),
            is_added_to_library: parse_bool(row.get(4)?),
            added_at: row.get(5)?,
            authors,
            year: row.get(7)?,
            literature_type,
            journal: row.get(9)?,
            publisher: row.get(10)?,
            abstract_text: row.get(11)?,
            doi: row.get(12)?,
            url: row.get(13)?,
            volume: row.get(14)?,
            issue: row.get(15)?,
            pages: row.get(16)?,
            published_at: row.get(17)?,
            is_dirty: parse_bool(row.get(18)?),
            version: row.get(19)?,
            updated_at: row.get(20)?,
        })
    })?;
    rows.collect()
}

/// 供测试与 services 使用：从快照汇总计数（未过滤前）
#[allow(dead_code)]
pub(crate) fn snapshot_raw_counts(snap: &LibraryExportSnapshot) -> models::ExportCounts {
    models::ExportCounts {
        literatures: snap.literatures.len(),
        authors: snap.authors.len(),
        publications: snap.publications.len(),
        tags: snap.tags.len(),
        folders: snap.folders.len(),
        attachments: snap.attachments_raw.len(),
        annotations: snap.annotations_raw.len(),
        literature_notes: snap.literature_notes.len(),
        citations: snap.citations.len(),
        feeds: snap.feeds.len(),
        feed_items: snap.feed_items.len(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::Database;
    use models::constructors::create_literature;
    use models::{Annotation, AnnotationColor, AnnotationKind, LiteratureType};
    use std::path::PathBuf;

    #[test]
    fn export_snapshot_excludes_soft_deleted() {
        let db = Database::new(":memory:").unwrap();
        let live = create_literature("lit-live", "Live", LiteratureType::Article);
        db.insert_literature(&live).unwrap();
        let dead = create_literature("lit-dead", "Dead", LiteratureType::Book);
        db.insert_literature(&dead).unwrap();
        db.delete_literature("lit-dead").unwrap();

        let snap = db.export_snapshot().unwrap();
        assert_eq!(snap.literatures.len(), 1);
        assert_eq!(snap.literatures[0].id, "lit-live");
        assert!(snap.literatures.iter().all(|l| l.id != "lit-dead"));
    }

    #[test]
    fn export_snapshot_includes_active_attachments_raw() {
        let db = Database::new(":memory:").unwrap();
        let lit = create_literature("lit-1", "T", LiteratureType::Article);
        db.insert_literature(&lit).unwrap();
        let mut att = models::constructors::create_attachment(
            "att-1".to_string(),
            "lit-1".to_string(),
            "/tmp/does-not-must-exist.pdf".to_string(),
            "paper.pdf".to_string(),
            10,
        );
        att.is_main = true;
        db.insert_attachment(&att).unwrap();

        let snap = db.export_snapshot().unwrap();
        assert_eq!(snap.attachments_raw.len(), 1);
        assert_eq!(snap.attachments_raw[0].file_name, "paper.pdf");
        assert_eq!(
            snap.attachments_raw[0].file_path,
            "/tmp/does-not-must-exist.pdf"
        );
    }

    #[test]
    fn export_snapshot_reads_annotations_raw() {
        let db = Database::new(":memory:").unwrap();
        let ann = Annotation {
            id: "ann-1".into(),
            document_id: "att-1".into(),
            page: 2,
            kind: AnnotationKind::Highlight,
            color: AnnotationColor::Yellow,
            range: None,
            note: Some("n".into()),
            created_at: 1,
            updated_at: 1,
            version: 1,
            is_deleted: false,
            is_dirty: false,
        };
        db.save_annotation(&ann).unwrap();
        let snap = db.export_snapshot().unwrap();
        assert_eq!(snap.annotations_raw.len(), 1);
        assert_eq!(snap.annotations_raw[0].document_id, "att-1");
        let _ = PathBuf::new();
    }
}
