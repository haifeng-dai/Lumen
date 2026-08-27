use anyhow::Result;
use chrono::{DateTime, NaiveDateTime};
use models::{Annotation, AnnotationColor, AnnotationKind, Citation, LiteratureNote};
use mysql_async::prelude::*;
use mysql_common::value::Value;

use super::{
    AttachmentRow, AuthorRow, FeedItemRow, FeedRow, FolderRow, LiteratureRow, MySqlManager,
    PublicationRow, TagRow,
};

pub struct MySqlSyncReader {
    conn: mysql_async::Conn,
}

impl MySqlManager {
    pub async fn open_sync_reader(&self) -> Result<MySqlSyncReader> {
        let pool = self.get_pool().await?;
        Ok(MySqlSyncReader {
            conn: pool.get_conn().await?,
        })
    }
}

impl MySqlSyncReader {
    pub async fn has_literatures(&mut self) -> Result<bool> {
        let count: Option<i64> = self
            .conn
            .query_first("SELECT COUNT(*) FROM literatures")
            .await?;
        Ok(count.unwrap_or(0) > 0)
    }

    async fn fetch_rows<T, F>(&mut self, query: &str, timestamp: &str, parse: F) -> Result<Vec<T>>
    where
        F: Fn(mysql_async::Row) -> Result<T>,
    {
        let rows = self
            .conn
            .exec(query, mysql_async::params! { "t" => timestamp })
            .await?;
        rows.into_iter().map(parse).collect()
    }

    pub async fn fetch_tags_since(&mut self, timestamp: &str) -> Result<Vec<TagRow>> {
        self.fetch_rows(
            "SELECT id, name, color, is_deleted, version, created_at, updated_at FROM tags WHERE updated_at > :t",
            timestamp,
            TagRow::from_mysql_row,
        )
        .await
    }

    pub async fn fetch_authors_since(&mut self, timestamp: &str) -> Result<Vec<AuthorRow>> {
        self.fetch_rows(
            "SELECT id, first_name, last_name, middle_name, is_deleted, version, created_at, updated_at FROM authors WHERE updated_at > :t",
            timestamp,
            AuthorRow::from_mysql_row,
        )
        .await
    }

    pub async fn fetch_folders_since(&mut self, timestamp: &str) -> Result<Vec<FolderRow>> {
        self.fetch_rows(
            "SELECT id, name, folder_type, parent_id, is_deleted, version, created_at, updated_at FROM folders WHERE updated_at > :t",
            timestamp,
            FolderRow::from_mysql_row,
        )
        .await
    }

    pub async fn fetch_publications_since(
        &mut self,
        timestamp: &str,
    ) -> Result<Vec<PublicationRow>> {
        self.fetch_rows(
            "SELECT id, name, publication_type, abbreviation, publisher, ccf_rank, jcr_rank, cas_rank, is_deleted, version, created_at, updated_at FROM publications WHERE updated_at > :t",
            timestamp,
            PublicationRow::from_mysql_row,
        )
        .await
    }

    pub async fn fetch_literatures_since(&mut self, timestamp: &str) -> Result<Vec<LiteratureRow>> {
        self.fetch_rows(
            "SELECT l.id, l.title, l.year, l.month, l.day, l.type, l.volume, l.issue, l.pages, l.abstract_text, l.doi, l.arxiv_id, l.url, l.rating, l.reading_status, l.is_deleted, l.version, l.created_at, l.updated_at, p.id, p.name, p.publication_type, p.abbreviation, p.publisher, p.ccf_rank, p.jcr_rank, p.cas_rank, p.is_deleted, p.version, p.created_at, p.updated_at FROM literatures l LEFT JOIN publications p ON l.publication_id = p.id WHERE l.updated_at > :t",
            timestamp,
            LiteratureRow::from_mysql_row,
        )
        .await
    }

