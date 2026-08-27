use anyhow::{Result, anyhow};
use log::{debug, error, warn};
use models::chat::ChatResponseChunk;
use reqwest::Client;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::types::*;

pub struct OllamaBackend {
    client: Client,
    config: AiConfig,
}

impl OllamaBackend {
    pub fn new(config: &AiConfig) -> Self {
        debug!(
            "OllamaBackend: 创建后端, model={}, api_base={}",
            config.model, config.api_base,
        );
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            config: config.clone(),
        }
    }

    fn build_request_body(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
        stream: bool,
    ) -> Value {
        let mut msgs = Vec::new();
        if let Some(sys) = system {
            msgs.push(serde_json::json!({
                "role": "system",
                "content": sys,
            }));
        }
        for msg in messages {
            let role = match msg.role {
                ChatRole::User => "user",
                ChatRole::Assistant => "assistant",
                ChatRole::System => "system",
            };
            let content = if msg.attachments.is_empty() {
                msg.content.clone()
            } else {
                let mut text = msg.content.clone();
                for att in &msg.attachments {
                    if let Some(ref extracted) = att.extracted_text {
                        text.push_str(&format!(
                            "\n\n[Attached: {}]\n---\n{}\n---",
                            att.file_name, extracted
                        ));
                    } else {
                        text.push_str(&format!("\n\n[Attached: {}]", att.file_name));
                    }
                }
                text
            };
            msgs.push(serde_json::json!({
                "role": role,
                "content": content,
            }));
        }
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": msgs,
            "stream": stream,
            "options": {
                "temperature": self.config.temperature,
                "num_predict": self.config.max_tokens,
            },
        });
        debug!(
            "OllamaBackend: 构建请求体, messages={}, stream={}",
            msgs.len(),
            stream,
        );
        body
    }
}

impl AiBackend for OllamaBackend {
    fn name(&self) -> &'static str {
        "ollama"
    }

    fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
        let client = self.client.clone();
        let config = self.config.clone();
        let body = self.build_request_body(messages, system, false);
        let url = format!("{}/api/chat", config.api_base.trim_end_matches('/'));

        Box::pin(async move {
            debug!("OllamaBackend::chat: POST {url} | model={}", config.model);

            let resp = match client.post(&url).json(&body).send().await {
                Ok(r) => r,
                Err(e) => {
                    error!("OllamaBackend::chat: HTTP 请求失败: {e:?}");
                    if e.is_connect() {
                        return Err(anyhow!(
                            "Ollama 连接失败，请检查 Ollama 是否运行在 {}: {e}",
                            config.api_base
                        ));
                    }
                    return Err(anyhow!("Ollama HTTP 错误: {e}"));
                }
            };

            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();

            if !status.is_success() {
                error!(
                    "OllamaBackend::chat: HTTP {} | URL={} | 响应体(前500字): {}",
                    status,
                    url,
                    &text[..text.len().min(500)],
                );
                return Err(anyhow!(
                    "Ollama API error ({}): {}",
                    status,
                    text.chars().take(200).collect::<String>(),
                ));
            }

            debug!(
                "OllamaBackend::chat: HTTP 200 OK, 响应体大小={}bytes",
                text.len()
            );

            let json: Value = match serde_json::from_str(&text) {
                Ok(j) => j,
                Err(e) => {
                    error!(
                        "OllamaBackend::chat: JSON 解析失败: {e}\n原始响应(前300字): {}",
                        &text[..text.len().min(300)],
                    );
                    return Err(anyhow!("Ollama 响应 JSON 解析失败: {e}"));
                }
            };

            let content = match json["message"]["content"].as_str() {
                Some(c) => c,
                None => {
                    error!(
                        "OllamaBackend::chat: 响应缺少 message.content, 完整响应: {}",
                        serde_json::to_string_pretty(&json).unwrap_or_default(),
                    );
                    return Err(anyhow!("Ollama 响应格式错误: 缺少 message.content"));
                }
            };

            debug!("OllamaBackend::chat: 成功, content_len={}", content.len());
            Ok(content.to_string())
        })
    }

    fn chat_stream(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<ChatResponseStream>> + Send>> {
        let client = self.client.clone();
        let config = self.config.clone();
        let body = self.build_request_body(messages, system, true);
        let url = format!("{}/api/chat", config.api_base.trim_end_matches('/'));

        Box::pin(async move {
            debug!(
                "OllamaBackend::chat_stream: POST {url} | model={}",
                config.model
            );

            let resp = match client.post(&url).json(&body).send().await {
                Ok(r) => r,
                Err(e) => {
                    error!("OllamaBackend::chat_stream: HTTP 请求失败: {e:?}");
                    return Err(anyhow!("Ollama 流式 HTTP 错误: {e}"));
                }
            };

            let status = resp.status();
            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                error!(
                    "OllamaBackend::chat_stream: HTTP {} | URL={} | 响应体(前500字): {}",
                    status,
                    url,
                    &text[..text.len().min(500)],
                );
                return Err(anyhow!(
                    "Ollama API error ({}): {}",
                    status,
                    text.chars().take(200).collect::<String>()
                ));
            }

            debug!("OllamaBackend::chat_stream: HTTP 200 OK, 开始处理 NDJSON 流");

            let (tx, rx) = mpsc::unbounded_channel();

            tokio::spawn(async move {
                if let Err(e) = process_ndjson_stream(resp, tx).await {
                    error!("OllamaBackend::chat_stream: 流式处理失败: {e}");
                }
            });

            Ok(ChatResponseStream::new(rx))
        })
    }
}

async fn process_ndjson_stream(
    mut resp: reqwest::Response,
    tx: mpsc::UnboundedSender<Result<ChatResponseChunk>>,
) -> Result<()> {
    let mut buffer = String::new();
    let mut line_count = 0u64;
    let mut interceptor = TagInterceptor::new(tx);

    while let Ok(Some(chunk)) = resp.chunk().await {
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer = buffer[pos + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            line_count += 1;

            match serde_json::from_str::<Value>(&line) {
                Ok(json) => {
                    if json["done"].as_bool().unwrap_or(false) {
                        debug!("Ollama NDJSON: 收到 done 信号, 共处理 {line_count} 行");
                        interceptor.finish();
                        return Ok(());
                    }
                    if let Some(content) = json["message"]["content"].as_str()
                        && !content.is_empty()
                        && !interceptor.send_chunk(ChatResponseChunk::Content(content.to_string()))
                    {
                        debug!("Ollama NDJSON: 接收端已关闭，停止发送");
                        return Ok(());
                    }
                }
                Err(e) => {
                    warn!(
                        "Ollama NDJSON: JSON 解析警告 (忽略): {e}, line={:?}",
                        line.chars().take(100).collect::<String>()
                    );
                }
            }
        }
    }

    debug!("Ollama NDJSON: 流结束, 共处理 {line_count} 行");
    interceptor.finish();
    Ok(())
}
