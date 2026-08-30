//! 组合根（Composition Root）
//!
//! 持有并组合所有服务实例（library / connector / analysis / sync / config 等），
//! 负责应用生命周期与"是否启动云同步"的唯一开关。
//!
//! `FetchSource` / `AdvancedSearchQuery` / `SearchField` 已下沉 `models`，
//! `SearchEngine` 已下沉 `services::query`，`services -> ui` 循环已断开，
//! 本模块完全不依赖 `gpui`（仅通过 `notify::RefreshMsg` 纯数据枚举上行通信）。

mod main_app;

pub use crate::library::{BatchRenameFailure, BatchRenameFailureKind};
pub use main_app::{BatchRenameSaveError, BatchRenameSummary, MainApp};
