use crate::backend::AttachmentBackend;
use crate::types::WebDavConfig;
use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use reqwest::{Client, Method};
use sha2::{Digest, Sha256};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::RwLock;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use unicode_normalization::UnicodeNormalization;
use urlencoding::decode;

/// WebDAV 文件同步后端
pub struct WebDavBackend {
    client: Client,
    config: RwLock<WebDavConfig>,
}

impl WebDavBackend {
    pub fn new(config: WebDavConfig) -> Self {
        Self {
            client: Client::new(),
            config: RwLock::new(config),
        }
    }

    fn get_effective_remote_path(&self) -> String {
        let path = {
            let c = self.config.read().unwrap();
            c.remote_path.clone()
        };
        if path.is_empty() {
            "Lumen".to_string()
        } else {
            path
        }
    }
}

impl AttachmentBackend for WebDavBackend {
    fn name(&self) -> &str {
        "webdav"
    }

    fn is_enabled(&self) -> bool {
        let c = self.config.read().unwrap();
        c.enabled && !c.endpoint.is_empty()
    }

    fn test_connection(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        let (enabled, endpoint, username, password) = {
            let c = self.config.read().unwrap();
            (
                c.enabled,
                c.endpoint.clone(),
                c.username.clone(),
                c.password.clone(),
            )
        };
        let client = self.client.clone();
        let remote_path = self.get_effective_remote_path();

        Box::pin(async move {
            if !enabled || endpoint.is_empty() {
                return Err(anyhow!("WebDAV 未启用或配置为空"));
            }

            let base_url = format!(
                "{}/{}",
                endpoint.trim_end_matches('/'),
                remote_path.trim_start_matches('/').trim_end_matches('/')
            );

            let resp = client
                .request(Method::from_bytes(b"PROPFIND").unwrap(), &base_url)
                .basic_auth(&username, Some(&password))
                .header("Depth", "0")
                .send()
                .await
                .map_err(|e| anyhow!("连接 WebDAV 失败: {e}"))?;

            let status = resp.status().as_u16();
            if status == 207 || resp.status().is_success() || status == 404 {
                Ok(())
            } else {
                Err(anyhow!("WebDAV 连接测试失败，状态码: {status}"))
            }
        })
    }

