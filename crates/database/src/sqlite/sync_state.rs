use rusqlite::{OptionalExtension, Result, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::Database;

const LIBRARY_ID_KEY: &str = "database_sync_library_id";
const LAST_SEQUENCE_KEY: &str = "database_sync_last_sequence";
const REMOTE_FINGERPRINT_KEY: &str = "database_sync_remote_fingerprint";
const LAST_SUMMARY_KEY: &str = "database_sync_last_summary";

/// 本地资料库级别的同步位置。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalSyncState {
    pub library_id: Option<String>,
    pub last_sequence: i64,
    pub remote_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseSyncSummary {
    pub uploaded: usize,
    pub downloaded: usize,
    pub conflicts: usize,
    pub failures: usize,
    pub complete: bool,
    pub updated_at: i64,
    pub identity_error: Option<String>,
}

/// 持久化的整条记录冲突。记录内容在数据库层保持 JSON 文本，业务层负责反序列化和裁决。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncConflict {
    pub entity_type: String,
    pub entity_id: String,
    pub local_record: String,
    pub remote_record: String,
    pub remote_version: i64,
    pub detected_at: i64,
}

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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncEntityKey {
    Id(String),
    Relation { left: String, right: String },
}

/// Canonical persisted key. Relation IDs are length-prefixed UTF-8 values.
pub fn canonical_key(key: &SyncEntityKey) -> String {
    match key {
        SyncEntityKey::Id(id) => id.clone(),
        SyncEntityKey::Relation { left, right } => {
            format!("r1:{}:{}{}:{}", left.len(), left, right.len(), right)
        }
    }
}

pub fn decode_relation_key(value: &str) -> std::result::Result<(String, String), String> {
    let rest = value
        .strip_prefix("r1:")
        .ok_or_else(|| "unsupported relation key version".to_string())?;
    let (left_len, rest) = rest
        .split_once(':')
        .ok_or_else(|| "missing relation key length".to_string())?;
    let left_len = left_len
        .parse::<usize>()
        .map_err(|_| "invalid relation key length".to_string())?;
    if rest.len() < left_len || !rest.is_char_boundary(left_len) {
        return Err("truncated relation key".to_string());
    }
    let left = &rest[..left_len];
    let rest = &rest[left_len..];
    let (right_len, right) = rest
        .split_once(':')
        .ok_or_else(|| "missing relation key value".to_string())?;
    let right_len = right_len
        .parse::<usize>()
        .map_err(|_| "invalid relation key length".to_string())?;
    if right.len() != right_len || !right.is_char_boundary(right_len) {
        return Err("invalid relation key byte length".to_string());
    }
    Ok((left.to_string(), right.to_string()))
}

