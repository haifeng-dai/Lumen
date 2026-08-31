//! 原子化的数据库同步下载数据结构与 SQLite 批处理原语。
//! 远端记录以 JSON 承载，实体表和列集合仍由本模块的静态映射限定。

use rusqlite::{OptionalExtension, Transaction, params, types::Value};
use serde_json::Value as JsonValue;

use crate::{Database, SyncEntityKey, SyncEntityType, canonical_key};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteRecord {
    pub entity_type: SyncEntityType,
    pub version: i64,
    pub payload: JsonValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RemoteReadBatch {
    pub records: Vec<RemoteRecord>,
    pub last_sequence: i64,
}

impl RemoteRecord {
    pub fn key(&self) -> Option<SyncEntityKey> {
        let entity = self.entity_type;
        let object = self.payload.as_object()?;
        let (_, keys, _) = columns(entity);
        if keys.len() == 1 {
            Some(SyncEntityKey::Id(
                object.get(keys[0])?.as_str()?.to_string(),
            ))
        } else {
            Some(SyncEntityKey::Relation {
                left: object.get(keys[0])?.as_str()?.to_string(),
                right: object.get(keys[1])?.as_str()?.to_string(),
            })
        }
    }

    pub fn canonical_key(&self) -> Option<String> {
        Some(canonical_key(&self.key()?))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemoteRecordValidationError {
    pub entity_type: SyncEntityType,
    pub batch_index: usize,
    pub reason: &'static str,
    pub kind_encoding: Option<&'static str>,
    pub rect_presence: Option<&'static str>,
    pub field: Option<&'static str>,
}

impl std::fmt::Display for RemoteRecordValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid_remote_record entity_type={} batch_index={} reason={}",
            self.entity_type.as_str(),
            self.batch_index,
            self.reason
        )?;
        if let Some(kind) = self.kind_encoding {
            write!(f, " kind_encoding={kind}")?;
        }
        if let Some(rect) = self.rect_presence {
            write!(f, " rect_presence={rect}")?;
        }
        if let Some(field) = self.field {
            write!(f, " field={field}")?;
        }
        Ok(())
    }
}

impl std::error::Error for RemoteRecordValidationError {}

impl From<RemoteRecordValidationError> for rusqlite::Error {
    fn from(err: RemoteRecordValidationError) -> Self {
        rusqlite::Error::ToSqlConversionFailure(Box::new(err))
    }
}

