//! 原子数据库同步原语。
//!
//! 这些接口只负责版本比较、资料库元数据和变化清单，不参与同步编排。

use anyhow::{Result, anyhow};
use models::{
    Annotation, Attachment, Author, Citation, Feed, FeedItem, Folder, Literature, LiteratureNote,
    Publication, Tag,
};
use mysql_async::{Params, TxOpts, Value as MySqlValue, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::future::Future;
use std::time::Instant;

use super::MySqlManager;

fn mysql_error_category(error: &anyhow::Error) -> &'static str {
    let text = error.to_string().to_ascii_lowercase();
    if text.contains("read-only") || text.contains("not read-only") {
        "read_only"
    } else if text.contains("timeout") || text.contains("timed out") {
        "timeout"
    } else if text.contains("connection") || text.contains("connect") {
        "connection"
    } else {
        "unknown"
    }
}

#[cfg(test)]
const MYSQL_SYNC_OPERATIONS: &[&str] = &[
    "snapshot_set_isolation",
    "snapshot_start_snapshot",
    "snapshot_read_sequence",
    "snapshot_read_entity_rows",
    "snapshot_commit",
    "snapshot_rollback",
    "incremental_set_isolation",
    "incremental_start_snapshot",
    "incremental_read_changes",
    "incremental_read_entity_rows",
    "incremental_commit",
    "incremental_rollback",
    "upload_start_transaction",
    "upload_version_check",
    "upload_business_row_write",
    "upload_change_log_write",
    "upload_commit",
];

async fn trace_mysql_operation<T, F, Fut>(
    run_id: Option<&str>,
    operation: &str,
    future: F,
) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let Some(run_id) = run_id else {
        return future().await;
    };
    let started = Instant::now();
    log::info!("[Sync][run={run_id}][stage=mysql] event=start elapsed_ms=0 operation={operation}");
    let result = future().await;
    let elapsed_ms = started.elapsed().as_millis();
    match &result {
        Ok(_) => log::info!(
            "[Sync][run={run_id}][stage=mysql] event=complete elapsed_ms={elapsed_ms} operation={operation}"
        ),
        Err(error) => log::warn!(
            "[Sync][run={run_id}][stage=mysql] event=failed elapsed_ms={elapsed_ms} operation={operation} error_category={}",
            mysql_error_category(error)
        ),
    }
    result
}

/// 变化清单支持的实体类别。使用枚举避免把调用方的任意字符串拼进 SQL。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncEntityType {
    Literature,
    Publication,
    Author,
    LiteratureAuthor,
    Folder,
    LiteratureFolder,
    Tag,
    LiteratureTag,
    Attachment,
    Feed,
    FeedItem,
    Citation,
    Annotation,
    LiteratureNote,
}

