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

pub struct OpenAiBackend {
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

impl OpenAiBackend {
    pub fn new(config: &AiConfig) -> Self {
        let is_local =
            config.api_base.contains("localhost") || config.api_base.contains("127.0.0.1");
        let timeout = Duration::from_secs(if is_local { 120 } else { 60 });
        debug!(
            "OpenAiBackend: 创建后端, model={}, api_base={}, api_key={}, timeout={}s, local={}",
            config.model,
            config.api_base,
            mask_key(&config.api_key),
            timeout.as_secs(),
            is_local,
        );
        Self {
            client: Client::builder()
                .timeout(timeout)
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
        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": msgs,
            "stream": stream,
        });
        if self.config.enable_thinking {
            body["reasoning_effort"] = serde_json::json!("medium");
            body["max_tokens"] = serde_json::json!(self.config.max_tokens.max(8192));
        } else {
            body["temperature"] =
                serde_json::json!((self.config.temperature as f64 * 100.0).round() / 100.0);
            body["max_tokens"] = serde_json::json!(self.config.max_tokens);
        }
        let msg_count = msgs.len();
        let total_chars: usize = msgs
            .iter()
            .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
            .map(|s| s.len())
            .sum();
        debug!(
            "OpenAiBackend: 构建请求体, message_count={}, total_chars={}, body_size={}bytes, stream={}, enable_thinking={}, reasoning_effort={:?}",
            msg_count,
            total_chars,
            serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0),
            stream,
            self.config.enable_thinking,
            body.get("reasoning_effort"),
        );
        body
    }
}

impl AiBackend for OpenAiBackend {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
        let client = self.client.clone();
        let config = self.config.clone();
        let body = self.build_request_body(messages, system, false);
        let url = format!("{}/chat/completions", config.api_base.trim_end_matches('/'));

        Box::pin(async move {
            debug!(
                "OpenAiBackend::chat: POST {} | model={} | key={}",
                url,
                config.model,
                mask_key(&config.api_key),
            );

            let body_str = serde_json::to_string(&body).unwrap_or_default();
            debug!("OpenAiBackend::chat: 请求体(完整): {body_str}");

            let resp = match client
                .post(&url)
                .bearer_auth(&config.api_key)
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    error!("OpenAiBackend::chat: HTTP 请求失败: {e:?}");
                    if e.is_timeout() {
                        return Err(anyhow!("OpenAI 请求超时 (timeout=60s): {e}"));
                    }
                    if e.is_connect() {
                        return Err(anyhow!("OpenAI 连接失败，请检查 API 地址是否正确: {e}"));
                    }
                    return Err(anyhow!("OpenAI HTTP 错误: {e}"));
                }
            };

            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();

            if !status.is_success() {
                error!(
                    "OpenAiBackend::chat: HTTP {} | URL={} | 响应体(前500字): {}",
                    status,
                    url,
                    &text[..text.len().min(500)],
                );
                return Err(anyhow!(
                    "OpenAI API error ({}): {}",
                    status,
                    text.chars().take(200).collect::<String>(),
                ));
            }

            debug!(
                "OpenAiBackend::chat: HTTP 200 OK, 响应体大小={}bytes",
                text.len()
            );

            let json: Value = match serde_json::from_str(&text) {
                Ok(j) => j,
                Err(e) => {
                    error!(
                        "OpenAiBackend::chat: JSON 解析失败: {e}\n原始响应(前300字): {}",
                        &text[..text.len().min(300)],
                    );
                    return Err(anyhow!("OpenAI 响应 JSON 解析失败: {e}"));
                }
            };

            let choice0 = match json["choices"].as_array().and_then(|a| a.first()) {
                Some(c) => c,
                None => {
                    error!(
                        "OpenAiBackend::chat: 响应缺少 choices[0], 完整响应: {}",
                        serde_json::to_string_pretty(&json).unwrap_or_default(),
                    );
                    return Err(anyhow!("OpenAI 响应格式错误: 缺少 choices[0]"));
                }
            };

            let content = match choice0["message"]["content"].as_str() {
                Some(c) => c,
                None => {
                    let finish_reason = choice0["finish_reason"].as_str().unwrap_or("unknown");
                    error!(
                        "OpenAiBackend::chat: choices[0] 缺少 message.content, finish_reason={:?}, 完整 choice: {}",
                        finish_reason,
                        serde_json::to_string(choice0).unwrap_or_default(),
                    );
                    return Err(anyhow!(
                        "OpenAI 响应格式错误: 缺少 content (finish_reason: {finish_reason})"
                    ));
                }
            };