pub(crate) fn normalize_annotation_payload(
    payload: &JsonValue,
    batch_index: usize,
) -> Result<(crate::RemoteAnnotationPayload, JsonValue), RemoteRecordValidationError> {
    let obj = payload
        .as_object()
        .ok_or_else(|| RemoteRecordValidationError {
            entity_type: SyncEntityType::Annotation,
            batch_index,
            reason: "invalid_payload_shape",
            kind_encoding: None,
            rect_presence: None,
            field: None,
        })?;

    let make_err = |reason: &'static str,
                    field: Option<&'static str>,
                    kind_encoding: Option<&'static str>,
                    rect_presence: Option<&'static str>| {
        RemoteRecordValidationError {
            entity_type: SyncEntityType::Annotation,
            batch_index,
            reason,
            kind_encoding,
            rect_presence,
            field,
        }
    };

    let id = match obj.get("id") {
        Some(JsonValue::String(s)) => s.clone(),
        None => return Err(make_err("missing_required_field", Some("id"), None, None)),
        Some(_) => return Err(make_err("invalid_field_type", Some("id"), None, None)),
    };

    let document_id = match obj.get("document_id") {
        Some(JsonValue::String(s)) => s.clone(),
        None => {
            return Err(make_err(
                "missing_required_field",
                Some("document_id"),
                None,
                None,
            ));
        }
        Some(_) => {
            return Err(make_err(
                "invalid_field_type",
                Some("document_id"),
                None,
                None,
            ));
        }
    };

    let page = match obj.get("page") {
        Some(JsonValue::Number(num)) => {
            let n = num
                .as_i64()
                .ok_or_else(|| make_err("invalid_field_type", Some("page"), None, None))?;
            u16::try_from(n)
                .map_err(|_| make_err("invalid_field_type", Some("page"), None, None))?
        }
        None => return Err(make_err("missing_required_field", Some("page"), None, None)),
        Some(_) => return Err(make_err("invalid_field_type", Some("page"), None, None)),
    };

    let is_deleted = match obj.get("is_deleted") {
        Some(JsonValue::Bool(b)) => *b,
        Some(JsonValue::Number(num)) => match num.as_i64() {
            Some(0) => false,
            Some(1) => true,
            _ => {
                return Err(make_err(
                    "invalid_field_type",
                    Some("is_deleted"),
                    None,
                    None,
                ));
            }
        },
        None => {
            return Err(make_err(
                "missing_required_field",
                Some("is_deleted"),
                None,
                None,
            ));
        }
        Some(_) => {
            return Err(make_err(
                "invalid_field_type",
                Some("is_deleted"),
                None,
                None,
            ));
        }
    };

    let version = match obj.get("version") {
        Some(JsonValue::Number(num)) => {
            let n = num
                .as_i64()
                .ok_or_else(|| make_err("invalid_field_type", Some("version"), None, None))?;
            i32::try_from(n)
                .map_err(|_| make_err("invalid_field_type", Some("version"), None, None))?
        }
        None => {
            return Err(make_err(
                "missing_required_field",
                Some("version"),
                None,
                None,
            ));
        }
        Some(_) => return Err(make_err("invalid_field_type", Some("version"), None, None)),
    };

    let created_at = match obj.get("created_at") {
        Some(JsonValue::Number(num)) => num
            .as_i64()
            .ok_or_else(|| make_err("invalid_field_type", Some("created_at"), None, None))?,
        None => {
            return Err(make_err(
                "missing_required_field",
                Some("created_at"),
                None,
                None,
            ));
        }
        Some(_) => {
            return Err(make_err(
                "invalid_field_type",
                Some("created_at"),
                None,
                None,
            ));
        }
    };

    let updated_at = match obj.get("updated_at") {
        Some(JsonValue::Number(num)) => num
            .as_i64()
            .ok_or_else(|| make_err("invalid_field_type", Some("updated_at"), None, None))?,
        None => {
            return Err(make_err(
                "missing_required_field",
                Some("updated_at"),
                None,
                None,
            ));
        }
        Some(_) => {
            return Err(make_err(
                "invalid_field_type",
                Some("updated_at"),
                None,
                None,
            ));
        }
    };

    let color_str = match obj.get("color") {
        Some(JsonValue::String(s)) => s.as_str(),
        None => {
            return Err(make_err(
                "missing_required_field",
                Some("color"),
                None,
                None,
            ));
        }
        Some(_) => return Err(make_err("invalid_field_type", Some("color"), None, None)),
    };

    let color = match color_str {
        "Yellow" | "Red" | "Green" | "Blue" | "Purple" | "Magenta" | "Orange" | "Gray" => {
            color_str.to_string()
        }
        _ => {
            return Err(make_err(
                "unknown_annotation_color",
                Some("color"),
                None,
                None,
            ));
        }
    };

    let range = match obj.get("range") {
        None | Some(JsonValue::Null) => None,
        Some(JsonValue::String(s)) => {
            if serde_json::from_str::<models::TextRange>(s).is_err() {
                return Err(make_err("invalid_range_json", Some("range"), None, None));
            }
            Some(s.clone())
        }
        Some(_) => return Err(make_err("invalid_range_json", Some("range"), None, None)),
    };

    let note = match obj.get("note") {
        None | Some(JsonValue::Null) => None,
        Some(JsonValue::String(s)) => Some(s.clone()),
        Some(_) => return Err(make_err("invalid_field_type", Some("note"), None, None)),
    };

    let parse_coord = |key: &'static str| -> Result<Option<f32>, RemoteRecordValidationError> {
        match obj.get(key) {
            None | Some(JsonValue::Null) => Ok(None),
            Some(JsonValue::Number(num)) => {
                let f = num
                    .as_f64()
                    .ok_or_else(|| make_err("invalid_coordinate_type", Some(key), None, None))?
                    as f32;
                if f.is_nan() || f.is_infinite() {
                    return Err(make_err("invalid_coordinate_type", Some(key), None, None));
                }
                Ok(Some(f))
            }
            _ => Err(make_err("invalid_coordinate_type", Some(key), None, None)),
        }
    };

    let flat_x = parse_coord("rect_x")?;
    let flat_y = parse_coord("rect_y")?;
    let flat_w = parse_coord("rect_w")?;
    let flat_h = parse_coord("rect_h")?;

    let rect_presence = match (flat_x, flat_y, flat_w, flat_h) {
        (Some(_), Some(_), Some(_), Some(_)) => "all_present",
        (None, None, None, None) => "all_null",
        _ => "partial",
    };

    let kind_val = obj.get("kind").ok_or_else(|| {
        make_err(
            "missing_required_field",
            Some("kind"),
            None,
            Some(rect_presence),
        )
    })?;

    let (canonical_kind, out_x, out_y, out_w, out_h) = match kind_val {
        JsonValue::String(s) => match s.as_str() {
            "Highlight" | "Underline" => {
                if rect_presence != "all_null" {
                    return Err(make_err(
                        "non_rectangle_has_coordinates",
                        None,
                        Some("canonical_string"),
                        Some(rect_presence),
                    ));
                }
                (s.clone(), None, None, None, None)
            }
            "Rectangle" => {
                if rect_presence != "all_present" {
                    return Err(make_err(
                        "missing_rectangle_coordinate",
                        None,
                        Some("canonical_string"),
                        Some(rect_presence),
                    ));
                }
                ("Rectangle".to_string(), flat_x, flat_y, flat_w, flat_h)
            }
            other => {
                if let Ok(parsed_json) = serde_json::from_str::<JsonValue>(other) {
                    if let Some(map) = parsed_json.as_object() {
                        decode_legacy_kind_object(
                            map,
                            flat_x,
                            flat_y,
                            flat_w,
                            flat_h,
                            rect_presence,
                            make_err,
                        )?
                    } else if let Some(unquoted) = parsed_json.as_str() {
                        match unquoted {
                            "Highlight" | "Underline" => {
                                if rect_presence != "all_null" {
                                    return Err(make_err(
                                        "non_rectangle_has_coordinates",
                                        None,
                                        Some("canonical_string"),
                                        Some(rect_presence),
                                    ));
                                }
                                (unquoted.to_string(), None, None, None, None)
                            }
                            "Rectangle" => {
                                if rect_presence != "all_present" {
                                    return Err(make_err(
                                        "missing_rectangle_coordinate",
                                        None,
                                        Some("canonical_string"),
                                        Some(rect_presence),
                                    ));
                                }
                                ("Rectangle".to_string(), flat_x, flat_y, flat_w, flat_h)
                            }
                            _ => {
                                return Err(make_err(
                                    "unknown_annotation_kind",
                                    Some("kind"),
                                    Some("unknown"),
                                    Some(rect_presence),
                                ));
                            }
                        }
                    } else {
                        return Err(make_err(
                            "unknown_annotation_kind",
                            Some("kind"),
                            Some("unknown"),
                            Some(rect_presence),
                        ));
                    }
                } else {
                    return Err(make_err(
                        "unknown_annotation_kind",
                        Some("kind"),
                        Some("unknown"),
                        Some(rect_presence),
                    ));
                }
            }
        },
        JsonValue::Object(map) => {
            decode_legacy_kind_object(map, flat_x, flat_y, flat_w, flat_h, rect_presence, make_err)?
        }
        _ => {
            return Err(make_err(
                "unsupported_legacy_annotation_encoding",
                Some("kind"),
                Some("unknown"),
                Some(rect_presence),
            ));
        }
    };

    let payload = crate::RemoteAnnotationPayload {
        id: id.clone(),
        document_id: document_id.clone(),
        page,
        kind: canonical_kind.clone(),
        color: color.clone(),
        range: range.clone(),
        note: note.clone(),
        rect_x: out_x,
        rect_y: out_y,
        rect_w: out_w,
        rect_h: out_h,
        is_deleted,
        version,
        created_at,
        updated_at,
    };

    let mut canonical_json = serde_json::Map::new();
    canonical_json.insert("id".into(), JsonValue::String(id));
    canonical_json.insert("document_id".into(), JsonValue::String(document_id));
    canonical_json.insert("page".into(), JsonValue::from(page));
    canonical_json.insert("kind".into(), JsonValue::String(canonical_kind));
    canonical_json.insert("color".into(), JsonValue::String(color));
    canonical_json.insert(
        "range".into(),
        range.map(JsonValue::String).unwrap_or(JsonValue::Null),
    );
    canonical_json.insert(
        "note".into(),
        note.map(JsonValue::String).unwrap_or(JsonValue::Null),
    );
    canonical_json.insert(
        "rect_x".into(),
        out_x.map(JsonValue::from).unwrap_or(JsonValue::Null),
    );
    canonical_json.insert(
        "rect_y".into(),
        out_y.map(JsonValue::from).unwrap_or(JsonValue::Null),
    );
    canonical_json.insert(
        "rect_w".into(),
        out_w.map(JsonValue::from).unwrap_or(JsonValue::Null),
    );
    canonical_json.insert(
        "rect_h".into(),
        out_h.map(JsonValue::from).unwrap_or(JsonValue::Null),
    );
    canonical_json.insert("is_deleted".into(), JsonValue::Bool(is_deleted));
    canonical_json.insert("version".into(), JsonValue::from(version));
    canonical_json.insert("created_at".into(), JsonValue::from(created_at));
    canonical_json.insert("updated_at".into(), JsonValue::from(updated_at));

    Ok((payload, JsonValue::Object(canonical_json)))
}