    fn inspect_library(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<crate::backend::LibraryInspection>> + Send>> {
        let (enabled, endpoint, username, password) = {
            let c = self.config.read().unwrap();
            (
                c.enabled,
                c.endpoint.clone(),
                c.username.clone(),
                c.password.clone(),
            )
        };
        let client = self.client.clone();
        let remote_path = self.get_effective_remote_path();

        Box::pin(async move {
            if !enabled || endpoint.is_empty() {
                return Err(anyhow!("WebDAV 未启用或配置为空"));
            }

            let base_url = format!(
                "{}/{}",
                endpoint.trim_end_matches('/'),
                remote_path.trim_start_matches('/').trim_end_matches('/')
            );

            let resp = client
                .request(Method::from_bytes(b"PROPFIND").unwrap(), &base_url)
                .basic_auth(&username, Some(&password))
                .header("Depth", "1")
                .send()
                .await
                .map_err(|e| anyhow!("WebDAV 根目录探测请求失败: {e}"))?;

            let status = resp.status().as_u16();
            if status == 404 {
                return Ok(crate::backend::LibraryInspection::MissingEmpty);
            }
            if status != 207 && !resp.status().is_success() {
                return Err(anyhow!("WebDAV 根目录探测失败，状态码: {status}"));
            }

            let xml_body = resp
                .text()
                .await
                .map_err(|e| anyhow!("读取 WebDAV 探测响应失败: {e}"))?;

            let items = parse_webdav_propfind_items(&xml_body, &base_url)?;

            match inspect_webdav_root_items(&items) {
                WebDavInspectDecision::SingleIdentity(id_item) => {
                    let id_url = format!(
                        "{}/{}",
                        endpoint.trim_end_matches('/'),
                        id_item.href.trim_start_matches('/')
                    );
                    let get_resp = client
                        .get(&id_url)
                        .basic_auth(&username, Some(&password))
                        .send()
                        .await
                        .map_err(|e| anyhow!("下载 identity 文件失败: {e}"))?;

                    if !get_resp.status().is_success() {
                        return Err(anyhow!(
                            "下载 identity 文件失败，状态码: {}",
                            get_resp.status()
                        ));
                    }

                    let json_text = get_resp
                        .text()
                        .await
                        .map_err(|e| anyhow!("读取 identity 文件内容失败: {e}"))?;

                    let identity = crate::backend::parse_file_library_identity_json(&json_text)?;
                    Ok(crate::backend::LibraryInspection::Present(identity))
                }
                WebDavInspectDecision::MultipleIdentities => {
                    Err(anyhow!("WebDAV 根目录下发现多个 identity 文件冲突"))
                }
                WebDavInspectDecision::IdentityIsDirectory => {
                    Err(anyhow!("WebDAV 根目录下的 identity 项是一个目录冲突"))
                }
                WebDavInspectDecision::MissingNonEmpty => {
                    Ok(crate::backend::LibraryInspection::MissingNonEmpty)
                }
                WebDavInspectDecision::MissingEmpty => {
                    Ok(crate::backend::LibraryInspection::MissingEmpty)
                }
                WebDavInspectDecision::NeedCheckObjectsDir => {
                    let objects_url = format!("{base_url}/objects");
                    let obj_resp = client
                        .request(Method::from_bytes(b"PROPFIND").unwrap(), &objects_url)
                        .basic_auth(&username, Some(&password))
                        .header("Depth", "1")
                        .send()
                        .await
                        .map_err(|e| anyhow!("WebDAV objects 目录探测请求失败: {e}"))?;

                    if obj_resp.status().as_u16() == 404 {
                        Ok(crate::backend::LibraryInspection::MissingEmpty)
                    } else if obj_resp.status().as_u16() == 207 || obj_resp.status().is_success() {
                        let obj_xml = obj_resp
                            .text()
                            .await
                            .map_err(|e| anyhow!("读取 WebDAV objects 探测响应失败: {e}"))?;
                        let obj_items = parse_webdav_propfind_items(&obj_xml, &objects_url)?;
                        if obj_items.is_empty() {
                            Ok(crate::backend::LibraryInspection::MissingEmpty)
                        } else {
                            Ok(crate::backend::LibraryInspection::MissingNonEmpty)
                        }
                    } else {
                        Err(anyhow!(
                            "WebDAV objects 探测失败，状态码: {}",
                            obj_resp.status()
                        ))
                    }
                }
            }
        })
    }

    fn initialize_library(
        &self,
        identity: crate::backend::FileLibraryIdentity,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        let (enabled, endpoint, username, password) = {
            let c = self.config.read().unwrap();
            (
                c.enabled,
                c.endpoint.clone(),
                c.username.clone(),
                c.password.clone(),
            )
        };
        let client = self.client.clone();
        let remote_path = self.get_effective_remote_path();

        Box::pin(async move {
            if !enabled || endpoint.is_empty() {
                return Err(anyhow!("WebDAV 未启用或配置为空"));
            }

            let base_url = format!(
                "{}/{}",
                endpoint.trim_end_matches('/'),
                remote_path.trim_start_matches('/').trim_end_matches('/')
            );

            // 1. 严格创建并验证根目录
            create_or_verify_collection(&client, &base_url, &username, &password)
                .await
                .map_err(|e| anyhow!("WebDAV 根目录校验失败: {e}"))?;

            // 2. 严格创建并验证 objects 父目录
            let objects_url = format!("{base_url}/objects");
            create_or_verify_collection(&client, &objects_url, &username, &password)
                .await
                .map_err(|e| anyhow!("WebDAV objects 父目录校验失败: {e}"))?;

            // 3. 严格创建并验证 objects/v1 目录
            let objects_v1_url = format!("{base_url}/objects/v1");
            create_or_verify_collection(&client, &objects_v1_url, &username, &password)
                .await
                .map_err(|e| anyhow!("WebDAV objects/v1 目录校验失败: {e}"))?;

            // 4. 条件创建 identity 文件（If-None-Match: *）
            let identity_url = format!("{base_url}/{IDENTITY_FILE_NAME}");
            let json_body = serde_json::to_vec_pretty(&identity)
                .map_err(|e| anyhow!("序列化 identity 失败: {e}"))?;

            let resp = client
                .request(Method::PUT, &identity_url)
                .basic_auth(&username, Some(&password))
                .header("If-None-Match", "*")
                .header("Content-Type", "application/json")
                .body(json_body)
                .send()
                .await
                .map_err(|e| anyhow!("WebDAV 初始化 identity PUT 请求网络错误: {e}"))?;

            let status = resp.status().as_u16();
            if status == 412 {
                // 如果已存在，校验内容是否完全一致（脱敏比较）
                let get_resp = client
                    .get(&identity_url)
                    .basic_auth(&username, Some(&password))
                    .send()
                    .await
                    .map_err(|e| anyhow!("读取已存在 identity 文件网络错误: {e}"))?;
                if get_resp.status().is_success() {
                    let existing_text = get_resp
                        .text()
                        .await
                        .map_err(|e| anyhow!("读取已存在 identity 文件失败: {e}"))?;
                    let existing_id =
                        crate::backend::parse_file_library_identity_json(&existing_text)
                            .map_err(|_| anyhow!("WebDAV 远端已存在损坏的 identity 文件"))?;
                    if existing_id != identity {
                        return Err(anyhow!(
                            "WebDAV 远端已存在不同资料库的 identity 文件，拒绝覆盖 (identity_conflict)"
                        ));
                    }
                } else {
                    return Err(anyhow!(
                        "WebDAV 远端已存在 identity 文件但读取失败 (identity_unreadable)"
                    ));
                }
            } else if !resp.status().is_success() {
                return Err(anyhow!("WebDAV 初始化 identity PUT 失败，状态码: {status}"));
            }

            // 5. 重新 inspect 确保进入 Present 状态
            let backend = WebDavBackend {
                client,
                config: RwLock::new(WebDavConfig {
                    enabled,
                    endpoint,
                    username,
                    password,
                    remote_path,
                }),
            };

            let inspection = backend.inspect_library().await?;
            match inspection {
                crate::backend::LibraryInspection::Present(p) if p == identity => Ok(()),
                _ => Err(anyhow!(
                    "WebDAV 初始化后重新探测未能确认有效 identity (identity_mismatch)"
                )),
            }
        })
    }

    fn list_objects(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<crate::backend::RemoteObjectEntry>>> + Send>> {
        let (enabled, endpoint, username, password) = {
            let c = self.config.read().unwrap();
            (
                c.enabled,
                c.endpoint.clone(),
                c.username.clone(),
                c.password.clone(),
            )
        };
        let client = self.client.clone();
        let remote_path = self.get_effective_remote_path();

        Box::pin(async move {
            if !enabled || endpoint.is_empty() {
                return Err(anyhow!("WebDAV 未启用或配置为空"));
            }

            let objects_v1_url = format!(
                "{}/{}/objects/v1",
                endpoint.trim_end_matches('/'),
                remote_path.trim_start_matches('/').trim_end_matches('/')
            );

            let resp = client
                .request(Method::from_bytes(b"PROPFIND").unwrap(), &objects_v1_url)
                .basic_auth(&username, Some(&password))
                .header("Depth", "1")
                .send()
                .await
                .map_err(|e| anyhow!("WebDAV 列出对象请求失败: {e}"))?;

            let status = resp.status().as_u16();
            if status == 404 {
                return Ok(Vec::new());
            }
            if status != 207 && !resp.status().is_success() {
                return Err(anyhow!("WebDAV 列出对象失败，状态码: {status}"));
            }

            let xml_body = resp
                .text()
                .await
                .map_err(|e| anyhow!("读取 WebDAV 对象列表响应失败: {e}"))?;

            let items = parse_webdav_propfind_items(&xml_body, &objects_v1_url)?;
            parse_webdav_objects_list(&items)
        })
    }

    fn upload_object_if_absent(
        &self,
        object_key: String,
        local_path: PathBuf,
    ) -> Pin<Box<dyn Future<Output = Result<crate::backend::UploadObjectResult>> + Send>> {
        let (enabled, endpoint, username, password) = {
            let c = self.config.read().unwrap();
            (
                c.enabled,
                c.endpoint.clone(),
                c.username.clone(),
                c.password.clone(),
            )
        };
        let client = self.client.clone();
        let remote_path = self.get_effective_remote_path();

        Box::pin(async move {
            if !enabled || endpoint.is_empty() {
                return Err(anyhow!("WebDAV 未启用或配置为空"));
            }

            let leaf = crate::backend::validate_canonical_object_key(&object_key)?;
            let target_url = format!(
                "{}/{}/objects/v1/{}",
                endpoint.trim_end_matches('/'),
                remote_path.trim_start_matches('/').trim_end_matches('/'),
                leaf
            );

            let file = tokio::fs::File::open(&local_path)
                .await
                .map_err(|e| anyhow!("打开本地待上传文件 '{}' 失败: {e}", local_path.display()))?;

            let file_size = file
                .metadata()
                .await
                .map_err(|e| anyhow!("读取待上传文件元数据失败: {e}"))?
                .len();

            let stream = ReaderStream::new(file);
            let body = reqwest::Body::wrap_stream(stream);

            let resp = client
                .request(Method::PUT, &target_url)
                .basic_auth(&username, Some(&password))
                .header("If-None-Match", "*")
                .header("Content-Length", file_size)
                .body(body)
                .send()
                .await
                .map_err(|e| anyhow!("WebDAV 上传对象请求失败: {e}"))?;

            let status = resp.status().as_u16();
            if status == 412 {
                return Ok(crate::backend::UploadObjectResult::AlreadyExists);
            }
            if !resp.status().is_success() {
                return Err(anyhow!("WebDAV 上传对象失败，状态码: {status}"));
            }

            let raw_etag = resp.headers().get("ETag").and_then(|h| h.to_str().ok());

            let remote_version = match normalize_required_etag(raw_etag) {
                Some(v) => v,
                None => {
                    let head_resp = client
                        .head(&target_url)
                        .basic_auth(&username, Some(&password))
                        .send()
                        .await
                        .map_err(|e| anyhow!("WebDAV 获取对象 ETag 失败: {e}"))?;
                    let head_etag = head_resp
                        .headers()
                        .get("ETag")
                        .and_then(|h| h.to_str().ok());
                    normalize_required_etag(head_etag)
                        .ok_or_else(|| anyhow!("WebDAV 上传后未获取到有效 ETag"))?
                }
            };

            Ok(crate::backend::UploadObjectResult::Created(remote_version))
        })
    }

    fn download_object(
        &self,
        object_key: String,
        temporary_path: PathBuf,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send>> {
        let (enabled, endpoint, username, password) = {
            let c = self.config.read().unwrap();
            (
                c.enabled,
                c.endpoint.clone(),
                c.username.clone(),
                c.password.clone(),
            )
        };
        let client = self.client.clone();
        let remote_path = self.get_effective_remote_path();

        Box::pin(async move {
            if !enabled || endpoint.is_empty() {
                return Err(anyhow!("WebDAV 未启用或配置为空"));
            }

            let leaf = crate::backend::validate_canonical_object_key(&object_key)?;
            let target_url = format!(
                "{}/{}/objects/v1/{}",
                endpoint.trim_end_matches('/'),
                remote_path.trim_start_matches('/').trim_end_matches('/'),
                leaf
            );

            let resp = client
                .get(&target_url)
                .basic_auth(&username, Some(&password))
                .send()
                .await
                .map_err(|e| anyhow!("WebDAV 下载对象请求失败: {e}"))?;

            let status = resp.status().as_u16();
            if status == 404 {
                return Ok(None);
            }
            if !resp.status().is_success() {
                return Err(anyhow!("WebDAV 下载对象失败，状态码: {status}"));
            }

            let etag_from_get =
                normalize_required_etag(resp.headers().get("ETag").and_then(|h| h.to_str().ok()));

            if let Some(parent) = temporary_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            let mut file = tokio::fs::File::create(&temporary_path)
                .await
                .map_err(|e| anyhow!("创建临时下载文件失败: {e}"))?;

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
            drop(file);

            // 若 GET 响应缺失 ETag，HEAD 补查
            let remote_version = if let Some(v) = etag_from_get {
                v
            } else {
                let head_resp = client
                    .head(&target_url)
                    .basic_auth(&username, Some(&password))
                    .send()
                    .await
                    .map_err(|e| anyhow!("WebDAV 下载后 HEAD 补查 ETag 请求失败: {e}"))?;
                let head_etag = head_resp
                    .headers()
                    .get("ETag")
                    .and_then(|h| h.to_str().ok());
                match normalize_required_etag(head_etag) {
                    Some(v) => v,
                    None => {
                        let _ = tokio::fs::remove_file(&temporary_path).await;
                        return Err(anyhow!(
                            "WebDAV 下载成功但未获取到有效 ETag，已删除临时文件"
                        ));
                    }
                }
            };

            Ok(Some(remote_version))
        })
    }

    fn delete_object(
        &self,
        object_key: String,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        let (enabled, endpoint, username, password) = {
            let c = self.config.read().unwrap();
            (
                c.enabled,
                c.endpoint.clone(),
                c.username.clone(),
                c.password.clone(),
            )
        };
        let client = self.client.clone();
        let remote_path = self.get_effective_remote_path();

        Box::pin(async move {
            if !enabled || endpoint.is_empty() {
                return Err(anyhow!("WebDAV 未启用或配置为空"));
            }

            let leaf = crate::backend::validate_canonical_object_key(&object_key)?;
            let target_url = format!(
                "{}/{}/objects/v1/{}",
                endpoint.trim_end_matches('/'),
                remote_path.trim_start_matches('/').trim_end_matches('/'),
                leaf
            );

            let resp = client
                .request(Method::DELETE, &target_url)
                .basic_auth(&username, Some(&password))
                .send()
                .await
                .map_err(|e| anyhow!("WebDAV 删除对象请求失败: {e}"))?;

            let status = resp.status().as_u16();
            if status == 404 || resp.status().is_success() {
                Ok(())
            } else {
                Err(anyhow!("WebDAV 删除对象失败，状态码: {status}"))
            }
        })
    }

    fn configuration_fingerprint(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
        let (endpoint, username) = {
            let c = self.config.read().unwrap();
            (c.endpoint.clone(), c.username.clone())
        };
        let remote_path = self.get_effective_remote_path();
        // 规范化 endpoint：统一 trim 尾部斜杠后转小写
        let normalized_endpoint = endpoint.trim_end_matches('/').to_lowercase();
        let raw = format!(
            "webdav:{e}:{p}:{u}",
            e = normalized_endpoint,
            p = remote_path,
            u = username,
        );
        let mut hasher = Sha256::new();
        hasher.update(raw.as_bytes());
        let hash = hasher.finalize();
        let fp = hash
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        Box::pin(async move { Ok(fp) })
    }
}

/// 规范化并提取必选的 ETag 字符串（若为空、仅双引号或去除 W/ 后为空则返回 None）
pub(crate) fn normalize_required_etag(raw: Option<&str>) -> Option<String> {
    raw.and_then(|s| {
        let cleaned = s
            .trim_matches('"')
            .trim_start_matches("W/")
            .trim_matches('"')
            .trim();
        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned.to_string())
        }
    })
}

/// 严格解析 Depth: 0 PROPFIND 多状态 XML，验证目标路径自身是否为合法的 collection 目录。
///
/// 严格断言：
/// 1. XML 必须且仅能包含一个 `<response>`；
/// 2. `<response>` 中必须且仅能包含一个 `<href>`；
/// 3. `<href>` 规范化路径分段必须与目标 `expected_url` 的路径分段完全一致；
/// 4. 该 `<response>` 中必须声明 `<resourcetype><collection/></resourcetype>`。
pub(crate) fn parse_webdav_depth0_is_collection(xml: &str, expected_url: &str) -> Result<bool> {
    let expected_segments = extract_path_segments(expected_url)?;
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut response_count = 0usize;
    let mut current_href = String::new();
    let mut current_href_count = 0usize;
    let mut current_is_collection = false;
    let mut in_href = false;
    let mut in_response = false;
    let mut in_resourcetype = false;
    let mut final_is_collection = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                "response" => {
                    if in_response {
                        return Err(anyhow!("WebDAV Depth:0 XML 包含嵌套的 response"));
                    }
                    in_response = true;
                    response_count += 1;
                    if response_count > 1 {
                        return Err(anyhow!(
                            "WebDAV Depth:0 响应包含多个 response，无法明确断言目标目录"
                        ));
                    }
                    current_href.clear();
                    current_href_count = 0;
                    current_is_collection = false;
                    in_resourcetype = false;
                }
                "href" if in_response => {
                    in_href = true;
                    current_href_count += 1;
                    if current_href_count > 1 {
                        return Err(anyhow!("WebDAV Depth:0 response 中包含多个 href"));
                    }
                }
                "resourcetype" if in_response => {
                    in_resourcetype = true;
                }
                "collection" if in_response && in_resourcetype => {
                    current_is_collection = true;
                }
                _ => {}
            },
            Ok(Event::Empty(ref e)) => match e.local_name().as_ref() {
                "collection" if in_response && in_resourcetype => {
                    current_is_collection = true;
                }
                _ => {}
            },
            Ok(Event::Text(ref e)) => {
                if in_href {
                    current_href = e.as_ref().to_string();
                }
            }
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                "href" => in_href = false,
                "resourcetype" => in_resourcetype = false,
                "response" => {
                    in_response = false;
                    in_resourcetype = false;
                    if current_href_count != 1 || current_href.is_empty() {
                        return Err(anyhow!("WebDAV Depth:0 response 必须有且仅有一个 href"));
                    }
                    let href_segments = extract_path_segments(&current_href)?;
                    if href_segments != expected_segments {
                        return Err(anyhow!(
                            "WebDAV Depth:0 href '{current_href}' 与预期目标 '{expected_url}' 路径不一致"
                        ));
                    }
                    final_is_collection = current_is_collection;
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("解析 WebDAV Depth:0 XML 失败: {e}")),
            _ => {}
        }
    }

    if response_count == 0 {
        return Err(anyhow!("WebDAV Depth:0 响应缺少 response 条目"));
    }

    Ok(final_is_collection)
}

