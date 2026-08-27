//! 服务层
//!
//! 承载跨 crate 的服务层编排（配置读写、同步/冲突、各功能域等），
//! 供 UI 与启动路径调用。底层 CRUD 由 `database` crate 提供；
//! 文件传输由 `file` crate（原 `sync`）提供。
//!
//! 架构红线：本 crate 不依赖 `gpui`；与 UI 仅通过 `RefreshMsg` 等纯数据枚举上行通信。
//! 依赖严格单向向下，无环。

pub mod analysis;
pub mod app;
pub mod config;
pub mod feed;
pub mod file_monitor;
pub mod library;
pub mod notify;
pub mod pdf;
pub mod query;
pub mod runtime;
pub mod sync;
pub mod theme;
pub mod utils;