    pub async fn fetch_author_relations_since(
        &mut self,
        timestamp: &str,
    ) -> Result<Vec<(String, String, i32, bool, i32, i64)>> {
        self.fetch_rows("SELECT literature_id, author_id, sort_order, is_deleted, version, updated_at FROM literature_authors WHERE updated_at > :t", timestamp, |row| Ok((row.get(0).unwrap_or_default(), row.get(1).unwrap_or_default(), row.get(2).unwrap_or(0), row.get(3).unwrap_or(false), row.get(4).unwrap_or(1), row.get(5).unwrap_or(0)))).await
    }

    pub async fn fetch_folder_relations_since(
        &mut self,
        timestamp: &str,
    ) -> Result<Vec<(String, String, bool, i32, i64)>> {
        self.fetch_rows("SELECT literature_id, folder_id, is_deleted, version, updated_at FROM literature_folders WHERE updated_at > :t", timestamp, |row| Ok((row.get(0).unwrap_or_default(), row.get(1).unwrap_or_default(), row.get(2).unwrap_or(false), row.get(3).unwrap_or(1), row.get(4).unwrap_or(0)))).await
    }

    pub async fn fetch_tag_relations_since(
        &mut self,
        timestamp: &str,
    ) -> Result<Vec<(String, String, bool, i32, i64)>> {
        self.fetch_rows("SELECT literature_id, tag_id, is_deleted, version, updated_at FROM literature_tags WHERE updated_at > :t", timestamp, |row| Ok((row.get(0).unwrap_or_default(), row.get(1).unwrap_or_default(), row.get(2).unwrap_or(false), row.get(3).unwrap_or(1), row.get(4).unwrap_or(0)))).await
    }

    pub async fn fetch_attachments_since(&mut self, timestamp: &str) -> Result<Vec<AttachmentRow>> {
        self.fetch_rows("SELECT id, literature_id, file_path, file_name, file_size, mime_type, etag, hash, is_main, is_deleted, version, created_at, updated_at FROM attachments WHERE updated_at > :t", timestamp, AttachmentRow::from_mysql_row).await
    }

    pub async fn fetch_feeds_since(&mut self, timestamp: &str) -> Result<Vec<FeedRow>> {
        self.fetch_rows("SELECT id, name, feed_type, url, last_updated_at, update_interval, is_deleted, version, created_at, updated_at FROM feeds WHERE updated_at > :t", timestamp, FeedRow::from_mysql_row).await
    }

    pub async fn fetch_feed_items_since(&mut self, timestamp: &str) -> Result<Vec<FeedItemRow>> {
        self.fetch_rows("SELECT id, title, feed_id, is_read, is_added_to_library, added_at, authors, year, type, journal, publisher, abstract_text, doi, url, volume, issue, pages, published_at, is_deleted, version, updated_at FROM feed_items WHERE updated_at > :t", timestamp, FeedItemRow::from_mysql_row).await
    }

    pub async fn fetch_citations_since(&mut self, timestamp: &str) -> Result<Vec<Citation>> {
        self.fetch_rows("SELECT source_id, target_id, is_deleted, version, updated_at FROM literature_citations WHERE updated_at > :t", timestamp, |row| Ok(Citation { source_id: row.get(0).unwrap_or_default(), target_id: row.get(1).unwrap_or_default(), is_deleted: row.get(2).unwrap_or(false), version: row.get(3).unwrap_or(1), updated_at: row.get(4).unwrap_or(0) })).await
    }

    pub async fn fetch_annotations_since(&mut self, timestamp: &str) -> Result<Vec<Annotation>> {
        let rows = self.conn.exec("SELECT id, document_id, page, kind, color, `range`, note, rect_x, rect_y, rect_w, rect_h, created_at, updated_at, version, is_deleted FROM annotations WHERE updated_at > :t", mysql_async::params! { "t" => timestamp }).await?;
        rows.into_iter().map(annotation_from_row).collect()
    }

    pub async fn fetch_notes_since(&mut self, timestamp: &str) -> Result<Vec<LiteratureNote>> {
        let rows = self.conn.exec("SELECT id, literature_id, title, content, sort_order, created_at, updated_at, is_deleted, version FROM literature_notes WHERE updated_at > :t", mysql_async::params! { "t" => timestamp }).await?;
        rows.into_iter().map(note_from_row).collect()
    }
}

