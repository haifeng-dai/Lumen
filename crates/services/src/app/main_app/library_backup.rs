use std::path::Path;

use anyhow::Result;
use models::library_export::{LibraryExportReport, LibraryImportReport};

use super::MainApp;
use crate::library::{export_library, import_library};
use crate::notify::RefreshMsg;

impl MainApp {
    /// 将当前有效文献库导出到 `parent_dir` 下的时间戳包根目录。
    pub fn export_library_to(&self, parent_dir: &Path) -> Result<LibraryExportReport> {
        let attachments_dir = self.file_manager.get_attachments_dir();
        export_library(&self.db, &attachments_dir, parent_dir)
    }

    /// 将导出包合并导入当前库（v2：文献 id 冲突则新建，词表按名收敛）。
    ///
    /// 导入后按当前 `filename_template` 对本次文献附件执行一次模板重命名。
    /// 不自动查重。`selected` 可为包根或父目录。
    pub fn import_library_from(&self, selected: &Path) -> Result<LibraryImportReport> {
        let attachments_dir = self.file_manager.get_attachments_dir();
        let template = self.config.lock().unwrap().filename_template.clone();
        let report = import_library(&self.db, &attachments_dir, selected, &template)?;
        if let Ok(tx) = self.refresh_tx.lock()
            && let Some(tx) = tx.as_ref()
        {
            let _ = tx.send(RefreshMsg::DataChanged);
        }
        Ok(report)
    }
}
