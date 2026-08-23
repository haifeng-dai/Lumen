use crate::ui::notification::show_notification;
use futures_util::{StreamExt, TryFutureExt};
use gpui::prelude::*;
use gpui::{AsyncApp, WeakEntity, Window};
use gpui_component::notification::NotificationType;
use log::error;

impl super::LiteratureDetailView {
    pub(super) fn generate_ai_summary(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let lit_id = match self.state.selected_ids.first() {
            Some(id) => id.clone(),
            None => return,
        };

        // 删除上一次的 AI 总结
        if let Some(last_id) = self.last_ai_summary_note_id.take() {
            let _ = self
                .app
                .literature_service
                .delete_note(&self.app.db, &last_id);
            self.notes_cache.retain(|n| n.id != last_id);
        }

        self.notes_cache.retain(|n| n.id != "ai_generating_note");

        let now = chrono::Utc::now().timestamp();
        self.notes_cache.push(models::LiteratureNote {
            id: "ai_generating_note".to_string(),
            literature_id: lit_id.clone(),
            title: "AI 总结生成中...".to_string(),
            content: "正在准备数据，请稍候...\n\n".to_string(),
            sort_order: self.notes_cache.len() as i32,
            created_at: now,
            updated_at: now,
            is_deleted: false,
            is_dirty: false,
            version: 1,
        });

        self.is_generating_summary = true;
        cx.notify();

        let app = self.app.clone();
        let lit_id_clone = lit_id.clone();

        let task = cx.spawn(|this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let result: Result<String, String> = async {
                    let lit = app
                        .db
                        .get_literature(&lit_id_clone)
                        .map_err(|e| format!("读取文献数据库失败: {:?}", e))?
                        .ok_or_else(|| "未找到指定文献".to_string())?;

                    let mut pdf_path = None;
                    for att in &lit.attachments {
                        if att.file_path.to_lowercase().ends_with(".pdf") {
                            pdf_path = Some(att.file_path.clone());
                            break;
                        }
                    }

                    let mut pdf_text = None;
                    if let Some(path) = pdf_path {
                        let _ = this.update(&mut cx, |this, cx| {
                            if let Some(n) = this.notes_cache.iter_mut().find(|n| n.id == "ai_generating_note") {
                                n.content = "正在提取 PDF 纯文本，这可能需要一点时间...\n\n".to_string();
                            }
                            cx.notify();
                        });

                        pdf_text = Some(services::pdf::extract_text_from_pdf(&path).map_err(|e| format!("PDF 文本解析失败: {:?}", e))?);
                    }

                    let _ = this.update(&mut cx, |this, cx| {
                        if let Some(n) = this.notes_cache.iter_mut().find(|n| n.id == "ai_generating_note") {
                            n.content = "正在发起 AI 总结生成...\n\n".to_string();
                        }
                        cx.notify();
                    });

                    let keys = app.local_state.read().unwrap().translation_keys.clone();
                    let entries_json = keys.get("ai.entries").cloned().unwrap_or_default();
                    let active_name = keys.get("chat.active").cloned().unwrap_or_default();
                    let entries: Vec<ai::AiBackendEntry> =
                        serde_json::from_str(&entries_json).unwrap_or_default();
                    let entry = entries
                        .iter()
                        .find(|e| e.name == active_name)
                        .ok_or_else(|| "未配置默认 AI 聊天模型，请在设置中配置".to_string())?;

                    let kind = ai::BackendKind::from_str(&entry.kind);
                    let config = entry.to_config();
                    let service = ai::AiService::new(kind, &config);

                    let mut prompt_content = format!(
                        "文献标题: {}\n摘要: {}\n",
                        lit.title,
                        lit.abstract_text.as_deref().unwrap_or("")
                    );
                    if let Some(text) = pdf_text {
                        prompt_content.push_str(&format!("\n正文全文:\n{}", text));
                    }

                    let messages = vec![
                        ai::ChatMessage {
                            role: ai::ChatRole::User,
                            content: prompt_content,
                            attachments: Vec::new(),
                        }
                    ];

                    let system_prompt = "你是一个精通学术论文分析的 AI 助手。请针对用户给出的文献（包含标题、摘要及提取的全文），写一份详细且条理清晰的学术总结。总结必须包含：1. 研究背景与动机（作者为什么要研究这个问题）；2. 核心方法与模型（作者是如何实现和解决这个问题的，包含哪些技术核心）；3. 关键实验结果（核心数据、结论等）；4. 主要结论与学术贡献。请用中文回答，并以清晰易读的 Markdown 格式输出。注意：必须直接输出 Markdown 纯文本，严禁在最外层使用 ```markdown ... ``` 或 ``` ... ``` 这样的代码块标记包裹整篇回答。所有数学符号、希腊字母、公式等使用 LaTeX 语法书写，公式必须且只能使用 $$ 包裹（例如 $$a^2 + b^2 = c^2$$，不要使用 \\(...\\) 或 \\[...\\] 等包裹方式），同时请避免输出复杂或多行的公式，尽量使用简单、单行的公式形式。";

                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                    let result_handle = crate::RUNTIME.spawn(async move {
                        let mut stream = service
                            .chat_stream(&messages, Some(system_prompt))
                            .map_err(|e| format!("AI 服务请求失败: {:?}", e))
                            .await?;

                        while let Some(chunk) = stream.next().await {
                            let chunk_text = chunk.map_err(|e| format!("流传输异常: {:?}", e))?;
                            match &chunk_text {
                                models::chat::ChatResponseChunk::Content(text) => {
                                    log::info!(
                                        "[AI Summary Chunk(detail)] Content: len={}, preview={:?}",
                                        text.len(),
                                        &text[..text.len().min(80)]
                                    );
                                    let _ = tx.send(text.clone());
                                }
                                other => {
                                    log::info!("[AI Summary Chunk(detail)] Other variant: {:?}", other);
                                }
                            }
                        }
                        Ok::<(), String>(())
                    });

                    let mut full_output = String::new();
                    while let Some(text) = rx.recv().await {
                        full_output.push_str(&text);
                        let display_output = full_output.clone();
                        let _ = this.update(&mut cx, |this, cx| {
                            if let Some(n) = this.notes_cache.iter_mut().find(|n| n.id == "ai_generating_note") {
                                n.content = display_output;
                            }
                            cx.notify();
                        });
                    }

                    match result_handle.await {
                        Ok(Ok(())) => {}
                        Ok(Err(err)) => return Err(err),
                        Err(e) => return Err(format!("Tokio 任务执行异常: {:?}", e)),
                    }

                    if full_output.trim().is_empty() {
                        return Err("AI 服务返回了空内容".to_string());
                    }

                    log::info!(
                        "[AI Summary Final(detail)] total_len={}, starts_with_code_block={}, ends_with_code_block={}, preview_end={:?}",
                        full_output.len(),
                        full_output.trim().starts_with("```"),
                        full_output.trim().ends_with("```"),
                        full_output.chars().rev().take(200).collect::<String>()
                    );

                    let note_id = app
                        .literature_service
                        .create_note(&app.db, &lit_id_clone, "AI 总结")
                        .ok_or_else(|| "创建文献笔记失败".to_string())?;

                    let _ = this.update(&mut cx, |this, _cx| {
                        this.last_ai_summary_note_id = Some(note_id.clone());
                    });

                    let ok = app
                        .literature_service
                        .update_note(&app.db, &note_id, Some("AI 总结"), Some(&full_output));

                    if !ok {
                        return Err("保存笔记内容失败".to_string());
                    }

                    app.notify_data_changed();

                    Ok(full_output)
                }.await;

                let _ = this.update(&mut cx, |this, cx| {
                    this.is_generating_summary = false;
                    this.notes_cache.retain(|n| n.id != "ai_generating_note");
                    match result {
                        Ok(_) => {
                            this.reload_notes(cx);
                        }
                        Err(err_msg) => {
                            error!("AI 总结生成失败: {}", err_msg);
                            show_notification(
                                NotificationType::Error,
                                format!("AI 总结生成失败: {}", err_msg),
                                cx,
                            );
                            this.reload_notes(cx);
                        }
                    }
                    cx.notify();
                });
            }
        });

        self.summary_task = Some(task);
    }
}