fn decode_legacy_kind_object<F>(
    map: &serde_json::Map<String, JsonValue>,
    flat_x: Option<f32>,
    flat_y: Option<f32>,
    flat_w: Option<f32>,
    flat_h: Option<f32>,
    rect_presence: &'static str,
    make_err: F,
) -> Result<(String, Option<f32>, Option<f32>, Option<f32>, Option<f32>), RemoteRecordValidationError>
where
    F: Fn(
        &'static str,
        Option<&'static str>,
        Option<&'static str>,
        Option<&'static str>,
    ) -> RemoteRecordValidationError,
{
    if map.len() != 1 {
        return Err(make_err(
            "unsupported_legacy_annotation_encoding",
            Some("kind"),
            Some("legacy_object"),
            Some(rect_presence),
        ));
    }

    if let Some((k, v)) = map.iter().next() {
        match k.as_str() {
            "Highlight" | "Underline" => {
                if rect_presence != "all_null" {
                    return Err(make_err(
                        "non_rectangle_has_coordinates",
                        None,
                        Some("legacy_object"),
                        Some(rect_presence),
                    ));
                }
                Ok((k.clone(), None, None, None, None))
            }
            "Rectangle" => {
                let rect_obj = v.as_object().ok_or_else(|| {
                    make_err(
                        "unsupported_legacy_annotation_encoding",
                        Some("kind"),
                        Some("legacy_object"),
                        Some(rect_presence),
                    )
                })?;
                let parse_nested =
                    |f_name: &'static str| -> Result<f32, RemoteRecordValidationError> {
                        let num = rect_obj
                            .get(f_name)
                            .and_then(JsonValue::as_f64)
                            .ok_or_else(|| {
                                make_err(
                                    "missing_rectangle_coordinate",
                                    Some(f_name),
                                    Some("legacy_object"),
                                    Some(rect_presence),
                                )
                            })? as f32;
                        if num.is_nan() || num.is_infinite() {
                            return Err(make_err(
                                "invalid_coordinate_type",
                                Some(f_name),
                                Some("legacy_object"),
                                Some(rect_presence),
                            ));
                        }
                        Ok(num)
                    };
                let nx = parse_nested("x")?;
                let ny = parse_nested("y")?;
                let nw = parse_nested("w")?;
                let nh = parse_nested("h")?;

                match rect_presence {
                    "all_null" => Ok((
                        "Rectangle".to_string(),
                        Some(nx),
                        Some(ny),
                        Some(nw),
                        Some(nh),
                    )),
                    "all_present" => {
                        let px = flat_x.unwrap();
                        let py = flat_y.unwrap();
                        let pw = flat_w.unwrap();
                        let ph = flat_h.unwrap();
                        let coords_match = (px - nx).abs() < 1e-5
                            && (py - ny).abs() < 1e-5
                            && (pw - nw).abs() < 1e-5
                            && (ph - nh).abs() < 1e-5;
                        if !coords_match {
                            return Err(make_err(
                                "legacy_rectangle_coordinate_mismatch",
                                None,
                                Some("legacy_object"),
                                Some("all_present"),
                            ));
                        }
                        Ok((
                            "Rectangle".to_string(),
                            Some(px),
                            Some(py),
                            Some(pw),
                            Some(ph),
                        ))
                    }
                    _ => Err(make_err(
                        "missing_rectangle_coordinate",
                        None,
                        Some("legacy_object"),
                        Some("partial"),
                    )),
                }
            }
            _ => Err(make_err(
                "unsupported_legacy_annotation_encoding",
                Some("kind"),
                Some("legacy_object"),
                Some(rect_presence),
            )),
        }
    } else {
        Err(make_err(
            "unsupported_legacy_annotation_encoding",
            Some("kind"),
            Some("legacy_object"),
            Some(rect_presence),
        ))
    }
}

