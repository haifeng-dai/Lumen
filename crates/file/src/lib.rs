pub mod backend;
pub mod google_drive;
pub mod local;
pub mod noop;
pub mod types;
pub mod webdav;

pub use backend::{
    AttachmentBackend, FileLibraryIdentity, LibraryInspection, RemoteObjectEntry,
    UploadObjectResult, parse_file_library_identity_json, validate_canonical_object_key,
    validate_canonical_uuid,
};
pub use local::LocalFileManager;
pub use types::{GoogleDriveConfig, WebDavConfig};

use crate::google_drive::GoogleDriveBackend;
use crate::noop::NoopBackend;
use crate::webdav::WebDavBackend;
use log::info;

/// 根据名称创建后端实例
///
/// `config_json` 是 JSON 字符串，由外部传入（来自 local_state）。
pub fn create_backend(name: &str, config_json: &str) -> Box<dyn AttachmentBackend> {
    match name {
        "webdav" => match serde_json::from_str::<WebDavConfig>(config_json) {
            Ok(cfg) => {
                info!("Sync: 创建 WebDAV 后端");
                Box::new(WebDavBackend::new(cfg))
            }
            Err(e) => {
                info!("Sync: WebDAV 配置解析失败 ({}), 使用 Noop 后端", e);
                Box::new(NoopBackend)
            }
        },
        "google_drive" => match serde_json::from_str::<GoogleDriveConfig>(config_json) {
            Ok(cfg) => {
                info!("Sync: 创建 Google Drive 后端");
                Box::new(GoogleDriveBackend::new(cfg))
            }
            Err(e) => {
                info!("Sync: Google Drive 配置解析失败 ({}), 使用 Noop 后端", e);
                Box::new(NoopBackend)
            }
        },
        _ => {
            info!("Sync: 未知后端 '{}', 使用 Noop 后端", name);
            Box::new(NoopBackend)
        }
    }
}
