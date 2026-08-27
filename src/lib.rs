//! Lumen — 现代化科研文献管理工具
//!
//! 一个现代化的文献管理器，使用 GPUI 框架构建。

// ============================
// 核心模块
// ============================
pub mod app_state;
pub mod assets;
pub mod pdf;
pub mod ui;

use std::sync::LazyLock;
use tokio::runtime::Runtime;

pub static RUNTIME: LazyLock<Runtime> =
    LazyLock::new(|| Runtime::new().expect("Failed to create Tokio runtime"));