/// 严格创建或验证指定 WebDAV 路径确为 collection 目录
///
/// 顺序：MKCOL -> 201/405/200 -> PROPFIND Depth: 0 -> 严格解析唯一 response 且包含 collection
pub(crate) async fn create_or_verify_collection(
    client: &Client,
    url: &str,
    username: &str,
    password: &str,
) -> Result<()> {
    let mkcol_resp = client
        .request(Method::from_bytes(b"MKCOL").unwrap(), url)
        .basic_auth(username, Some(password))
        .send()
        .await
        .map_err(|e| anyhow!("MKCOL 网络请求失败: {e}"))?;

    let status = mkcol_resp.status().as_u16();
    if status != 201 && status != 405 && status != 200 {
        return Err(anyhow!("MKCOL 状态码非预期: {status}"));
    }

    // Depth: 0 验证目标自身确为 collection
    let propfind_resp = client
        .request(Method::from_bytes(b"PROPFIND").unwrap(), url)
        .basic_auth(username, Some(password))
        .header("Depth", "0")
        .send()
        .await
        .map_err(|e| anyhow!("PROPFIND Depth: 0 网络请求失败: {e}"))?;

    let p_status = propfind_resp.status().as_u16();
    if p_status != 207 && !propfind_resp.status().is_success() {
        return Err(anyhow!("PROPFIND Depth: 0 失败，状态码: {p_status}"));
    }

    let xml = propfind_resp
        .text()
        .await
        .map_err(|e| anyhow!("读取 PROPFIND XML 失败: {e}"))?;

    let is_collection = parse_webdav_depth0_is_collection(&xml, url)?;
    if !is_collection {
        return Err(anyhow!("目标路径存在但并非有效 collection 目录"));
    }

    Ok(())
}

