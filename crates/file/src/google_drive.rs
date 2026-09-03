use crate::backend::AttachmentBackend;
use crate::types::GoogleDriveConfig;
use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use log::{debug, error, info};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::RwLock;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use urlencoding;

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3/files";
const DRIVE_ABOUT_BASE: &str = "https://www.googleapis.com/drive/v3/about";
const FOLDER_MIME_TYPE: &str = "application/vnd.google-apps.folder";
const FOLDER_NAME: &str = "LumenAttachments";

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[allow(dead_code)]
    expires_in: u64,
    #[serde(default)]
    #[allow(dead_code)]
    scope: String,
    #[serde(default)]
    #[allow(dead_code)]
    token_type: String,
}

#[derive(Debug, Deserialize)]
struct DriveAboutUser {
    #[serde(rename = "permissionId")]
    permission_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DriveAboutResponse {
    user: Option<DriveAboutUser>,
}

#[derive(Debug, Deserialize)]
struct FileListResponse {
    files: Vec<FileResource>,
    #[serde(default, rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct FileResource {
    pub id: String,
    pub name: String,
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<String>,
    /// RFC 3339 时间戳，文件内容变化时更新。Google Drive v3 的"版本"标识。
    #[serde(default, rename = "modifiedTime")]
    pub modified_time: Option<String>,
    #[serde(default, rename = "md5Checksum")]
    #[allow(dead_code)]
    pub md5_checksum: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub size: Option<String>,
}

#[derive(Debug, Serialize)]
struct TokenRefreshBody {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    grant_type: String,
}

#[derive(Debug, Serialize)]
struct TokenExchangeBody {
    code: String,
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    grant_type: String,
}

#[derive(Debug)]
struct OAuthState {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    access_token: String,
    token_expires_at: Instant,
}

/// Google Drive 附件同步后端
pub struct GoogleDriveBackend {
    client: Client,
    state: RwLock<OAuthState>,
}

impl GoogleDriveBackend {
    pub fn new(config: GoogleDriveConfig) -> Self {
        Self {
            client: Client::new(),
            state: RwLock::new(OAuthState {
                client_id: config.client_id,
                client_secret: config.client_secret,
                refresh_token: config.refresh_token,
                access_token: String::new(),
                token_expires_at: Instant::now(),
            }),
        }
    }

    async fn ensure_token(&self) -> Result<String> {
        {
            let s = self.state.read().unwrap();
            if !s.access_token.is_empty() && Instant::now() < s.token_expires_at {
                return Ok(s.access_token.clone());
            }
        }

        let (client_id, client_secret, refresh_token) = {
            let s = self.state.read().unwrap();
            (
                s.client_id.clone(),
                s.client_secret.clone(),
                s.refresh_token.clone(),
            )
        };

        if refresh_token.is_empty() {
            return Err(anyhow!("Google Drive 未授权，缺少 refresh_token"));
        }

        let body = TokenRefreshBody {
            client_id,
            client_secret,
            refresh_token,
            grant_type: "refresh_token".to_string(),
        };

        let resp = self
            .client
            .post(TOKEN_URL)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("刷新 token 请求失败: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("刷新 token 失败 ({status}): {err_text}"));
        }

        let token_resp: TokenResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("解析 token 响应失败: {e}"))?;

        let access_token = token_resp.access_token.clone();
        let expires_at =
            Instant::now() + Duration::from_secs(token_resp.expires_in.saturating_sub(60));

        {
            let mut s = self.state.write().unwrap();
            s.access_token = access_token.clone();
            s.token_expires_at = expires_at;
            if let Some(rt) = token_resp.refresh_token {
                s.refresh_token = rt;
            }
        }

        debug!("Google Drive: access_token 刷新成功");
        Ok(access_token)
    }

    /// 只读获取用户稳定 permissionId（用于生成账户隔离指纹）
    async fn fetch_user_permission_id(&self) -> Result<String> {
        let token = self.ensure_token().await?;
        let url = format!("{DRIVE_ABOUT_BASE}?fields=user(permissionId)");
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| anyhow!("获取 Drive 账户信息请求失败: {e}"))?;

        if !resp.status().is_success() {
            return Err(anyhow!("获取 Drive 账户信息失败: {}", resp.status()));
        }

        let about: DriveAboutResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("解析 Drive 账户信息响应失败: {e}"))?;

        let perm_id = about
            .user
            .and_then(|u| u.permission_id)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| anyhow!("Drive 账户信息缺少 permissionId"))?;

        Ok(perm_id)
    }
}

impl AttachmentBackend for GoogleDriveBackend {
    fn name(&self) -> &str {
        "google_drive"
    }

