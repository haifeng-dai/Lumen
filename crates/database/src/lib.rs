//! 数据库层
//!
//! 本地 SQLite CRUD + MySQL 远程同步。

pub mod mysql;
pub mod sqlite;
pub mod state;
pub mod sync_download;
pub mod sync_merge;
pub mod sync_upload;

pub use mysql::MySqlManager;
pub use mysql::{
    RelationAuthor, RelationFolder, RelationTag, RemoteAnnotationPayload, RemoteLibraryInfo,
    SyncEntityPayload, VersionedWriteResult,
};
pub use sqlite::{
    Database, DatabaseSyncSummary, LocalSyncState, SyncConflict, SyncEntityKey, SyncEntityType,
    canonical_key, decode_relation_key, object_key_from_attachment_id,
};
pub use sync_download::{RemoteReadBatch, RemoteRecord};
pub use sync_merge::MergeOutcome;
pub use sync_upload::LocalDirtyRecord;