const IDENTITY_FILE_NAME: &str = ".lumen-file-library-v1.json";

/// WebDAV 子项条目
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WebDavItem {
    pub href: String,
    pub leaf_name: String,
    pub is_collection: bool,
    pub etag: Option<String>,
}

/// WebDAV 根目录探测决策
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WebDavInspectDecision {
    SingleIdentity(WebDavItem),
    MultipleIdentities,
    IdentityIsDirectory,
    MissingNonEmpty,
    MissingEmpty,
    NeedCheckObjectsDir,
}

/// 纯函数：根据根目录 WebDAV 子项决策探测状态
pub(crate) fn inspect_webdav_root_items(items: &[WebDavItem]) -> WebDavInspectDecision {
    let identity_items: Vec<&WebDavItem> = items
        .iter()
        .filter(|it| it.leaf_name == IDENTITY_FILE_NAME)
        .collect();

    if identity_items.len() > 1 {
        return WebDavInspectDecision::MultipleIdentities;
    }

    if let Some(id_item) = identity_items.first() {
        if id_item.is_collection {
            return WebDavInspectDecision::IdentityIsDirectory;
        }
        return WebDavInspectDecision::SingleIdentity((*id_item).clone());
    }

    let has_non_identity_files = items.iter().any(|it| !it.is_collection);
    let has_unknown_dirs = items
        .iter()
        .any(|it| it.is_collection && it.leaf_name != "objects" && it.leaf_name != "objects/");

    if has_non_identity_files || has_unknown_dirs {
        return WebDavInspectDecision::MissingNonEmpty;
    }

    let has_objects_dir = items
        .iter()
        .any(|it| it.is_collection && (it.leaf_name == "objects" || it.leaf_name == "objects/"));
    if has_objects_dir {
        return WebDavInspectDecision::NeedCheckObjectsDir;
    }

    WebDavInspectDecision::MissingEmpty
}

