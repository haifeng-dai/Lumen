use std::path::Path;

use anyhow::Result;
use models::library_export::LibraryExportReport;

use super::MainApp;
use crate::library::export_library;

impl MainApp {
    /// 将当前有效文献库导出到 `dest_dir`（逻辑 JSON + 附件目录）。
    ///
    /// 只读导出：不写库、不要求危险操作二次确认。
    pub fn export_library_to(&self, dest_dir: &Path) -> Result<LibraryExportReport> {
        let attachments_dir = self.file_manager.get_attachments_dir();
        export_library(&self.db, &attachments_dir, dest_dir)
    }
}