            debug!(
                "OpenAiBackend::chat: 成功, content_len={}, finish_reason={:?}",
                content.len(),
                choice0["finish_reason"].as_str(),
            );
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
        let url = format!("{}/chat/completions", config.api_base.trim_end_matches('/'));

        Box::pin(async move {
            debug!(
                "OpenAiBackend::chat_stream: POST {} | model={} | key={}",
                url,
                config.model,
                mask_key(&config.api_key),
            );

            let resp = match client
                .post(&url)
                .bearer_auth(&config.api_key)
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    error!("OpenAiBackend::chat_stream: HTTP 请求失败: {e:?}");
                    if e.is_timeout() {
                        return Err(anyhow!("OpenAI 流式请求超时: {e}"));
                    }
                    return Err(anyhow!("OpenAI 流式 HTTP 错误: {e}"));
                }
            };

            let status = resp.status();
            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                error!(
                    "OpenAiBackend::chat_stream: HTTP {} | URL={} | 响应体(前500字): {}",
                    status,
                    url,
                    &text[..text.len().min(500)],
                );
                return Err(anyhow!(
                    "OpenAI API error ({}): {}",
                    status,
                    text.chars().take(200).collect::<String>()
                ));
            }

            debug!("OpenAiBackend::chat_stream: HTTP 200 OK, 开始处理 SSE 流");

            let (tx, rx) = mpsc::unbounded_channel();

            tokio::spawn(async move {
                if let Err(e) = process_sse_stream(resp, tx).await {
                    error!("OpenAiBackend::chat_stream: 流式处理失败: {e}");
                }
            });

            Ok(ChatResponseStream::new(rx))
        })
    }
}

async fn process_sse_stream(
    mut resp: reqwest::Response,
    tx: mpsc::UnboundedSender<Result<ChatResponseChunk>>,
) -> Result<()> {
    let mut buffer = String::new();
    let mut chunk_count = 0u64;
    let mut content_chunks = 0u64;
    let mut interceptor = TagInterceptor::new(tx);
    let mut has_logged_reasoning = false;

    while let Ok(Some(chunk)) = resp.chunk().await {
        chunk_count += 1;
        let chunk_str = String::from_utf8_lossy(&chunk);
        if chunk_count <= 3 {
            debug!(
                "OpenAiBackend SSE: chunk#{chunk_count}, size={}bytes, preview={:?}",
                chunk.len(),
                &chunk_str[..chunk_str.len().min(80)]
            );
        }
        buffer.push_str(&chunk_str);

        while let Some(pos) = buffer.find("\n\n") {
            let event = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();

            for line in event.lines() {
                let data = line.strip_prefix("data: ").unwrap_or(line);
                let data = data.trim();

                if data == "[DONE]" {
                    debug!(
                        "OpenAiBackend SSE: 收到 [DONE] 信号, 共处理 {chunk_count} chunks, {content_chunks} 个内容块"
                    );
                    interceptor.finish();
                    return Ok(());
                }

                if data.is_empty() {
                    continue;
                }

                match serde_json::from_str::<Value>(data) {
                    Ok(json) => {
                        let delta = &json["choices"][0]["delta"];

                        // 1. 优先提取推理字段
                        if let Some(reasoning) = delta["reasoning_content"].as_str()
                            && !reasoning.is_empty()
                        {
                            if !has_logged_reasoning {
                                debug!(
                                    "OpenAiBackend: Incoming stream contains structured reasoning_content."
                                );
                                has_logged_reasoning = true;
                            }
                            if !interceptor
                                .send_chunk(ChatResponseChunk::Reasoning(reasoning.to_string()))
                            {
                                debug!("OpenAiBackend SSE: 接收端已关闭，停止发送");
                                return Ok(());
                            }
                        }

                        // 2. 提取标准回答字段并进行 think 拦截过滤
                        if let Some(content) = delta["content"].as_str()
                            && !content.is_empty()
                        {
                            content_chunks += 1;
                            if !interceptor
                                .send_chunk(ChatResponseChunk::Content(content.to_string()))
                            {
                                debug!("OpenAiBackend SSE: 接收端已关闭，停止发送");
                                return Ok(());
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            "OpenAiBackend SSE: JSON 解析警告 (忽略): {e}, line={:?}",
                            data.chars().take(100).collect::<String>()
                        );
                    }
                }
            }
        }
    }

    debug!("OpenAiBackend SSE: 流结束, 共处理 {chunk_count} chunks, {content_chunks} 个内容块");
    interceptor.finish();
    Ok(())
}
