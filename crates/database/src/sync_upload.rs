//! Typed local dirty-record collection and upload confirmation primitives.

use crate::mysql::{RelationAuthor, RelationFolder, RelationTag, SyncEntityPayload};
use crate::{Database, SyncEntityKey, SyncEntityType, canonical_key};
use anyhow::Result;
use models::LiteratureNote;
use rusqlite::{OptionalExtension, params};

#[derive(Clone, Debug)]
pub struct LocalDirtyRecord {
    pub entity: SyncEntityType,
    pub key: SyncEntityKey,
    pub local_generation: i64,
    pub expected_remote_version: i32,
    pub payload: SyncEntityPayload,
}

impl Database {
    /// Collect all dirty records in dependency order. This is a closed, typed
    /// list; callers cannot provide table names or SQL.
    pub fn collect_dirty_records(&self) -> Result<Vec<LocalDirtyRecord>> {
        let mut out = Vec::new();
        macro_rules! add {
            ($entity:expr, $rows:expr, $payload:ident) => {
                for value in $rows? {
                    let payload = SyncEntityPayload::$payload(value);
                    let key = payload_key(&payload);
                    if self.has_sync_conflict($entity.as_str(), &canonical_key(&key))? {
                        continue;
                    }
                    let expected_remote_version =
                        i32::try_from(self.get_synced_version($entity, &key)?.unwrap_or(0))?;
                    let local_generation = value_generation(&payload)?;
                    out.push(LocalDirtyRecord {
                        entity: $entity,
                        key,
                        local_generation,
                        expected_remote_version,
                        payload,
                    });
                }
            };
        }
        add!(
            SyncEntityType::Publication,
            self.get_dirty_publications(),
            Publication
        );
        add!(SyncEntityType::Author, self.get_dirty_authors(), Author);
        add!(SyncEntityType::Folder, self.get_dirty_folders(), Folder);
        add!(SyncEntityType::Tag, self.get_dirty_tags(), Tag);
        add!(SyncEntityType::Feed, self.get_dirty_feeds(), Feed);
        add!(
            SyncEntityType::Literature,
            self.get_dirty_literatures(),
            Literature
        );
        add!(
            SyncEntityType::FeedItem,
            self.get_dirty_feed_items(),
            FeedItem
        );
        for value in self.get_dirty_annotations()? {
            let payload = SyncEntityPayload::Annotation(value.into());
            let key = payload_key(&payload);
            if self.has_sync_conflict(SyncEntityType::Annotation.as_str(), &canonical_key(&key))? {
                continue;
            }
            let expected_remote_version = self
                .get_synced_version(SyncEntityType::Annotation, &key)?
                .unwrap_or(0);
            let local_generation = value_generation(&payload)?;
            out.push(LocalDirtyRecord {
                entity: SyncEntityType::Annotation,
                key,
                local_generation,
                expected_remote_version: i32::try_from(expected_remote_version)?,
                payload,
            });
        }
        add!(
            SyncEntityType::LiteratureNote,
            dirty_notes(self),
            LiteratureNote
        );
        add!(
            SyncEntityType::Attachment,
            self.get_dirty_attachments(),
            Attachment
        );
        add!(
            SyncEntityType::Citation,
            self.get_dirty_citations(),
            Citation
        );

        let (authors, folders, tags) = self.get_dirty_relations()?;
        for (literature_id, author_id, sort_order, is_deleted, version) in authors {
            let payload = SyncEntityPayload::LiteratureAuthor(RelationAuthor {
                literature_id: literature_id.clone(),
                author_id: author_id.clone(),
                sort_order: sort_order.unwrap_or(0),
                is_deleted,
                version,
                updated_at: 0,
            });
            let key = SyncEntityKey::Relation {
                left: literature_id,
                right: author_id,
            };
            if self.has_sync_conflict(
                SyncEntityType::LiteratureAuthor.as_str(),
                &canonical_key(&key),
            )? {
                continue;
            }
            let expected_remote_version = self
                .get_synced_version(SyncEntityType::LiteratureAuthor, &key)?
                .unwrap_or(0);
            let local_generation = i64::from(version);
            out.push(LocalDirtyRecord {
                entity: SyncEntityType::LiteratureAuthor,
                key,
                local_generation,
                expected_remote_version: i32::try_from(expected_remote_version)?,
                payload,
            });
        }
        for (literature_id, folder_id, is_deleted, version) in folders {
            let payload = SyncEntityPayload::LiteratureFolder(RelationFolder {
                literature_id: literature_id.clone(),
                folder_id: folder_id.clone(),
                is_deleted,
                version,
                updated_at: 0,
            });
            let key = SyncEntityKey::Relation {
                left: literature_id,
                right: folder_id,
            };
            if self.has_sync_conflict(
                SyncEntityType::LiteratureFolder.as_str(),
                &canonical_key(&key),
            )? {
                continue;
            }
            let expected_remote_version = self
                .get_synced_version(SyncEntityType::LiteratureFolder, &key)?
                .unwrap_or(0);
            let local_generation = i64::from(version);
            out.push(LocalDirtyRecord {
                entity: SyncEntityType::LiteratureFolder,
                key,
                local_generation,
                expected_remote_version: i32::try_from(expected_remote_version)?,
                payload,
            });
        }
        for (literature_id, tag_id, is_deleted, version) in tags {
            let payload = SyncEntityPayload::LiteratureTag(RelationTag {
                literature_id: literature_id.clone(),
                tag_id: tag_id.clone(),
                is_deleted,
                version,
                updated_at: 0,
            });
            let key = SyncEntityKey::Relation {
                left: literature_id,
                right: tag_id,
            };
            if self
                .has_sync_conflict(SyncEntityType::LiteratureTag.as_str(), &canonical_key(&key))?
            {
                continue;
            }
            let expected_remote_version = self
                .get_synced_version(SyncEntityType::LiteratureTag, &key)?
                .unwrap_or(0);
            let local_generation = i64::from(version);
            out.push(LocalDirtyRecord {
                entity: SyncEntityType::LiteratureTag,
                key,
                local_generation,
                expected_remote_version: i32::try_from(expected_remote_version)?,
                payload,
            });
        }
        // Citations are collected above; order relations after their entities.
        Ok(out)
    }

