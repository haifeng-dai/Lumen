use crate::backend::{AttachmentBackend, RemoteFileEntry};
use crate::types::GoogleDriveConfig;
use anyhow::{Result, anyhow};
use log::{debug, error, info};
use reqwest::Client;
use serde::{Deserialize, Serialize};
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
const DRIVE_UPLOAD_BASE: &str = "https://www.googleapis.com/upload/drive/v3/files";
const FOLDER_MIME_TYPE: &str = "application/vnd.google-apps.folder";
const FOLDER_NAME: &str = "LumenAttachments";

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: u64,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    token_type: String,
}

#[derive(Debug, Deserialize)]
struct FileListResponse {
    files: Vec<FileResource>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FileResource {
    id: String,
    name: String,
    /// RFC 3339 时间戳，文件内容变化时更新。Google Drive v3 的"版本"标识。
    #[serde(default)]
    modified_time: Option<String>,
    #[serde(default)]
    md5_checksum: Option<String>,
    #[serde(default)]
    size: Option<String>,
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

#[derive(Debug, Serialize)]
struct FileMetadata {
    name: String,
    parents: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RenameBody {
    name: String,
}

#[derive(Debug, Serialize)]
struct FolderCreateBody {
    name: String,
    mime_type: String,
}

#[derive(Debug)]
struct OAuthState {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    access_token: String,
    token_expires_at: Instant,
}

/// Google Drive 文件同步后端
pub struct GoogleDriveBackend {
    client: Client,
    state: RwLock<OAuthState>,
    folder_id: RwLock<Option<String>>,
}

impl GoogleDriveBackend {
    pub fn new(config: GoogleDriveConfig) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            state: RwLock::new(OAuthState {
                client_id: config.client_id,
                client_secret: config.client_secret,
                refresh_token: config.refresh_token,
                access_token: String::new(),
                token_expires_at: Instant::now(),
            }),
            folder_id: RwLock::new(None),
        }
    }

    async fn ensure_token(&self) -> Result<String> {
        let needs_refresh = {
            let s = self.state.read().unwrap();
            s.access_token.is_empty() || s.token_expires_at <= Instant::now()
        };

        if !needs_refresh {
            return Ok(self.state.read().unwrap().access_token.clone());
        }

        let (client_id, client_secret, refresh_token) = {
            let s = self.state.read().unwrap();
            if s.refresh_token.is_empty() {
                return Err(anyhow!("Google Drive 未授权，缺少 refresh_token"));
            }
            (
                s.client_id.clone(),
                s.client_secret.clone(),
                s.refresh_token.clone(),
            )
        };

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
            .map_err(|e| anyhow!("刷新 token 失败: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("刷新 token 失败 ({}): {}", status, err_text));
        }

        let token: TokenResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("解析 token 响应失败: {e}"))?;

        {
            let mut s = self.state.write().unwrap();
            s.access_token = token.access_token;
            s.token_expires_at =
                Instant::now() + Duration::from_secs(token.expires_in.saturating_sub(60));
        }

        Ok(self.state.read().unwrap().access_token.clone())
    }

    async fn send_get(&self, url: &str) -> Result<reqwest::Response> {
        let token = self.ensure_token().await?;
        self.client
            .get(url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| anyhow!("请求失败: {e}"))
    }

    async fn send_patch(&self, url: &str, body: impl Serialize) -> Result<reqwest::Response> {
        let token = self.ensure_token().await?;
        self.client
            .patch(url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("请求失败: {e}"))
    }

    async fn send_delete(&self, url: &str) -> Result<reqwest::Response> {
        let token = self.ensure_token().await?;
        self.client
            .delete(url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| anyhow!("请求失败: {e}"))
    }

    async fn ensure_folder_id(&self) -> Result<String> {
        if let Some(id) = self.folder_id.read().unwrap().clone() {
            return Ok(id);
        }

        let token = self.ensure_token().await?;

        let q = format!(
            "name='{}' and mimeType='{}' and trashed=false",
            FOLDER_NAME.replace('\'', "\\'"),
            FOLDER_MIME_TYPE
        );
        let url = format!(
            "{DRIVE_API_BASE}?q={}&fields=files(id,name)&pageSize=10",
            urlencoding::encode(&q)
        );
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| anyhow!("查找文件夹失败: {e}"))?;

        if !resp.status().is_success() {
            if resp.status() == 403 {
                return Err(anyhow!(
                    "权限不足 (403)：请在设置页点击「Authorize」重新授权 Google Drive"
                ));
            }
            return Err(anyhow!("查找文件夹失败: {}", resp.status()));
        }

        let list: FileListResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("解析文件夹列表失败: {e}"))?;

