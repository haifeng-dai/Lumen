//! PDF 阅读器持久化（注解）
//!
//! 仅做 DB 编排，收 `&Database` / `Arc<Database>`，不感知 UI / 同步
//! （架构红线）。`notify_data_changed` 等跨域副作用由调用方
//! （`AppPdfDelegate`）负责。

use std::collections::HashSet;

use database::Database;
use models::Annotation;

use crate::library::annotation_document_id_keys;

pub struct PdfPersistence;

impl PdfPersistence {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for PdfPersistence {
    fn default() -> Self {
        Self::new()
    }
}

impl PdfPersistence {
    // ── 注解 ────────────────────────────────────────────

    /// 按 document_id 加载批注。
    ///
    /// 阅读器使用 `lit_id::att_id`；兼容仅存 `att_id` / `lit_id` 的历史与导入数据，
    /// 合并去重后返回。
    pub fn load_annotations(&self, db: &Database, id: &str) -> Vec<Annotation> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for key in annotation_document_id_keys(id) {
            for ann in db.load_annotations(&key).unwrap_or_default() {
                if seen.insert(ann.id.clone()) {
                    out.push(ann);
                }
            }
        }
        out
    }

    pub fn save_annotation(&self, db: &Database, annotation: &Annotation) {
        let _ = db.save_annotation(annotation);
    }

    pub fn delete_annotation(&self, db: &Database, id: &str) {
        let _ = db.delete_annotation(id);
    }
}
