use super::Database;
use log::{debug, info};
use models::{Annotation, AnnotationColor, AnnotationKind, TextRange};
use rusqlite::{Result, params};
use serde_json;

impl Database {
    pub fn load_annotations(&self, document_id: &str) -> Result<Vec<Annotation>> {
        debug!("数据库: 正在加载注释 (document_id: {document_id})");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, document_id, page, kind, color, range, note,
                        rect_x, rect_y, rect_w, rect_h,
                        created_at, updated_at, version, is_deleted, is_dirty
                 FROM annotations
                 WHERE document_id = ?1 AND is_deleted = 0",
            )?;

            let rows = stmt.query_map([document_id], |row| {
                let kind_str: String = row.get(3)?;
                let color_str: String = row.get(4)?;
                let range_str: Option<String> = row.get(5)?;

                let rect_x: Option<f32> = row.get(7)?;
                let rect_y: Option<f32> = row.get(8)?;
                let rect_w: Option<f32> = row.get(9)?;
                let rect_h: Option<f32> = row.get(10)?;

                let kind = match kind_str.as_str() {
                    "Highlight" => AnnotationKind::Highlight,
                    "Underline" => AnnotationKind::Underline,
                    "Rectangle" => AnnotationKind::Rectangle {
                        x: rect_x.unwrap_or(0.0),
                        y: rect_y.unwrap_or(0.0),
                        w: rect_w.unwrap_or(0.0),
                        h: rect_h.unwrap_or(0.0),
                    },
                    _ => AnnotationKind::Highlight, // Fallback
                };

                let color = match color_str.as_str() {
                    "Yellow" => AnnotationColor::Yellow,
                    "Red" => AnnotationColor::Red,
                    "Green" => AnnotationColor::Green,
                    "Blue" => AnnotationColor::Blue,
                    "Purple" => AnnotationColor::Purple,
                    "Magenta" => AnnotationColor::Magenta,
                    "Orange" => AnnotationColor::Orange,
                    "Gray" => AnnotationColor::Gray,
                    _ => AnnotationColor::Yellow,
                };

                let range: Option<TextRange> =
                    range_str.and_then(|s| serde_json::from_str(&s).ok());

                Ok(Annotation {
                    id: row.get(0)?,
                    document_id: row.get(1)?,
                    page: row.get(2)?,
                    kind,
                    color,
                    range,
                    note: row.get(6)?,
                    created_at: row.get(11)?,
                    updated_at: row.get(12)?,
                    version: row.get(13)?,
                    is_deleted: row.get::<_, i32>(14)? != 0,
                    is_dirty: row.get::<_, i32>(15)? != 0,
                })
            })?;