        if let Some(folder) = list.files.into_iter().next() {
            info!(
                "Google Drive: 找到文件夹 '{}' (id={})",
                FOLDER_NAME, folder.id
            );
            *self.folder_id.write().unwrap() = Some(folder.id.clone());
            return Ok(folder.id);
        }

        let create_url = format!("{DRIVE_API_BASE}?fields=id");
        let create_body = FolderCreateBody {
            name: FOLDER_NAME.to_string(),
            mime_type: FOLDER_MIME_TYPE.to_string(),
        };
        let create_resp = self
            .client
            .post(&create_url)
            .bearer_auth(&token)
            .json(&create_body)
            .send()
            .await
            .map_err(|e| anyhow!("创建文件夹失败: {e}"))?;

        if !create_resp.status().is_success() {
            if create_resp.status() == 403 {
                return Err(anyhow!(
                    "权限不足 (403)：请在设置页点击「Authorize」重新授权 Google Drive"
                ));
            }
            let status = create_resp.status();
            let err_text = create_resp.text().await.unwrap_or_default();
            return Err(anyhow!("创建文件夹失败 ({}): {}", status, err_text));
        }

        let created: FileResource = create_resp
            .json()
            .await
            .map_err(|e| anyhow!("解析创建文件夹响应失败: {e}"))?;