/// 提取 URL 或相对路径中的归一化路径分段
fn extract_path_segments(path_or_url: &str) -> Result<Vec<String>> {
    let raw_path = if let Some(idx) = path_or_url.find("://") {
        let after_scheme = &path_or_url[idx + 3..];
        match after_scheme.find('/') {
            Some(p_idx) => &after_scheme[p_idx..],
            None => "/",
        }
    } else {
        path_or_url
    };

    let decoded = decode(raw_path)
        .map(|s| s.nfc().collect::<String>())
        .unwrap_or_else(|_| raw_path.to_string());

    let mut segments = Vec::new();
    for seg in decoded.split('/') {
        let seg = seg.trim();
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            return Err(anyhow!("路径包含非法的回退分段 '..'"));
        }
        segments.push(seg.to_string());
    }
    Ok(segments)
}

/// 解析 PROPFIND 多状态 XML，严格提取所有直接子项（验证直接下一层，拒绝深层路径和逃逸）
pub(crate) fn parse_webdav_propfind_items(
    xml: &str,
    request_path: &str,
) -> Result<Vec<WebDavItem>> {
    let req_segments = extract_path_segments(request_path)?;
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut items = Vec::new();
    let mut current_href = String::new();
    let mut current_etag = String::new();
    let mut current_is_collection = false;
    let mut current_href_count = 0usize;
    let mut in_href = false;
    let mut in_etag = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                "href" => {
                    in_href = true;
                    current_href_count += 1;
                    if current_href_count > 1 {
                        return Err(anyhow!("WebDAV 响应条目包含多个 href"));
                    }
                }
                "getetag" => in_etag = true,
                "collection" => current_is_collection = true,
                _ => {}
            },
            Ok(Event::Empty(ref e)) => {
                if e.local_name().as_ref() == "collection" {
                    current_is_collection = true;
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_href {
                    current_href = e.as_ref().to_string();
                } else if in_etag {
                    current_etag = e.as_ref().to_string();
                }
            }
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                "href" => in_href = false,
                "getetag" => in_etag = false,
                "response" => {
                    if current_href_count != 1 || current_href.is_empty() {
                        return Err(anyhow!("WebDAV 响应条目必须有且仅有一个 href"));
                    }
                    let href_segments = extract_path_segments(&current_href)?;

                    if href_segments.len() < req_segments.len() {
                        return Err(anyhow!(
                            "WebDAV 响应 href '{current_href}' 层级高于请求路径 '{request_path}'"
                        ));
                    }

                    let is_prefix = req_segments
                        .iter()
                        .enumerate()
                        .all(|(i, req_seg)| href_segments[i].eq_ignore_ascii_case(req_seg));

                    if !is_prefix {
                        return Err(anyhow!(
                            "WebDAV 响应 href '{current_href}' 与请求路径 '{request_path}' 前缀不匹配"
                        ));
                    }

                    if href_segments.len() == req_segments.len() {
                        // 请求目录自身，跳过
                    } else if href_segments.len() == req_segments.len() + 1 {
                        // 直接子项
                        let leaf_name = href_segments[req_segments.len()].clone();
                        let clean_etag = if current_etag.is_empty() {
                            None
                        } else {
                            Some(
                                current_etag
                                    .trim_matches('"')
                                    .trim_start_matches("W/")
                                    .trim_matches('"')
                                    .to_string(),
                            )
                        };

                        items.push(WebDavItem {
                            href: current_href.clone(),
                            leaf_name,
                            is_collection: current_is_collection,
                            etag: clean_etag,
                        });
                    } else {
                        // 深层嵌套项，违反 Depth: 1 直接子项约定
                        return Err(anyhow!(
                            "WebDAV 响应 href '{current_href}' 为深层嵌套路径，非请求路径 '{request_path}' 的直接子项"
                        ));
                    }
                    current_href.clear();
                    current_etag.clear();
                    current_is_collection = false;
                    current_href_count = 0;
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("解析 WebDAV PROPFIND XML 失败: {e}")),
            _ => {}
        }
    }

    Ok(items)
}

