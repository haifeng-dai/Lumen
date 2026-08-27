use anyhow::Result;
use models::{
    Annotation, AnnotationColor, AnnotationKind, Attachment, Author, Citation, Feed, FeedItem,
    Folder, Literature, LiteratureNote, Publication, Tag,
};
use mysql_async::prelude::*;

use super::MySqlManager;

pub struct MySqlSyncWriter {
    conn: mysql_async::Conn,
}

impl MySqlManager {
    pub async fn open_sync_writer(&self) -> Result<MySqlSyncWriter> {
        let pool = self.get_pool().await?;
        Ok(MySqlSyncWriter {
            conn: pool.get_conn().await?,
        })
    }
}

impl MySqlSyncWriter {
    pub async fn upsert_feed(
        &mut self,
        feed: &Feed,
        normalized_last_updated_at: Option<&str>,
    ) -> Result<()> {
        self.conn.exec_drop(
            "INSERT INTO feeds (id, name, feed_type, url, last_updated_at, update_interval, is_deleted, version, created_at, updated_at) VALUES (:id, :name, :type, :url, :last_up, :interval, :is_deleted, :version, :created_at, UNIX_TIMESTAMP()) ON DUPLICATE KEY UPDATE name=VALUES(name), feed_type=VALUES(feed_type), url=VALUES(url), last_updated_at=VALUES(last_updated_at), update_interval=VALUES(update_interval), is_deleted=VALUES(is_deleted), version=VALUES(version), updated_at=UNIX_TIMESTAMP()",
            mysql_async::params! {
                "id" => &feed.id,
                "name" => &feed.name,
                "type" => serde_json::to_string(&feed.feed_type).unwrap_or_default().replace('"', ""),
                "url" => &feed.url,
                "last_up" => normalized_last_updated_at,
                "interval" => feed.update_interval,
                "is_deleted" => feed.is_deleted,
                "version" => feed.version,
                "created_at" => &feed.created_at,
            },
        )
        .await?;
        Ok(())
    }

    pub async fn upsert_feed_item(
        &mut self,
        item: &FeedItem,
        normalized_published_at: Option<&str>,
    ) -> Result<()> {
        self.conn.exec_drop(
            "INSERT INTO feed_items (id, title, feed_id, is_read, is_added_to_library, added_at, authors, year, type, journal, publisher, abstract_text, doi, url, volume, issue, pages, published_at, is_deleted, version, updated_at) VALUES (:id, :title, :fid, :read, :added, :added_at, :authors, :year, :type, :journal, :publisher, :abstract, :doi, :url, :vol, :issue, :pages, :pub_at, :is_deleted, :version, UNIX_TIMESTAMP()) ON DUPLICATE KEY UPDATE title=VALUES(title), is_read=VALUES(is_read), is_added_to_library=VALUES(is_added_to_library), authors=VALUES(authors), abstract_text=VALUES(abstract_text), is_deleted=VALUES(is_deleted), version=VALUES(version), updated_at=UNIX_TIMESTAMP()",
            mysql_async::params! {
                "id" => &item.id,
                "title" => &item.title,
                "fid" => &item.feed_id,
                "read" => item.is_read,
                "added" => item.is_added_to_library,
                "added_at" => &item.added_at,
                "authors" => serde_json::to_string(&item.authors).unwrap_or_default(),
                "year" => item.year,
                "type" => serde_json::to_string(&item.literature_type).unwrap_or_default().replace('"', ""),
                "journal" => &item.journal,
                "publisher" => &item.publisher,
                "abstract" => &item.abstract_text,
                "doi" => &item.doi,
                "url" => &item.url,
                "vol" => &item.volume,
                "issue" => &item.issue,
                "pages" => &item.pages,
                "pub_at" => normalized_published_at,
                "is_deleted" => item.is_deleted,
                "version" => item.version,
            },
        )
        .await?;
        Ok(())
    }

    pub async fn upsert_citation(&mut self, citation: &Citation) -> Result<()> {
        self.conn.exec_drop(
            "INSERT INTO literature_citations (source_id, target_id, is_deleted, version, updated_at) VALUES (:sid, :tid, :is_deleted, :version, UNIX_TIMESTAMP()) ON DUPLICATE KEY UPDATE is_deleted=VALUES(is_deleted), version=VALUES(version), updated_at=UNIX_TIMESTAMP()",
            mysql_async::params! {
                "sid" => &citation.source_id,
                "tid" => &citation.target_id,
                "is_deleted" => citation.is_deleted,
                "version" => citation.version,
            },
        )
        .await?;
        Ok(())
    }

