//! 纯数据类型
//!
//! 仅包含 struct/enum 定义 + serde + Display + Default，不含业务逻辑。

pub mod annotation;
pub mod attachment;
pub mod author;
pub mod chat;
pub mod citation;
pub mod config;
pub mod constructors;
pub mod feed;
pub mod fetch;
pub mod folder;
pub mod library_export;
pub mod literature;
pub mod literature_note;
pub mod local_state;
pub mod publication;
pub mod search_query;
pub mod tag;
pub mod theme;
pub mod time;

pub use annotation::{Annotation, AnnotationColor, AnnotationKind, TextRange};
pub use attachment::Attachment;
pub use author::Author;
pub use citation::Citation;
pub use config::{
    AppConfig, DatabaseConfig, GoogleDriveConfig, PdfViewerConfig, ProxyConfig, TranslationConfig,
    UiConfig, WebDavConfig,
};
pub use feed::{Feed, FeedItem, FeedType};
pub use fetch::FetchSource;
pub use folder::{Folder, FolderType};
pub use library_export::{
    AnnotationExportRaw, AnnotationExportRow, AttachmentExportRaw, AttachmentExportRow,
    AuthorExportAuthor, AuthorExportRow, CitationExportRow, ExportCounts, ExportManifest,
    FeedExportRow, FeedItemExportRow, FolderExportRow, ImportCounts, ImportLookupMaps,
    LIBRARY_EXPORT_FORMAT, LIBRARY_EXPORT_FORMAT_VERSION, LibraryExportBundle, LibraryExportReport,
    LibraryExportSnapshot, LibraryImportAttachmentPaths, LibraryImportReport,
    LiteratureAuthorExportRow, LiteratureExportRow, LiteratureFolderExportRow,
    LiteratureNoteExportRow, LiteratureTagExportRow, PreparedAttachmentInsert,
    PreparedLibraryImport, PublicationExportRow, TagExportRow,
};
pub use literature::{Literature, LiteratureType, ReadingStatus};
pub use literature_note::LiteratureNote;
pub use local_state::{AppUiState, PdfState, WindowState};
pub use publication::{Publication, PublicationType};
pub use search_query::{AdvancedSearchQuery, SearchField};
pub use tag::{DEFAULT_TAG_COLOR, DEFAULT_VERSION, TAG_COLORS, Tag};
pub use theme::{ResolvedSurface, SurfaceColors, ThemeColors, ThemeScheme};
pub use time::ts_to_str;

// 构造器（纯 struct 构造，与类型同处 models 层）
pub use constructors::{
    create_attachment, create_author, create_feed, create_feed_item, create_folder,
    create_literature, create_publication, create_tag, create_tag_with_color,
};