/// 解析 objects/v1 的子项列表，生成严格 RemoteObjectEntry 列表
pub(crate) fn parse_webdav_objects_list(
    items: &[WebDavItem],
) -> Result<Vec<crate::backend::RemoteObjectEntry>> {
    use std::collections::HashSet;
    let mut seen_keys = HashSet::new();
    let mut entries = Vec::with_capacity(items.len());

    for it in items {
        if it.is_collection {
            return Err(anyhow!("objects/v1 目录包含意外的子目录: {}", it.leaf_name));
        }

        crate::backend::validate_canonical_uuid(&it.leaf_name, "object leaf")?;
        let object_key = format!("objects/v1/{}", it.leaf_name);

        let remote_version = it
            .etag
            .as_ref()
            .filter(|s| !s.is_empty())
            .cloned()
            .ok_or_else(|| anyhow!("对象缺失 ETag 版本标识: {object_key}"))?;

        if !seen_keys.insert(object_key.clone()) {
            return Err(anyhow!("objects/v1 中发现重复对象键: {object_key}"));
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
    fn parse_propfind_multistatus_with_identity_and_files() {
        let xml = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/Lumen/</d:href>
    <d:propstat>
      <d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/Lumen/.lumen-file-library-v1.json</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"etag-id-123"</d:getetag>
        <d:resourcetype/>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/Lumen/objects/</d:href>
    <d:propstat>
      <d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

        let items = parse_webdav_propfind_items(xml, "/Lumen").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].leaf_name, ".lumen-file-library-v1.json");
        assert!(!items[0].is_collection);
        assert_eq!(items[0].etag.as_deref(), Some("etag-id-123"));

        assert_eq!(items[1].leaf_name, "objects");
        assert!(items[1].is_collection);
    }

    #[test]
    fn parse_webdav_objects_list_success() {
        let items = vec![
            WebDavItem {
                href: "/Lumen/objects/v1/550e8400-e29b-41d4-a716-446655440000".to_string(),
                leaf_name: "550e8400-e29b-41d4-a716-446655440000".to_string(),
                is_collection: false,
                etag: Some("etag-1".to_string()),
            },
            WebDavItem {
                href: "/Lumen/objects/v1/6ba7b810-9dad-11d1-80b4-00c04fd430c8".to_string(),
                leaf_name: "6ba7b810-9dad-11d1-80b4-00c04fd430c8".to_string(),
                is_collection: false,
                etag: Some("etag-2".to_string()),
            },
        ];

        let entries = parse_webdav_objects_list(&items).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].object_key,
            "objects/v1/550e8400-e29b-41d4-a716-446655440000"
        );
        assert_eq!(entries[0].remote_version, "etag-1");
        assert_eq!(
            entries[1].object_key,
            "objects/v1/6ba7b810-9dad-11d1-80b4-00c04fd430c8"
        );
        assert_eq!(entries[1].remote_version, "etag-2");
    }

    #[test]
    fn parse_propfind_multistatus_rejects_deep_href_and_mismatch() {
        // Deep nested href rejected
        let xml_deep = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/Lumen/objects/v1/sub/extra.pdf</d:href>
    <d:propstat>
      <d:prop><d:getetag>"etag-1"</d:getetag><d:resourcetype/></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        assert!(parse_webdav_propfind_items(xml_deep, "/Lumen/objects/v1").is_err());

        // Prefix mismatch rejected
        let xml_mismatch = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/OtherFolder/file.pdf</d:href>
    <d:propstat>
      <d:prop><d:getetag>"etag-1"</d:getetag><d:resourcetype/></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        assert!(parse_webdav_propfind_items(xml_mismatch, "/Lumen").is_err());

        // Traversal rejected
        let xml_traversal = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/Lumen/../secret.txt</d:href>
    <d:propstat>
      <d:prop><d:getetag>"etag-1"</d:getetag><d:resourcetype/></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        assert!(parse_webdav_propfind_items(xml_traversal, "/Lumen").is_err());
    }

    #[test]
    fn parse_propfind_multistatus_rejects_missing_href() {
        let xml_missing_href = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:propstat>
      <d:prop><d:getetag>"etag-1"</d:getetag><d:resourcetype/></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        assert!(parse_webdav_propfind_items(xml_missing_href, "/Lumen").is_err());
    }

    #[test]
    fn parse_propfind_multistatus_rejects_multiple_hrefs() {
        let xml_multiple_hrefs = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/Lumen/file1.pdf</d:href>
    <d:href>/Lumen/file2.pdf</d:href>
    <d:propstat>
      <d:prop><d:getetag>"etag-1"</d:getetag><d:resourcetype/></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        assert!(parse_webdav_propfind_items(xml_multiple_hrefs, "/Lumen").is_err());
    }

    #[test]
    fn parse_webdav_objects_list_rejects_subfolders_missing_etag_and_duplicates() {
        // Subfolder in objects/v1
        let folder_item = vec![WebDavItem {
            href: "/Lumen/objects/v1/sub".to_string(),
            leaf_name: "sub".to_string(),
            is_collection: true,
            etag: None,
        }];
        assert!(parse_webdav_objects_list(&folder_item).is_err());

        // Non-UUID name
        let invalid_name = vec![WebDavItem {
            href: "/Lumen/objects/v1/some_paper.pdf".to_string(),
            leaf_name: "some_paper.pdf".to_string(),
            is_collection: false,
            etag: Some("e1".to_string()),
        }];
        assert!(parse_webdav_objects_list(&invalid_name).is_err());

        // Missing ETag
        let no_etag = vec![WebDavItem {
            href: "/Lumen/objects/v1/550e8400-e29b-41d4-a716-446655440000".to_string(),
            leaf_name: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            is_collection: false,
            etag: None,
        }];
        assert!(parse_webdav_objects_list(&no_etag).is_err());

        // Duplicate UUID
        let dup = vec![
            WebDavItem {
                href: "/Lumen/objects/v1/550e8400-e29b-41d4-a716-446655440000".to_string(),
                leaf_name: "550e8400-e29b-41d4-a716-446655440000".to_string(),
                is_collection: false,
                etag: Some("e1".to_string()),
            },
            WebDavItem {
                href: "/Lumen/objects/v1/550e8400-e29b-41d4-a716-446655440000".to_string(),
                leaf_name: "550e8400-e29b-41d4-a716-446655440000".to_string(),
                is_collection: false,
                etag: Some("e2".to_string()),
            },
        ];
        assert!(parse_webdav_objects_list(&dup).is_err());
    }

    #[test]
    fn inspect_webdav_root_items_rejects_duplicate_identity_and_directory_conflict() {
        let id_item1 = WebDavItem {
            href: "/Lumen/.lumen-file-library-v1.json".to_string(),
            leaf_name: IDENTITY_FILE_NAME.to_string(),
            is_collection: false,
            etag: Some("etag-1".to_string()),
        };

        // 单合法 identity 文件
        assert_eq!(
            inspect_webdav_root_items(&[id_item1.clone()]),
            WebDavInspectDecision::SingleIdentity(id_item1.clone())
        );

        // 重复 identity 文件冲突
        let id_item2 = WebDavItem {
            href: "/Lumen/.lumen-file-library-v1.json".to_string(),
            leaf_name: IDENTITY_FILE_NAME.to_string(),
            is_collection: false,
            etag: Some("etag-2".to_string()),
        };
        assert_eq!(
            inspect_webdav_root_items(&[id_item1.clone(), id_item2]),
            WebDavInspectDecision::MultipleIdentities
        );

        // Identity 项为目录（类型冲突）
        let id_dir = WebDavItem {
            href: "/Lumen/.lumen-file-library-v1.json/".to_string(),
            leaf_name: IDENTITY_FILE_NAME.to_string(),
            is_collection: true,
            etag: None,
        };
        assert_eq!(
            inspect_webdav_root_items(&[id_dir]),
            WebDavInspectDecision::IdentityIsDirectory
        );
    }

    #[test]
    fn parse_webdav_depth0_is_collection_matrix() {
        let valid_collection_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/remote.php/dav/files/user/Lumen/objects</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype>
          <d:collection/>
        </d:resourcetype>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

        assert_eq!(
            parse_webdav_depth0_is_collection(
                valid_collection_xml,
                "https://dav.example.com/remote.php/dav/files/user/Lumen/objects"
            )
            .unwrap(),
            true
        );

        let file_not_collection_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/remote.php/dav/files/user/Lumen/objects</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype/>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

        assert_eq!(
            parse_webdav_depth0_is_collection(
                file_not_collection_xml,
                "https://dav.example.com/remote.php/dav/files/user/Lumen/objects"
            )
            .unwrap(),
            false
        );

        let mismatched_href_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/remote.php/dav/files/user/OtherDir</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype><d:collection/></d:resourcetype>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

        assert!(
            parse_webdav_depth0_is_collection(
                mismatched_href_xml,
                "https://dav.example.com/remote.php/dav/files/user/Lumen/objects"
            )
            .is_err()
        );

        let multi_response_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/remote.php/dav/files/user/Lumen/objects</d:href>
    <d:propstat>
      <d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/dav/files/user/Lumen/objects/sub</d:href>
    <d:propstat>
      <d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

        assert!(
            parse_webdav_depth0_is_collection(
                multi_response_xml,
                "https://dav.example.com/remote.php/dav/files/user/Lumen/objects"
            )
            .is_err()
        );

        // 1. 无关 collection 标签（不在 resourcetype 内部，而在自定义属性内）必须返回 false
        let collection_outside_resourcetype_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/remote.php/dav/files/user/Lumen/objects</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype/>
        <custom:tag xmlns:custom="http://example.com/ns">
          <collection/>
        </custom:tag>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

        assert_eq!(
            parse_webdav_depth0_is_collection(
                collection_outside_resourcetype_xml,
                "https://dav.example.com/remote.php/dav/files/user/Lumen/objects"
            )
            .unwrap(),
            false
        );

        // 2. 同一 response 包含多个 href 必须拒绝报错
        let multiple_hrefs_in_single_response_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/remote.php/dav/files/user/Lumen/objects</d:href>
    <d:href>/remote.php/dav/files/user/Lumen/objects_alias</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype><d:collection/></d:resourcetype>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

        assert!(
            parse_webdav_depth0_is_collection(
                multiple_hrefs_in_single_response_xml,
                "https://dav.example.com/remote.php/dav/files/user/Lumen/objects"
            )
            .is_err()
        );
    }
}
