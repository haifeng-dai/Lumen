use crate::backend::AttachmentBackend;
use anyhow::{Result, anyhow};
use log::warn;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

/// 空操作后端 — 未启用任何远端后端时使用
pub struct NoopBackend;

impl AttachmentBackend for NoopBackend {
    fn name(&self) -> &str {
        "noop"
    }

    fn is_enabled(&self) -> bool {
        false
    }

    fn test_connection(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        Box::pin(async {
            warn!("NoopBackend: test_connection — 未配置文件同步后端");
            Err(anyhow!("未配置文件同步后端"))
        })
    }

    fn inspect_library(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<crate::backend::LibraryInspection>> + Send>> {
        Box::pin(async {
            warn!("NoopBackend: inspect_library — 未配置文件同步后端");
            Err(anyhow!("backend disabled"))
        })
    }

    fn initialize_library(
        &self,
        _identity: crate::backend::FileLibraryIdentity,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        Box::pin(async {
            warn!("NoopBackend: initialize_library — 未配置文件同步后端");
            Err(anyhow!("backend disabled"))
        })
    }

    fn list_objects(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<crate::backend::RemoteObjectEntry>>> + Send>> {
        Box::pin(async {
            warn!("NoopBackend: list_objects — 未配置文件同步后端");
            Err(anyhow!("backend disabled"))
        })
    }

    fn upload_object_if_absent(
        &self,
        _object_key: String,
        _local_path: PathBuf,
    ) -> Pin<Box<dyn Future<Output = Result<crate::backend::UploadObjectResult>> + Send>> {
        Box::pin(async {
            warn!("NoopBackend: upload_object_if_absent — 未配置文件同步后端");
            Err(anyhow!("backend disabled"))
        })
    }

    fn download_object(
        &self,
        _object_key: String,
        _temporary_path: PathBuf,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send>> {
        Box::pin(async {
            warn!("NoopBackend: download_object — 未配置文件同步后端");
            Err(anyhow!("backend disabled"))
        })
    }

    fn delete_object(
        &self,
        _object_key: String,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        Box::pin(async {
            warn!("NoopBackend: delete_object — 未配置文件同步后端");
            Err(anyhow!("backend disabled"))
        })
    }

    fn configuration_fingerprint(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
        Box::pin(async { Ok("noop".to_string()) })
    }
}