impl Database {
    pub fn get_database_sync_summary(&self) -> Result<Option<DatabaseSyncSummary>> {
        self.with_conn(|conn| {
            let value = conn
                .query_row(
                    "SELECT value FROM sync_meta WHERE key = ?1",
                    [LAST_SUMMARY_KEY],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            value
                .map(|json| serde_json::from_str(&json).map_err(|_| rusqlite::Error::InvalidQuery))
                .transpose()
        })
    }

    pub fn set_database_sync_summary(&self, summary: &DatabaseSyncSummary) -> Result<()> {
        let value = serde_json::to_string(summary).map_err(|_| rusqlite::Error::InvalidQuery)?;
        self.set_sync_meta(LAST_SUMMARY_KEY, &value)
    }

    /// Resolve a persisted conflict by applying its remote record and removing
    /// the conflict in the same transaction. Entity/table and key mapping is
    /// intentionally closed over the known sync entity set.
    pub fn resolve_sync_conflict_remote(&self, entity_type: &str, entity_id: &str) -> Result<()> {
        let entity = parse_entity_type(entity_type)?;
        self.with_transaction(|tx| {
            let conflict = tx.query_row(
                "SELECT remote_record, remote_version FROM sync_conflicts WHERE entity_type = ?1 AND entity_id = ?2",
                params![entity_type, entity_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )?;
            let payload: JsonValue = serde_json::from_str(&conflict.0)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let record = crate::RemoteRecord { entity_type: entity, version: conflict.1, payload };
            let key = record.key().ok_or(rusqlite::Error::InvalidQuery)?;
            if record.canonical_key().as_deref() != Some(entity_id) {
                return Err(rusqlite::Error::InvalidQuery);
            }
            crate::sync_download::apply_remote_records(tx, &[record])?;
            let changed = tx.execute(
                "DELETE FROM sync_conflicts WHERE entity_type = ?1 AND entity_id = ?2",
                params![entity_type, entity_id],
            )?;
            if changed != 1 || self_key_exists(tx, entity, &key)? == false {
                return Err(rusqlite::Error::InvalidQuery);
            }
            Ok(())
        })
    }

    /// Keep the local/manual result: retain business columns and dirty state,
    /// acknowledge the remote version, and remove the conflict atomically.
    pub fn resolve_sync_conflict_local(&self, entity_type: &str, entity_id: &str) -> Result<()> {
        let entity = parse_entity_type(entity_type)?;
        self.with_transaction(|tx| {
            let remote_version: i64 = tx.query_row(
                "SELECT remote_version FROM sync_conflicts WHERE entity_type = ?1 AND entity_id = ?2",
                params![entity_type, entity_id],
                |row| row.get(0),
            )?;
            let key = parse_entity_key(entity, entity_id)?;
            let changed = update_local_conflict_state(tx, entity, &key, remote_version)?;
            if changed != 1 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            let deleted = tx.execute(
                "DELETE FROM sync_conflicts WHERE entity_type = ?1 AND entity_id = ?2",
                params![entity_type, entity_id],
            )?;
            if deleted != 1 { return Err(rusqlite::Error::InvalidQuery); }
            Ok(())
        })
    }

    pub fn get_synced_version(
        &self,
        entity: SyncEntityType,
        key: &SyncEntityKey,
    ) -> Result<Option<i64>> {
        self.with_conn(|conn| match (entity, key) {
            (SyncEntityType::Literature, SyncEntityKey::Id(id)) => conn.query_row("SELECT synced_version FROM literatures WHERE id = ?1", [id], |r| r.get(0)).optional(),
            (SyncEntityType::Publication, SyncEntityKey::Id(id)) => conn.query_row("SELECT synced_version FROM publications WHERE id = ?1", [id], |r| r.get(0)).optional(),
            (SyncEntityType::Author, SyncEntityKey::Id(id)) => conn.query_row("SELECT synced_version FROM authors WHERE id = ?1", [id], |r| r.get(0)).optional(),
            (SyncEntityType::Folder, SyncEntityKey::Id(id)) => conn.query_row("SELECT synced_version FROM folders WHERE id = ?1", [id], |r| r.get(0)).optional(),
            (SyncEntityType::Tag, SyncEntityKey::Id(id)) => conn.query_row("SELECT synced_version FROM tags WHERE id = ?1", [id], |r| r.get(0)).optional(),
            (SyncEntityType::Attachment, SyncEntityKey::Id(id)) => conn.query_row("SELECT synced_version FROM attachments WHERE id = ?1", [id], |r| r.get(0)).optional(),
            (SyncEntityType::Feed, SyncEntityKey::Id(id)) => conn.query_row("SELECT synced_version FROM feeds WHERE id = ?1", [id], |r| r.get(0)).optional(),
            (SyncEntityType::FeedItem, SyncEntityKey::Id(id)) => conn.query_row("SELECT synced_version FROM feed_items WHERE id = ?1", [id], |r| r.get(0)).optional(),
            (SyncEntityType::Annotation, SyncEntityKey::Id(id)) => conn.query_row("SELECT synced_version FROM annotations WHERE id = ?1", [id], |r| r.get(0)).optional(),
            (SyncEntityType::LiteratureNote, SyncEntityKey::Id(id)) => conn.query_row("SELECT synced_version FROM literature_notes WHERE id = ?1", [id], |r| r.get(0)).optional(),
            (SyncEntityType::LiteratureAuthor, SyncEntityKey::Relation { left, right }) => conn.query_row("SELECT synced_version FROM literature_authors WHERE literature_id = ?1 AND author_id = ?2", params![left, right], |r| r.get(0)).optional(),
            (SyncEntityType::LiteratureFolder, SyncEntityKey::Relation { left, right }) => conn.query_row("SELECT synced_version FROM literature_folders WHERE literature_id = ?1 AND folder_id = ?2", params![left, right], |r| r.get(0)).optional(),
            (SyncEntityType::LiteratureTag, SyncEntityKey::Relation { left, right }) => conn.query_row("SELECT synced_version FROM literature_tags WHERE literature_id = ?1 AND tag_id = ?2", params![left, right], |r| r.get(0)).optional(),
            (SyncEntityType::Citation, SyncEntityKey::Relation { left, right }) => conn.query_row("SELECT synced_version FROM literature_citations WHERE source_id = ?1 AND target_id = ?2", params![left, right], |r| r.get(0)).optional(),
            _ => Err(rusqlite::Error::InvalidQuery),
        })
    }

    pub fn set_synced_version(
        &self,
        entity: SyncEntityType,
        key: &SyncEntityKey,
        version: i64,
    ) -> Result<bool> {
        self.with_conn(|conn| {
            let changed = match (entity, key) {
                (SyncEntityType::Literature, SyncEntityKey::Id(id)) => conn.execute("UPDATE literatures SET synced_version = ?1 WHERE id = ?2", params![version, id])?,
                (SyncEntityType::Publication, SyncEntityKey::Id(id)) => conn.execute("UPDATE publications SET synced_version = ?1 WHERE id = ?2", params![version, id])?,
                (SyncEntityType::Author, SyncEntityKey::Id(id)) => conn.execute("UPDATE authors SET synced_version = ?1 WHERE id = ?2", params![version, id])?,
                (SyncEntityType::Folder, SyncEntityKey::Id(id)) => conn.execute("UPDATE folders SET synced_version = ?1 WHERE id = ?2", params![version, id])?,
                (SyncEntityType::Tag, SyncEntityKey::Id(id)) => conn.execute("UPDATE tags SET synced_version = ?1 WHERE id = ?2", params![version, id])?,
                (SyncEntityType::Attachment, SyncEntityKey::Id(id)) => conn.execute("UPDATE attachments SET synced_version = ?1 WHERE id = ?2", params![version, id])?,
                (SyncEntityType::Feed, SyncEntityKey::Id(id)) => conn.execute("UPDATE feeds SET synced_version = ?1 WHERE id = ?2", params![version, id])?,
                (SyncEntityType::FeedItem, SyncEntityKey::Id(id)) => conn.execute("UPDATE feed_items SET synced_version = ?1 WHERE id = ?2", params![version, id])?,
                (SyncEntityType::Annotation, SyncEntityKey::Id(id)) => conn.execute("UPDATE annotations SET synced_version = ?1 WHERE id = ?2", params![version, id])?,
                (SyncEntityType::LiteratureNote, SyncEntityKey::Id(id)) => conn.execute("UPDATE literature_notes SET synced_version = ?1 WHERE id = ?2", params![version, id])?,
                (SyncEntityType::LiteratureAuthor, SyncEntityKey::Relation { left, right }) => conn.execute("UPDATE literature_authors SET synced_version = ?1 WHERE literature_id = ?2 AND author_id = ?3", params![version, left, right])?,
                (SyncEntityType::LiteratureFolder, SyncEntityKey::Relation { left, right }) => conn.execute("UPDATE literature_folders SET synced_version = ?1 WHERE literature_id = ?2 AND folder_id = ?3", params![version, left, right])?,
                (SyncEntityType::LiteratureTag, SyncEntityKey::Relation { left, right }) => conn.execute("UPDATE literature_tags SET synced_version = ?1 WHERE literature_id = ?2 AND tag_id = ?3", params![version, left, right])?,
                (SyncEntityType::Citation, SyncEntityKey::Relation { left, right }) => conn.execute("UPDATE literature_citations SET synced_version = ?1 WHERE source_id = ?2 AND target_id = ?3", params![version, left, right])?,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            Ok(changed > 0)
        })
    }
    pub fn get_local_sync_state(&self) -> Result<LocalSyncState> {
        self.with_conn(|conn| {
            let library_id = conn
                .query_row(
                    "SELECT value FROM sync_meta WHERE key = ?1",
                    [LIBRARY_ID_KEY],
                    |row| row.get(0),
                )
                .optional()?;
            let last_sequence = conn
                .query_row(
                    "SELECT value FROM sync_meta WHERE key = ?1",
                    [LAST_SEQUENCE_KEY],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|value| value.parse().unwrap_or(0))
                .unwrap_or(0);
            let remote_fingerprint = conn
                .query_row(
                    "SELECT value FROM sync_meta WHERE key = ?1",
                    [REMOTE_FINGERPRINT_KEY],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(LocalSyncState {
                library_id,
                last_sequence,
                remote_fingerprint,
            })
        })
    }

    pub fn set_local_library_id(&self, library_id: &str) -> Result<()> {
        self.set_sync_meta(LIBRARY_ID_KEY, library_id)
    }

    pub fn set_last_sequence(&self, sequence: i64) -> Result<()> {
        self.set_sync_meta(LAST_SEQUENCE_KEY, &sequence.to_string())
    }

    pub fn set_remote_fingerprint(&self, fingerprint: &str) -> Result<()> {
        self.set_sync_meta(REMOTE_FINGERPRINT_KEY, fingerprint)
    }

    /// Reset the local database synchronization position after an explicit
    /// remote database clear.  The user-confirmed library identity is kept,
    /// while every local row is conservatively scheduled for a fresh upload.
    /// Attachment file synchronization state is intentionally untouched.
    pub fn reset_database_sync_state_after_remote_clear(&self) -> Result<()> {
        self.with_transaction(|tx| {
            for table in [
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
                "literature_citations",
                "annotations",
                "literature_notes",
            ] {
                tx.execute(
                    &format!("UPDATE {table} SET is_dirty = 1, synced_version = 0"),
                    [],
                )?;
            }
            tx.execute(
                "DELETE FROM sync_meta WHERE key IN (?1, ?2)",
                params![LAST_SEQUENCE_KEY, REMOTE_FINGERPRINT_KEY],
            )?;
            tx.execute("DELETE FROM sync_conflicts", [])?;
            Ok(())
        })
    }

    /// Persist the identity and sync position together. This is used only
    /// after a user-confirmed identity decision or a successful snapshot.
    pub fn set_identity_state(
        &self,
        library_id: &str,
        fingerprint: &str,
        last_sequence: i64,
    ) -> Result<()> {
        self.with_transaction(|tx| {
            for (key, value) in [
                (LIBRARY_ID_KEY, library_id.to_string()),
                (REMOTE_FINGERPRINT_KEY, fingerprint.to_string()),
                (LAST_SEQUENCE_KEY, last_sequence.to_string()),
            ] {
                tx.execute(
                    "INSERT INTO sync_meta (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![key, value],
                )?;
            }
            Ok(())
        })
    }

    /// 在同一 SQLite 事务中应用一批远程记录并推进下载位置。
    ///
    /// 具体记录的盲写由后续阶段的类型化数据库原语提供；此处只提供
    /// 不会让 `last_sequence` 独自前进的事务边界。
    pub fn apply_remote_batch<F, R>(&self, last_sequence: i64, apply: F) -> Result<R>
    where
        F: FnOnce(&Transaction<'_>) -> Result<R>,
    {
        self.with_transaction(|tx| {
            let result = apply(tx)?;
            tx.execute(
                "INSERT INTO sync_meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![LAST_SEQUENCE_KEY, last_sequence.to_string()],
            )?;
            Ok(result)
        })
    }

    pub fn save_sync_conflict(&self, conflict: &SyncConflict) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO sync_conflicts
                    (entity_type, entity_id, local_record, remote_record, remote_version, detected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(entity_type, entity_id) DO UPDATE SET
                    local_record = excluded.local_record,
                    remote_record = excluded.remote_record,
                    remote_version = excluded.remote_version,
                    detected_at = excluded.detected_at",
                params![
                    conflict.entity_type,
                    conflict.entity_id,
                    conflict.local_record,
                    conflict.remote_record,
                    conflict.remote_version,
                    conflict.detected_at,
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_sync_conflicts(&self) -> Result<Vec<SyncConflict>> {
        self.with_conn(|conn| {
            let mut statement = conn.prepare(
                "SELECT entity_type, entity_id, local_record, remote_record, remote_version, detected_at
                 FROM sync_conflicts ORDER BY detected_at, entity_type, entity_id",
            )?;
            statement
                .query_map([], |row| {
                    Ok(SyncConflict {
                        entity_type: row.get(0)?,
                        entity_id: row.get(1)?,
                        local_record: row.get(2)?,
                        remote_record: row.get(3)?,
                        remote_version: row.get(4)?,
                        detected_at: row.get(5)?,
                    })
                })?
                .collect()
        })
    }

    pub fn has_sync_conflict(&self, entity_type: &str, entity_id: &str) -> Result<bool> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT 1 FROM sync_conflicts WHERE entity_type = ?1 AND entity_id = ?2 LIMIT 1",
                params![entity_type, entity_id],
                |_| Ok(()),
            )
            .optional()
            .map(|value| value.is_some())
        })
    }

    pub fn delete_sync_conflict(&self, entity_type: &str, entity_id: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM sync_conflicts WHERE entity_type = ?1 AND entity_id = ?2",
                params![entity_type, entity_id],
            )?;
            Ok(())
        })
    }
}

