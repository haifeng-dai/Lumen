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

pub struct SiliconFlowBackend {
    client: Client,
    config: AiConfig,
}

impl SiliconFlowBackend {
    pub fn new(config: &AiConfig) -> Self {
        debug!("SiliconFlowBackend: 创建后端, model={}", config.model);
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
                blocks.push(serde_json::json!({"type": "text", "text": msg.content}));
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
                            warn!("SiliconFlowBackend: 读取附件失败 {}: {e}", att.file_path);
                        }
                    }
                }
                Value::Array(blocks)
            };
            msgs.push(serde_json::json!({"role": role, "content": content}));
        }
        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": msgs,
            "stream": stream,
            "max_tokens": self.config.max_tokens,
        });
        body["temperature"] =
            serde_json::json!((self.config.temperature as f64 * 100.0).round() / 100.0);
        if let Some(sys) = system {
            body["system"] = serde_json::Value::String(sys.to_string());
        }
        debug!(
            "SiliconFlowBackend: 构建请求体, message_count={}, stream={}",
            msgs.len(),
            stream,
        );
        body
    }
}

impl AiBackend for SiliconFlowBackend {
    fn name(&self) -> &'static str {
        "siliconflow"
    }

    fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
        let client = self.client.clone();
        let config = self.config.clone();
        let body = self.build_request_body(messages, system, false);
        Box::pin(async move {
            debug!("SiliconFlowBackend::chat: model={}", config.model);

            let resp = match client
                .post("https://api.siliconflow.cn/v1/messages")
                .header("Authorization", format!("Bearer {}", config.api_key))
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    error!("SiliconFlowBackend::chat: HTTP 请求失败: {e:?}");
                    if e.is_timeout() {
                        return Err(anyhow!("SiliconFlow 请求超时: {e}"));
                    }
                    if e.is_connect() {
                        return Err(anyhow!("SiliconFlow 连接失败: {e}"));
                    }
                    return Err(anyhow!("SiliconFlow HTTP 错误: {e}"));
                }
            };

            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();

            if !status.is_success() {
                error!(
                    "SiliconFlowBackend::chat: HTTP {} | 响应体(前500字): {}",
                    status,
                    &text[..text.len().min(500)],
                );
                return Err(anyhow!(
                    "SiliconFlow API error ({}): {}",
                    status,
                    text.chars().take(200).collect::<String>(),
                ));
            }

            debug!(
                "SiliconFlowBackend::chat: HTTP 200 OK, 响应体大小={}bytes",
                text.len()
            );

            let json: Value = match serde_json::from_str(&text) {
                Ok(j) => j,
                Err(e) => {
                    error!(
                        "SiliconFlowBackend::chat: JSON 解析失败: {e}\n原始响应(前300字): {}",
                        &text[..text.len().min(300)],
                    );
                    return Err(anyhow!("SiliconFlow 响应 JSON 解析失败: {e}"));
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
                        "SiliconFlowBackend::chat: 响应缺少 content 数组, 完整响应: {}",
                        serde_json::to_string_pretty(&json).unwrap_or_default(),
                    );
                    return Err(anyhow!("SiliconFlow 响应格式错误: 缺少 content 数组"));
                }
            };

            debug!(
                "SiliconFlowBackend::chat: 成功, content_len={}",
                content.len()
            );
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
        Box::pin(async move {
            debug!("SiliconFlowBackend::chat_stream: model={}", config.model);

            let resp = match client
                .post("https://api.siliconflow.cn/v1/messages")
                .header("Authorization", format!("Bearer {}", config.api_key))
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    error!("SiliconFlowBackend::chat_stream: HTTP 请求失败: {e:?}");
                    if e.is_timeout() {
                        return Err(anyhow!("SiliconFlow 流式请求超时: {e}"));
                    }
                    return Err(anyhow!("SiliconFlow 流式 HTTP 错误: {e}"));
                }
            };

            let status = resp.status();
            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                error!(
                    "SiliconFlowBackend::chat_stream: HTTP {} | 响应体(前500字): {}",
                    status,
                    &text[..text.len().min(500)],
                );
                return Err(anyhow!(
                    "SiliconFlow API error ({}): {}",
                    status,
                    text.chars().take(200).collect::<String>()
                ));
            }

            debug!("SiliconFlowBackend::chat_stream: HTTP 200 OK, 开始处理 SSE 流");

            let (tx, rx) = mpsc::unbounded_channel();

            tokio::spawn(async move {
                if let Err(e) = process_siliconflow_sse(resp, tx).await {
                    error!("SiliconFlowBackend::chat_stream: 流式处理失败: {e}");
                }
            });

            Ok(ChatResponseStream::new(rx))
        })
    }
}

async fn process_siliconflow_sse(
    mut resp: reqwest::Response,
    tx: mpsc::UnboundedSender<Result<ChatResponseChunk>>,
) -> Result<()> {
    let mut buffer = String::new();
    let mut chunk_count = 0u64;
    let mut interceptor = TagInterceptor::new(tx);

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

            if data == "[DONE]" {
                debug!("SiliconFlow SSE: 收到 [DONE] 信号, 共处理 {chunk_count} chunks");
                interceptor.finish();
                return Ok(());
            }

            if event_type.is_empty() {
                continue;
            }

            if event_type != "content_block_delta" {
                if event_type == "message_stop" {
                    debug!("SiliconFlow SSE: 收到 message_stop, 共处理 {chunk_count} chunks");
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
                            && !interceptor
                                .send_chunk(ChatResponseChunk::Reasoning(thinking.to_string()))
                        {
                            debug!("SiliconFlow SSE: 接收端已关闭，停止发送");
                            return Ok(());
                        }
                    } else if delta_type == Some("text_delta")
                        && let Some(text) = json["delta"]["text"].as_str()
                        && !text.is_empty()
                        && !interceptor.send_chunk(ChatResponseChunk::Content(text.to_string()))
                    {
                        debug!("SiliconFlow SSE: 接收端已关闭，停止发送");
                        return Ok(());
                    }
                }
                Err(e) => {
                    warn!(
                        "SiliconFlow SSE: JSON 解析警告 (忽略): {e}, data={:?}",
                        data.chars().take(100).collect::<String>()
                    );
                }
            }
        }
    }

    debug!("SiliconFlow SSE: 流结束, 共处理 {chunk_count} chunks");
    interceptor.finish();
    Ok(())
}