        info!(
            "Google Drive: 创建文件夹 '{}' (id={})",
            FOLDER_NAME, created.id
        );
        *self.folder_id.write().unwrap() = Some(created.id.clone());
        Ok(created.id)
    }

    async fn upload_multipart(
        &self,
        file_path: PathBuf,
        file_name: String,
    ) -> Result<reqwest::Response> {
        let token = self.ensure_token().await?;
        let folder_id = self.ensure_folder_id().await?;

        let metadata = FileMetadata {
            name: file_name.clone(),
            parents: vec![folder_id],
        };
        let metadata_json = serde_json::to_vec(&metadata)?;

        let file_bytes = tokio::fs::read(&file_path)
            .await
            .map_err(|e| anyhow!("读取文件失败 '{}': {}", file_path.display(), e))?;

        let boundary = format!("boundary_{}", uuid::Uuid::new_v4());
        let mut body_bytes = Vec::new();

        body_bytes.extend_from_slice(b"--");
        body_bytes.extend_from_slice(boundary.as_bytes());
        body_bytes.extend_from_slice(b"\r\n");
        body_bytes.extend_from_slice(b"Content-Type: application/json; charset=UTF-8\r\n\r\n");
        body_bytes.extend_from_slice(&metadata_json);
        body_bytes.extend_from_slice(b"\r\n");

        body_bytes.extend_from_slice(b"--");
        body_bytes.extend_from_slice(boundary.as_bytes());
        body_bytes.extend_from_slice(b"\r\n");
        body_bytes.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
        body_bytes.extend_from_slice(&file_bytes);
        body_bytes.extend_from_slice(b"\r\n");

        body_bytes.extend_from_slice(b"--");
        body_bytes.extend_from_slice(boundary.as_bytes());
        body_bytes.extend_from_slice(b"--\r\n");

        let url = format!(
            "{}?uploadType=multipart&fields=id,name,modifiedTime",
            DRIVE_UPLOAD_BASE
        );

        self.client
            .post(&url)
            .bearer_auth(&token)
            .header(
                "Content-Type",
                format!("multipart/related; boundary={boundary}"),
            )
            .body(body_bytes)
            .send()
            .await
            .map_err(|e| anyhow!("上传失败: {e}"))
    }

    async fn find_file_id_by_name(&self, name: &str) -> Result<(String, Option<String>)> {
        let folder_id = self.ensure_folder_id().await?;
        let safe_name = name.replace('\'', "\\'");
        let q = format!("name='{safe_name}' and '{folder_id}' in parents and trashed=false");
        let url = format!(
            "{DRIVE_API_BASE}?q={}&fields=files(id,modifiedTime)&pageSize=100",
            urlencoding::encode(&q)
        );
        let resp = self.send_get(&url).await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("查找文件失败 ({}): {}", status, err_text));
        }

        let list: FileListResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("解析文件列表失败: {e}"))?;

        list.files
            .into_iter()
            .next()
            .map(|f| (f.id, f.modified_time))
            .ok_or_else(|| anyhow!("未找到文件 '{name}'"))
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
                folder_id: RwLock::new(None),
            };
            let token = backend.ensure_token().await?;
            let folder_id = backend.ensure_folder_id().await?;

            let q = format!("'{folder_id}' in parents");
            let url = format!(
                "{DRIVE_API_BASE}?q={}&pageSize=1&fields=files(id)",
                urlencoding::encode(&q)
            );

            let resp = backend
                .client
                .get(&url)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|e| anyhow!("连接失败: {e}"))?;

            if resp.status().is_success() {
                Ok(())
            } else {
                Err(anyhow!("连接失败: {}", resp.status()))
            }
        })
    }

    fn upload(
        &self,
        local_path: PathBuf,
        name: String,
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
            let backend = GoogleDriveBackend {
                client,
                state: RwLock::new(state),
                folder_id: RwLock::new(None),
            };

            let resp = backend
                .upload_multipart(local_path.clone(), name.clone())
                .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let err_text = resp.text().await.unwrap_or_default();
                return Err(anyhow!("上传失败 ({}): {}", status, err_text));
            }

            let file: FileResource = resp
                .json()
                .await
                .map_err(|e| anyhow!("解析上传响应失败: {e}"))?;

            info!("Google Drive: 上传成功 '{}', fileId={}", name, file.id);
            Ok(Some(
                file.modified_time
                    .clone()
                    .unwrap_or_else(|| file.id.clone()),
            ))
        })
    }

    fn download(
        &self,
        name: String,
        local_path: PathBuf,
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
            let backend = GoogleDriveBackend {
                client,
                state: RwLock::new(state),
                folder_id: RwLock::new(None),
            };

            let (file_id, modified_time) = backend.find_file_id_by_name(&name).await?;

            let url = format!(
                "{drive_api}/{file_id}?alt=media",
                drive_api = DRIVE_API_BASE
            );
            let token = backend.ensure_token().await?;

            let resp = backend
                .client
                .get(&url)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|e| anyhow!("下载失败: {e}"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let err_text = resp.text().await.unwrap_or_default();
                return Err(anyhow!("下载失败 ({}): {}", status, err_text));
            }

            if let Some(parent) = local_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            let bytes = resp
                .bytes()
                .await
                .map_err(|e| anyhow!("读取响应失败: {e}"))?;
            tokio::fs::write(&local_path, &bytes)
                .await
                .map_err(|e| anyhow!("写入文件失败 '{}': {}", local_path.display(), e))?;

            let version = modified_time.unwrap_or(file_id);
            info!("Google Drive: 下载成功 '{name}', version={version}");
            Ok(Some(version))
        })
    }

    fn list(&self) -> Pin<Box<dyn Future<Output = Result<Vec<RemoteFileEntry>>> + Send>> {
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
                folder_id: RwLock::new(None),
            };

            let token = backend.ensure_token().await?;
            let folder_id = backend.ensure_folder_id().await?;

            let q = format!("'{folder_id}' in parents and trashed=false");
            let url = format!(
                "{DRIVE_API_BASE}?q={}&fields=files(id,name,modifiedTime)&pageSize=1000",
                urlencoding::encode(&q)
            );

            let resp = backend
                .client
                .get(&url)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|e| anyhow!("获取文件列表失败: {e}"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let err_text = resp.text().await.unwrap_or_default();
                return Err(anyhow!("获取文件列表失败 ({}): {}", status, err_text));
            }

            let list: FileListResponse = resp
                .json()
                .await
                .map_err(|e| anyhow!("解析文件列表失败: {e}"))?;

            let entries: Vec<RemoteFileEntry> = list
                .files
                .into_iter()
                .map(|f| RemoteFileEntry {
                    name: f.name,
                    version: f.modified_time.clone().unwrap_or_else(|| f.id.clone()),
                })
                .collect();

            debug!("Google Drive: 获取到 {} 个远程文件", entries.len());
            Ok(entries)
        })
    }

    fn delete(&self, name: String) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
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
                folder_id: RwLock::new(None),
            };

            let (file_id, _) = backend.find_file_id_by_name(&name).await?;
            let url = format!("{drive_api}/{file_id}", drive_api = DRIVE_API_BASE);
            let resp = backend.send_delete(&url).await?;

            if resp.status().is_success() || resp.status() == 404 {
                info!("Google Drive: 删除成功 '{name}'");
                Ok(())
            } else {
                Err(anyhow!("删除失败 '{}': {}", name, resp.status()))
            }
        })
    }

    fn rename(&self, old: String, new: String) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
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
                folder_id: RwLock::new(None),
            };

            let (file_id, _) = backend.find_file_id_by_name(&old).await?;
            let url = format!("{drive_api}/{file_id}", drive_api = DRIVE_API_BASE);
            let resp = backend
                .send_patch(&url, RenameBody { name: new.clone() })
                .await?;

            if resp.status().is_success() {
                info!("Google Drive: 重命名成功 '{old}' -> '{new}'");
                Ok(())
            } else {
                Err(anyhow!("重命名失败: {}", resp.status()))
            }
        })
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
///
/// 1. 绑定本地端口，启动 HTTP 服务器
/// 2. 打开浏览器跳转到 Google 授权页面
/// 3. 接收回调，提取授权码
/// 4. 交换授权码为 refresh_token
///
/// 返回值: refresh_token
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
