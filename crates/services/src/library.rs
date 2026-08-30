//! 文献库域（library）
//!
//! literature / folder / tag / attachment 四个服务。仅做 DB 编排，
//! 不再收 `&MainApp`——数据库句柄收 `&Database` / `Arc<Database>`，
//! 后台通知与跨域操作（回收站、云端重命名、刷新）通过注入的闭包上抛，
//! 自身不感知 UI / 同步（架构红线）。

mod annotation;
mod attachment;
mod folder;
mod literature;
mod tag;

pub use annotation::PdfPersistence;
pub use attachment::AttachmentService;
pub use folder::FolderService;
pub use literature::{BatchRenameFailure, BatchRenameFailureKind, LiteratureService};
pub use tag::TagService;
