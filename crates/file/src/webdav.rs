use crate::backend::{AttachmentBackend, RemoteFileEntry};
use crate::types::WebDavConfig;
use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use log::{error, info};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use reqwest::{Client, Method};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::RwLock;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use tokio_util::io::StreamReader;
use unicode_normalization::UnicodeNormalization;
use urlencoding::{decode, encode};

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
        let c = self.config.read().unwrap();
        format!("{}/Lumen", c.remote_path.trim_end_matches('/'))
    }
}

impl AttachmentBackend for WebDavBackend {
    fn name(&self) -> &str {
        "webdav"
    }

    fn is_enabled(&self) -> bool {
        self.config.read().unwrap().enabled
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

        Box::pin(async move {
            if !enabled || endpoint.is_empty() {
                return Err(anyhow!("WebDAV 未启用或地址为空"));
            }

            let resp = client
                .request(Method::from_bytes(b"PROPFIND")?, &endpoint)
                .basic_auth(&username, Some(&password))
                .header("Depth", "0")
                .send()
                .await
                .map_err(|e| anyhow!(e))?;

            if !resp.status().is_success() {
                return Err(anyhow!("连接失败: {}", resp.status()));
            }

            // 确保/Lumen子目录存在
            let dir_url = format!("{}/Lumen", endpoint.trim_end_matches('/'));

            let resp = client
                .request(Method::from_bytes(b"MKCOL")?, dir_url)
                .basic_auth(&username, Some(&password))
                .send()
                .await
                .map_err(|e| anyhow!(e))?;

            if resp.status().is_success() || resp.status() == 405 {
                Ok(())
            } else {
                Err(anyhow!("创建远程目录失败: {}", resp.status()))
            }
        })
    }