    fn is_enabled(&self) -> bool {
        let s = self.state.read().unwrap();
        !s.refresh_token.is_empty()
    }

    fn configuration_fingerprint(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
        let client = self.client.clone();
        let state = {
            let s = self.state.read().unwrap();
            OAuthState {
                client_id: s.client_id.clone(),
                client_secret: s.client_secret.clone(),
                refresh_token: s.refresh_token.clone(),
                access_token: s.access_token.clone(),
                token_expires_at: s.token_expires_at,
            }
        };

        Box::pin(async move {
            let backend = GoogleDriveBackend {
                client,
                state: RwLock::new(state),
            };
            let perm_id = backend.fetch_user_permission_id().await?;
            let raw = format!("google_drive:v1:{f}:{p}", f = FOLDER_NAME, p = perm_id);
            let mut hasher = Sha256::new();
            hasher.update(raw.as_bytes());
            let hash = hasher.finalize();
            let fp = hash
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            Ok(fp)
        })
    }

    fn test_connection(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        let client = self.client.clone();
        let state = {
            let s = self.state.read().unwrap();
            OAuthState {
                client_id: s.client_id.clone(),
                client_secret: s.client_secret.clone(),
                refresh_token: s.refresh_token.clone(),
                access_token: String::new(),
                token_expires_at: Instant::now(),
            }
        };

        Box::pin(async move {
            let backend = GoogleDriveBackend {
                client,
                state: RwLock::new(state),
            };
            let token = backend.ensure_token().await?;

            // 纯只读测试：调用 about API 检查认证与权限，零写请求
            let url = format!("{DRIVE_ABOUT_BASE}?fields=user(permissionId)");
            let resp = backend
                .client
                .get(&url)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|e| anyhow!("连接失败: {e}"))?;

            if resp.status().is_success() {
                Ok(())
            } else if resp.status() == 403 {
                Err(anyhow!(
                    "权限不足 (403)：请在设置页点击「Authorize」重新授权 Google Drive"
                ))
            } else {
                Err(anyhow!("连接失败: {}", resp.status()))
            }
        })
    }

    fn inspect_library(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<crate::backend::LibraryInspection>> + Send>> {
        let client = self.client.clone();
        let state = {
            let s = self.state.read().unwrap();
            OAuthState {
                client_id: s.client_id.clone(),
                client_secret: s.client_secret.clone(),
                refresh_token: s.refresh_token.clone(),
                access_token: s.access_token.clone(),
                token_expires_at: s.token_expires_at,
            }
        };

        Box::pin(async move {
            let backend = GoogleDriveBackend {
                client,
                state: RwLock::new(state),
            };

            let token = backend.ensure_token().await?;

            // 1. 查找根文件夹（只读，绝不创建）
            let root_id = match backend.find_root_folder_id_readonly(&token).await? {
                Some(id) => id,
                None => return Ok(crate::backend::LibraryInspection::MissingEmpty),
            };

            // 2. 列出根目录下全部未删除项
            let items = backend
                .list_folder_items_all_pages(&token, &root_id)
                .await?;

            match inspect_google_drive_root_items(&items) {
                GoogleDriveInspectDecision::SingleIdentity(file) => {
                    let text = backend.download_file_text_by_id(&token, &file.id).await?;
                    let identity = crate::backend::parse_file_library_identity_json(&text)?;
                    Ok(crate::backend::LibraryInspection::Present(identity))
                }
                GoogleDriveInspectDecision::MultipleIdentities => {
                    Err(anyhow!("根目录中发现多个同名 identity 文件冲突"))
                }
                GoogleDriveInspectDecision::MultipleObjectsDirs => Err(anyhow!(
                    "根目录中发现多个同名 {DRIVE_OBJECTS_DIR_NAME} 子文件夹冲突"
                )),
                GoogleDriveInspectDecision::MissingNonEmpty => {
                    Ok(crate::backend::LibraryInspection::MissingNonEmpty)
                }
                GoogleDriveInspectDecision::MissingEmpty => {
                    Ok(crate::backend::LibraryInspection::MissingEmpty)
                }
                GoogleDriveInspectDecision::NeedCheckObjectsDir(folder) => {
                    let obj_items = backend
                        .list_folder_items_all_pages(&token, &folder.id)
                        .await?;
                    if obj_items.is_empty() {
                        Ok(crate::backend::LibraryInspection::MissingEmpty)
                    } else {
                        Ok(crate::backend::LibraryInspection::MissingNonEmpty)
                    }
                }
            }
        })
    }

    fn initialize_library(
        &self,
        _identity: crate::backend::FileLibraryIdentity,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        // Google Drive files.create API 不支持按名称的服务端原子条件创建（无 If-None-Match 等效机制）。
        // 并发时可产生多个同名对象，无法保证安全唯一性，必须显式拒绝。
        // 只有未来取得官方可验证的条件创建机制并通过并发真实集成测试后，才可重新开放。
        Box::pin(async move {
            Err(anyhow!(
                "UnsupportedSafeInitialization: Google Drive 当前不支持服务端原子条件初始化，\
                 零写请求已返回。如需文件同步，请改用 WebDAV 后端。"
            ))
        })
    }

    fn list_objects(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<crate::backend::RemoteObjectEntry>>> + Send>> {
        let client = self.client.clone();
        let state = {
            let s = self.state.read().unwrap();
            OAuthState {
                client_id: s.client_id.clone(),
                client_secret: s.client_secret.clone(),
                refresh_token: s.refresh_token.clone(),
                access_token: s.access_token.clone(),
                token_expires_at: s.token_expires_at,
            }
        };

        Box::pin(async move {
            let backend = GoogleDriveBackend {
                client,
                state: RwLock::new(state),
            };

            let token = backend.ensure_token().await?;

            // 查找根文件夹
            let root_id = match backend.find_root_folder_id_readonly(&token).await? {
                Some(id) => id,
                None => return Ok(Vec::new()),
            };

            // 查找 .lumen-objects-v1 子文件夹
            let objects_folder_id = match backend
                .find_subfolder_id_readonly(&token, &root_id, DRIVE_OBJECTS_DIR_NAME)
                .await?
            {
                Some(id) => id,
                None => return Ok(Vec::new()),
            };

            // 列出全部对象项
            let files = backend
                .list_folder_items_all_pages(&token, &objects_folder_id)
                .await?;

            parse_google_drive_objects_list(&files)
        })
    }

    fn upload_object_if_absent(
        &self,
        _object_key: String,
        _local_path: PathBuf,
    ) -> Pin<Box<dyn Future<Output = Result<crate::backend::UploadObjectResult>> + Send>> {
        // Google Drive files.create API 不支持按名称的服务端原子条件上传（无 If-None-Match 等效机制）。
        // 并发时可产生多个同名对象，无法保证安全唯一性，必须显式拒绝。
        // 只有未来取得官方可验证的条件创建机制并通过并发真实集成测试后，才可重新开放。
        Box::pin(async move {
            Err(anyhow!(
                "UnsupportedSafeCreate: Google Drive 当前不支持服务端原子条件上传，\
                 零写请求已返回。如需文件同步，请改用 WebDAV 后端。"
            ))
        })
    }

    fn download_object(
        &self,
        object_key: String,
        temporary_path: PathBuf,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send>> {
        let client = self.client.clone();
        let state = {
            let s = self.state.read().unwrap();
            OAuthState {
                client_id: s.client_id.clone(),
                client_secret: s.client_secret.clone(),
                refresh_token: s.refresh_token.clone(),
                access_token: s.access_token.clone(),
                token_expires_at: s.token_expires_at,
            }
        };
        Box::pin(async move {
            let leaf = crate::backend::validate_canonical_object_key(&object_key)?;
            let backend = GoogleDriveBackend {
                client,
                state: RwLock::new(state),
            };

            let token = backend.ensure_token().await?;

            let root_id = match backend.find_root_folder_id_readonly(&token).await? {
                Some(id) => id,
                None => return Ok(None),
            };

            let objects_id = match backend
                .find_subfolder_id_readonly(&token, &root_id, DRIVE_OBJECTS_DIR_NAME)
                .await?
            {
                Some(id) => id,
                None => return Ok(None),
            };

            let files = backend
                .find_files_in_folder_by_name(&token, &objects_id, &leaf)
                .await?;

            if files.is_empty() {
                return Ok(None);
            }
            if files.len() > 1 {
                return Err(anyhow!("Google Drive 发现多个同名对象文件: {leaf}"));
            }

            let target_file = &files[0];
            let file_id = &target_file.id;
            let version = target_file
                .modified_time
                .as_ref()
                .filter(|s| !s.is_empty())
                .cloned()
                .unwrap_or_else(|| file_id.clone());

            let url = format!("{DRIVE_API_BASE}/{file_id}?alt=media");
            let resp = backend
                .client
                .get(&url)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|e| anyhow!("Google Drive 下载请求失败: {e}"))?;

            if resp.status().as_u16() == 404 {
                return Ok(None);
            }
            if !resp.status().is_success() {
                return Err(anyhow!("Google Drive 下载失败，状态码: {}", resp.status()));
            }

            if let Some(parent) = temporary_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            let mut file = tokio::fs::File::create(&temporary_path)
                .await
                .map_err(|e| {
                    anyhow!("创建临时下载文件 '{}' 失败: {e}", temporary_path.display())
                })?;

            let mut stream = resp.bytes_stream();
            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result.map_err(|e| anyhow!("读取下载流数据块失败: {e}"))?;
                file.write_all(&chunk)
                    .await
                    .map_err(|e| anyhow!("写入临时文件失败: {e}"))?;
            }
            file.flush()
                .await
                .map_err(|e| anyhow!("刷新临时文件失败: {e}"))?;

            Ok(Some(version))
        })
    }

    fn delete_object(
        &self,
        object_key: String,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        let client = self.client.clone();
        let state = {
            let s = self.state.read().unwrap();
            OAuthState {
                client_id: s.client_id.clone(),
                client_secret: s.client_secret.clone(),
                refresh_token: s.refresh_token.clone(),
                access_token: s.access_token.clone(),
                token_expires_at: s.token_expires_at,
            }
        };

        Box::pin(async move {
            let leaf = crate::backend::validate_canonical_object_key(&object_key)?;
            let backend = GoogleDriveBackend {
                client,
                state: RwLock::new(state),
            };

            let token = backend.ensure_token().await?;

            let root_id = match backend.find_root_folder_id_readonly(&token).await? {
                Some(id) => id,
                None => return Ok(()),
            };

            let objects_id = match backend
                .find_subfolder_id_readonly(&token, &root_id, DRIVE_OBJECTS_DIR_NAME)
                .await?
            {
                Some(id) => id,
                None => return Ok(()),
            };

            let files = backend
                .find_files_in_folder_by_name(&token, &objects_id, &leaf)
                .await?;

            for f in files {
                let url = format!("{DRIVE_API_BASE}/{}", f.id);
                let resp = backend
                    .client
                    .delete(&url)
                    .bearer_auth(&token)
                    .send()
                    .await
                    .map_err(|e| anyhow!("Google Drive 删除请求失败: {e}"))?;

                let status = resp.status().as_u16();
                if status != 404 && !resp.status().is_success() {
                    return Err(anyhow!("Google Drive 删除失败，状态码: {status}"));
                }
            }

            Ok(())
        })
    }
}

