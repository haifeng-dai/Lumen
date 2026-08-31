//! Typed local dirty-record collection and upload confirmation primitives.

use crate::mysql::{RelationAuthor, RelationFolder, RelationTag, SyncEntityPayload};
use crate::{Database, SyncEntityKey, SyncEntityType, canonical_key};
use anyhow::Result;
use models::LiteratureNote;
use rusqlite::params;

#[derive(Clone, Debug)]
pub struct LocalDirtyRecord {
    pub entity: SyncEntityType,
    pub key: SyncEntityKey,
    pub expected_version: i32,
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
                    let expected_version =
                        self.get_synced_version($entity, &key)?.unwrap_or(0) as i32;
                    out.push(LocalDirtyRecord {
                        entity: $entity,
                        key,
                        expected_version,
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
            let expected_version = self
                .get_synced_version(SyncEntityType::Annotation, &key)?
                .unwrap_or(0) as i32;
            out.push(LocalDirtyRecord {
                entity: SyncEntityType::Annotation,
                key,
                expected_version,
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
            let expected_version = self
                .get_synced_version(SyncEntityType::LiteratureAuthor, &key)?
                .unwrap_or(0) as i32;
            out.push(LocalDirtyRecord {
                entity: SyncEntityType::LiteratureAuthor,
                key,
                expected_version,
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
            let expected_version = self
                .get_synced_version(SyncEntityType::LiteratureFolder, &key)?
                .unwrap_or(0) as i32;
            out.push(LocalDirtyRecord {
                entity: SyncEntityType::LiteratureFolder,
                key,
                expected_version,
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
            let expected_version = self
                .get_synced_version(SyncEntityType::LiteratureTag, &key)?
                .unwrap_or(0) as i32;
            out.push(LocalDirtyRecord {
                entity: SyncEntityType::LiteratureTag,
                key,
                expected_version,
                payload,
            });
        }
        // Citations are collected above; order relations after their entities.
        Ok(out)
    }

    /// Atomically confirms only synchronization metadata; business columns are untouched.
    pub fn confirm_upload(
        &self,
        entity: SyncEntityType,
        key: &SyncEntityKey,
        remote_version: i32,
    ) -> Result<bool> {
        Ok(self.with_conn(|conn| {
            let changed = match (entity, key) {
                (SyncEntityType::Literature, SyncEntityKey::Id(id)) => conn.execute("UPDATE literatures SET is_dirty=0, synced_version=?1 WHERE id=?2", params![remote_version, id])?,
                (SyncEntityType::Publication, SyncEntityKey::Id(id)) => conn.execute("UPDATE publications SET is_dirty=0, synced_version=?1 WHERE id=?2", params![remote_version, id])?,
                (SyncEntityType::Author, SyncEntityKey::Id(id)) => conn.execute("UPDATE authors SET is_dirty=0, synced_version=?1 WHERE id=?2", params![remote_version, id])?,
                (SyncEntityType::Folder, SyncEntityKey::Id(id)) => conn.execute("UPDATE folders SET is_dirty=0, synced_version=?1 WHERE id=?2", params![remote_version, id])?,
                (SyncEntityType::Tag, SyncEntityKey::Id(id)) => conn.execute("UPDATE tags SET is_dirty=0, synced_version=?1 WHERE id=?2", params![remote_version, id])?,
                (SyncEntityType::Attachment, SyncEntityKey::Id(id)) => conn.execute("UPDATE attachments SET is_dirty=0, synced_version=?1 WHERE id=?2", params![remote_version, id])?,
                (SyncEntityType::Feed, SyncEntityKey::Id(id)) => conn.execute("UPDATE feeds SET is_dirty=0, synced_version=?1 WHERE id=?2", params![remote_version, id])?,
                (SyncEntityType::FeedItem, SyncEntityKey::Id(id)) => conn.execute("UPDATE feed_items SET is_dirty=0, synced_version=?1 WHERE id=?2", params![remote_version, id])?,
                (SyncEntityType::Annotation, SyncEntityKey::Id(id)) => conn.execute("UPDATE annotations SET is_dirty=0, synced_version=?1 WHERE id=?2", params![remote_version, id])?,
                (SyncEntityType::LiteratureNote, SyncEntityKey::Id(id)) => conn.execute("UPDATE literature_notes SET is_dirty=0, synced_version=?1 WHERE id=?2", params![remote_version, id])?,
                (SyncEntityType::LiteratureAuthor, SyncEntityKey::Relation{left,right}) => conn.execute("UPDATE literature_authors SET is_dirty=0, synced_version=?1 WHERE literature_id=?2 AND author_id=?3", params![remote_version,left,right])?,
                (SyncEntityType::LiteratureFolder, SyncEntityKey::Relation{left,right}) => conn.execute("UPDATE literature_folders SET is_dirty=0, synced_version=?1 WHERE literature_id=?2 AND folder_id=?3", params![remote_version,left,right])?,
                (SyncEntityType::LiteratureTag, SyncEntityKey::Relation{left,right}) => conn.execute("UPDATE literature_tags SET is_dirty=0, synced_version=?1 WHERE literature_id=?2 AND tag_id=?3", params![remote_version,left,right])?,
                (SyncEntityType::Citation, SyncEntityKey::Relation{left,right}) => conn.execute("UPDATE literature_citations SET is_dirty=0, synced_version=?1 WHERE source_id=?2 AND target_id=?3", params![remote_version,left,right])?,
                _ => 0,
            };
            Ok(changed > 0)
        })?)
    }
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
        assert!(
            db.confirm_upload(SyncEntityType::LiteratureAuthor, &relation.key, 2)
                .unwrap()
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
}