    pub async fn upsert_annotation(&mut self, annotation: &Annotation) -> Result<()> {
        let kind = match annotation.kind {
            AnnotationKind::Highlight => "Highlight",
            AnnotationKind::Underline => "Underline",
            AnnotationKind::Rectangle { .. } => "Rectangle",
        };
        let color = match annotation.color {
            AnnotationColor::Yellow => "Yellow",
            AnnotationColor::Red => "Red",
            AnnotationColor::Green => "Green",
            AnnotationColor::Blue => "Blue",
            AnnotationColor::Purple => "Purple",
            AnnotationColor::Magenta => "Magenta",
            AnnotationColor::Orange => "Orange",
            AnnotationColor::Gray => "Gray",
        };
        let range = annotation
            .range
            .as_ref()
            .and_then(|value| serde_json::to_string(value).ok());
        let (rect_x, rect_y, rect_w, rect_h) = match annotation.kind {
            AnnotationKind::Rectangle { x, y, w, h } => (Some(x), Some(y), Some(w), Some(h)),
            _ => (None, None, None, None),
        };
        self.conn.exec_drop(
            "INSERT INTO annotations (id, document_id, page, kind, color, `range`, note, rect_x, rect_y, rect_w, rect_h, is_deleted, version, created_at, updated_at) VALUES (:id, :document_id, :page, :kind, :color, :range, :note, :rect_x, :rect_y, :rect_w, :rect_h, :is_deleted, :version, :created_at, :updated_at) ON DUPLICATE KEY UPDATE page=VALUES(page), kind=VALUES(kind), color=VALUES(color), `range`=VALUES(`range`), note=VALUES(note), rect_x=VALUES(rect_x), rect_y=VALUES(rect_y), rect_w=VALUES(rect_w), rect_h=VALUES(rect_h), is_deleted=VALUES(is_deleted), version=VALUES(version), updated_at=VALUES(updated_at)",
            mysql_async::params! {
                "id" => &annotation.id,
                "document_id" => &annotation.document_id,
                "page" => annotation.page,
                "kind" => kind,
                "color" => color,
                "range" => range,
                "note" => &annotation.note,
                "rect_x" => rect_x,
                "rect_y" => rect_y,
                "rect_w" => rect_w,
                "rect_h" => rect_h,
                "is_deleted" => annotation.is_deleted,
                "version" => annotation.version,
                "created_at" => annotation.created_at,
                "updated_at" => annotation.updated_at,
            },
        )
        .await?;
        Ok(())
    }

    pub async fn upsert_note(&mut self, note: &LiteratureNote) -> Result<()> {
        self.conn.exec_drop(
            "INSERT INTO literature_notes (id, literature_id, title, content, sort_order, created_at, updated_at, is_deleted, version) VALUES (:id, :lit_id, :title, :content, :sort_order, :created_at, :updated_at, :is_deleted, :version) ON DUPLICATE KEY UPDATE literature_id=VALUES(literature_id), title=VALUES(title), content=VALUES(content), sort_order=VALUES(sort_order), updated_at=VALUES(updated_at), is_deleted=VALUES(is_deleted), version=VALUES(version)",
            mysql_async::params! {
                "id" => &note.id,
                "lit_id" => &note.literature_id,
                "title" => &note.title,
                "content" => &note.content,
                "sort_order" => note.sort_order,
                "created_at" => note.created_at,
                "updated_at" => note.updated_at,
                "is_deleted" => note.is_deleted,
                "version" => note.version,
            },
        )
        .await?;
        Ok(())
    }

    async fn upsert_relation(
        &mut self,
        table: &str,
        target_column: &str,
        literature_id: &str,
        target_id: &str,
        sort_order: Option<i32>,
        is_deleted: bool,
        version: i32,
    ) -> Result<()> {
        let query = if sort_order.is_some() {
            format!(
                "INSERT INTO {table} (literature_id, {target_column}, sort_order, is_deleted, version, updated_at) VALUES (:lid, :tid, :sort_order, :is_deleted, :version, UNIX_TIMESTAMP()) ON DUPLICATE KEY UPDATE sort_order=VALUES(sort_order), is_deleted=VALUES(is_deleted), version=VALUES(version), updated_at=UNIX_TIMESTAMP()"
            )
        } else {
            format!(
                "INSERT INTO {table} (literature_id, {target_column}, is_deleted, version, updated_at) VALUES (:lid, :tid, :is_deleted, :version, UNIX_TIMESTAMP()) ON DUPLICATE KEY UPDATE is_deleted=VALUES(is_deleted), version=VALUES(version), updated_at=UNIX_TIMESTAMP()"
            )
        };
        self.conn
            .exec_drop(
                query,
                mysql_async::params! {
                    "lid" => literature_id,
                    "tid" => target_id,
                    "sort_order" => sort_order,
                    "is_deleted" => is_deleted,
                    "version" => version,
                },
            )
            .await?;
        Ok(())
    }