const DRIVE_IDENTITY_FILE_NAME: &str = ".lumen-file-library-v1.json";
const DRIVE_OBJECTS_DIR_NAME: &str = ".lumen-objects-v1";

impl GoogleDriveBackend {
    /// 只读查找根文件夹（绝不自动创建）
    async fn find_root_folder_id_readonly(&self, token: &str) -> Result<Option<String>> {
        let q = format!("name='{FOLDER_NAME}' and mimeType='{FOLDER_MIME_TYPE}' and trashed=false");
        let url = format!(
            "{DRIVE_API_BASE}?q={}&fields=files(id,name)&pageSize=10",
            urlencoding::encode(&q)
        );

        let resp = self
            .client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| anyhow!("查找根文件夹请求失败: {e}"))?;

        if !resp.status().is_success() {
            return Err(anyhow!("查找根文件夹失败，状态码: {}", resp.status()));
        }

        let list: FileListResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("解析查找根文件夹响应失败: {e}"))?;

        if list.files.is_empty() {
            Ok(None)
        } else if list.files.len() == 1 {
            Ok(Some(list.files[0].id.clone()))
        } else {
            Err(anyhow!(
                "Google Drive 中发现多个名为 {FOLDER_NAME} 的根文件夹冲突"
            ))
        }
    }

    /// 只读查找指定父目录下的子文件夹
    async fn find_subfolder_id_readonly(
        &self,
        token: &str,
        parent_id: &str,
        subfolder_name: &str,
    ) -> Result<Option<String>> {
        let q = format!(
            "'{parent_id}' in parents and name='{subfolder_name}' and mimeType='{FOLDER_MIME_TYPE}' and trashed=false"
        );
        let url = format!(
            "{DRIVE_API_BASE}?q={}&fields=files(id,name)&pageSize=10",
            urlencoding::encode(&q)
        );

        let resp = self
            .client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| anyhow!("查找子文件夹 {subfolder_name} 请求失败: {e}"))?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "查找子文件夹 {subfolder_name} 失败，状态码: {}",
                resp.status()
            ));
        }

        let list: FileListResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("解析查找子文件夹响应失败: {e}"))?;

        if list.files.is_empty() {
            Ok(None)
        } else if list.files.len() == 1 {
            Ok(Some(list.files[0].id.clone()))
        } else {
            Err(anyhow!(
                "父目录 {parent_id} 下发现多个名为 {subfolder_name} 的子文件夹冲突"
            ))
        }
    }

    /// 循环分页列出文件夹下的全部未删除项目（防止无限循环）
    async fn list_folder_items_all_pages(
        &self,
        token: &str,
        parent_id: &str,
    ) -> Result<Vec<FileResource>> {
        let mut all_files = Vec::new();
        let mut page_token: Option<String> = None;
        let mut seen_tokens = std::collections::HashSet::new();

        loop {
            let q = format!("'{parent_id}' in parents and trashed=false");
            let mut url = format!(
                "{DRIVE_API_BASE}?q={}&fields=nextPageToken,files(id,name,mimeType,modifiedTime,md5Checksum,size)&pageSize=1000",
                urlencoding::encode(&q)
            );
            if let Some(ref pt) = page_token {
                url.push_str(&format!("&pageToken={}", urlencoding::encode(pt)));
            }

            let resp = self
                .client
                .get(&url)
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| anyhow!("列出文件夹内容请求失败: {e}"))?;

            if !resp.status().is_success() {
                return Err(anyhow!("列出文件夹内容失败，状态码: {}", resp.status()));
            }

            let list: FileListResponse = resp
                .json()
                .await
                .map_err(|e| anyhow!("解析文件列表响应失败: {e}"))?;

            all_files.extend(list.files);

            match check_and_record_next_page_token(
                &mut seen_tokens,
                list.next_page_token.as_deref(),
            )? {
                Some(pt) => page_token = Some(pt),
                None => break,
            }
        }

        Ok(all_files)
    }

    /// 下载指定文件 ID 的文本内容
    async fn download_file_text_by_id(&self, token: &str, file_id: &str) -> Result<String> {
        let url = format!("{DRIVE_API_BASE}/{file_id}?alt=media");
        let resp = self
            .client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| anyhow!("下载文件 {file_id} 请求失败: {e}"))?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "下载文件 {file_id} 失败，状态码: {}",
                resp.status()
            ));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| anyhow!("读取文件 {file_id} 文本内容失败: {e}"))?;
        Ok(body)
    }

    /// 按文件名在指定文件夹内查找所有文件
    async fn find_files_in_folder_by_name(
        &self,
        token: &str,
        folder_id: &str,
        file_name: &str,
    ) -> Result<Vec<FileResource>> {
        let q = format!("'{folder_id}' in parents and name='{file_name}' and trashed=false");
        let url = format!(
            "{DRIVE_API_BASE}?q={}&fields=files(id,name,mimeType,modifiedTime)&pageSize=10",
            urlencoding::encode(&q)
        );

        let resp = self
            .client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| anyhow!("查找文件 {file_name} 请求失败: {e}"))?;

        if !resp.status().is_success() {
            return Err(anyhow!("查找文件失败，状态码: {}", resp.status()));
        }

        let list: FileListResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("解析文件查找响应失败: {e}"))?;

        Ok(list.files)
    }
}

