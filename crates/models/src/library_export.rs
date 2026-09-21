//! 全库导出包 DTO（format_version = 1）
//!
//! 仅描述导出侧逻辑 JSON 形态；无 IO、无 database 依赖。
//! 导入/恢复不在本模块范围。

use serde::{Deserialize, Serialize};

use crate::{
    AnnotationColor, AnnotationKind, FeedType, LiteratureType, PublicationType, ReadingStatus,
    TextRange,
};

/// 导出包格式标识
pub const LIBRARY_EXPORT_FORMAT: &str = "lumen-library-export";
/// 当前导出包 schema 版本
pub const LIBRARY_EXPORT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportManifest {
    pub format: String,
    pub format_version: u32,
    pub exported_at: i64,
    pub app_version: String,
    /// 相对用户所选父目录的包根名，例如 `Lumen-Library-20260921-143022`
    pub package_root: String,
    pub soft_deleted_included: bool,
    pub attachment_layout: String,
    pub attachment_file_rename: bool,
    pub import_supported: bool,
    pub counts: ExportCounts,
}

impl ExportManifest {
    pub fn new(
        exported_at: i64,
        app_version: impl Into<String>,
        package_root: impl Into<String>,
        counts: ExportCounts,
    ) -> Self {
        Self {
            format: LIBRARY_EXPORT_FORMAT.to_string(),
            format_version: LIBRARY_EXPORT_FORMAT_VERSION,
            exported_at,
            app_version: app_version.into(),
            package_root: package_root.into(),
            soft_deleted_included: false,
            attachment_layout: "per_literature".to_string(),
            attachment_file_rename: false,
            import_supported: false,
            counts,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportCounts {
    pub literatures: usize,
    pub authors: usize,
    pub publications: usize,
    pub tags: usize,
    pub folders: usize,
    pub attachments: usize,
    pub attachments_skipped_missing_file: usize,
    pub annotations: usize,
    pub annotations_skipped_orphan: usize,
    pub literature_notes: usize,
    pub citations: usize,
    pub feeds: usize,
    pub feed_items: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiteratureExportRow {
    pub id: String,
    pub title: String,
    pub year: Option<i32>,
    pub month: Option<i32>,
    pub day: Option<i32>,
    #[serde(rename = "type")]
    pub literature_type: LiteratureType,
    pub publication_id: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub abstract_text: Option<String>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub url: Option<String>,
    pub rating: i32,
    pub reading_status: ReadingStatus,
    pub is_dirty: bool,
    pub version: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicationExportRow {
    pub id: String,
    pub name: String,
    pub publication_type: PublicationType,
    pub abbreviation: Option<String>,
    pub publisher: Option<String>,
    pub ccf_rank: Option<String>,
    pub jcr_rank: Option<String>,
    pub cas_rank: Option<String>,
    pub is_dirty: bool,
    pub version: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorExportRow {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub middle_name: Option<String>,
    pub is_dirty: bool,
    pub version: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiteratureAuthorExportRow {
    pub literature_id: String,
    pub author_id: String,
    pub sort_order: i32,
    pub is_dirty: bool,
    pub version: i32,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderExportRow {
    pub id: String,
    pub name: String,
    pub folder_type: String,
    pub parent_id: Option<String>,
    pub is_dirty: bool,
    pub version: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiteratureFolderExportRow {
    pub literature_id: String,
    pub folder_id: String,
    pub is_dirty: bool,
    pub version: i32,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagExportRow {
    pub id: String,
    pub name: String,
    pub color: String,
    pub is_dirty: bool,
    pub version: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiteratureTagExportRow {
    pub literature_id: String,
    pub tag_id: String,
    pub is_dirty: bool,
    pub version: i32,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentExportRow {
    pub id: String,
    pub literature_id: String,
    pub file_name: String,
    pub export_relative_path: String,
    pub file_size: u64,
    pub mime_type: Option<String>,
    pub etag: Option<String>,
    pub hash: Option<String>,
    pub is_main: bool,
    pub is_dirty: bool,
    pub version: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationExportRow {
    pub id: String,
    pub document_id: String,
    pub page: u16,
    pub kind: AnnotationKind,
    pub color: AnnotationColor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<TextRange>,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiteratureNoteExportRow {
    pub id: String,
    pub literature_id: String,
    pub title: String,
    pub content: String,
    pub sort_order: i32,
    pub is_dirty: bool,
    pub version: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitationExportRow {
    pub source_id: String,
    pub target_id: String,
    pub is_dirty: bool,
    pub version: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedExportRow {
    pub id: String,
    pub name: String,
    pub title: Option<String>,
    pub feed_type: FeedType,
    pub url: Option<String>,
    pub last_updated_at: Option<String>,
    pub update_interval: i32,
    pub is_dirty: bool,
    pub version: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedItemExportRow {
    pub id: String,
    pub title: String,
    pub feed_id: String,
    pub is_read: bool,
    pub is_added_to_library: bool,
    pub added_at: String,
    pub authors: Vec<crate::AuthorExportAuthor>,
    pub year: Option<i32>,
    #[serde(rename = "type")]
    pub literature_type: LiteratureType,
    pub journal: Option<String>,
    pub publisher: Option<String>,
    pub abstract_text: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub published_at: Option<String>,
    pub is_dirty: bool,
    pub version: i32,
    pub updated_at: i64,
}

/// 订阅条目作者（与 `models::Author` 兼容的导出视图）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorExportAuthor {
    pub last_name: String,
    pub first_name: String,
    pub middle_name: Option<String>,
}

impl From<&crate::Author> for AuthorExportAuthor {
    fn from(a: &crate::Author) -> Self {
        Self {
            last_name: a.last_name.clone(),
            first_name: a.first_name.clone(),
            middle_name: a.middle_name.clone(),
        }
    }
}

/// database 层组装的原始快照（附件文件是否存在的判定不在本层）
#[derive(Debug, Clone, Default)]
pub struct LibraryExportSnapshot {
    pub literatures: Vec<LiteratureExportRow>,
    pub publications: Vec<PublicationExportRow>,
    pub authors: Vec<AuthorExportRow>,
    pub literature_authors: Vec<LiteratureAuthorExportRow>,
    pub folders: Vec<FolderExportRow>,
    pub literature_folders: Vec<LiteratureFolderExportRow>,
    pub tags: Vec<TagExportRow>,
    pub literature_tags: Vec<LiteratureTagExportRow>,
    /// active 附件（含绝对 `file_path`，供 services 探测）
    pub attachments_raw: Vec<AttachmentExportRaw>,
    pub annotations_raw: Vec<AnnotationExportRaw>,
    pub literature_notes: Vec<LiteratureNoteExportRow>,
    pub citations: Vec<CitationExportRow>,
    pub feeds: Vec<FeedExportRow>,
    pub feed_items: Vec<FeedItemExportRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentExportRaw {
    pub id: String,
    pub literature_id: String,
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
    pub mime_type: Option<String>,
    pub etag: Option<String>,
    pub hash: Option<String>,
    pub is_main: bool,
    pub is_dirty: bool,
    pub version: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationExportRaw {
    pub id: String,
    pub document_id: String,
    pub page: u16,
    pub kind: AnnotationKind,
    pub color: AnnotationColor,
    pub range: Option<TextRange>,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: i32,
}

/// 最终写入磁盘的业务数据包
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryExportBundle {
    pub literatures: Vec<LiteratureExportRow>,
    pub publications: Vec<PublicationExportRow>,
    pub authors: Vec<AuthorExportRow>,
    pub literature_authors: Vec<LiteratureAuthorExportRow>,
    pub folders: Vec<FolderExportRow>,
    pub literature_folders: Vec<LiteratureFolderExportRow>,
    pub tags: Vec<TagExportRow>,
    pub literature_tags: Vec<LiteratureTagExportRow>,
    pub attachments: Vec<AttachmentExportRow>,
    pub annotations: Vec<AnnotationExportRow>,
    pub literature_notes: Vec<LiteratureNoteExportRow>,
    pub citations: Vec<CitationExportRow>,
    pub feeds: Vec<FeedExportRow>,
    pub feed_items: Vec<FeedItemExportRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryExportReport {
    pub dest_dir: String,
    pub counts: ExportCounts,
}