impl SyncEntityType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Literature => "literatures",
            Self::Publication => "publications",
            Self::Author => "authors",
            Self::LiteratureAuthor => "literature_authors",
            Self::Folder => "folders",
            Self::LiteratureFolder => "literature_folders",
            Self::Tag => "tags",
            Self::LiteratureTag => "literature_tags",
            Self::Attachment => "attachments",
            Self::Feed => "feeds",
            Self::FeedItem => "feed_items",
            Self::Citation => "literature_citations",
            Self::Annotation => "annotations",
            Self::LiteratureNote => "literature_notes",
        }
    }

    /// 主键列顺序，关系表使用复合主键。
    pub const fn key_columns(self) -> &'static [&'static str] {
        match self {
            Self::LiteratureAuthor => &["literature_id", "author_id"],
            Self::LiteratureFolder => &["literature_id", "folder_id"],
            Self::LiteratureTag => &["literature_id", "tag_id"],
            Self::Citation => &["source_id", "target_id"],
            _ => &["id"],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteLibraryInfo {
    pub library_id: String,
    pub schema_version: i32,
    pub created_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteChange {
    pub sequence: u64,
    pub entity_type: String,
    pub entity_id: String,
    pub version: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VersionedWriteResult {
    Applied {
        sequence: u64,
        version: i32,
    },
    VersionConflict {
        current_version: Option<i32>,
        remote_record: Option<crate::RemoteRecord>,
    },
}

/// 新同步写入的受限 payload。调用方只能选择已知实体类别，SQL 列集合由
/// `entity_spec` 固定，payload 仅承载该实体的序列化业务字段。
#[derive(Clone, Debug)]
pub enum SyncEntityPayload {
    Literature(Literature),
    Publication(Publication),
    Author(Author),
    LiteratureAuthor(RelationAuthor),
    Folder(Folder),
    LiteratureFolder(RelationFolder),
    Tag(Tag),
    LiteratureTag(RelationTag),
    Attachment(Attachment),
    Feed(Feed),
    FeedItem(FeedItem),
    Citation(Citation),
    Annotation(RemoteAnnotationPayload),
    LiteratureNote(LiteratureNote),
}

/// Flat storage representation shared by the MySQL and SQLite annotation
/// schemas. It keeps rectangle coordinates nullable outside the domain model.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RemoteAnnotationPayload {
    pub id: String,
    pub document_id: String,
    pub page: u16,
    pub kind: String,
    pub color: String,
    pub range: Option<String>,
    pub note: Option<String>,
    pub rect_x: Option<f32>,
    pub rect_y: Option<f32>,
    pub rect_w: Option<f32>,
    pub rect_h: Option<f32>,
    pub is_deleted: bool,
    pub version: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

impl RemoteAnnotationPayload {
    pub fn try_into_annotation(self) -> Result<Annotation> {
        let kind = match self.kind.as_str() {
            "Highlight" => {
                if self.rect_x.is_some()
                    || self.rect_y.is_some()
                    || self.rect_w.is_some()
                    || self.rect_h.is_some()
                {
                    return Err(anyhow!(
                        "non-rectangle annotation has rectangle coordinates"
                    ));
                }
                models::AnnotationKind::Highlight
            }
            "Underline" => {
                if self.rect_x.is_some()
                    || self.rect_y.is_some()
                    || self.rect_w.is_some()
                    || self.rect_h.is_some()
                {
                    return Err(anyhow!(
                        "non-rectangle annotation has rectangle coordinates"
                    ));
                }
                models::AnnotationKind::Underline
            }
            "Rectangle" => models::AnnotationKind::Rectangle {
                x: self
                    .rect_x
                    .ok_or_else(|| anyhow!("rectangle is missing rect_x"))?,
                y: self
                    .rect_y
                    .ok_or_else(|| anyhow!("rectangle is missing rect_y"))?,
                w: self
                    .rect_w
                    .ok_or_else(|| anyhow!("rectangle is missing rect_w"))?,
                h: self
                    .rect_h
                    .ok_or_else(|| anyhow!("rectangle is missing rect_h"))?,
            },
            kind => return Err(anyhow!("unknown annotation kind: {kind}")),
        };
        let color = match self.color.as_str() {
            "Yellow" => models::AnnotationColor::Yellow,
            "Red" => models::AnnotationColor::Red,
            "Green" => models::AnnotationColor::Green,
            "Blue" => models::AnnotationColor::Blue,
            "Purple" => models::AnnotationColor::Purple,
            "Magenta" => models::AnnotationColor::Magenta,
            "Orange" => models::AnnotationColor::Orange,
            "Gray" => models::AnnotationColor::Gray,
            color => return Err(anyhow!("unknown annotation color: {color}")),
        };
        let range = self
            .range
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|error| anyhow!("invalid annotation range: {error}"))?;
        Ok(Annotation {
            id: self.id,
            document_id: self.document_id,
            page: self.page,
            kind,
            color,
            range,
            note: self.note,
            created_at: self.created_at,
            updated_at: self.updated_at,
            version: self.version,
            is_deleted: self.is_deleted,
            is_dirty: false,
        })
    }
}

impl From<Annotation> for RemoteAnnotationPayload {
    fn from(annotation: Annotation) -> Self {
        Self::from(&annotation)
    }
}

impl From<&Annotation> for RemoteAnnotationPayload {
    fn from(annotation: &Annotation) -> Self {
        let (kind, rect_x, rect_y, rect_w, rect_h) = match annotation.kind {
            models::AnnotationKind::Highlight => ("Highlight", None, None, None, None),
            models::AnnotationKind::Underline => ("Underline", None, None, None, None),
            models::AnnotationKind::Rectangle { x, y, w, h } => {
                ("Rectangle", Some(x), Some(y), Some(w), Some(h))
            }
        };
        Self {
            id: annotation.id.clone(),
            document_id: annotation.document_id.clone(),
            page: annotation.page,
            kind: kind.to_string(),
            color: format!("{:?}", annotation.color),
            range: annotation
                .range
                .as_ref()
                .and_then(|v| serde_json::to_string(v).ok()),
            note: annotation.note.clone(),
            rect_x,
            rect_y,
            rect_w,
            rect_h,
            is_deleted: annotation.is_deleted,
            version: annotation.version,
            created_at: annotation.created_at,
            updated_at: annotation.updated_at,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RelationAuthor {
    pub literature_id: String,
    pub author_id: String,
    pub sort_order: i32,
    pub is_deleted: bool,
    pub version: i32,
    pub updated_at: i64,
}
#[derive(Clone, Debug, Serialize)]
pub struct RelationFolder {
    pub literature_id: String,
    pub folder_id: String,
    pub is_deleted: bool,
    pub version: i32,
    pub updated_at: i64,
}
#[derive(Clone, Debug, Serialize)]
pub struct RelationTag {
    pub literature_id: String,
    pub tag_id: String,
    pub is_deleted: bool,
    pub version: i32,
    pub updated_at: i64,
}

impl SyncEntityPayload {
    pub fn entity_type(&self) -> SyncEntityType {
        match self {
            Self::Literature(_) => SyncEntityType::Literature,
            Self::Publication(_) => SyncEntityType::Publication,
            Self::Author(_) => SyncEntityType::Author,
            Self::LiteratureAuthor(_) => SyncEntityType::LiteratureAuthor,
            Self::Folder(_) => SyncEntityType::Folder,
            Self::LiteratureFolder(_) => SyncEntityType::LiteratureFolder,
            Self::Tag(_) => SyncEntityType::Tag,
            Self::LiteratureTag(_) => SyncEntityType::LiteratureTag,
            Self::Attachment(_) => SyncEntityType::Attachment,
            Self::Feed(_) => SyncEntityType::Feed,
            Self::FeedItem(_) => SyncEntityType::FeedItem,
            Self::Citation(_) => SyncEntityType::Citation,
            Self::Annotation(_) => SyncEntityType::Annotation,
            Self::LiteratureNote(_) => SyncEntityType::LiteratureNote,
        }
    }

    pub fn value(&self) -> JsonValue {
        match self {
            Self::Literature(v) => serde_json::to_value(v),
            Self::Publication(v) => serde_json::to_value(v),
            Self::Author(v) => serde_json::to_value(v),
            Self::Folder(v) => serde_json::to_value(v),
            Self::Tag(v) => serde_json::to_value(v),
            Self::Attachment(v) => serde_json::to_value(v),
            Self::Feed(v) => serde_json::to_value(v),
            Self::FeedItem(v) => serde_json::to_value(v),
            Self::Citation(v) => serde_json::to_value(v),
            Self::Annotation(v) => serde_json::to_value(v),
            Self::LiteratureNote(v) => serde_json::to_value(v),
            Self::LiteratureAuthor(v) => serde_json::to_value(v),
            Self::LiteratureFolder(v) => serde_json::to_value(v),
            Self::LiteratureTag(v) => serde_json::to_value(v),
        }
        .expect("database sync payload models are serializable")
    }

    pub fn canonical_id(&self) -> String {
        let v = self.value();
        let entity = self.entity_type();
        let key = if entity.key_columns().len() == 1 {
            crate::SyncEntityKey::Id(
                v[entity.key_columns()[0]]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
            )
        } else {
            crate::SyncEntityKey::Relation {
                left: v[entity.key_columns()[0]]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
                right: v[entity.key_columns()[1]]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
            }
        };
        crate::canonical_key(&key)
    }
}

fn entity_spec(
    entity: SyncEntityType,
) -> (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
) {
    match entity {
        SyncEntityType::Literature => (
            "literatures",
            &["id"],
            &[
                "id",
                "title",
                "year",
                "month",
                "day",
                "type",
                "publication_id",
                "volume",
                "issue",
                "pages",
                "abstract_text",
                "doi",
                "arxiv_id",
                "url",
                "rating",
                "reading_status",
                "is_deleted",
                "version",
                "created_at",
                "updated_at",
            ],
        ),
        SyncEntityType::Publication => (
            "publications",
            &["id"],
            &[
                "id",
                "name",
                "publication_type",
                "abbreviation",
                "publisher",
                "ccf_rank",
                "jcr_rank",
                "cas_rank",
                "is_deleted",
                "version",
                "created_at",
                "updated_at",
            ],
        ),
        SyncEntityType::Author => (
            "authors",
            &["id"],
            &[
                "id",
                "first_name",
                "last_name",
                "middle_name",
                "is_deleted",
                "version",
                "created_at",
                "updated_at",
            ],
        ),
        SyncEntityType::LiteratureAuthor => (
            "literature_authors",
            &["literature_id", "author_id"],
            &[
                "literature_id",
                "author_id",
                "sort_order",
                "is_deleted",
                "version",
                "updated_at",
            ],
        ),
        SyncEntityType::Folder => (
            "folders",
            &["id"],
            &[
                "id",
                "name",
                "folder_type",
                "parent_id",
                "is_deleted",
                "version",
                "created_at",
                "updated_at",
            ],
        ),
        SyncEntityType::LiteratureFolder => (
            "literature_folders",
            &["literature_id", "folder_id"],
            &[
                "literature_id",
                "folder_id",
                "is_deleted",
                "version",
                "updated_at",
            ],
        ),
        SyncEntityType::Tag => (
            "tags",
            &["id"],
            &[
                "id",
                "name",
                "color",
                "is_deleted",
                "version",
                "created_at",
                "updated_at",
            ],
        ),
        SyncEntityType::LiteratureTag => (
            "literature_tags",
            &["literature_id", "tag_id"],
            &[
                "literature_id",
                "tag_id",
                "is_deleted",
                "version",
                "updated_at",
            ],
        ),
        SyncEntityType::Attachment => (
            "attachments",
            &["id"],
            &[
                "id",
                "literature_id",
                "file_path",
                "file_name",
                "file_size",
                "mime_type",
                "etag",
                "hash",
                "is_main",
                "is_deleted",
                "version",
                "created_at",
                "updated_at",
            ],
        ),
        SyncEntityType::Feed => (
            "feeds",
            &["id"],
            &[
                "id",
                "name",
                "title",
                "feed_type",
                "url",
                "last_updated_at",
                "update_interval",
                "is_deleted",
                "version",
                "created_at",
                "updated_at",
            ],
        ),
        SyncEntityType::FeedItem => (
            "feed_items",
            &["id"],
            &[
                "id",
                "title",
                "feed_id",
                "is_read",
                "is_added_to_library",
                "added_at",
                "authors",
                "year",
                "type",
                "journal",
                "publisher",
                "abstract_text",
                "doi",
                "url",
                "volume",
                "issue",
                "pages",
                "published_at",
                "is_deleted",
                "version",
                "updated_at",
            ],
        ),
        SyncEntityType::Citation => (
            "literature_citations",
            &["source_id", "target_id"],
            &[
                "source_id",
                "target_id",
                "is_deleted",
                "version",
                "updated_at",
            ],
        ),
        SyncEntityType::Annotation => (
            "annotations",
            &["id"],
            &[
                "id",
                "document_id",
                "page",
                "kind",
                "color",
                "range",
                "note",
                "rect_x",
                "rect_y",
                "rect_w",
                "rect_h",
                "is_deleted",
                "version",
                "created_at",
                "updated_at",
            ],
        ),
        SyncEntityType::LiteratureNote => (
            "literature_notes",
            &["id"],
            &[
                "id",
                "literature_id",
                "title",
                "content",
                "sort_order",
                "created_at",
                "updated_at",
                "is_deleted",
                "version",
            ],
        ),
    }
}

impl MySqlManager {
    /// 读取远端完整快照。表与列集合来自受限 entity_spec，不暴露 SQL 给上层。
    pub async fn read_sync_snapshot(&self) -> Result<crate::RemoteReadBatch> {
        self.read_sync_snapshot_with_context(None).await
    }

    pub async fn read_sync_snapshot_with_context(
        &self,
        run_id: Option<&str>,
    ) -> Result<crate::RemoteReadBatch> {
        let pool = self.get_pool().await?;
        let mut conn = pool.get_conn().await?;
        // The snapshot position and every entity table are read inside the same
        // REPEATABLE READ consistent snapshot. This prevents advancing past rows
        // that become visible between the position query and table reads.
        trace_mysql_operation(run_id, "snapshot_set_isolation", || async {
            conn.query_drop("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
                .await
                .map_err(Into::into)
        })
        .await?;
        trace_mysql_operation(run_id, "snapshot_start_snapshot", || async {
            conn.query_drop("START TRANSACTION WITH CONSISTENT SNAPSHOT")
                .await
                .map_err(Into::into)
        })
        .await?;
        let result: Result<crate::RemoteReadBatch> = async {
            let latest: u64 = trace_mysql_operation(run_id, "snapshot_read_sequence", || async {
                conn.query_first("SELECT COALESCE(MAX(sequence), 0) FROM sync_changes")
                    .await
                    .map(|value| value.unwrap_or(0))
                    .map_err(Into::into)
            })
            .await?;
            let mut records = Vec::new();
            for entity in all_sync_entities() {
                let operation = format!("snapshot_read_entity_rows_{}", entity.as_str());
                records.extend(
                    trace_mysql_operation(run_id, &operation, || async {
                        read_entity_rows(&mut conn, entity, None).await
                    })
                    .await?,
                );
            }
            Ok(crate::RemoteReadBatch {
                records,
                last_sequence: latest as i64,
            })
        }
        .await;
        if result.is_err() {
            // Preserve the original read error even if rollback itself fails.
            let _ = trace_mysql_operation(run_id, "snapshot_rollback", || async {
                conn.query_drop("ROLLBACK").await.map_err(Into::into)
            })
            .await;
        } else {
            trace_mysql_operation(run_id, "snapshot_commit", || async {
                conn.query_drop("COMMIT").await.map_err(Into::into)
            })
            .await?;
        }
        result
    }

    /// 按变化清单 sequence 读取远端记录，不使用时间戳。
    pub async fn read_sync_changes_after(
        &self,
        sequence: u64,
        limit: u32,
    ) -> Result<crate::RemoteReadBatch> {
        self.read_sync_changes_after_with_context(sequence, limit, None)
            .await
    }

    pub async fn read_sync_changes_after_with_context(
        &self,
        sequence: u64,
        limit: u32,
        run_id: Option<&str>,
    ) -> Result<crate::RemoteReadBatch> {
        let operation_started = Instant::now();
        let pool = self.get_pool().await?;
        let mut conn = pool.get_conn().await?;
        trace_mysql_operation(run_id, "incremental_set_isolation", || async {
            conn.query_drop("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
                .await
                .map_err(Into::into)
        })
        .await?;
        trace_mysql_operation(run_id, "incremental_start_snapshot", || async {
            conn.query_drop("START TRANSACTION WITH CONSISTENT SNAPSHOT")
                .await
                .map_err(Into::into)
        })
        .await?;
        let result: Result<crate::RemoteReadBatch> = async {
            let changes: Vec<RemoteChange> = trace_mysql_operation(run_id, "incremental_read_changes", || async {
                conn.exec_map("SELECT sequence, entity_type, entity_id, version FROM sync_changes WHERE sequence > :sequence ORDER BY sequence LIMIT :limit", mysql_async::params! { "sequence" => sequence, "limit" => limit }, |(sequence, entity_type, entity_id, version)| RemoteChange { sequence, entity_type, entity_id, version }).await.map_err(Into::into)
            }).await?;
            let change_count = changes.len();
            let folded = fold_changes(changes, sequence);
            if let Some(run_id) = run_id {
                log::info!(
                    "[Sync][run={run_id}][stage=mysql] event=progress elapsed_ms={} operation=incremental_read_changes changes={} folded={} to_sequence={}",
                    operation_started.elapsed().as_millis(),
                    change_count,
                    folded.0.len(),
                    folded.1
                );
            }
            let last_sequence = folded.1;
            let mut records = Vec::new();
            for change in folded.0 {
                let entity = all_sync_entities().into_iter().find(|e| e.as_str() == change.entity_type).ok_or_else(|| anyhow!("unknown remote entity type"))?;
                let operation = format!("incremental_read_entity_rows_{}", entity.as_str());
                let rows = trace_mysql_operation(run_id, &operation, || async {
                    read_entity_rows(&mut conn, entity, Some(&change.entity_id)).await
                }).await?;
                let row = rows.into_iter().next().ok_or_else(|| anyhow!("remote entity disappeared"))?;
                if row.version != i64::from(change.version) { return Err(anyhow!("remote entity version changed during download")); }
                records.push(row);
            }
            Ok(crate::RemoteReadBatch { records, last_sequence: last_sequence as i64 })
        }.await;
        if result.is_err() {
            let _ = trace_mysql_operation(run_id, "incremental_rollback", || async {
                conn.query_drop("ROLLBACK").await.map_err(Into::into)
            })
            .await;
        } else {
            trace_mysql_operation(run_id, "incremental_commit", || async {
                conn.query_drop("COMMIT").await.map_err(Into::into)
            })
            .await?;
        }
        result
    }

    /// 安全写入一个完整实体：版本检查、业务列写入和变化清单写入共享一个事务。
    pub async fn write_versioned_entity(
        &self,
        payload: &SyncEntityPayload,
        expected_version: i32,
    ) -> Result<VersionedWriteResult> {
        self.write_versioned_entity_with_context(payload, expected_version, None)
            .await
    }

    pub async fn write_versioned_entity_with_context(
        &self,
        payload: &SyncEntityPayload,
        expected_version: i32,
        run_id: Option<&str>,
    ) -> Result<VersionedWriteResult> {
        if expected_version < 0 {
            return Err(anyhow!("expected version cannot be negative"));
        }
        let entity = payload.entity_type();
        let (table, keys, columns) = entity_spec(entity);
        let value = normalized_payload(payload);
        validate_fields(&value, columns)?;
        let new_version = next_remote_version(expected_version)?;
        let entity_id = payload.canonical_id();
        let _payload_version = value
            .get("version")
            .and_then(JsonValue::as_i64)
            .ok_or_else(|| anyhow!("sync payload is missing integer version"))?;
        let payload_param = || typed_params(&value, columns, new_version);
        let where_clause = keys
            .iter()
            .map(|key| format!("`{key}` = :{key}"))
            .collect::<Vec<_>>()
            .join(" AND ");
        let pool = self.get_pool().await?;
        let mut conn = pool.get_conn().await?;
        let mut tx = trace_mysql_operation(run_id, "upload_start_transaction", || async {
            conn.start_transaction(TxOpts::default())
                .await
                .map_err(Into::into)
        })
        .await?;
        let current: Option<i32> =
            trace_mysql_operation(run_id, "upload_version_check", || async {
                tx.exec_first(
                    format!("SELECT `version` FROM `{table}` WHERE {where_clause} FOR UPDATE"),
                    payload_param(),
                )
                .await
                .map_err(Into::into)
            })
            .await?;
        if current.is_some() && current != Some(expected_version) {
            tx.rollback().await?;
            let remote_record = read_entity_rows(&mut conn, entity, Some(&entity_id))
                .await?
                .into_iter()
                .next();
            return Ok(VersionedWriteResult::VersionConflict {
                current_version: current,
                remote_record,
            });
        }
        if current.is_none() && expected_version != 0 {
            tx.rollback().await?;
            return Ok(VersionedWriteResult::VersionConflict {
                current_version: None,
                remote_record: None,
            });
        }
        let values = columns
            .iter()
            .map(|column| format!(":{column}"))
            .collect::<Vec<_>>();
        if current.is_none() {
            trace_mysql_operation(run_id, "upload_business_row_write", || async {
                tx.exec_drop(
                    format!(
                        "INSERT INTO `{table}` ({}) VALUES ({})",
                        columns
                            .iter()
                            .map(|c| format!("`{c}`"))
                            .collect::<Vec<_>>()
                            .join(", "),
                        values.join(", ")
                    ),
                    payload_param(),
                )
                .await
                .map_err(Into::into)
            })
            .await?;
        } else {
            let assignments = columns
                .iter()
                .filter(|column| !keys.contains(column))
                .map(|column| format!("`{column}` = :{column}"))
                .collect::<Vec<_>>();
            trace_mysql_operation(run_id, "upload_business_row_write", || async {
                tx.exec_drop(
                    format!(
                        "UPDATE `{table}` SET {} WHERE {where_clause}",
                        assignments.join(", ")
                    ),
                    payload_param(),
                )
                .await
                .map_err(Into::into)
            })
            .await?;
        }
        trace_mysql_operation(run_id, "upload_change_log_write", || async {
            tx.exec_drop(
                "INSERT INTO sync_changes (entity_type, entity_id, version)
             VALUES (:entity_type, :entity_id, :version)",
                mysql_async::params! {
                    "entity_type" => entity.as_str(),
                    "entity_id" => entity_id,
                    "version" => new_version,
                },
            )
            .await
            .map_err(Into::into)
        })
        .await?;
        let sequence = tx.last_insert_id().unwrap_or_default();
        trace_mysql_operation(run_id, "upload_commit", || async {
            tx.commit().await.map_err(Into::into)
        })
        .await?;
        Ok(VersionedWriteResult::Applied {
            sequence,
            version: new_version,
        })
    }

    pub async fn get_library_info(&self) -> Result<Option<RemoteLibraryInfo>> {
        self.get_library_info_with_context(None).await
    }

    pub async fn get_library_info_with_context(
        &self,
        run_id: Option<&str>,
    ) -> Result<Option<RemoteLibraryInfo>> {
        let pool = self.get_pool().await?;
        let mut conn = pool.get_conn().await?;
        let row: Option<(String, i32, i64)> = trace_mysql_operation(run_id, "get_library_info", || async {
            conn.exec_first(
                "SELECT library_id, schema_version, created_at FROM library_info WHERE singleton = 1",
                (),
            )
            .await
            .map_err(Into::into)
        }).await?;
        Ok(row.map(
            |(library_id, schema_version, created_at)| RemoteLibraryInfo {
                library_id,
                schema_version,
                created_at,
            },
        ))
    }

    pub async fn initialize_library_info(&self, info: &RemoteLibraryInfo) -> Result<()> {
        let pool = self.get_pool().await?;
        let mut conn = pool.get_conn().await?;
        let existing: Option<String> = conn
            .exec_first(
                "SELECT library_id FROM library_info WHERE singleton = 1",
                (),
            )
            .await?;
        if let Some(existing) = existing {
            if existing != info.library_id {
                return Err(anyhow!("remote library identity mismatch"));
            }
            return Ok(());
        }
        conn.exec_drop(
            "INSERT INTO library_info (singleton, library_id, schema_version, created_at)
             VALUES (1, :id, :schema, :created)",
            mysql_async::params! {
                "id" => &info.library_id,
                "schema" => info.schema_version,
                "created" => info.created_at,
            },
        )
        .await?;
        Ok(())
    }

    pub async fn list_changes_after(&self, sequence: u64, limit: u32) -> Result<Vec<RemoteChange>> {
        let pool = self.get_pool().await?;
        let mut conn = pool.get_conn().await?;
        conn.exec_map(
            "SELECT sequence, entity_type, entity_id, version
             FROM sync_changes WHERE sequence > :sequence
             ORDER BY sequence LIMIT :limit",
            mysql_async::params! { "sequence" => sequence, "limit" => limit },
            |(sequence, entity_type, entity_id, version)| RemoteChange {
                sequence,
                entity_type,
                entity_id,
                version,
            },
        )
        .await
        .map_err(Into::into)
    }
}

fn all_sync_entities() -> [SyncEntityType; 14] {
    [
        SyncEntityType::Literature,
        SyncEntityType::Publication,
        SyncEntityType::Author,
        SyncEntityType::LiteratureAuthor,
        SyncEntityType::Folder,
        SyncEntityType::LiteratureFolder,
        SyncEntityType::Tag,
        SyncEntityType::LiteratureTag,
        SyncEntityType::Attachment,
        SyncEntityType::Feed,
        SyncEntityType::FeedItem,
        SyncEntityType::Citation,
        SyncEntityType::Annotation,
        SyncEntityType::LiteratureNote,
    ]
}

fn fold_changes(changes: Vec<RemoteChange>, input: u64) -> (Vec<RemoteChange>, u64) {
    let last_sequence = changes.last().map(|c| c.sequence).unwrap_or(input);
    let mut folded = std::collections::HashMap::<(String, String), RemoteChange>::new();
    for change in changes {
        folded.insert(
            (change.entity_type.clone(), change.entity_id.clone()),
            change,
        );
    }
    let mut values = folded.into_values().collect::<Vec<_>>();
    values.sort_by_key(|c| c.sequence);
    (values, last_sequence)
}

async fn read_entity_rows(
    conn: &mut mysql_async::Conn,
    entity: SyncEntityType,
    canonical: Option<&str>,
) -> Result<Vec<crate::RemoteRecord>> {
    let (table, keys, columns) = entity_spec(entity);
    let select = columns
        .iter()
        .map(|c| format!("`{c}`"))
        .collect::<Vec<_>>()
        .join(",");
    let (where_sql, params) = if let Some(canonical) = canonical {
        let values = if keys.len() == 1 {
            vec![canonical.to_string()]
        } else {
            let (left, right) = crate::decode_relation_key(canonical)
                .map_err(|error| anyhow!("invalid remote relation key: {error}"))?;
            vec![left, right]
        };
        let mut named = std::collections::HashMap::new();
        let clauses = keys
            .iter()
            .map(|key| format!("`{key}` = :{key}"))
            .collect::<Vec<_>>();
        for (key, value) in keys.iter().zip(values) {
            named.insert(
                (*key).as_bytes().to_vec(),
                MySqlValue::Bytes(value.into_bytes()),
            );
        }
        (
            format!(" WHERE {}", clauses.join(" AND ")),
            mysql_async::Params::Named(named),
        )
    } else {
        (String::new(), mysql_async::Params::Empty)
    };
    let rows = conn
        .exec::<mysql_async::Row, _, _>(
            format!("SELECT {select} FROM `{table}`{where_sql}"),
            params,
        )
        .await?;
    let mut records = Vec::new();
    for (batch_index, row) in rows.into_iter().enumerate() {
        let mut payload = serde_json::Map::new();
        for (index, column) in columns.iter().enumerate() {
            let value = row
                .get::<MySqlValue, _>(index)
                .ok_or_else(|| anyhow!("missing remote column {column}"))?;
            payload.insert((*column).to_string(), mysql_json(value));
        }
        let entity_type = local_entity_type(entity);
        let canonical_payload = if entity_type == crate::SyncEntityType::Annotation {
            let (_, canon_json) = crate::sync_download::normalize_annotation_payload(
                &JsonValue::Object(payload),
                batch_index,
            )?;
            canon_json
        } else {
            JsonValue::Object(payload)
        };
        records.push(crate::RemoteRecord {
            entity_type,
            version: canonical_payload
                .get("version")
                .and_then(JsonValue::as_i64)
                .unwrap_or_default(),
            payload: canonical_payload,
        });
    }
    Ok(records)
}

fn local_entity_type(entity: SyncEntityType) -> crate::SyncEntityType {
    match entity {
        SyncEntityType::Literature => crate::SyncEntityType::Literature,
        SyncEntityType::Publication => crate::SyncEntityType::Publication,
        SyncEntityType::Author => crate::SyncEntityType::Author,
        SyncEntityType::LiteratureAuthor => crate::SyncEntityType::LiteratureAuthor,
        SyncEntityType::Folder => crate::SyncEntityType::Folder,
        SyncEntityType::LiteratureFolder => crate::SyncEntityType::LiteratureFolder,
        SyncEntityType::Tag => crate::SyncEntityType::Tag,
        SyncEntityType::LiteratureTag => crate::SyncEntityType::LiteratureTag,
        SyncEntityType::Attachment => crate::SyncEntityType::Attachment,
        SyncEntityType::Feed => crate::SyncEntityType::Feed,
        SyncEntityType::FeedItem => crate::SyncEntityType::FeedItem,
        SyncEntityType::Citation => crate::SyncEntityType::Citation,
        SyncEntityType::Annotation => crate::SyncEntityType::Annotation,
        SyncEntityType::LiteratureNote => crate::SyncEntityType::LiteratureNote,
    }
}

fn mysql_json(value: MySqlValue) -> JsonValue {
    match value {
        MySqlValue::NULL => JsonValue::Null,
        MySqlValue::Int(v) => JsonValue::from(v),
        MySqlValue::UInt(v) => JsonValue::from(v),
        MySqlValue::Float(v) => JsonValue::from(v),
        MySqlValue::Double(v) => JsonValue::from(v),
        MySqlValue::Bytes(v) => String::from_utf8(v)
            .map(JsonValue::String)
            .unwrap_or(JsonValue::Null),
        MySqlValue::Date(y, m, d, h, min, s, mic) => JsonValue::String(format!(
            "{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{s:02}.{mic:06}"
        )),
        MySqlValue::Time(_, d, h, m, s, mic) => {
            JsonValue::String(format!("{d} {h:02}:{m:02}:{s:02}.{mic:06}"))
        }
    }
}

fn normalized_payload(payload: &SyncEntityPayload) -> JsonValue {
    normalized_fields(payload.entity_type(), payload.value())
}

fn normalized_fields(entity: SyncEntityType, mut value: JsonValue) -> JsonValue {
    if let JsonValue::Object(ref mut fields) = value {
        let rename = match entity {
            SyncEntityType::Literature | SyncEntityType::FeedItem => Some("literature_type"),
            _ => None,
        };
        let target = if matches!(
            entity,
            SyncEntityType::Literature | SyncEntityType::FeedItem
        ) {
            "type"
        } else {
            ""
        };
        if let Some(source) = rename {
            if let Some(item) = fields.remove(source) {
                fields.insert(target.to_string(), item);
            }
        }
        if matches!(entity, SyncEntityType::Literature) {
            let publication_id = fields
                .remove("publication")
                .and_then(|publication| publication.get("id").cloned())
                .unwrap_or(JsonValue::Null);
            fields.insert("publication_id".to_string(), publication_id);
        }
    }
    value
}

fn next_remote_version(expected_version: i32) -> Result<i32> {
    expected_version
        .checked_add(1)
        .ok_or_else(|| anyhow!("remote version overflow"))
}

fn validate_fields(value: &JsonValue, columns: &[&str]) -> Result<()> {
    for column in columns {
        if value.get(*column).is_none() {
            return Err(anyhow!("sync payload is missing required field: {column}"));
        }
    }
    if columns.contains(&"rect_x") {
        let kind = value["kind"]
            .as_str()
            .ok_or_else(|| anyhow!("annotation kind must be a string"))?;
        match kind {
            "Highlight" | "Underline" => {
                if ["rect_x", "rect_y", "rect_w", "rect_h"]
                    .iter()
                    .any(|field| !value[*field].is_null())
                {
                    return Err(anyhow!(
                        "non-rectangle annotation has rectangle coordinates"
                    ));
                }
            }
            "Rectangle" => {
                if ["rect_x", "rect_y", "rect_w", "rect_h"]
                    .iter()
                    .any(|field| !value[*field].is_number())
                {
                    return Err(anyhow!("rectangle annotation is missing coordinates"));
                }
            }
            _ => return Err(anyhow!("unknown annotation kind: {kind}")),
        }
    }
    Ok(())
}

fn mysql_value(value: Option<&JsonValue>) -> MySqlValue {
    match value {
        None | Some(JsonValue::Null) => MySqlValue::NULL,
        Some(JsonValue::Bool(value)) => MySqlValue::Int(i64::from(*value)),
        Some(JsonValue::Number(value)) if value.as_i64().is_some() => {
            MySqlValue::Int(value.as_i64().unwrap())
        }
        Some(JsonValue::Number(value)) if value.as_u64().is_some() => {
            MySqlValue::UInt(value.as_u64().unwrap())
        }
        Some(JsonValue::Number(value)) => MySqlValue::Double(value.as_f64().unwrap()),
        Some(JsonValue::String(value)) => MySqlValue::Bytes(value.as_bytes().to_vec()),
        Some(value) => MySqlValue::Bytes(value.to_string().into_bytes()),
    }
}

fn typed_params(value: &JsonValue, columns: &[&str], version: i32) -> Params {
    let mut params = std::collections::HashMap::new();
    for column in columns {
        params.insert(
            (*column).as_bytes().to_vec(),
            if *column == "version" {
                MySqlValue::Int(i64::from(version))
            } else {
                mysql_value(value.get(*column))
            },
        );
    }
    Params::Named(params)
}

#[cfg(test)]
mod tests {
    use super::{
        MYSQL_SYNC_OPERATIONS, MySqlValue, RemoteAnnotationPayload, RemoteChange, SyncEntityType,
        entity_spec, fold_changes, mysql_error_category, mysql_value, next_remote_version,
        normalized_fields, validate_fields,
    };
    use models::{Annotation, AnnotationColor, AnnotationKind};
    use serde_json::Value as JsonValue;

    #[test]
    fn mysql_operation_context_is_closed_and_errors_are_classified_without_text() {
        assert!(MYSQL_SYNC_OPERATIONS.contains(&"incremental_read_changes"));
        assert!(MYSQL_SYNC_OPERATIONS.contains(&"incremental_read_entity_rows"));
        assert_eq!(
            mysql_error_category(&anyhow::anyhow!("Query is not read-only for secret-entity")),
            "read_only"
        );
    }

    #[test]
    fn all_entity_types_have_restricted_table_and_key_mapping() {
        let entities = [
            SyncEntityType::Literature,
            SyncEntityType::Publication,
            SyncEntityType::Author,
            SyncEntityType::LiteratureAuthor,
            SyncEntityType::Folder,
            SyncEntityType::LiteratureFolder,
            SyncEntityType::Tag,
            SyncEntityType::LiteratureTag,
            SyncEntityType::Attachment,
            SyncEntityType::Feed,
            SyncEntityType::FeedItem,
            SyncEntityType::Citation,
            SyncEntityType::Annotation,
            SyncEntityType::LiteratureNote,
        ];
        assert_eq!(entities.len(), 14);
        assert!(entities.iter().all(|entity| !entity.as_str().is_empty()));
        assert_eq!(
            SyncEntityType::Citation.key_columns(),
            &["source_id", "target_id"]
        );
    }

    #[test]
    fn composite_keys_are_explicit() {
        assert_eq!(
            SyncEntityType::Citation.key_columns(),
            &["source_id", "target_id"]
        );
        assert_eq!(
            SyncEntityType::LiteratureTag.key_columns(),
            &["literature_id", "tag_id"]
        );
    }

    #[test]
    fn remote_version_is_derived_from_expected_version() {
        assert_eq!(next_remote_version(0).unwrap(), 1);
        assert_eq!(next_remote_version(7).unwrap(), 8);
        assert!(next_remote_version(i32::MAX).is_err());
    }

    #[test]
    fn typed_values_preserve_null_boolean_number_and_string() {
        assert_eq!(mysql_value(Some(&JsonValue::Null)), MySqlValue::NULL);
        assert_eq!(
            mysql_value(Some(&JsonValue::Bool(true))),
            MySqlValue::Int(1)
        );
        assert_eq!(mysql_value(Some(&JsonValue::from(7))), MySqlValue::Int(7));
        assert_eq!(
            mysql_value(Some(&JsonValue::from("null"))),
            MySqlValue::Bytes(b"null".to_vec())
        );
    }

    #[test]
    fn annotation_range_is_a_quoted_fixed_column() {
        let (_, _, columns) = entity_spec(SyncEntityType::Annotation);
        assert!(columns.contains(&"range"));
        let sql = format!("`{}`", columns.iter().find(|c| **c == "range").unwrap());
        assert_eq!(sql, "`range`");
    }

    #[test]
    fn every_entity_spec_has_all_required_normalized_fields() {
        let entities = [
            SyncEntityType::Literature,
            SyncEntityType::Publication,
            SyncEntityType::Author,
            SyncEntityType::LiteratureAuthor,
            SyncEntityType::Folder,
            SyncEntityType::LiteratureFolder,
            SyncEntityType::Tag,
            SyncEntityType::LiteratureTag,
            SyncEntityType::Attachment,
            SyncEntityType::Feed,
            SyncEntityType::FeedItem,
            SyncEntityType::Citation,
            SyncEntityType::Annotation,
            SyncEntityType::LiteratureNote,
        ];
        for entity in entities {
            let (_, _, columns) = entity_spec(entity);
            let mut object = serde_json::Map::new();
            for column in columns {
                object.insert((*column).to_string(), JsonValue::Null);
            }
            if matches!(
                entity,
                SyncEntityType::Literature | SyncEntityType::FeedItem
            ) {
                object.remove("type");
                object.insert(
                    "literature_type".into(),
                    JsonValue::String("article".into()),
                );
            }
            if entity == SyncEntityType::Annotation {
                object.insert("kind".into(), JsonValue::String("Highlight".into()));
            }
            let normalized = normalized_fields(entity, JsonValue::Object(object));
            validate_fields(&normalized, columns).unwrap();
        }
    }

    #[test]
    fn literature_publication_is_normalized_to_id_or_null() {
        let mut with_publication =
            serde_json::json!({"literature_type":"article", "publication":{"id":"pub-1"}});
        with_publication
            .as_object_mut()
            .unwrap()
            .insert("version".into(), 1.into());
        let normalized = normalized_fields(SyncEntityType::Literature, with_publication);
        assert_eq!(normalized["publication_id"], "pub-1");
        assert_eq!(normalized["type"], "article");
        let without_publication = normalized_fields(
            SyncEntityType::Literature,
            serde_json::json!({"literature_type":"article", "publication":null}),
        );
        assert_eq!(without_publication["publication_id"], JsonValue::Null);
    }

    #[test]
    fn change_folding_keeps_latest_per_entity_and_original_position() {
        let changes = vec![
            RemoteChange {
                sequence: 4,
                entity_type: "tags".into(),
                entity_id: "id=x".into(),
                version: 1,
            },
            RemoteChange {
                sequence: 7,
                entity_type: "tags".into(),
                entity_id: "id=x".into(),
                version: 2,
            },
            RemoteChange {
                sequence: 9,
                entity_type: "folders".into(),
                entity_id: "id=f".into(),
                version: 1,
            },
        ];
        let (folded, position) = fold_changes(changes, 3);
        assert_eq!(position, 9);
        assert_eq!(folded.len(), 2);
        assert_eq!(
            folded
                .iter()
                .find(|c| c.entity_type == "tags")
                .unwrap()
                .version,
            2
        );
    }

    #[test]
    fn empty_change_folding_preserves_input_position() {
        assert_eq!(fold_changes(Vec::new(), 11).1, 11);
    }

    #[test]
    fn annotation_dto_is_flat_and_preserves_rectangle_nullability() {
        let annotation = Annotation {
            id: "a".into(),
            document_id: "d".into(),
            page: 2,
            kind: AnnotationKind::Rectangle {
                x: 1.25,
                y: 2.5,
                w: 3.75,
                h: 4.0,
            },
            color: AnnotationColor::Blue,
            range: None,
            note: None,
            created_at: 1,
            updated_at: 2,
            version: 3,
            is_deleted: false,
            is_dirty: true,
        };
        let dto = RemoteAnnotationPayload::from(&annotation);
        let value = serde_json::to_value(dto).unwrap();
        assert_eq!(value["kind"], "Rectangle");
        assert_eq!(value["rect_x"], 1.25);
        assert!(value["range"].is_null());
        validate_fields(&value, entity_spec(SyncEntityType::Annotation).2).unwrap();
    }
}