/// 校验并记录分页 token，若遇到循环或重复 token 则报错拒绝
pub(crate) fn check_and_record_next_page_token(
    seen_tokens: &mut std::collections::HashSet<String>,
    next_token: Option<&str>,
) -> Result<Option<String>> {
    match next_token {
        Some(pt) if !pt.is_empty() => {
            if !seen_tokens.insert(pt.to_string()) {
                return Err(anyhow!(
                    "Google Drive 分页检测到重复或循环的 nextPageToken: {pt}"
                ));
            }
            Ok(Some(pt.to_string()))
        }
        _ => Ok(None),
    }
}

/// Google Drive 根目录探测决策
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GoogleDriveInspectDecision {
    SingleIdentity(FileResource),
    MultipleIdentities,
    MultipleObjectsDirs,
    MissingEmpty,
    MissingNonEmpty,
    NeedCheckObjectsDir(FileResource),
}

/// 纯函数：根据根目录文件项决策探测状态（优先校验重名与歧义）
pub(crate) fn inspect_google_drive_root_items(
    files: &[FileResource],
) -> GoogleDriveInspectDecision {
    // 1. 优先检查是否存在多个同名 .lumen-objects-v1 目录（无论 identity 是否存在，均直接报错拒绝）
    let objects_dirs: Vec<&FileResource> = files
        .iter()
        .filter(|f| {
            f.mime_type.as_deref() == Some(FOLDER_MIME_TYPE) && f.name == DRIVE_OBJECTS_DIR_NAME
        })
        .collect();

    if objects_dirs.len() > 1 {
        return GoogleDriveInspectDecision::MultipleObjectsDirs;
    }

    // 2. 检查是否存在同名重复的 identity 文件
    let identity_files: Vec<&FileResource> = files
        .iter()
        .filter(|f| {
            f.name == DRIVE_IDENTITY_FILE_NAME && f.mime_type.as_deref() != Some(FOLDER_MIME_TYPE)
        })
        .collect();

    if identity_files.len() > 1 {
        return GoogleDriveInspectDecision::MultipleIdentities;
    }
    if identity_files.len() == 1 {
        return GoogleDriveInspectDecision::SingleIdentity(identity_files[0].clone());
    }

    // 3. 无 identity 文件，检查是否有非 identity 文件或未知子目录
    let has_non_identity_files = files.iter().any(|f| {
        f.name != DRIVE_IDENTITY_FILE_NAME && f.mime_type.as_deref() != Some(FOLDER_MIME_TYPE)
    });
    let has_unknown_dirs = files.iter().any(|f| {
        f.mime_type.as_deref() == Some(FOLDER_MIME_TYPE) && f.name != DRIVE_OBJECTS_DIR_NAME
    });

    if has_non_identity_files || has_unknown_dirs {
        return GoogleDriveInspectDecision::MissingNonEmpty;
    }

    if objects_dirs.len() == 1 {
        return GoogleDriveInspectDecision::NeedCheckObjectsDir(objects_dirs[0].clone());
    }

    GoogleDriveInspectDecision::MissingEmpty
}

