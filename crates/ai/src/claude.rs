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

const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct ClaudeBackend {
    client: Client,
    config: AiConfig,
}

fn mask_key(key: &str) -> String {
    if key.is_empty() {
        return "[empty]".to_string();
    }
    if key.len() <= 8 {
        return format!("[short:{}chars]", key.len());
    }
    format!(
        "{}...{} (len={})",
        &key[..4],
        &key[key.len() - 4..],
        key.len()
    )
}

impl ClaudeBackend {
    pub fn new(config: &AiConfig) -> Self {
        debug!(
            "ClaudeBackend: 创建后端, model={}, api_base={}, api_key={}",
            config.model,
            config.api_base,
            mask_key(&config.api_key),
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
        for msg in messages {
            let role = match msg.role {
                ChatRole::User => "user",
                ChatRole::Assistant => "assistant",
                ChatRole::System => "user",
            };
            let content = if msg.attachments.is_empty() {
                Value::String(msg.content.clone())
            } else {
                let mut blocks = Vec::new();
                blocks.push(serde_json::json!({
                    "type": "text",
                    "text": msg.content,
                }));
                for att in &msg.attachments {
                    match std::fs::read(&att.file_path) {
                        Ok(bytes) => {
                            let encoded = base64::Engine::encode(
                                &base64::engine::general_purpose::STANDARD,
                                &bytes,
                            );
                            blocks.push(serde_json::json!({
                                "type": "document",
                                "source": {
                                    "type": "base64",
                                    "media_type": att.mime_type.as_deref().unwrap_or("application/pdf"),
                                    "data": encoded,
                                }
                            }));
                        }
                        Err(e) => {
                            warn!("ClaudeBackend: 读取附件失败 {}: {e}", att.file_path);
                        }
                    }
                }
                Value::Array(blocks)
            };
            msgs.push(serde_json::json!({
                "role": role,
                "content": content,
            }));
        }
        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": msgs,
            "stream": stream,
        });
        if self.config.enable_thinking {
            let limit = self.config.max_tokens.max(8192);
            body["thinking"] = serde_json::json!({
                "type": "enabled",
                "budget_tokens": limit / 2
            });
            body["max_tokens"] = serde_json::json!(limit);
        } else {
            body["max_tokens"] = serde_json::json!(self.config.max_tokens);
            body["temperature"] =
                serde_json::json!((self.config.temperature as f64 * 100.0).round() / 100.0);
        }
        if let Some(sys) = system {
            body["system"] = serde_json::Value::String(sys.to_string());
        }
        let msg_count = msgs.len();
        debug!(
            "ClaudeBackend: 构建请求体, message_count={}, stream={}, enable_thinking={}, thinking_param={:?}",
            msg_count,
            stream,
            self.config.enable_thinking,
            body.get("thinking"),
        );
        body
    }
}

impl AiBackend for ClaudeBackend {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
        let client = self.client.clone();
        let config = self.config.clone();
        let body = self.build_request_body(messages, system, false);
        let url = format!("{}/v1/messages", config.api_base.trim_end_matches('/'));