fn value_to_timestamp(value: Value) -> i64 {
    match value {
        Value::Int(value) => value,
        Value::UInt(value) => value as i64,
        Value::Bytes(bytes) => String::from_utf8(bytes)
            .ok()
            .and_then(|value| {
                value
                    .trim()
                    .parse()
                    .ok()
                    .or_else(|| parse_timestamp(value.trim()))
            })
            .unwrap_or(0),
        _ => 0,
    }
}

fn parse_timestamp(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc2822(value)
        .ok()
        .map(|value| value.timestamp())
        .or_else(|| {
            NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|value| value.and_utc().timestamp())
        })
        .or_else(|| {
            DateTime::parse_from_rfc3339(value)
                .ok()
                .map(|value| value.timestamp())
        })
        .or_else(|| {
            NaiveDateTime::parse_from_str(value, "%d %b %Y %H:%M:%S")
                .ok()
                .map(|value| value.and_utc().timestamp())
        })
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(value, "%d %b %Y")
                .ok()
                .and_then(|value| value.and_hms_opt(0, 0, 0))
                .map(|value| value.and_utc().timestamp())
        })
}

fn annotation_from_row(mut row: mysql_async::Row) -> Result<Annotation> {
    let kind = match row.take::<String, _>("kind").unwrap_or_default().as_str() {
        "Underline" => AnnotationKind::Underline,
        "Rectangle" => AnnotationKind::Rectangle {
            x: row.get("rect_x").unwrap_or(0.0),
            y: row.get("rect_y").unwrap_or(0.0),
            w: row.get("rect_w").unwrap_or(0.0),
            h: row.get("rect_h").unwrap_or(0.0),
        },
        _ => AnnotationKind::Highlight,
    };
    let color = match row.take::<String, _>("color").unwrap_or_default().as_str() {
        "Red" => AnnotationColor::Red,
        "Green" => AnnotationColor::Green,
        "Blue" => AnnotationColor::Blue,
        "Purple" => AnnotationColor::Purple,
        "Magenta" => AnnotationColor::Magenta,
        "Orange" => AnnotationColor::Orange,
        "Gray" => AnnotationColor::Gray,
        _ => AnnotationColor::Yellow,
    };
    let range = row
        .take::<Option<String>, _>("range")
        .flatten()
        .and_then(|value| serde_json::from_str(&value).ok());
    let created_at = value_to_timestamp(row.take::<Value, _>("created_at").unwrap_or(Value::NULL));
    Ok(Annotation {
        id: row.take("id").unwrap_or_default(),
        document_id: row.take("document_id").unwrap_or_default(),
        page: row.take("page").unwrap_or(0),
        kind,
        color,
        range,
        note: row.take::<Option<String>, _>("note").flatten(),
        created_at,
        updated_at: row.take("updated_at").unwrap_or(0),
        version: row.take::<Option<i32>, _>("version").flatten().unwrap_or(1),
        is_deleted: row
            .take::<Option<bool>, _>("is_deleted")
            .flatten()
            .unwrap_or(false),
        is_dirty: false,
    })
}

fn note_from_row(mut row: mysql_async::Row) -> Result<LiteratureNote> {
    Ok(LiteratureNote {
        id: row.take("id").unwrap_or_default(),
        literature_id: row.take("literature_id").unwrap_or_default(),
        title: row.take("title").unwrap_or_default(),
        content: row.take("content").unwrap_or_default(),
        sort_order: row.take("sort_order").unwrap_or(0),
        created_at: value_to_timestamp(row.take::<Value, _>("created_at").unwrap_or(Value::NULL)),
        updated_at: row.take("updated_at").unwrap_or(0),
        is_deleted: row
            .take::<Option<bool>, _>("is_deleted")
            .flatten()
            .unwrap_or(false),
        is_dirty: false,
        version: row.take::<Option<i32>, _>("version").flatten().unwrap_or(1),
    })
}