/// 纯函数：解析 Google Drive objects 子目录下的文件列表为 RemoteObjectEntry
pub(crate) fn parse_google_drive_objects_list(
    files: &[FileResource],
) -> Result<Vec<crate::backend::RemoteObjectEntry>> {
    use std::collections::HashSet;
    let mut seen_keys = HashSet::new();
    let mut entries = Vec::with_capacity(files.len());

    for f in files {
        if f.mime_type.as_deref() == Some(FOLDER_MIME_TYPE) {
            return Err(anyhow!("objects 目录下包含意外的子目录: {}", f.name));
        }

        crate::backend::validate_canonical_uuid(&f.name, "object file")?;
        let object_key = format!("objects/v1/{}", f.name);

        let remote_version = f
            .modified_time
            .clone()
            .or_else(|| Some(f.id.clone()))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("对象缺失 remote_version: {object_key}"))?;

        if !seen_keys.insert(object_key.clone()) {
            return Err(anyhow!("Google Drive 中发现重复对象键: {object_key}"));
        }

        entries.push(crate::backend::RemoteObjectEntry {
            object_key,
            remote_version,
        });
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_google_drive_root_empty_and_identity() {
        // Empty root
        let empty_files: Vec<FileResource> = vec![];
        assert_eq!(
            inspect_google_drive_root_items(&empty_files),
            GoogleDriveInspectDecision::MissingEmpty
        );

        // Single identity
        let id_file = FileResource {
            id: "id-123".to_string(),
            name: DRIVE_IDENTITY_FILE_NAME.to_string(),
            mime_type: Some("application/json".to_string()),
            modified_time: None,
            md5_checksum: None,
            size: None,
        };
        assert_eq!(
            inspect_google_drive_root_items(&[id_file.clone()]),
            GoogleDriveInspectDecision::SingleIdentity(id_file.clone())
        );

        // Multiple identities
        let id_file2 = FileResource {
            id: "id-456".to_string(),
            name: DRIVE_IDENTITY_FILE_NAME.to_string(),
            mime_type: Some("application/json".to_string()),
            modified_time: None,
            md5_checksum: None,
            size: None,
        };
        assert_eq!(
            inspect_google_drive_root_items(&[id_file.clone(), id_file2]),
            GoogleDriveInspectDecision::MultipleIdentities
        );

        // Multiple objects dirs (even when identity is present)
        let obj_dir1 = FileResource {
            id: "dir-1".to_string(),
            name: DRIVE_OBJECTS_DIR_NAME.to_string(),
            mime_type: Some(FOLDER_MIME_TYPE.to_string()),
            modified_time: None,
            md5_checksum: None,
            size: None,
        };
        let obj_dir2 = FileResource {
            id: "dir-2".to_string(),
            name: DRIVE_OBJECTS_DIR_NAME.to_string(),
            mime_type: Some(FOLDER_MIME_TYPE.to_string()),
            modified_time: None,
            md5_checksum: None,
            size: None,
        };
        assert_eq!(
            inspect_google_drive_root_items(&[id_file.clone(), obj_dir1, obj_dir2]),
            GoogleDriveInspectDecision::MultipleObjectsDirs
        );
    }

    #[test]
    fn inspect_google_drive_root_missing_non_empty() {
        // Contains legacy attachment
        let legacy_file = FileResource {
            id: "f-1".to_string(),
            name: "paper.pdf".to_string(),
            mime_type: Some("application/pdf".to_string()),
            modified_time: None,
            md5_checksum: None,
            size: None,
        };
        assert_eq!(
            inspect_google_drive_root_items(&[legacy_file]),
            GoogleDriveInspectDecision::MissingNonEmpty
        );

        // Contains unknown folder
        let unknown_folder = FileResource {
            id: "f-2".to_string(),
            name: "random_folder".to_string(),
            mime_type: Some(FOLDER_MIME_TYPE.to_string()),
            modified_time: None,
            md5_checksum: None,
            size: None,
        };
        assert_eq!(
            inspect_google_drive_root_items(&[unknown_folder]),
            GoogleDriveInspectDecision::MissingNonEmpty
        );
    }

    #[test]
    fn parse_google_drive_objects_list_success_and_validations() {
        let valid_files = vec![
            FileResource {
                id: "drive-id-1".to_string(),
                name: "550e8400-e29b-41d4-a716-446655440000".to_string(),
                mime_type: Some("application/pdf".to_string()),
                modified_time: Some("2026-08-31T12:00:00Z".to_string()),
                md5_checksum: None,
                size: Some("1024".to_string()),
            },
            FileResource {
                id: "drive-id-2".to_string(),
                name: "6ba7b810-9dad-11d1-80b4-00c04fd430c8".to_string(),
                mime_type: Some("application/pdf".to_string()),
                modified_time: None, // fallback to id
                md5_checksum: None,
                size: Some("2048".to_string()),
            },
        ];

        let entries = parse_google_drive_objects_list(&valid_files).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].object_key,
            "objects/v1/550e8400-e29b-41d4-a716-446655440000"
        );
        assert_eq!(entries[0].remote_version, "2026-08-31T12:00:00Z");
        assert_eq!(
            entries[1].object_key,
            "objects/v1/6ba7b810-9dad-11d1-80b4-00c04fd430c8"
        );
        assert_eq!(entries[1].remote_version, "drive-id-2");

        // Subfolder in objects dir rejected
        let folder_item = vec![FileResource {
            id: "f-sub".to_string(),
            name: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            mime_type: Some(FOLDER_MIME_TYPE.to_string()),
            modified_time: None,
            md5_checksum: None,
            size: None,
        }];
        assert!(parse_google_drive_objects_list(&folder_item).is_err());

        // Non-UUID name rejected
        let non_uuid = vec![FileResource {
            id: "f-invalid".to_string(),
            name: "test.pdf".to_string(),
            mime_type: Some("application/pdf".to_string()),
            modified_time: Some("2026-08-31T12:00:00Z".to_string()),
            md5_checksum: None,
            size: None,
        }];
        assert!(parse_google_drive_objects_list(&non_uuid).is_err());

        // Duplicate name rejected
        let dup = vec![
            FileResource {
                id: "id1".to_string(),
                name: "550e8400-e29b-41d4-a716-446655440000".to_string(),
                mime_type: Some("application/pdf".to_string()),
                modified_time: Some("2026-08-31T12:00:00Z".to_string()),
                md5_checksum: None,
                size: None,
            },
            FileResource {
                id: "id2".to_string(),
                name: "550e8400-e29b-41d4-a716-446655440000".to_string(),
                mime_type: Some("application/pdf".to_string()),
                modified_time: Some("2026-08-31T13:00:00Z".to_string()),
                md5_checksum: None,
                size: None,
            },
        ];
        assert!(parse_google_drive_objects_list(&dup).is_err());
    }

    #[test]
    fn check_and_record_next_page_token_detects_cycles_and_advances() {
        let mut seen = std::collections::HashSet::new();

        // 首次推进
        assert_eq!(
            check_and_record_next_page_token(&mut seen, Some("page-2")).unwrap(),
            Some("page-2".to_string())
        );

        // 第二次推进不同 token
        assert_eq!(
            check_and_record_next_page_token(&mut seen, Some("page-3")).unwrap(),
            Some("page-3".to_string())
        );

        // 重复/循环 token 立即报错
        assert!(check_and_record_next_page_token(&mut seen, Some("page-2")).is_err());

        // 结束 token 返回 None
        assert_eq!(
            check_and_record_next_page_token(&mut seen, None).unwrap(),
            None
        );
        assert_eq!(
            check_and_record_next_page_token(&mut seen, Some("")).unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn google_drive_initialize_and_upload_return_unsupported() {
        let backend = GoogleDriveBackend::new(GoogleDriveConfig {
            enabled: true,
            client_id: "test-client".to_string(),
            client_secret: "test-secret".to_string(),
            refresh_token: "test-refresh".to_string(),
        });

        let id = crate::backend::FileLibraryIdentity {
            protocol_version: 1,
            file_library_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            database_library_id: "6ba7b810-9dad-11d1-80b4-00c04fd430c8".to_string(),
            created_at: 100,
        };

        let init_res = backend.initialize_library(id).await;
        assert!(init_res.is_err());
        assert!(
            init_res
                .unwrap_err()
                .to_string()
                .contains("UnsupportedSafeInitialization")
        );

        let upload_res = backend
            .upload_object_if_absent(
                "objects/v1/550e8400-e29b-41d4-a716-446655440000".to_string(),
                PathBuf::from("/tmp/foo.pdf"),
            )
            .await;
        assert!(upload_res.is_err());
        assert!(
            upload_res
                .unwrap_err()
                .to_string()
                .contains("UnsupportedSafeCreate")
        );
    }
}

/// 打开系统浏览器
fn open_browser(url: &str) {
    let result = match std::env::consts::OS {
        "windows" => std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn(),
        "macos" => std::process::Command::new("open").arg(url).spawn(),
        _ => std::process::Command::new("xdg-open").arg(url).spawn(),
    };
    if let Err(e) = result {
        error!("打开浏览器失败: {e}");
    }
}

/// 处理本地 HTTP 回调请求
async fn handle_oauth_callback(stream: &mut TcpStream, code: &mut String) -> Result<()> {
    let mut buf_reader = BufReader::new(&mut *stream);
    let mut request_line = String::new();
    buf_reader
        .read_line(&mut request_line)
        .await
        .map_err(|e| anyhow!("读取请求失败: {e}"))?;

    debug!("OAuth: 回调请求行: {request_line:?}");

    if let Some(query_start) = request_line.split_whitespace().nth(1)
        && let Some(query) = query_start.split('?').nth(1)
    {
        for param in query.split('&') {
            if let Some(code_val) = param.strip_prefix("code=") {
                *code = code_val.to_string();
                break;
            }
        }
    }

    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\n\r\n<html><body><h1>授权成功！可以关闭此页面。</h1></body></html>";
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|e| anyhow!("写入响应失败: {e}"))?;
    stream.flush().await?;

    Ok(())
}

