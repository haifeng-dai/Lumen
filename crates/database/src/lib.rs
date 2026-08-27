//! 数据库层
//!
//! 本地 SQLite CRUD + MySQL 远程同步。

pub mod mysql;
pub mod sqlite;
pub mod state;
pub mod sync_merge;

pub use mysql::MySqlManager;
pub use sqlite::Database;
pub use sync_merge::MergeOutcome;