/// 在单个事务中应用所有记录并推进 sequence；任一记录失败时整体回滚。
pub fn apply_remote_records(
    tx: &Transaction<'_>,
    records: &[RemoteRecord],
) -> rusqlite::Result<usize> {
    let mut count = 0;
    for (batch_index, record) in records.iter().enumerate() {
        let entity = record.entity_type;
        let (table, keys, columns) = columns(entity);
        let _object = record
            .payload
            .as_object()
            .ok_or_else(|| RemoteRecordValidationError {
                entity_type: entity,
                batch_index,
                reason: "invalid_payload_shape",
                kind_encoding: None,
                rect_presence: None,
                field: None,
            })?;

        let canonical_payload = if entity == SyncEntityType::Annotation {
            let (_, canonical_json) = normalize_annotation_payload(&record.payload, batch_index)?;
            canonical_json
        } else {
            record.payload.clone()
        };

        let canon_obj =
            canonical_payload
                .as_object()
                .ok_or_else(|| RemoteRecordValidationError {
                    entity_type: entity,
                    batch_index,
                    reason: "invalid_payload_shape",
                    kind_encoding: None,
                    rect_presence: None,
                    field: None,
                })?;

        for column in columns {
            if !canon_obj.contains_key(*column) {
                return Err(RemoteRecordValidationError {
                    entity_type: entity,
                    batch_index,
                    reason: "missing_required_field",
                    kind_encoding: None,
                    rect_presence: None,
                    field: Some(*column),
                }
                .into());
            }
        }

        let mut names = columns.to_vec();
        names.extend(["is_dirty", "synced_version"]);
        let placeholders = (1..=names.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>();
        let values = names
            .iter()
            .map(|name| match *name {
                "is_dirty" => Value::Integer(0),
                "synced_version" => Value::Integer(record.version),
                _ => json_value(canon_obj.get(*name)),
            })
            .collect::<Vec<_>>();
        let updates = names
            .iter()
            .filter(|name| !keys.contains(name))
            .map(|name| format!("`{name}` = excluded.`{name}`"))
            .collect::<Vec<_>>();
        let sql = format!(
            "INSERT INTO `{table}` ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {}",
            names
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(","),
            placeholders.join(","),
            keys.iter()
                .map(|k| format!("`{k}`"))
                .collect::<Vec<_>>()
                .join(","),
            updates.join(",")
        );
        tx.execute(&sql, rusqlite::params_from_iter(values))?;
        count += 1;
    }
    Ok(count)
}

fn columns(
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
    }
}