    pub async fn upsert_author_relation(
        &mut self,
        literature_id: &str,
        author_id: &str,
        sort_order: i32,
        is_deleted: bool,
        version: i32,
    ) -> Result<()> {
        self.upsert_relation(
            "literature_authors",
            "author_id",
            literature_id,
            author_id,
            Some(sort_order),
            is_deleted,
            version,
        )
        .await
    }

    pub async fn upsert_folder_relation(
        &mut self,
        literature_id: &str,
        folder_id: &str,
        is_deleted: bool,
        version: i32,
    ) -> Result<()> {
        self.upsert_relation(
            "literature_folders",
            "folder_id",
            literature_id,
            folder_id,
            None,
            is_deleted,
            version,
        )
        .await
    }

    pub async fn upsert_tag_relation(
        &mut self,
        literature_id: &str,
        tag_id: &str,
        is_deleted: bool,
        version: i32,
    ) -> Result<()> {
        self.upsert_relation(
            "literature_tags",
            "tag_id",
            literature_id,
            tag_id,
            None,
            is_deleted,
            version,
        )
        .await
    }

    pub async fn upsert_attachment(
        &mut self,
        attachment: &Attachment,
        relative_path: &str,
    ) -> Result<()> {
        self.conn.exec_drop(
            "INSERT INTO attachments (id, literature_id, file_path, file_name, file_size, mime_type, etag, hash, is_main, is_deleted, version, created_at, updated_at) VALUES (:id, :lit_id, :path, :name, :size, :mime, :etag, :hash, :is_main, :is_deleted, :version, :created_at, UNIX_TIMESTAMP()) ON DUPLICATE KEY UPDATE file_path=VALUES(file_path), file_name=VALUES(file_name), file_size=VALUES(file_size), mime_type=VALUES(mime_type), etag=VALUES(etag), hash=VALUES(hash), is_main=VALUES(is_main), is_deleted=VALUES(is_deleted), version=VALUES(version), updated_at=UNIX_TIMESTAMP()",
            mysql_async::params! {
                "id" => &attachment.id,
                "lit_id" => &attachment.literature_id,
                "path" => relative_path,
                "name" => &attachment.file_name,
                "size" => attachment.file_size,
                "mime" => &attachment.mime_type,
                "etag" => &attachment.etag,
                "hash" => &attachment.hash,
                "is_main" => attachment.is_main,
                "is_deleted" => attachment.is_deleted,
                "version" => attachment.version,
                "created_at" => &attachment.created_at,
            },
        )
        .await?;
        Ok(())
    }

    pub async fn upsert_author(&mut self, author: &Author) -> Result<()> {
        self.conn.exec_drop(
            "INSERT INTO authors (id, first_name, last_name, middle_name, is_deleted, version, created_at, updated_at) VALUES (:id, :first_name, :last_name, :middle_name, :is_deleted, :version, :created_at, UNIX_TIMESTAMP()) ON DUPLICATE KEY UPDATE first_name=VALUES(first_name), last_name=VALUES(last_name), middle_name=VALUES(middle_name), is_deleted=VALUES(is_deleted), version=VALUES(version), updated_at=UNIX_TIMESTAMP()",
            mysql_async::params! {
                "id" => &author.id,
                "first_name" => &author.first_name,
                "last_name" => &author.last_name,
                "middle_name" => &author.middle_name,
                "is_deleted" => author.is_deleted,
                "version" => author.version,
                "created_at" => &author.created_at,
            },
        )
        .await?;
        Ok(())
    }

    pub async fn upsert_folder(&mut self, folder: &Folder) -> Result<()> {
        self.conn.exec_drop(
            "INSERT INTO folders (id, name, folder_type, parent_id, is_deleted, version, created_at, updated_at) VALUES (:id, :name, :type, :parent_id, :is_deleted, :version, :created_at, UNIX_TIMESTAMP()) ON DUPLICATE KEY UPDATE name=VALUES(name), folder_type=VALUES(folder_type), parent_id=VALUES(parent_id), is_deleted=VALUES(is_deleted), version=VALUES(version), updated_at=UNIX_TIMESTAMP()",
            mysql_async::params! {
                "id" => &folder.id,
                "name" => &folder.name,
                "type" => serde_json::to_string(&folder.folder_type).unwrap_or_default().replace('"', ""),
                "parent_id" => &folder.parent_id,
                "is_deleted" => folder.is_deleted,
                "version" => folder.version,
                "created_at" => &folder.created_at,
            },
        )
        .await?;
        Ok(())
    }