/// 完成完整的 OAuth 授权流程
pub async fn complete_oauth_flow(client_id: &str, client_secret: &str) -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| anyhow!("无法绑定本地端口: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| anyhow!("获取端口失败: {e}"))?
        .port();
    info!("OAuth: 端口绑定成功: {port}");

    let redirect_uri = format!("http://127.0.0.1:{port}");
    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth\
         ?client_id={}\
         &redirect_uri={}\
         &scope={}\
         &response_type=code\
         &access_type=offline\
         &prompt=consent",
        urlencoding::encode(client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode("https://www.googleapis.com/auth/drive.file"),
    );

    info!("OAuth: 打开浏览器, 端口={port}");
    open_browser(&auth_url);

    let mut code = String::new();
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|e| anyhow!("接收连接失败: {e}"))?;

        let cb_ok = handle_oauth_callback(&mut stream, &mut code).await.is_ok();
        if cb_ok && !code.is_empty() {
            info!("OAuth: 收到授权码, 正在交换 token...");
            break;
        }
    }

    let exchange_body = TokenExchangeBody {
        code: code.clone(),
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
        redirect_uri,
        grant_type: "authorization_code".to_string(),
    };

    let client = Client::new();
    let resp = client.post(TOKEN_URL).json(&exchange_body).send().await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            error!("OAuth: token 交换请求失败: {e}");
            return Err(anyhow!("token 交换请求失败: {e}"));
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let err_text = resp.text().await.unwrap_or_default();
        error!("OAuth: token 交换失败 ({}): {}", status, err_text);
        return Err(anyhow!("token 交换失败 ({}): {}", status, err_text));
    }

    let token: TokenResponse = resp.json().await.map_err(|e| {
        error!("OAuth: 解析 token 响应失败: {e}");
        anyhow!("解析 token 响应失败: {e}")
    })?;

    let refresh_token = token.refresh_token.ok_or_else(|| {
        error!("OAuth: 未收到 refresh_token");
        anyhow!("未收到 refresh_token (请检查是否设置了 access_type=offline)")
    })?;

    info!("OAuth: 授权成功");
    Ok(refresh_token)
}