        Box::pin(async move {
            debug!(
                "ClaudeBackend::chat: POST {} | model={} | key={}",
                url,
                config.model,
                mask_key(&config.api_key),
            );

            let resp = match client
                .post(&url)
                .header("x-api-key", &config.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    error!("ClaudeBackend::chat: HTTP 请求失败: {e:?}");
                    if e.is_timeout() {
                        return Err(anyhow!("Claude 请求超时: {e}"));
                    }
                    if e.is_connect() {
                        return Err(anyhow!("Claude 连接失败，请检查 API 地址是否正确: {e}"));
                    }
                    return Err(anyhow!("Claude HTTP 错误: {e}"));
                }
            };

            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();

            if !status.is_success() {
                error!(
                    "ClaudeBackend::chat: HTTP {} | URL={} | 响应体(前500字): {}",
                    status,
                    url,
                    &text[..text.len().min(500)],
                );
                return Err(anyhow!(
                    "Claude API error ({}): {}",
                    status,
                    text.chars().take(200).collect::<String>(),
                ));
            }

            debug!(
                "ClaudeBackend::chat: HTTP 200 OK, 响应体大小={}bytes",
                text.len()
            );

            let json: Value = match serde_json::from_str(&text) {
                Ok(j) => j,
                Err(e) => {
                    error!(
                        "ClaudeBackend::chat: JSON 解析失败: {e}\n原始响应(前300字): {}",
                        &text[..text.len().min(300)],
                    );
                    return Err(anyhow!("Claude 响应 JSON 解析失败: {e}"));
                }
            };

            let content = match json["content"].as_array() {
                Some(arr) => arr
                    .iter()
                    .filter_map(|block| {
                        if block["type"].as_str() == Some("text") {
                            block["text"].as_str()
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(""),
                None => {
                    error!(
                        "ClaudeBackend::chat: 响应缺少 content 数组, 完整响应: {}",
                        serde_json::to_string_pretty(&json).unwrap_or_default(),
                    );
                    return Err(anyhow!("Claude 响应格式错误: 缺少 content 数组"));
                }
            };

            debug!("ClaudeBackend::chat: 成功, content_len={}", content.len());
            Ok(content)
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
        let url = format!("{}/v1/messages", config.api_base.trim_end_matches('/'));

        Box::pin(async move {
            debug!(
                "ClaudeBackend::chat_stream: POST {} | model={} | key={}",
                url,
                config.model,
                mask_key(&config.api_key),
            );

            let resp = match client
                .post(&url)
                .header("x-api-key", &config.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    error!("ClaudeBackend::chat_stream: HTTP 请求失败: {e:?}");
                    if e.is_timeout() {
                        return Err(anyhow!("Claude 流式请求超时: {e}"));
                    }
                    return Err(anyhow!("Claude 流式 HTTP 错误: {e}"));
                }
            };

            let status = resp.status();
            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                error!(
                    "ClaudeBackend::chat_stream: HTTP {} | URL={} | 响应体(前500字): {}",
                    status,
                    url,
                    &text[..text.len().min(500)],
                );
                return Err(anyhow!(
                    "Claude API error ({}): {}",
                    status,
                    text.chars().take(200).collect::<String>()
                ));
            }

            debug!("ClaudeBackend::chat_stream: HTTP 200 OK, 开始处理 SSE 流");

            let (tx, rx) = mpsc::unbounded_channel();

            tokio::spawn(async move {
                if let Err(e) = process_claude_sse(resp, tx).await {
                    error!("ClaudeBackend::chat_stream: 流式处理失败: {e}");
                }
            });

            Ok(ChatResponseStream::new(rx))
        })
    }
}

async fn process_claude_sse(
    mut resp: reqwest::Response,
    tx: mpsc::UnboundedSender<Result<ChatResponseChunk>>,
) -> Result<()> {
    let mut buffer = String::new();
    let mut chunk_count = 0u64;
    let mut interceptor = TagInterceptor::new(tx);
    let mut has_logged_reasoning = false;

    while let Ok(Some(chunk)) = resp.chunk().await {
        chunk_count += 1;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buffer.find("\n\n") {
            let block = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();

            let mut event_type = String::new();
            let mut data = String::new();

            for line in block.lines() {
                if let Some(value) = line.strip_prefix("event: ") {
                    event_type = value.trim().to_string();
                } else if let Some(value) = line.strip_prefix("data: ") {
                    data = value.trim().to_string();
                }
            }

            if data.is_empty() {
                continue;
            }

            if event_type != "content_block_delta" {
                if event_type == "message_stop" {
                    debug!("Claude SSE: 收到 message_stop, 共处理 {chunk_count} chunks");
                    interceptor.finish();
                    return Ok(());
                }
                continue;
            }

            match serde_json::from_str::<Value>(&data) {
                Ok(json) => {
                    let delta_type = json["delta"]["type"].as_str();
                    if delta_type == Some("thinking_delta") {
                        if let Some(thinking) = json["delta"]["thinking"].as_str()
                            && !thinking.is_empty()
                        {
                            if !has_logged_reasoning {
                                debug!(
                                    "ClaudeBackend: Incoming stream contains structured thinking_delta."
                                );
                                has_logged_reasoning = true;
                            }
                            if !interceptor
                                .send_chunk(ChatResponseChunk::Reasoning(thinking.to_string()))
                            {
                                debug!("Claude SSE: 接收端已关闭，停止发送");
                                return Ok(());
                            }
                        }
                    } else if delta_type == Some("text_delta")
                        && let Some(text) = json["delta"]["text"].as_str()
                        && !text.is_empty()
                        && !interceptor.send_chunk(ChatResponseChunk::Content(text.to_string()))
                    {
                        debug!("Claude SSE: 接收端已关闭，停止发送");
                        return Ok(());
                    }
                }
                Err(e) => {
                    warn!(
                        "Claude SSE: JSON 解析警告 (忽略): {e}, data={:?}",
                        data.chars().take(100).collect::<String>()
                    );
                }
            }
        }
    }

    debug!("Claude SSE: 流结束, 共处理 {chunk_count} chunks");
    interceptor.finish();
    Ok(())
}