    /// Atomically confirms exactly the snapshot that was uploaded.
    pub fn confirm_uploaded_snapshot(
        &self,
        entity: SyncEntityType,
        key: &SyncEntityKey,
        local_generation: i64,
        expected_remote_version: i32,
        accepted_remote_version: i32,
    ) -> Result<UploadConfirmation> {
        if local_generation <= 0
            || accepted_remote_version < 0
            || accepted_remote_version <= expected_remote_version
        {
            return Ok(UploadConfirmation::InvalidGeneration);
        }
        Ok(self.with_transaction(|tx| {
            let state = select_upload_state(tx, entity, key)?;
            let Some((version, dirty, synced)) = state else {
                return Ok(UploadConfirmation::Missing);
            };
            if synced != i64::from(expected_remote_version) {
                return Ok(UploadConfirmation::StaleBase);
            }
            if !dirty || version < local_generation {
                return Ok(UploadConfirmation::InvalidGeneration);
            }
            let superseded = version > local_generation;
            update_upload_state(
                tx,
                entity,
                key,
                !superseded,
                i64::from(accepted_remote_version),
            )?;
            Ok(if superseded {
                UploadConfirmation::Superseded
            } else {
                UploadConfirmation::Confirmed
            })
        })?)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadConfirmation {
    Confirmed,
    Superseded,
    Missing,
    StaleBase,
    InvalidGeneration,
}

fn value_generation(payload: &SyncEntityPayload) -> Result<i64> {
    let generation = payload.value()["version"]
        .as_i64()
        .ok_or_else(|| anyhow::anyhow!("sync payload is missing version"))?;
    if generation <= 0 {
        return Err(anyhow::anyhow!("sync payload version must be positive"));
    }
    Ok(generation)
}

fn select_upload_state(
    tx: &rusqlite::Transaction<'_>,
    entity: SyncEntityType,
    key: &SyncEntityKey,
) -> rusqlite::Result<Option<(i64, bool, i64)>> {
    macro_rules! one {
        ($sql:expr, $params:expr) => {
            tx.query_row($sql, $params, |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .optional()
        };
    }
    match (entity, key) {
        (SyncEntityType::Literature, SyncEntityKey::Id(id)) => one!(
            "SELECT version,is_dirty,synced_version FROM literatures WHERE id=?1",
            [id]
        ),
        (SyncEntityType::Publication, SyncEntityKey::Id(id)) => one!(
            "SELECT version,is_dirty,synced_version FROM publications WHERE id=?1",
            [id]
        ),
        (SyncEntityType::Author, SyncEntityKey::Id(id)) => one!(
            "SELECT version,is_dirty,synced_version FROM authors WHERE id=?1",
            [id]
        ),
        (SyncEntityType::Folder, SyncEntityKey::Id(id)) => one!(
            "SELECT version,is_dirty,synced_version FROM folders WHERE id=?1",
            [id]
        ),
        (SyncEntityType::Tag, SyncEntityKey::Id(id)) => one!(
            "SELECT version,is_dirty,synced_version FROM tags WHERE id=?1",
            [id]
        ),
        (SyncEntityType::Attachment, SyncEntityKey::Id(id)) => one!(
            "SELECT version,is_dirty,synced_version FROM attachments WHERE id=?1",
            [id]
        ),
        (SyncEntityType::Feed, SyncEntityKey::Id(id)) => one!(
            "SELECT version,is_dirty,synced_version FROM feeds WHERE id=?1",
            [id]
        ),
        (SyncEntityType::FeedItem, SyncEntityKey::Id(id)) => one!(
            "SELECT version,is_dirty,synced_version FROM feed_items WHERE id=?1",
            [id]
        ),
        (SyncEntityType::Annotation, SyncEntityKey::Id(id)) => one!(
            "SELECT version,is_dirty,synced_version FROM annotations WHERE id=?1",
            [id]
        ),
        (SyncEntityType::LiteratureNote, SyncEntityKey::Id(id)) => one!(
            "SELECT version,is_dirty,synced_version FROM literature_notes WHERE id=?1",
            [id]
        ),
        (SyncEntityType::LiteratureAuthor, SyncEntityKey::Relation { left, right }) => one!(
            "SELECT version,is_dirty,synced_version FROM literature_authors WHERE literature_id=?1 AND author_id=?2",
            params![left, right]
        ),
        (SyncEntityType::LiteratureFolder, SyncEntityKey::Relation { left, right }) => one!(
            "SELECT version,is_dirty,synced_version FROM literature_folders WHERE literature_id=?1 AND folder_id=?2",
            params![left, right]
        ),
        (SyncEntityType::LiteratureTag, SyncEntityKey::Relation { left, right }) => one!(
            "SELECT version,is_dirty,synced_version FROM literature_tags WHERE literature_id=?1 AND tag_id=?2",
            params![left, right]
        ),
        (SyncEntityType::Citation, SyncEntityKey::Relation { left, right }) => one!(
            "SELECT version,is_dirty,synced_version FROM literature_citations WHERE source_id=?1 AND target_id=?2",
            params![left, right]
        ),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn update_upload_state(
    tx: &rusqlite::Transaction<'_>,
    entity: SyncEntityType,
    key: &SyncEntityKey,
    clean: bool,
    accepted: i64,
) -> rusqlite::Result<()> {
    let dirty = !clean;
    let changed = match (entity, key) {
        (SyncEntityType::Literature, SyncEntityKey::Id(id)) => tx.execute("UPDATE literatures SET is_dirty=?1,synced_version=?2 WHERE id=?3", params![dirty,accepted,id]),
        (SyncEntityType::Publication, SyncEntityKey::Id(id)) => tx.execute("UPDATE publications SET is_dirty=?1,synced_version=?2 WHERE id=?3", params![dirty,accepted,id]),
        (SyncEntityType::Author, SyncEntityKey::Id(id)) => tx.execute("UPDATE authors SET is_dirty=?1,synced_version=?2 WHERE id=?3", params![dirty,accepted,id]),
        (SyncEntityType::Folder, SyncEntityKey::Id(id)) => tx.execute("UPDATE folders SET is_dirty=?1,synced_version=?2 WHERE id=?3", params![dirty,accepted,id]),
        (SyncEntityType::Tag, SyncEntityKey::Id(id)) => tx.execute("UPDATE tags SET is_dirty=?1,synced_version=?2 WHERE id=?3", params![dirty,accepted,id]),
        (SyncEntityType::Attachment, SyncEntityKey::Id(id)) => tx.execute("UPDATE attachments SET is_dirty=?1,synced_version=?2 WHERE id=?3", params![dirty,accepted,id]),
        (SyncEntityType::Feed, SyncEntityKey::Id(id)) => tx.execute("UPDATE feeds SET is_dirty=?1,synced_version=?2 WHERE id=?3", params![dirty,accepted,id]),
        (SyncEntityType::FeedItem, SyncEntityKey::Id(id)) => tx.execute("UPDATE feed_items SET is_dirty=?1,synced_version=?2 WHERE id=?3", params![dirty,accepted,id]),
        (SyncEntityType::Annotation, SyncEntityKey::Id(id)) => tx.execute("UPDATE annotations SET is_dirty=?1,synced_version=?2 WHERE id=?3", params![dirty,accepted,id]),
        (SyncEntityType::LiteratureNote, SyncEntityKey::Id(id)) => tx.execute("UPDATE literature_notes SET is_dirty=?1,synced_version=?2 WHERE id=?3", params![dirty,accepted,id]),
        (SyncEntityType::LiteratureAuthor, SyncEntityKey::Relation{left,right}) => tx.execute("UPDATE literature_authors SET is_dirty=?1,synced_version=?2 WHERE literature_id=?3 AND author_id=?4", params![dirty,accepted,left,right]),
        (SyncEntityType::LiteratureFolder, SyncEntityKey::Relation{left,right}) => tx.execute("UPDATE literature_folders SET is_dirty=?1,synced_version=?2 WHERE literature_id=?3 AND folder_id=?4", params![dirty,accepted,left,right]),
        (SyncEntityType::LiteratureTag, SyncEntityKey::Relation{left,right}) => tx.execute("UPDATE literature_tags SET is_dirty=?1,synced_version=?2 WHERE literature_id=?3 AND tag_id=?4", params![dirty,accepted,left,right]),
        (SyncEntityType::Citation, SyncEntityKey::Relation{left,right}) => tx.execute("UPDATE literature_citations SET is_dirty=?1,synced_version=?2 WHERE source_id=?3 AND target_id=?4", params![dirty,accepted,left,right]),
        _ => Err(rusqlite::Error::InvalidQuery),
    }?;
    if changed != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}

fn payload_key(payload: &SyncEntityPayload) -> SyncEntityKey {
    let value = payload.value();
    let keys = payload.entity_type().key_columns();
    if keys.len() == 1 {
        SyncEntityKey::Id(value[keys[0]].as_str().unwrap_or_default().to_string())
    } else {
        SyncEntityKey::Relation {
            left: value[keys[0]].as_str().unwrap_or_default().to_string(),
            right: value[keys[1]].as_str().unwrap_or_default().to_string(),
        }
    }
}

fn dirty_notes(db: &Database) -> rusqlite::Result<Vec<LiteratureNote>> {
    db.get_dirty_notes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_collection_and_confirmation_support_composite_relation_key() {
        let db = Database::new(":memory:").unwrap();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO literature_authors (literature_id, author_id, sort_order, is_dirty, is_deleted, version, updated_at, synced_version) VALUES ('lit', 'auth', 0, 1, 0, 1, 0, 0)", [])?;
            Ok(())
        }).unwrap();
        let records = db.collect_dirty_records().unwrap();
        let relation = records
            .iter()
            .find(|r| r.entity == SyncEntityType::LiteratureAuthor)
            .unwrap();
        assert_eq!(
            relation.key,
            SyncEntityKey::Relation {
                left: "lit".into(),
                right: "auth".into()
            }
        );
        assert_eq!(
            db.confirm_uploaded_snapshot(
                SyncEntityType::LiteratureAuthor,
                &relation.key,
                relation.local_generation,
                relation.expected_remote_version,
                2
            )
            .unwrap(),
            UploadConfirmation::Confirmed
        );
        assert_eq!(
            db.get_synced_version(SyncEntityType::LiteratureAuthor, &relation.key)
                .unwrap(),
            Some(2)
        );
    }

    #[test]
    fn persisted_conflicts_exclude_single_and_composite_dirty_records_until_resolved() {
        let db = Database::new(":memory:").unwrap();
        let tag = db.create_tag("local", None).unwrap();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO literature_authors (literature_id, author_id, sort_order, is_dirty, is_deleted, version, updated_at, synced_version) VALUES ('lit', 'auth', 0, 1, 0, 1, 0, 0)", [])?;
            Ok(())
        }).unwrap();
        db.save_sync_conflict(&crate::SyncConflict {
            entity_type: "tags".into(),
            entity_id: tag.id.clone(),
            local_record: "{}".into(),
            remote_record: "{}".into(),
            remote_version: 2,
            detected_at: 1,
        })
        .unwrap();
        db.save_sync_conflict(&crate::SyncConflict {
            entity_type: "literature_authors".into(),
            entity_id: crate::canonical_key(&SyncEntityKey::Relation {
                left: "lit".into(),
                right: "auth".into(),
            }),
            local_record: "{}".into(),
            remote_record: "{}".into(),
            remote_version: 2,
            detected_at: 1,
        })
        .unwrap();
        let records = db.collect_dirty_records().unwrap();
        assert!(!records.iter().any(|r| r.entity == SyncEntityType::Tag));
        assert!(
            !records
                .iter()
                .any(|r| r.entity == SyncEntityType::LiteratureAuthor)
        );
        db.delete_sync_conflict("tags", &tag.id).unwrap();
        db.delete_sync_conflict(
            "literature_authors",
            &crate::canonical_key(&SyncEntityKey::Relation {
                left: "lit".into(),
                right: "auth".into(),
            }),
        )
        .unwrap();
        let records = db.collect_dirty_records().unwrap();
        assert!(records.iter().any(|r| r.entity == SyncEntityType::Tag));
        assert!(
            records
                .iter()
                .any(|r| r.entity == SyncEntityType::LiteratureAuthor)
        );
    }

    #[test]
    fn confirmation_preserves_concurrent_edit_and_advances_remote_base() {
        let db = Database::new(":memory:").unwrap();
        let tag = db.create_tag("before", None).unwrap();
        let snapshot = db
            .collect_dirty_records()
            .unwrap()
            .into_iter()
            .find(|record| record.entity == SyncEntityType::Tag)
            .unwrap();
        db.update_tag_name(&tag.id, "after").unwrap();
        assert_eq!(
            db.confirm_uploaded_snapshot(
                snapshot.entity,
                &snapshot.key,
                snapshot.local_generation,
                snapshot.expected_remote_version,
                4,
            )
            .unwrap(),
            UploadConfirmation::Superseded
        );
        assert_eq!(
            db.get_download_state(snapshot.entity, &snapshot.key)
                .unwrap(),
            Some((4, true))
        );
        let current = db
            .collect_dirty_records()
            .unwrap()
            .into_iter()
            .find(|r| r.key == snapshot.key)
            .unwrap();
        assert_eq!(
            db.confirm_uploaded_snapshot(
                current.entity,
                &current.key,
                current.local_generation,
                4,
                5
            )
            .unwrap(),
            UploadConfirmation::Confirmed
        );
        assert_eq!(
            db.get_download_state(snapshot.entity, &snapshot.key)
                .unwrap(),
            Some((5, false))
        );
    }

    #[test]
    fn confirmation_rejects_missing_stale_invalid_and_bad_remote_versions() {
        let db = Database::new(":memory:").unwrap();
        let tag = db.create_tag("x", None).unwrap();
        let key = SyncEntityKey::Id(tag.id.clone());
        assert_eq!(
            db.confirm_uploaded_snapshot(
                SyncEntityType::Tag,
                &SyncEntityKey::Id("missing".into()),
                1,
                0,
                1
            )
            .unwrap(),
            UploadConfirmation::Missing
        );
        assert_eq!(
            db.confirm_uploaded_snapshot(SyncEntityType::Tag, &key, 1, 9, 10)
                .unwrap(),
            UploadConfirmation::StaleBase
        );
        assert_eq!(
            db.confirm_uploaded_snapshot(SyncEntityType::Tag, &key, 0, 0, 1)
                .unwrap(),
            UploadConfirmation::InvalidGeneration
        );
        assert_eq!(
            db.confirm_uploaded_snapshot(SyncEntityType::Tag, &key, 1, 0, 0)
                .unwrap(),
            UploadConfirmation::InvalidGeneration
        );
        assert_eq!(
            db.get_download_state(SyncEntityType::Tag, &key).unwrap(),
            Some((0, true))
        );
    }

    #[test]
    fn relation_tombstone_confirmation_preserves_newer_generation() {
        let db = Database::new(":memory:").unwrap();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO literature_tags (literature_id,tag_id,is_dirty,is_deleted,version,updated_at,synced_version) VALUES ('l','t',1,1,2,0,0)", [])?;
            Ok(())
        }).unwrap();
        let key = SyncEntityKey::Relation {
            left: "l".into(),
            right: "t".into(),
        };
        assert_eq!(
            db.confirm_uploaded_snapshot(SyncEntityType::LiteratureTag, &key, 1, 0, 2)
                .unwrap(),
            UploadConfirmation::Superseded
        );
        assert_eq!(
            db.get_download_state(SyncEntityType::LiteratureTag, &key)
                .unwrap(),
            Some((2, true))
        );
    }
}
