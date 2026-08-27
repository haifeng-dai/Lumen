use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone)]
pub struct AttachmentInfo {
    pub file_path: String,
    pub file_name: String,
    pub mime_type: Option<String>,
    pub extracted_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub attachments: Vec<AttachmentInfo>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
            attachments: Vec::new(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
            attachments: Vec::new(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
            attachments: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    OpenAI,
    Ollama,
    Claude,
    SiliconFlow,
}

impl BackendKind {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "openai" => Self::OpenAI,
            "ollama" => Self::Ollama,
            "claude" => Self::Claude,
            "siliconflow" => Self::SiliconFlow,
            _ => Self::OpenAI,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub api_key: String,
    pub api_base: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub context_window: u32,
    pub enable_thinking: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_base: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o-mini".to_string(),
            temperature: 0.3,
            max_tokens: 4096,
            context_window: 128000,
            enable_thinking: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiBackendEntry {
    pub name: String,
    pub kind: String,
    pub api_base: String,
    pub api_key: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    #[serde(default = "default_context_window")]
    pub context_window: u32,
    #[serde(default = "default_compression_strategy")]
    pub compression_strategy: String,
    #[serde(default = "default_enable_thinking")]
    pub enable_thinking: bool,
}

fn default_context_window() -> u32 {
    128000
}

fn default_compression_strategy() -> String {
    "sliding_window".into()
}

fn default_enable_thinking() -> bool {
    false
}

impl AiBackendEntry {
    pub fn to_config(&self) -> AiConfig {
        AiConfig {
            api_key: self.api_key.clone(),
            api_base: self.api_base.clone(),
            model: self.model.clone(),
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            context_window: self.context_window,
            enable_thinking: self.enable_thinking,
        }
    }
}

use models::chat::ChatResponseChunk;

pub struct ChatResponseStream {
    rx: mpsc::UnboundedReceiver<Result<ChatResponseChunk>>,
}

impl ChatResponseStream {
    pub fn new(rx: mpsc::UnboundedReceiver<Result<ChatResponseChunk>>) -> Self {
        Self { rx }
    }
}

impl futures_core::Stream for ChatResponseStream {
    type Item = Result<ChatResponseChunk>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

pub trait AiBackend: Send + Sync {
    fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send>>;

    fn chat_stream(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<ChatResponseStream>> + Send>>;

    fn name(&self) -> &'static str;
}

pub struct TagInterceptor {
    tx: mpsc::UnboundedSender<Result<ChatResponseChunk>>,
    buffer: String,
    in_reasoning: bool,
    reasoning_chars: usize,
}

impl TagInterceptor {
    pub fn new(tx: mpsc::UnboundedSender<Result<ChatResponseChunk>>) -> Self {
        Self {
            tx,
            buffer: String::new(),
            in_reasoning: false,
            reasoning_chars: 0,
        }
    }

    pub fn send_chunk(&mut self, chunk: ChatResponseChunk) -> bool {
        match chunk {
            ChatResponseChunk::Reasoning(r) => {
                self.reasoning_chars += r.len();
                self.tx.send(Ok(ChatResponseChunk::Reasoning(r))).is_ok()
            }
            ChatResponseChunk::Content(c) => {
                self.buffer.push_str(&c);
                let mut processed_anything = true;
                while processed_anything {
                    processed_anything = false;
                    if !self.in_reasoning {
                        if let Some(start_idx) = self.buffer.find("<think>") {
                            log::debug!(
                                "TagInterceptor: Detected '<think>' tag. Commencing reasoning extraction."
                            );
                            let prefix = self.buffer[..start_idx].to_string();
                            if !prefix.is_empty()
                                && self
                                    .tx
                                    .send(Ok(ChatResponseChunk::Content(prefix)))
                                    .is_err()
                            {
                                return false;
                            }
                            self.in_reasoning = true;
                            self.buffer = self.buffer[start_idx + 7..].to_string();
                            processed_anything = true;
                        } else {
                            let mut pending_suffix_len = 0;
                            for prefix in &["<think", "<thin", "<thi", "<th", "<t", "<"] {
                                if self.buffer.ends_with(prefix) {
                                    pending_suffix_len = prefix.len();
                                    break;
                                }
                            }
                            let send_len = self.buffer.len() - pending_suffix_len;
                            if send_len > 0 {
                                let to_send = self.buffer[..send_len].to_string();
                                if self
                                    .tx
                                    .send(Ok(ChatResponseChunk::Content(to_send)))
                                    .is_err()
                                {
                                    return false;
                                }
                                self.buffer = self.buffer[send_len..].to_string();
                            }
                        }
                    } else {
                        if let Some(end_idx) = self.buffer.find("</think>") {
                            log::debug!(
                                "TagInterceptor: Detected '</think>' tag. Commencing content extraction."
                            );
                            let reasoning_text = self.buffer[..end_idx].to_string();
                            if !reasoning_text.is_empty() {
                                self.reasoning_chars += reasoning_text.len();
                                if self
                                    .tx
                                    .send(Ok(ChatResponseChunk::Reasoning(reasoning_text)))
                                    .is_err()
                                {
                                    return false;
                                }
                            }
                            self.in_reasoning = false;
                            self.buffer = self.buffer[end_idx + 8..].to_string();
                            processed_anything = true;
                        } else {
                            let mut pending_suffix_len = 0;
                            for prefix in &["</think", "</thin", "</thi", "</th", "</t", "</", "<"]
                            {
                                if self.buffer.ends_with(prefix) {
                                    pending_suffix_len = prefix.len();
                                    break;
                                }
                            }
                            let send_len = self.buffer.len() - pending_suffix_len;
                            if send_len > 0 {
                                let to_send = self.buffer[..send_len].to_string();
                                self.reasoning_chars += to_send.len();
                                if self
                                    .tx
                                    .send(Ok(ChatResponseChunk::Reasoning(to_send)))
                                    .is_err()
                                {
                                    return false;
                                }
                                self.buffer = self.buffer[send_len..].to_string();
                            }
                        }
                    }
                }
                true
            }
        }
    }

    pub fn finish(mut self) {
        if !self.buffer.is_empty() {
            let chunk = if self.in_reasoning {
                self.reasoning_chars += self.buffer.len();
                ChatResponseChunk::Reasoning(self.buffer.clone())
            } else {
                ChatResponseChunk::Content(self.buffer.clone())
            };
            let _ = self.tx.send(Ok(chunk));
        }
        log::debug!(
            "TagInterceptor: Stream finished. Extracted reasoning length: {} chars.",
            self.reasoning_chars
        );
    }
}