    pub async fn upsert_publication(&mut self, publication: &Publication) -> Result<()> {
        self.conn.exec_drop(
            "INSERT INTO publications (id, name, publication_type, abbreviation, publisher, ccf_rank, jcr_rank, cas_rank, is_deleted, version, created_at, updated_at) VALUES (:id, :name, :type, :abbr, :pub, :ccf, :jcr, :cas, :is_deleted, :version, :created_at, UNIX_TIMESTAMP()) ON DUPLICATE KEY UPDATE name=VALUES(name), publication_type=VALUES(publication_type), abbreviation=VALUES(abbreviation), publisher=VALUES(publisher), ccf_rank=VALUES(ccf_rank), jcr_rank=VALUES(jcr_rank), cas_rank=VALUES(cas_rank), is_deleted=VALUES(is_deleted), version=VALUES(version), updated_at=UNIX_TIMESTAMP()",
            mysql_async::params! {
                "id" => &publication.id,
                "name" => &publication.name,
                "type" => publication.publication_type.to_string(),
                "abbr" => &publication.abbreviation,
                "pub" => &publication.publisher,
                "ccf" => &publication.ccf_rank,
                "jcr" => &publication.jcr_rank,
                "cas" => &publication.cas_rank,
                "is_deleted" => publication.is_deleted,
                "version" => publication.version,
                "created_at" => &publication.created_at,
            },
        )
        .await?;
        Ok(())
    }

    pub async fn upsert_literature(&mut self, literature: &Literature) -> Result<()> {
        let publication_id = literature
            .publication
            .as_ref()
            .map(|publication| publication.id.clone());
        self.conn.exec_drop(
            "INSERT INTO literatures (id, title, year, month, day, type, publication_id, volume, issue, pages, abstract_text, doi, arxiv_id, url, rating, reading_status, is_deleted, version, created_at, updated_at) VALUES (:id, :title, :year, :month, :day, :type, :pub_id, :volume, :issue, :pages, :abstract_text, :doi, :arxiv_id, :url, :rating, :reading_status, :is_deleted, :version, :created_at, UNIX_TIMESTAMP()) ON DUPLICATE KEY UPDATE title=VALUES(title), year=VALUES(year), month=VALUES(month), day=VALUES(day), type=VALUES(type), publication_id=VALUES(publication_id), volume=VALUES(volume), issue=VALUES(issue), pages=VALUES(pages), abstract_text=VALUES(abstract_text), doi=VALUES(doi), arxiv_id=VALUES(arxiv_id), url=VALUES(url), rating=VALUES(rating), reading_status=VALUES(reading_status), is_deleted=VALUES(is_deleted), version=VALUES(version), updated_at=UNIX_TIMESTAMP()",
            mysql_async::params! {
                "id" => &literature.id,
                "title" => &literature.title,
                "year" => literature.year,
                "month" => literature.month,
                "day" => literature.day,
                "type" => serde_json::to_string(&literature.literature_type).unwrap_or_default().replace('"', ""),
                "pub_id" => &publication_id,
                "volume" => &literature.volume,
                "issue" => &literature.issue,
                "pages" => &literature.pages,
                "abstract_text" => &literature.abstract_text,
                "doi" => &literature.doi,
                "arxiv_id" => &literature.arxiv_id,
                "url" => &literature.url,
                "rating" => literature.rating,
                "reading_status" => literature.reading_status.to_string(),
                "is_deleted" => literature.is_deleted,
                "version" => literature.version,
                "created_at" => &literature.created_at,
            },
        )
        .await?;
        Ok(())
    }

    /// 将单条标签记录写入远程数据库。
    ///
    /// 这里只负责远程 SQL 原语；是否应该推送以及推送成功后的本地清脏，
    /// 由 services::sync 负责。
    pub async fn upsert_tag(&mut self, tag: &Tag) -> Result<()> {
        self.conn.exec_drop(
            "INSERT INTO tags (id, name, color, is_deleted, version, created_at, updated_at) VALUES (:id, :name, :color, :is_deleted, :version, :created_at, UNIX_TIMESTAMP()) ON DUPLICATE KEY UPDATE name=VALUES(name), color=VALUES(color), is_deleted=VALUES(is_deleted), version=VALUES(version), updated_at=UNIX_TIMESTAMP()",
            mysql_async::params! {
                "id" => &tag.id,
                "name" => &tag.name,
                "color" => &tag.color,
                "is_deleted" => tag.is_deleted,
                "version" => tag.version,
                "created_at" => &tag.created_at,
            },
        )
        .await?;
        Ok(())
    }
}