fn json_value(value: Option<&JsonValue>) -> Value {
    match value {
        None | Some(JsonValue::Null) => Value::Null,
        Some(JsonValue::Bool(v)) => Value::Integer(i64::from(*v)),
        Some(JsonValue::Number(v)) => v
            .as_i64()
            .map(Value::Integer)
            .or_else(|| v.as_f64().map(Value::Real))
            .unwrap_or(Value::Null),
        Some(JsonValue::String(v)) => Value::Text(v.clone()),
        Some(v) => Value::Text(v.to_string()),
    }
}

impl Database {
    pub fn get_download_state(
        &self,
        entity: SyncEntityType,
        key: &SyncEntityKey,
    ) -> rusqlite::Result<Option<(i64, bool)>> {
        self.with_conn(|conn| {
            let (table, keys, values) = match (entity, key) {
                (SyncEntityType::Literature, SyncEntityKey::Id(v)) => {
                    ("literatures", &["id"][..], vec![v.clone()])
                }
                (SyncEntityType::Publication, SyncEntityKey::Id(v)) => {
                    ("publications", &["id"][..], vec![v.clone()])
                }
                (SyncEntityType::Author, SyncEntityKey::Id(v)) => {
                    ("authors", &["id"][..], vec![v.clone()])
                }
                (SyncEntityType::Folder, SyncEntityKey::Id(v)) => {
                    ("folders", &["id"][..], vec![v.clone()])
                }
                (SyncEntityType::Tag, SyncEntityKey::Id(v)) => {
                    ("tags", &["id"][..], vec![v.clone()])
                }
                (SyncEntityType::Attachment, SyncEntityKey::Id(v)) => {
                    ("attachments", &["id"][..], vec![v.clone()])
                }
                (SyncEntityType::Feed, SyncEntityKey::Id(v)) => {
                    ("feeds", &["id"][..], vec![v.clone()])
                }
                (SyncEntityType::FeedItem, SyncEntityKey::Id(v)) => {
                    ("feed_items", &["id"][..], vec![v.clone()])
                }
                (SyncEntityType::Annotation, SyncEntityKey::Id(v)) => {
                    ("annotations", &["id"][..], vec![v.clone()])
                }
                (SyncEntityType::LiteratureNote, SyncEntityKey::Id(v)) => {
                    ("literature_notes", &["id"][..], vec![v.clone()])
                }
                (SyncEntityType::LiteratureAuthor, SyncEntityKey::Relation { left, right }) => (
                    "literature_authors",
                    &["literature_id", "author_id"][..],
                    vec![left.clone(), right.clone()],
                ),
                (SyncEntityType::LiteratureFolder, SyncEntityKey::Relation { left, right }) => (
                    "literature_folders",
                    &["literature_id", "folder_id"][..],
                    vec![left.clone(), right.clone()],
                ),
                (SyncEntityType::LiteratureTag, SyncEntityKey::Relation { left, right }) => (
                    "literature_tags",
                    &["literature_id", "tag_id"][..],
                    vec![left.clone(), right.clone()],
                ),
                (SyncEntityType::Citation, SyncEntityKey::Relation { left, right }) => (
                    "literature_citations",
                    &["source_id", "target_id"][..],
                    vec![left.clone(), right.clone()],
                ),
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            let where_clause = keys
                .iter()
                .enumerate()
                .map(|(i, k)| format!("`{k}` = ?{}", i + 1))
                .collect::<Vec<_>>()
                .join(" AND ");
            let sql =
                format!("SELECT synced_version, is_dirty FROM `{table}` WHERE {where_clause}");
            let params = values.iter().map(String::as_str).collect::<Vec<_>>();
            conn.query_row(&sql, rusqlite::params_from_iter(params), |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
        })
    }

    pub fn apply_remote_download_batch(
        &self,
        last_sequence: i64,
        records: &[RemoteRecord],
    ) -> rusqlite::Result<usize> {
        self.apply_remote_download_batch_with_conflicts(last_sequence, records, &[])
    }

    pub fn apply_remote_download_batch_with_conflicts(
        &self,
        last_sequence: i64,
        records: &[RemoteRecord],
        conflicts: &[crate::SyncConflict],
    ) -> rusqlite::Result<usize> {
        self.apply_remote_download_batch_with_conflicts_and_identity(
            last_sequence,
            records,
            conflicts,
            None,
        )
    }

    /// Apply records, conflicts, sequence and (when supplied) remote identity
    /// in one transaction. A failed batch therefore cannot advance either
    /// the download position or the server binding.
    pub fn apply_remote_download_batch_with_conflicts_and_identity(
        &self,
        last_sequence: i64,
        records: &[RemoteRecord],
        conflicts: &[crate::SyncConflict],
        identity: Option<(&str, &str)>,
    ) -> rusqlite::Result<usize> {
        self.with_transaction(|tx| {
            let count = apply_remote_records(tx, records)?;
            for conflict in conflicts {
                tx.execute("INSERT INTO sync_conflicts (entity_type,entity_id,local_record,remote_record,remote_version,detected_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(entity_type,entity_id) DO UPDATE SET local_record=excluded.local_record, remote_record=excluded.remote_record, remote_version=excluded.remote_version, detected_at=excluded.detected_at", params![conflict.entity_type, conflict.entity_id, conflict.local_record, conflict.remote_record, conflict.remote_version, conflict.detected_at])?;
            }
            tx.execute("INSERT INTO sync_meta (key,value) VALUES ('database_sync_last_sequence',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![last_sequence.to_string()])?;
            if let Some((library_id, fingerprint)) = identity {
                for (key, value) in [
                    ("database_sync_library_id", library_id.to_string()),
                    ("database_sync_remote_fingerprint", fingerprint.to_string()),
                ] {
                    tx.execute("INSERT INTO sync_meta (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value])?;
                }
            }
            Ok(count)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    #[test]
    fn invalid_record_rolls_back_sequence() {
        let db = Database::new(":memory:").unwrap();
        db.set_last_sequence(3).unwrap();
        let record = RemoteRecord {
            entity_type: SyncEntityType::Tag,
            version: 1,
            payload: serde_json::json!({"id":"x"}),
        };
        assert!(db.apply_remote_download_batch(4, &[record]).is_err());
        assert_eq!(db.get_local_sync_state().unwrap().last_sequence, 3);
    }

    #[test]
    fn invalid_identity_batch_rolls_back_sequence_and_fingerprint() {
        let db = Database::new(":memory:").unwrap();
        db.set_local_library_id("old").unwrap();
        db.set_last_sequence(3).unwrap();
        db.set_remote_fingerprint("old-fp").unwrap();
        let record = RemoteRecord {
            entity_type: SyncEntityType::Tag,
            version: 1,
            payload: serde_json::json!({"id":"x"}),
        };
        assert!(
            db.apply_remote_download_batch_with_conflicts_and_identity(
                9,
                &[record],
                &[],
                Some(("new", "new-fp")),
            )
            .is_err()
        );
        let state = db.get_local_sync_state().unwrap();
        assert_eq!(state.library_id.as_deref(), Some("old"));
        assert_eq!(state.last_sequence, 3);
        assert_eq!(state.remote_fingerprint.as_deref(), Some("old-fp"));
    }

    #[test]
    fn canonical_and_legacy_annotations_normalize_and_apply_cleanly() {
        let db = Database::new(":memory:").unwrap();
        let canonical_rect = RemoteRecord {
            entity_type: SyncEntityType::Annotation,
            version: 2,
            payload: serde_json::json!({
                "id": "ann-1",
                "document_id": "doc-1",
                "page": 1,
                "kind": "Rectangle",
                "color": "Yellow",
                "range": null,
                "note": "sample",
                "rect_x": 10.0,
                "rect_y": 20.0,
                "rect_w": 30.0,
                "rect_h": 40.0,
                "is_deleted": false,
                "version": 2,
                "created_at": 100,
                "updated_at": 200
            }),
        };
        let legacy_rect = RemoteRecord {
            entity_type: SyncEntityType::Annotation,
            version: 3,
            payload: serde_json::json!({
                "id": "ann-2",
                "document_id": "doc-1",
                "page": 2,
                "kind": {"Rectangle": {"x": 5.0, "y": 6.0, "w": 7.0, "h": 8.0}},
                "color": "Blue",
                "range": null,
                "note": null,
                "rect_x": null,
                "rect_y": null,
                "rect_w": null,
                "rect_h": null,
                "is_deleted": false,
                "version": 3,
                "created_at": 300,
                "updated_at": 400
            }),
        };
        let applied = db
            .apply_remote_download_batch(10, &[canonical_rect, legacy_rect])
            .unwrap();
        assert_eq!(applied, 2);
        assert_eq!(db.get_local_sync_state().unwrap().last_sequence, 10);
    }

    #[test]
    fn annotation_validation_errors_give_descriptive_reasons() {
        let missing_rect = serde_json::json!({
            "id": "ann-bad-1",
            "document_id": "doc-1",
            "page": 1,
            "kind": "Rectangle",
            "color": "Red",
            "range": null,
            "note": null,
            "rect_x": 1.0,
            "rect_y": null,
            "rect_w": 3.0,
            "rect_h": 4.0,
            "is_deleted": false,
            "version": 1,
            "created_at": 0,
            "updated_at": 0
        });
        let err = normalize_annotation_payload(&missing_rect, 0).unwrap_err();
        assert_eq!(err.reason, "missing_rectangle_coordinate");

        let highlight_with_rect = serde_json::json!({
            "id": "ann-bad-2",
            "document_id": "doc-1",
            "page": 1,
            "kind": "Highlight",
            "color": "Green",
            "range": null,
            "note": null,
            "rect_x": 1.0,
            "rect_y": 2.0,
            "rect_w": 3.0,
            "rect_h": 4.0,
            "is_deleted": false,
            "version": 1,
            "created_at": 0,
            "updated_at": 0
        });
        let err2 = normalize_annotation_payload(&highlight_with_rect, 1).unwrap_err();
        assert_eq!(err2.reason, "non_rectangle_has_coordinates");

        let mismatch_rect = serde_json::json!({
            "id": "ann-bad-3",
            "document_id": "doc-1",
            "page": 1,
            "kind": {"Rectangle": {"x": 1.0, "y": 2.0, "w": 3.0, "h": 4.0}},
            "color": "Purple",
            "range": null,
            "note": null,
            "rect_x": 99.0,
            "rect_y": 2.0,
            "rect_w": 3.0,
            "rect_h": 4.0,
            "is_deleted": false,
            "version": 1,
            "created_at": 0,
            "updated_at": 0
        });
        let err3 = normalize_annotation_payload(&mismatch_rect, 2).unwrap_err();
        assert_eq!(err3.reason, "legacy_rectangle_coordinate_mismatch");
    }

    #[test]
    fn mixed_valid_and_invalid_batch_rolls_back_completely() {
        let db = Database::new(":memory:").unwrap();
        db.set_last_sequence(5).unwrap();
        let valid_tag = RemoteRecord {
            entity_type: SyncEntityType::Tag,
            version: 1,
            payload: serde_json::json!({
                "id": "tag-good",
                "name": "Rust",
                "color": "#fff",
                "is_deleted": false,
                "version": 1,
                "created_at": 0,
                "updated_at": 0
            }),
        };
        let bad_annotation = RemoteRecord {
            entity_type: SyncEntityType::Annotation,
            version: 1,
            payload: serde_json::json!({
                "id": "ann-bad",
                "document_id": "doc-1",
                "page": 1,
                "kind": "UnknownKind",
                "color": "Yellow",
                "range": null,
                "note": null,
                "rect_x": null,
                "rect_y": null,
                "rect_w": null,
                "rect_h": null,
                "is_deleted": false,
                "version": 1,
                "created_at": 0,
                "updated_at": 0
            }),
        };
        let err = db
            .apply_remote_download_batch(6, &[valid_tag, bad_annotation])
            .unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("invalid_remote_record"));
        assert!(err_msg.contains("reason=unknown_annotation_kind"));
        assert_eq!(db.get_local_sync_state().unwrap().last_sequence, 5);
        let tag_exists = db
            .with_conn(|conn| {
                conn.query_row("SELECT id FROM tags WHERE id='tag-good'", [], |_| Ok(()))
                    .optional()
            })
            .unwrap();
        assert!(tag_exists.is_none());
    }

    #[test]
    fn invalid_field_types_and_overflows_are_rejected_without_silent_truncation() {
        let page_overflow = serde_json::json!({
            "id": "ann-overflow-1",
            "document_id": "doc-1",
            "page": 70000,
            "kind": "Highlight",
            "color": "Yellow",
            "range": null,
            "note": null,
            "rect_x": null,
            "rect_y": null,
            "rect_w": null,
            "rect_h": null,
            "is_deleted": false,
            "version": 1,
            "created_at": 0,
            "updated_at": 0
        });
        let err = normalize_annotation_payload(&page_overflow, 0).unwrap_err();
        assert_eq!(err.reason, "invalid_field_type");
        assert_eq!(err.field, Some("page"));

        let version_overflow = serde_json::json!({
            "id": "ann-overflow-2",
            "document_id": "doc-1",
            "page": 1,
            "kind": "Highlight",
            "color": "Yellow",
            "range": null,
            "note": null,
            "rect_x": null,
            "rect_y": null,
            "rect_w": null,
            "rect_h": null,
            "is_deleted": false,
            "version": (i32::MAX as i64) + 10,
            "created_at": 0,
            "updated_at": 0
        });
        let err2 = normalize_annotation_payload(&version_overflow, 0).unwrap_err();
        assert_eq!(err2.reason, "invalid_field_type");
        assert_eq!(err2.field, Some("version"));

        let non_string_note = serde_json::json!({
            "id": "ann-bad-note",
            "document_id": "doc-1",
            "page": 1,
            "kind": "Highlight",
            "color": "Yellow",
            "range": null,
            "note": 12345,
            "rect_x": null,
            "rect_y": null,
            "rect_w": null,
            "rect_h": null,
            "is_deleted": false,
            "version": 1,
            "created_at": 0,
            "updated_at": 0
        });
        let err3 = normalize_annotation_payload(&non_string_note, 0).unwrap_err();
        assert_eq!(err3.reason, "invalid_field_type");
        assert_eq!(err3.field, Some("note"));
    }

    #[test]
    fn is_deleted_accepts_bool_and_zero_one_numbers_and_rejects_others() {
        let base = serde_json::json!({
            "id": "ann-del-test",
            "document_id": "doc-1",
            "page": 1,
            "kind": "Highlight",
            "color": "Yellow",
            "range": null,
            "note": null,
            "rect_x": null,
            "rect_y": null,
            "rect_w": null,
            "rect_h": null,
            "version": 1,
            "created_at": 0,
            "updated_at": 0
        });

        // 1. Bool(true/false) -> true/false
        let mut t1 = base.clone();
        t1["is_deleted"] = serde_json::json!(true);
        let (dto1, _) = normalize_annotation_payload(&t1, 0).unwrap();
        assert!(dto1.is_deleted);

        let mut t2 = base.clone();
        t2["is_deleted"] = serde_json::json!(false);
        let (dto2, _) = normalize_annotation_payload(&t2, 0).unwrap();
        assert!(!dto2.is_deleted);

        // 2. Number(0) -> false, Number(1) -> true
        let mut t3 = base.clone();
        t3["is_deleted"] = serde_json::json!(0);
        let (dto3, _) = normalize_annotation_payload(&t3, 0).unwrap();
        assert!(!dto3.is_deleted);

        let mut t4 = base.clone();
        t4["is_deleted"] = serde_json::json!(1);
        let (dto4, _) = normalize_annotation_payload(&t4, 0).unwrap();
        assert!(dto4.is_deleted);

        // 3. Other Numbers -> invalid_field_type
        let mut t5 = base.clone();
        t5["is_deleted"] = serde_json::json!(2);
        let err5 = normalize_annotation_payload(&t5, 0).unwrap_err();
        assert_eq!(err5.reason, "invalid_field_type");
        assert_eq!(err5.field, Some("is_deleted"));

        let mut t6 = base.clone();
        t6["is_deleted"] = serde_json::json!(-1);
        let err6 = normalize_annotation_payload(&t6, 0).unwrap_err();
        assert_eq!(err6.reason, "invalid_field_type");
        assert_eq!(err6.field, Some("is_deleted"));

        // 4. String("0") or other types -> invalid_field_type
        let mut t7 = base.clone();
        t7["is_deleted"] = serde_json::json!("0");
        let err7 = normalize_annotation_payload(&t7, 0).unwrap_err();
        assert_eq!(err7.reason, "invalid_field_type");
        assert_eq!(err7.field, Some("is_deleted"));
    }
}