    fn upload(
        &self,
        local_path: PathBuf,
        remote_filename: String,
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

        let remote_path = self.get_effective_remote_path();
        let url = format!(
            "{}/{}",
            endpoint.trim_end_matches('/'),
            format!(
                "{}/{}",
                remote_path.trim_start_matches('/').trim_end_matches('/'),
                encode(&remote_filename)
            )
            .trim_start_matches('/')
        );

        let client = self.client.clone();

        Box::pin(async move {
            if !enabled {
                return Err(anyhow!("WebDAV 未启用"));
            }

            let file = tokio::fs::File::open(&local_path)
                .await
                .map_err(|e| anyhow!("打开文件失败 '{}': {}", local_path.display(), e))?;
            let file_size = file.metadata().await?.len();
            info!(
                "WebDAV: 准备上传 '{}', 大小: {} 字节 ({:.2} MB)",
                remote_filename,
                file_size,
                file_size as f64 / 1_048_576.0
            );

            let stream = ReaderStream::new(file);
            let body = reqwest::Body::wrap_stream(stream);

            let resp = client
                .request(Method::PUT, &url)
                .basic_auth(&username, Some(&password))
                .header("Content-Length", file_size)
                .body(body)
                .send()
                .await
                .map_err(|e| anyhow!(e))?;

            if resp.status().is_success() || resp.status() == 201 || resp.status() == 204 {
                let etag = resp
                    .headers()
                    .get("ETag")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.trim_matches('"').to_string());
                info!("WebDAV: 上传成功 {remote_filename}, ETag: {etag:?}");
                Ok(etag)
            } else {
                error!(
                    "WebDAV: 上传失败 {}, 状态码: {}",
                    remote_filename,
                    resp.status()
                );
                Err(anyhow!("上传失败: {}", resp.status()))
            }
        })
    }

    fn download(
        &self,
        remote_filename: String,
        local_path: PathBuf,
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

        let remote_path = self.get_effective_remote_path();
        let url = format!(
            "{}/{}",
            endpoint.trim_end_matches('/'),
            format!(
                "{}/{}",
                remote_path.trim_start_matches('/').trim_end_matches('/'),
                encode(&remote_filename)
            )
            .trim_start_matches('/')
        );

        let client = self.client.clone();

        Box::pin(async move {
            if !enabled {
                return Err(anyhow!("WebDAV 未启用"));
            }

            if let Some(parent) = local_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            let resp = client
                .request(Method::GET, &url)
                .basic_auth(&username, Some(&password))
                .send()
                .await
                .map_err(|e| anyhow!(e))?;

            if !resp.status().is_success() {
                return Err(anyhow!("下载失败: {}", resp.status()));
            }

            let etag = resp
                .headers()
                .get("ETag")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim_matches('"').to_string());

            let mut dest_file = tokio::fs::File::create(&local_path)
                .await
                .map_err(|e| anyhow!("创建目标文件失败: {e}"))?;

            let stream = resp
                .bytes_stream()
                .map(|r| r.map_err(std::io::Error::other));
            let mut reader = StreamReader::new(stream);
            tokio::io::copy(&mut reader, &mut dest_file)
                .await
                .map_err(|e| anyhow!("写入文件失败: {e}"))?;

            dest_file.flush().await?;

            info!("WebDAV: 下载成功 {remote_filename}, ETag: {etag:?}");
            Ok(etag)
        })
    }

    fn list(&self) -> Pin<Box<dyn Future<Output = Result<Vec<RemoteFileEntry>>> + Send>> {
        let enabled = self.config.read().unwrap().enabled;
        if !enabled {
            return Box::pin(async { Ok(Vec::new()) });
        }
        let remote_path = self.get_effective_remote_path();
        let (endpoint, username, password) = {
            let c = self.config.read().unwrap();
            (c.endpoint.clone(), c.username.clone(), c.password.clone())
        };
        let client = self.client.clone();

        Box::pin(async move {
            let url = format!(
                "{}/{}",
                endpoint.trim_end_matches('/'),
                remote_path.trim_start_matches('/')
            );

            let rb = client
                .request(Method::from_bytes(b"PROPFIND")?, url)
                .basic_auth(&username, Some(&password))
                .header("Depth", "1");

            let resp = rb.send().await.map_err(|e| anyhow!(e))?;
            if !resp.status().is_success() {
                return Err(anyhow!("获取文件列表失败: {}", resp.status()));
            }

            let body = resp.text().await?;
            let mut reader = Reader::from_str(&body);
            reader.config_mut().trim_text(true);

            let mut entries = Vec::new();
            let mut current_href = String::new();
            let mut current_etag = String::new();
            let mut in_href = false;
            let mut in_etag = false;

            loop {
                match reader.read_event() {
                    Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                        "href" => in_href = true,
                        "getetag" => in_etag = true,
                        _ => {}
                    },
                    Ok(Event::Text(ref e)) => {
                        if in_href {
                            current_href = e.as_ref().to_string();
                        } else if in_etag {
                            current_etag = e.as_ref().trim_matches('"').to_string();
                        }
                    }
                    Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                        "href" => in_href = false,
                        "getetag" => in_etag = false,
                        "response" => {
                            if !current_href.is_empty()
                                && !current_etag.is_empty()
                                && let Some(filename) = current_href.split('/').next_back()
                                && !filename.is_empty()
                            {
                                let key = match decode(filename) {
                                    Ok(decoded) => decoded.nfc().collect::<String>(),
                                    Err(_) => filename.to_string(),
                                };
                                if key != "lumen" {
                                    entries.push(RemoteFileEntry {
                                        name: key,
                                        version: current_etag.clone(),
                                    });
                                }
                            }
                            current_href.clear();
                            current_etag.clear();
                        }
                        _ => {}
                    },
                    Ok(Event::Eof) => break,
                    Err(e) => return Err(anyhow!("解析 XML 失败: {e}")),
                    _ => {}
                }
            }

            Ok(entries)
        })
    }

    fn delete(&self, name: String) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        let enabled = self.config.read().unwrap().enabled;
        if !enabled {
            return Box::pin(async { Err(anyhow!("WebDAV 未启用")) });
        }

        let remote_path = self.get_effective_remote_path();
        let (endpoint, username, password) = {
            let c = self.config.read().unwrap();
            (c.endpoint.clone(), c.username.clone(), c.password.clone())
        };
        let url = format!(
            "{}/{}",
            endpoint.trim_end_matches('/'),
            format!(
                "{}/{}",
                remote_path.trim_start_matches('/').trim_end_matches('/'),
                encode(&name)
            )
            .trim_start_matches('/')
        );

        let client = self.client.clone();

        Box::pin(async move {
            let resp = client
                .request(Method::DELETE, &url)
                .basic_auth(&username, Some(&password))
                .send()
                .await
                .map_err(|e| anyhow!(e))?;

            if resp.status().is_success() || resp.status() == 404 {
                info!("WebDAV: 删除成功 (或已不存在) {name}");
                Ok(())
            } else {
                Err(anyhow!("删除失败: {}", resp.status()))
            }
        })
    }

    fn rename(&self, old: String, new: String) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        let enabled = self.config.read().unwrap().enabled;
        if !enabled {
            return Box::pin(async { Err(anyhow!("WebDAV 未启用")) });
        }

        let (endpoint, username, password) = {
            let c = self.config.read().unwrap();
            (c.endpoint.clone(), c.username.clone(), c.password.clone())
        };
        let remote_path = self.get_effective_remote_path();
        let old_url = format!("{}/{}", remote_path.trim_end_matches('/'), encode(&old));
        let destination_url = format!("{}/Lumen/{}", endpoint.trim_end_matches('/'), encode(&new));

        let client = self.client.clone();

        Box::pin(async move {
            let resp = client
                .request(Method::from_bytes(b"MOVE")?, old_url)
                .basic_auth(username, Some(password))
                .header("Destination", destination_url)
                .header("Overwrite", "T")
                .send()
                .await
                .map_err(|e| anyhow!(e))?;

            if resp.status().is_success() {
                Ok(())
            } else {
                Err(anyhow!("WebDAV 重命名失败: {}", resp.status()))
            }
        })
    }
}