            let mut annotations = Vec::new();
            for ann in rows {
                annotations.push(ann?);
            }
            debug!(
                "数据库: 加载了 {} 条注释 (document_id: {document_id})",
                annotations.len()
            );
            Ok(annotations)
        })
    }

    pub fn save_annotation(&self, ann: &Annotation) -> Result<()> {
        debug!(
            "数据库: 保存注释 (ID: {}, document_id: {})",
            ann.id, ann.document_id
        );
        let kind_str = match ann.kind {
            AnnotationKind::Highlight => "Highlight",
            AnnotationKind::Underline => "Underline",
            AnnotationKind::Rectangle { .. } => "Rectangle",
        };

        let color_str = match ann.color {
            AnnotationColor::Yellow => "Yellow",
            AnnotationColor::Red => "Red",
            AnnotationColor::Green => "Green",
            AnnotationColor::Blue => "Blue",
            AnnotationColor::Purple => "Purple",
            AnnotationColor::Magenta => "Magenta",
            AnnotationColor::Orange => "Orange",
            AnnotationColor::Gray => "Gray",
        };

        let range_json = ann
            .range
            .as_ref()
            .and_then(|r| serde_json::to_string(r).ok());

        let (rx, ry, rw, rh) = match ann.kind {
            AnnotationKind::Rectangle { x, y, w, h } => (Some(x), Some(y), Some(w), Some(h)),
            _ => (None, None, None, None),
        };

        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO annotations (
                    id, document_id, page, kind, color, range, note,
                    rect_x, rect_y, rect_w, rect_h,
                    created_at, updated_at, version, is_deleted, is_dirty
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    ann.id,
                    ann.document_id,
                    ann.page,
                    kind_str,
                    color_str,
                    range_json,
                    ann.note,
                    rx,
                    ry,
                    rw,
                    rh,
                    ann.created_at,
                    ann.updated_at,
                    ann.version,
                    if ann.is_deleted { 1 } else { 0 },
                    if ann.is_dirty { 1 } else { 0 },
                ],
            )?;
            Ok(())
        })
    }

    pub fn delete_annotation(&self, id: &str) -> Result<()> {
        debug!("数据库: 删除注释 (ID: {id})");
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE annotations SET is_deleted = 1, is_dirty = 1, updated_at = ?2, version = version + 1 WHERE id = ?1",
                params![id, chrono::Utc::now().timestamp()],
            )?;
            Ok(())
        })
    }

    // --- Sync Support ---

    pub fn get_dirty_annotations(&self) -> Result<Vec<Annotation>> {
        debug!("数据库: 正在获取待同步注释");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, document_id, page, kind, color, range, note,
                        rect_x, rect_y, rect_w, rect_h,
                        created_at, updated_at, version, is_deleted, is_dirty
                 FROM annotations
                 WHERE is_dirty = 1",
            )?;

            let rows = stmt.query_map([], |row| {
                let kind_str: String = row.get(3)?;
                let color_str: String = row.get(4)?;
                let range_str: Option<String> = row.get(5)?;

                let rect_x: Option<f32> = row.get(7)?;
                let rect_y: Option<f32> = row.get(8)?;
                let rect_w: Option<f32> = row.get(9)?;
                let rect_h: Option<f32> = row.get(10)?;

                let kind = match kind_str.as_str() {
                    "Highlight" => AnnotationKind::Highlight,
                    "Underline" => AnnotationKind::Underline,
                    "Rectangle" => AnnotationKind::Rectangle {
                        x: rect_x.unwrap_or(0.0),
                        y: rect_y.unwrap_or(0.0),
                        w: rect_w.unwrap_or(0.0),
                        h: rect_h.unwrap_or(0.0),
                    },
                    _ => AnnotationKind::Highlight,
                };

                let color = match color_str.as_str() {
                    "Yellow" => AnnotationColor::Yellow,
                    "Red" => AnnotationColor::Red,
                    "Green" => AnnotationColor::Green,
                    "Blue" => AnnotationColor::Blue,
                    "Purple" => AnnotationColor::Purple,
                    "Magenta" => AnnotationColor::Magenta,
                    "Orange" => AnnotationColor::Orange,
                    "Gray" => AnnotationColor::Gray,
                    _ => AnnotationColor::Yellow,
                };

                let range: Option<TextRange> =
                    range_str.and_then(|s| serde_json::from_str(&s).ok());

                Ok(Annotation {
                    id: row.get(0)?,
                    document_id: row.get(1)?,
                    page: row.get(2)?,
                    kind,
                    color,
                    range,
                    note: row.get(6)?,
                    created_at: row.get(11)?,
                    updated_at: row.get(12)?,
                    version: row.get(13)?,
                    is_deleted: row.get::<_, i32>(14)? != 0,
                    is_dirty: row.get::<_, i32>(15)? != 0,
                })
            })?;

            let mut annotations = Vec::new();
            for ann in rows {
                annotations.push(ann?);
            }
            debug!("数据库: 获取到 {} 条待同步注释", annotations.len());
            Ok(annotations)
        })
    }

    pub fn mark_annotation_synced(&self, id: &str) -> Result<()> {
        debug!("数据库: 标记注释已同步 (ID: {id})");
        self.with_conn(|conn| {
            conn.execute("UPDATE annotations SET is_dirty = 0 WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// 原子原语：把远程注释盲目 upsert 到本地（覆盖写或插入）。
    pub fn apply_remote_annotation(&self, ann: &Annotation) -> Result<()> {
        let kind_str = match ann.kind {
            AnnotationKind::Highlight => "Highlight",
            AnnotationKind::Underline => "Underline",
            AnnotationKind::Rectangle { .. } => "Rectangle",
        };
        let color_str = match ann.color {
            AnnotationColor::Yellow => "Yellow",
            AnnotationColor::Red => "Red",
            AnnotationColor::Green => "Green",
            AnnotationColor::Blue => "Blue",
            AnnotationColor::Purple => "Purple",
            AnnotationColor::Magenta => "Magenta",
            AnnotationColor::Orange => "Orange",
            AnnotationColor::Gray => "Gray",
        };
        let range_json = ann
            .range
            .as_ref()
            .and_then(|r| serde_json::to_string(r).ok());
        let (rx, ry, rw, rh) = match ann.kind {
            AnnotationKind::Rectangle { x, y, w, h } => (Some(x), Some(y), Some(w), Some(h)),
            _ => (None, None, None, None),
        };
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO annotations (
                    id, document_id, page, kind, color, range, note,
                    rect_x, rect_y, rect_w, rect_h,
                    created_at, updated_at, version, is_deleted, is_dirty
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    ann.id,
                    ann.document_id,
                    ann.page,
                    kind_str,
                    color_str,
                    range_json,
                    ann.note,
                    rx,
                    ry,
                    rw,
                    rh,
                    ann.created_at,
                    ann.updated_at,
                    ann.version,
                    if ann.is_deleted { 1 } else { 0 },
                    0, // merged from remote, clean
                ],
            )?;
            Ok(())
        })
    }

    /// 原子原语：版本一致且本地无修改时，仅刷新时间戳并清脏标记。
    pub fn mark_annotation_up_to_date(&self, ann: &Annotation) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE annotations SET updated_at = ?1, is_dirty = 0 WHERE id = ?2",
                params![ann.updated_at, ann.id],
            )?;
            Ok(())
        })
    }

    pub fn purge_synced_annotations(&self) -> Result<usize> {
        info!("数据库: 正在清理已同步的删除注释记录");
        self.with_conn(|conn| {
            let count = conn.execute(
                "DELETE FROM annotations WHERE is_deleted = 1 AND is_dirty = 0",
                [],
            )?;
            info!("数据库: 已清理 {count} 条已同步删除的注释");
            Ok(count)
        })
    }
}