fn parse_entity_type(value: &str) -> Result<SyncEntityType> {
    match value {
        "literatures" => Ok(SyncEntityType::Literature),
        "publications" => Ok(SyncEntityType::Publication),
        "authors" => Ok(SyncEntityType::Author),
        "literature_authors" => Ok(SyncEntityType::LiteratureAuthor),
        "folders" => Ok(SyncEntityType::Folder),
        "literature_folders" => Ok(SyncEntityType::LiteratureFolder),
        "tags" => Ok(SyncEntityType::Tag),
        "literature_tags" => Ok(SyncEntityType::LiteratureTag),
        "attachments" => Ok(SyncEntityType::Attachment),
        "feeds" => Ok(SyncEntityType::Feed),
        "feed_items" => Ok(SyncEntityType::FeedItem),
        "literature_citations" => Ok(SyncEntityType::Citation),
        "annotations" => Ok(SyncEntityType::Annotation),
        "literature_notes" => Ok(SyncEntityType::LiteratureNote),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn parse_entity_key(entity: SyncEntityType, value: &str) -> Result<SyncEntityKey> {
    if matches!(
        entity,
        SyncEntityType::LiteratureAuthor
            | SyncEntityType::LiteratureFolder
            | SyncEntityType::LiteratureTag
            | SyncEntityType::Citation
    ) {
        let (left, right) =
            decode_relation_key(value).map_err(|_| rusqlite::Error::InvalidQuery)?;
        Ok(SyncEntityKey::Relation { left, right })
    } else if value.is_empty() {
        Err(rusqlite::Error::InvalidQuery)
    } else {
        Ok(SyncEntityKey::Id(value.to_string()))
    }
}

fn self_key_exists(
    tx: &Transaction<'_>,
    entity: SyncEntityType,
    key: &SyncEntityKey,
) -> Result<bool> {
    let (table, where_sql, values) = key_sql(entity, key, 0)?;
    let sql = format!("SELECT 1 FROM {table} WHERE {where_sql} LIMIT 1");
    Ok(tx
        .query_row(&sql, rusqlite::params_from_iter(values.iter()), |_| Ok(()))
        .optional()?
        .is_some())
}

fn update_local_conflict_state(
    tx: &Transaction<'_>,
    entity: SyncEntityType,
    key: &SyncEntityKey,
    version: i64,
) -> Result<usize> {
    let (table, where_sql, values) = key_sql(entity, key, 1)?;
    let sql = format!("UPDATE {table} SET is_dirty = 1, synced_version = ?1 WHERE {where_sql}");
    let mut bind = vec![rusqlite::types::Value::Integer(version)];
    bind.extend(values);
    Ok(tx.execute(&sql, rusqlite::params_from_iter(bind))?)
}

fn key_sql(
    entity: SyncEntityType,
    key: &SyncEntityKey,
    offset: usize,
) -> Result<(&'static str, String, Vec<rusqlite::types::Value>)> {
    let (table, names) = match (entity, key) {
        (SyncEntityType::Literature, SyncEntityKey::Id(_)) => ("literatures", vec!["id"]),
        (SyncEntityType::Publication, SyncEntityKey::Id(_)) => ("publications", vec!["id"]),
        (SyncEntityType::Author, SyncEntityKey::Id(_)) => ("authors", vec!["id"]),
        (SyncEntityType::Folder, SyncEntityKey::Id(_)) => ("folders", vec!["id"]),
        (SyncEntityType::Tag, SyncEntityKey::Id(_)) => ("tags", vec!["id"]),
        (SyncEntityType::Attachment, SyncEntityKey::Id(_)) => ("attachments", vec!["id"]),
        (SyncEntityType::Feed, SyncEntityKey::Id(_)) => ("feeds", vec!["id"]),
        (SyncEntityType::FeedItem, SyncEntityKey::Id(_)) => ("feed_items", vec!["id"]),
        (SyncEntityType::Annotation, SyncEntityKey::Id(_)) => ("annotations", vec!["id"]),
        (SyncEntityType::LiteratureNote, SyncEntityKey::Id(_)) => ("literature_notes", vec!["id"]),
        (SyncEntityType::LiteratureAuthor, SyncEntityKey::Relation { .. }) => {
            ("literature_authors", vec!["literature_id", "author_id"])
        }
        (SyncEntityType::LiteratureFolder, SyncEntityKey::Relation { .. }) => {
            ("literature_folders", vec!["literature_id", "folder_id"])
        }
        (SyncEntityType::LiteratureTag, SyncEntityKey::Relation { .. }) => {
            ("literature_tags", vec!["literature_id", "tag_id"])
        }
        (SyncEntityType::Citation, SyncEntityKey::Relation { .. }) => {
            ("literature_citations", vec!["source_id", "target_id"])
        }
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    let values = match key {
        SyncEntityKey::Id(v) => vec![rusqlite::types::Value::Text(v.clone())],
        SyncEntityKey::Relation { left, right } => vec![
            rusqlite::types::Value::Text(left.clone()),
            rusqlite::types::Value::Text(right.clone()),
        ],
    };
    let where_sql = names
        .iter()
        .enumerate()
        .map(|(i, n)| format!("{n} = ?{}", i + offset + 1))
        .collect::<Vec<_>>()
        .join(" AND ");
    Ok((table, where_sql, values))
}

#[cfg(test)]
mod tests {
    use super::{LocalSyncState, SyncConflict};
    use crate::{Database, SyncEntityKey, SyncEntityType, canonical_key, decode_relation_key};

    #[test]
    fn local_sync_state_and_conflicts_are_persistent() {
        let db = Database::new(":memory:").unwrap();
        assert_eq!(
            db.get_local_sync_state().unwrap(),
            LocalSyncState::default()
        );

        db.set_local_library_id("library-a").unwrap();
        db.set_last_sequence(42).unwrap();
        assert_eq!(
            db.get_local_sync_state().unwrap(),
            LocalSyncState {
                library_id: Some("library-a".to_string()),
                last_sequence: 42,
                remote_fingerprint: None,
            }
        );

        let conflict = SyncConflict {
            entity_type: "literature".to_string(),
            entity_id: "item-1".to_string(),
            local_record: r#"{\"title\":\"local\"}"#.to_string(),
            remote_record: r#"{\"title\":\"remote\"}"#.to_string(),
            remote_version: 7,
            detected_at: 123,
        };
        db.save_sync_conflict(&conflict).unwrap();
        assert_eq!(db.list_sync_conflicts().unwrap(), vec![conflict]);

        db.delete_sync_conflict("literature", "item-1").unwrap();
        assert!(db.list_sync_conflicts().unwrap().is_empty());
    }

    #[test]
    fn failed_remote_batch_does_not_advance_sequence() {
        let db = Database::new(":memory:").unwrap();
        db.set_last_sequence(4).unwrap();

        let result = db.apply_remote_batch(5, |tx| {
            tx.execute(
                "INSERT INTO sync_conflicts
                 (entity_type, entity_id, local_record, remote_record, remote_version, detected_at)
                 VALUES ('literature', 'rolled-back', '{}', '{}', 1, 1)",
                [],
            )?;
            Err::<(), _>(rusqlite::Error::InvalidQuery)
        });

        assert!(result.is_err());
        assert_eq!(db.get_local_sync_state().unwrap().last_sequence, 4);
        assert!(db.list_sync_conflicts().unwrap().is_empty());
    }

    #[test]
    fn synced_version_reads_and_writes_single_and_composite_keys() {
        let db = Database::new(":memory:").unwrap();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO literature_authors (literature_id, author_id, sort_order, is_deleted, version, synced_version) VALUES ('l', 'a', 0, 0, 1, 3)", [])?;
            Ok(())
        }).unwrap();
        let key = SyncEntityKey::Relation {
            left: "l".into(),
            right: "a".into(),
        };
        assert_eq!(
            db.get_synced_version(SyncEntityType::LiteratureAuthor, &key)
                .unwrap(),
            Some(3)
        );
        assert!(
            db.set_synced_version(SyncEntityType::LiteratureAuthor, &key, 8)
                .unwrap()
        );
        assert_eq!(
            db.get_synced_version(SyncEntityType::LiteratureAuthor, &key)
                .unwrap(),
            Some(8)
        );
        assert_eq!(
            db.get_synced_version(
                SyncEntityType::Literature,
                &SyncEntityKey::Id("missing".into())
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn choosing_remote_delete_marks_record_deleted_and_removes_conflict() {
        let db = Database::new(":memory:").unwrap();
        let tag = db.create_tag("local", None).unwrap();
        let remote = serde_json::json!({
            "id": tag.id.clone(), "name": "remote", "color": null,
            "is_deleted": true, "version": 8, "created_at": 0, "updated_at": 0
        });
        db.save_sync_conflict(&SyncConflict {
            entity_type: "tags".into(),
            entity_id: tag.id.clone(),
            local_record: "{}".into(),
            remote_record: remote.to_string(),
            remote_version: 8,
            detected_at: 1,
        })
        .unwrap();
        db.resolve_sync_conflict_remote("tags", &tag.id).unwrap();
        let state = db
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT is_deleted, is_dirty, version FROM tags WHERE id = ?1",
                    [&tag.id],
                    |row| {
                        Ok((
                            row.get::<_, bool>(0)?,
                            row.get::<_, bool>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    },
                )
            })
            .unwrap();
        assert_eq!(state, (true, false, 8));
        assert_eq!(
            db.get_synced_version(SyncEntityType::Tag, &SyncEntityKey::Id(tag.id.clone()))
                .unwrap(),
            Some(8)
        );
        assert!(db.list_sync_conflicts().unwrap().is_empty());
    }

    #[test]
    fn keeping_local_updates_sync_version_and_removes_conflict_atomically() {
        let db = Database::new(":memory:").unwrap();
        let tag = db.create_tag("local", None).unwrap();
        db.save_sync_conflict(&SyncConflict {
            entity_type: "tags".into(),
            entity_id: tag.id.clone(),
            local_record: "{}".into(),
            remote_record: "{}".into(),
            remote_version: 9,
            detected_at: 1,
        })
        .unwrap();
        db.resolve_sync_conflict_local("tags", &tag.id).unwrap();
        assert_eq!(
            db.get_download_state(SyncEntityType::Tag, &SyncEntityKey::Id(tag.id.clone()))
                .unwrap(),
            Some((9, true))
        );
        assert_eq!(
            db.get_synced_version(SyncEntityType::Tag, &SyncEntityKey::Id(tag.id.clone()))
                .unwrap(),
            Some(9)
        );
        assert_eq!(
            db.get_all_tags_with_counts()
                .unwrap()
                .into_iter()
                .find(|(value, _)| value.id == tag.id)
                .unwrap()
                .0
                .name,
            "local"
        );
        assert!(db.list_sync_conflicts().unwrap().is_empty());
    }

    #[test]
    fn choosing_remote_rejects_composite_key_mismatch_without_deleting_conflict() {
        let db = Database::new(":memory:").unwrap();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO literature_authors (literature_id, author_id, sort_order, is_deleted, version, synced_version) VALUES ('l', 'a', 0, 0, 1, 1)", [])?;
            Ok(())
        }).unwrap();
        let remote = serde_json::json!({"literature_id":"l","author_id":"a","sort_order":1,"is_deleted":true,"version":4,"updated_at":0});
        db.save_sync_conflict(&SyncConflict {
            entity_type: "literature_authors".into(),
            entity_id: canonical_key(&SyncEntityKey::Relation {
                left: "l".into(),
                right: "other".into(),
            }),
            local_record: "{}".into(),
            remote_record: remote.to_string(),
            remote_version: 4,
            detected_at: 1,
        })
        .unwrap();
        assert!(
            db.resolve_sync_conflict_remote(
                "literature_authors",
                &canonical_key(&SyncEntityKey::Relation {
                    left: "l".into(),
                    right: "other".into(),
                }),
            )
            .is_err()
        );
        assert_eq!(db.list_sync_conflicts().unwrap().len(), 1);
    }

    #[test]
    fn remote_clear_resets_database_position_without_forgetting_identity() {
        let db = Database::new(":memory:").unwrap();
        let tag = db.create_tag("local", None).unwrap();
        db.set_identity_state("library", "fingerprint", 42).unwrap();
        db.confirm_upload(SyncEntityType::Tag, &SyncEntityKey::Id(tag.id.clone()), 7)
            .unwrap();

        db.reset_database_sync_state_after_remote_clear().unwrap();

        let state = db.get_local_sync_state().unwrap();
        assert_eq!(state.library_id.as_deref(), Some("library"));
        assert_eq!(state.last_sequence, 0);
        assert_eq!(state.remote_fingerprint, None);
        assert_eq!(
            db.get_download_state(SyncEntityType::Tag, &SyncEntityKey::Id(tag.id))
                .unwrap(),
            Some((0, true))
        );
    }

    #[test]
    fn relation_key_is_reversible_and_delimiter_safe() {
        let key = SyncEntityKey::Relation {
            left: "left;含冒号-aaaaaaaaaaaaaaaaaaaaaaaa".into(),
            right: "right;含冒号-bbbbbbbbbbbbbbbbbbbbbbbb".into(),
        };
        let encoded = canonical_key(&key);
        assert!(encoded.len() > 64);
        assert_eq!(
            decode_relation_key(&encoded).unwrap(),
            (
                "left;含冒号-aaaaaaaaaaaaaaaaaaaaaaaa".into(),
                "right;含冒号-bbbbbbbbbbbbbbbbbbbbbbbb".into()
            )
        );
    }
}
