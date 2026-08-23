use crate::pdf::PdfReaderView;
use crate::pdf::components::edit_chat_dialog::EditChatSessionDialog;
use crate::pdf::components::streaming_bubble::{CHAT_BODY_FONT_SIZE, StreamingBubbleView};
use components::IconName;
use gpui::prelude::*;
use gpui::{
    Bounds, Context, FontWeight, Point, TitlebarOptions, WeakEntity, Window, WindowBounds,
    WindowKind, WindowOptions, div, list, px, relative, size,
};
use gpui_component::Root;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::{ActiveTheme, Disableable, Icon, h_flex, label::Label, v_flex};
use i18n::{I18nKey, Language};
use log::debug;
use services::pdf::PdfReaderDelegate;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct ChatSessionView {
    delegate: Option<Arc<dyn PdfReaderDelegate>>,
    language: Language,
    session_id: String,
    session_title: String,
    system_prompt: String,

    chat_messages: Vec<models::chat::ChatMessage>,
    streaming_bubble_view: Option<gpui::Entity<StreamingBubbleView>>,
    chat_reasoning_expanded: std::collections::HashSet<i64>,
    is_chat_streaming: bool,
    chat_input_state: Option<gpui::Entity<TextareaState>>,
    chat_input_sub: Option<gpui::Subscription>,
    list_state: gpui::ListState,
    chat_quote_expanded: std::collections::HashSet<i64>,
    chat_selected_attachments: Vec<models::Attachment>,
    chat_show_attachment_picker: bool,
    pending_quote: Option<String>,

    parent_handle: WeakEntity<PdfReaderView>,

    editing_message_id: Option<String>,
    editing_input_state: Option<gpui::Entity<InputState>>,

    message_siblings: std::collections::HashMap<String, Vec<String>>,
    cached_attachments: Vec<models::Attachment>,
}

impl ChatSessionView {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        delegate: Option<Arc<dyn PdfReaderDelegate>>,
        language: Language,
        session_id: String,
        session_title: String,
        system_prompt: String,
        messages: Vec<models::chat::ChatMessage>,
        parent: WeakEntity<PdfReaderView>,
        _cx: &mut Context<Self>,
    ) -> Self {
        let msg_count = messages.len();
        let mut view = Self {
            delegate,
            language,
            session_id,
            session_title,
            system_prompt,
            chat_messages: messages,
            streaming_bubble_view: None,
            chat_reasoning_expanded: std::collections::HashSet::new(),
            is_chat_streaming: false,
            chat_input_state: None,
            chat_input_sub: None,
            list_state: gpui::ListState::new(msg_count, gpui::ListAlignment::Bottom, px(1000.)),
            chat_quote_expanded: std::collections::HashSet::new(),
            chat_selected_attachments: Vec::new(),
            chat_show_attachment_picker: false,
            pending_quote: None,
            parent_handle: parent,
            editing_message_id: None,
            editing_input_state: None,
            message_siblings: std::collections::HashMap::new(),
            cached_attachments: Vec::new(),
        };
        view.reload_siblings();
        view.cached_attachments = view
            .delegate
            .as_ref()
            .map(|d| d.current_literature_attachments())
            .unwrap_or_default();
        view
    }

    fn reload_siblings(&mut self) {
        self.message_siblings.clear();
        if let Some(ref delegate) = self.delegate {
            for msg in &self.chat_messages {
                if !msg.id.is_empty() {
                    let sibs = delegate.get_message_siblings(&msg.id);
                    self.message_siblings.insert(msg.id.clone(), sibs);
                }
            }
        }
    }

    fn start_chat_stream(&mut self, session_id: String, cx: &mut Context<Self>) {
        let messages_for_ai: Vec<models::chat::ChatMessage> = self.chat_messages.clone();
        let system_prompt = self.system_prompt.clone();

        debug!(
            "[Chat] start_chat_stream: session_id={session_id}, messages_count={}, system_prompt_len={}",
            messages_for_ai.len(),
            system_prompt.len(),
        );

        let lang = self.language;
        let bubble = cx.new(|cx| StreamingBubbleView::new(lang, cx));
        self.streaming_bubble_view = Some(bubble.clone());
        self.is_chat_streaming = true;
        self.list_state.reset(self.chat_messages.len() + 1);
        cx.notify();

        if let Some(ref delegate) = self.delegate.clone() {
            let delegate = delegate.clone();
            cx.spawn(|this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    match delegate
                        .chat_stream(
                            session_id.clone(),
                            messages_for_ai.clone(),
                            system_prompt.clone(),
                        )
                        .await
                    {
                        Ok(mut rx) => {
                            let notify_interval = Duration::from_millis(50);
                            let mut last_notify = Instant::now() - notify_interval;
                            while let Some(chunk) = rx.recv().await {
                                let now = Instant::now();
                                let should_notify =
                                    now.duration_since(last_notify) >= notify_interval;
                                if should_notify {
                                    bubble.update(&mut cx, |v, cx| match chunk {
                                        models::chat::ChatResponseChunk::Content(ref text) => {
                                            v.append_content(text, cx);
                                        }
                                        models::chat::ChatResponseChunk::Reasoning(ref text) => {
                                            v.append_reasoning(text, cx);
                                        }
                                    });
                                    last_notify = now;
                                } else {
                                    bubble.update(&mut cx, |v, _cx| match chunk {
                                        models::chat::ChatResponseChunk::Content(ref text) => {
                                            v.text.push_str(text);
                                        }
                                        models::chat::ChatResponseChunk::Reasoning(ref text) => {
                                            v.reasoning.push_str(text);
                                        }
                                    });
                                }
                            }
                            let _ = this.update(&mut cx, |this, cx| {
                                let (msg, reasoning) =
                                    if let Some(ref sv) = this.streaming_bubble_view {
                                        sv.update(cx, |v, _| v.take_final())
                                    } else {
                                        (String::new(), String::new())
                                    };
                                this.streaming_bubble_view = None;
                                this.is_chat_streaming = false;
                                let sid = session_id.clone();
                                let reasoning_opt = if reasoning.is_empty() {
                                    None
                                } else {
                                    Some(reasoning.as_str())
                                };
                                let parent_id = this.chat_messages.last().map(|m| m.id.clone());
                                let mut msg_id = String::new();
                                if let Some(ref delegate) = this.delegate
                                    && let Some(id) = delegate.add_chat_message_with_parent(
                                        &sid,
                                        "assistant",
                                        &msg,
                                        &[],
                                        reasoning_opt,
                                        parent_id.as_deref(),
                                    )
                                {
                                    msg_id = id;
                                }
                                let assistant_msg = models::chat::ChatMessage {
                                    id: msg_id,
                                    session_id: sid,
                                    role: "assistant".to_string(),
                                    content: msg,
                                    reasoning: if reasoning.is_empty() {
                                        None
                                    } else {
                                        Some(reasoning)
                                    },
                                    attachments: Vec::new(),
                                    created_at: chrono::Utc::now().timestamp(),
                                    parent_id,
                                };
                                this.chat_messages.push(assistant_msg);
                                this.list_state.reset(this.chat_messages.len());
                                cx.notify();
                            });
                        }
                        Err(e) => {
                            let _ = this.update(&mut cx, |this, cx| {
                                this.streaming_bubble_view = None;
                                this.is_chat_streaming = false;
                                let err_msg = format!("Error: {e}");
                                let sid = session_id.clone();
                                let parent_id = this.chat_messages.last().map(|m| m.id.clone());
                                let mut msg_id = String::new();
                                if let Some(ref delegate) = this.delegate
                                    && let Some(id) = delegate.add_chat_message_with_parent(
                                        &sid,
                                        "assistant",
                                        &err_msg,
                                        &[],
                                        None,
                                        parent_id.as_deref(),
                                    )
                                {
                                    msg_id = id;
                                }
                                this.chat_messages.push(models::chat::ChatMessage {
                                    id: msg_id,
                                    session_id: sid,
                                    role: "assistant".to_string(),
                                    content: err_msg,
                                    reasoning: None,
                                    attachments: Vec::new(),
                                    created_at: chrono::Utc::now().timestamp(),
                                    parent_id,
                                });
                                this.list_state.reset(this.chat_messages.len());
                                cx.notify();
                            });
                        }
                    }
                }
            })
            .detach();
        }
    }

    fn render_chat_bubble(
        &self,
        msg: &models::chat::ChatMessage,
        cached_attachments: &[models::Attachment],
        theme: &gpui_component::Theme,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_user = msg.role == "user";
        let is_quote = msg.role == "quote";

        let bubble_color = if is_user { theme.primary } else { theme.muted };

        let reasoning = msg.reasoning.as_deref();

        let display_content = msg.content.clone();

        if is_quote {
            let expanded = self.chat_quote_expanded.contains(&msg.created_at);
            let is_long = display_content.len() > 100;
            let created_at = msg.created_at;
            return v_flex().w_full().items_end().child(
                v_flex()
                    .relative()
                    .group("chat-bubble-hover-group")
                    .w(relative(0.8))
                    .bg(theme.primary.opacity(0.04))
                    .border_l_3()
                    .border_color(theme.primary)
                    .rounded_md()
                    .px_3()
                    .py_2()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .child(
                                Label::new(i18n::t(I18nKey::QuoteLabel, self.language))
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme.primary),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .when(is_long, |this| {
                                        this.child(
                                            div()
                                                .id(gpui::SharedString::from(format!(
                                                    "quote-toggle-{}",
                                                    created_at
                                                )))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(move |this, _, _, cx| {
                                                        if this
                                                            .chat_quote_expanded
                                                            .contains(&created_at)
                                                        {
                                                            this.chat_quote_expanded
                                                                .remove(&created_at);
                                                        } else {
                                                            this.chat_quote_expanded
                                                                .insert(created_at);
                                                        }
                                                        cx.notify();
                                                    }),
                                                )
                                                .child(
                                                    Icon::new(if expanded {
                                                        IconName::ChevronDown
                                                    } else {
                                                        IconName::ChevronRight
                                                    })
                                                    .size(px(12.0))
                                                    .text_color(theme.muted_foreground),
                                                ),
                                        )
                                    })
                                    .child(
                                        div()
                                            .id(gpui::SharedString::from(format!(
                                                "quote-rollback-{}",
                                                msg.id
                                            )))
                                            .cursor_pointer()
                                            .invisible()
                                            .group_hover("chat-bubble-hover-group", |style| {
                                                style.visible()
                                            })
                                            .on_mouse_down(
                                                gpui::MouseButton::Left,
                                                cx.listener({
                                                    let msg_id = msg.id.clone();
                                                    let session_id = self.session_id.clone();
                                                    move |this, _, _window, cx| {
                                                        if let Some(ref delegate) = this.delegate {
                                                            let _ = delegate
                                                                .truncate_chat_messages_after(
                                                                    &session_id,
                                                                    &msg_id,
                                                                );
                                                            this.chat_messages = delegate
                                                                .list_chat_messages(&session_id);
                                                            this.reload_siblings();
                                                            this.list_state
                                                                .reset(this.chat_messages.len());
                                                            cx.notify();
                                                        }
                                                    }
                                                }),
                                            )
                                            .child(
                                                Icon::new(IconName::Close)
                                                    .size(px(12.0))
                                                    .text_color(theme.primary),
                                            ),
                                    ),
                            ),
                    )
                    .child(if expanded || !is_long {
                        Label::new(display_content)
                            .text_xs()
                            .text_color(theme.foreground.opacity(0.8))
                            .into_any_element()
                    } else {
                        Label::new(display_content)
                            .text_xs()
                            .text_color(theme.foreground.opacity(0.8))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .into_any_element()
                    }),
            );
        }

        let file_labels = models::Attachment::compute_labels(cached_attachments);
        let mut path_labels = std::collections::HashMap::new();
        for file in cached_attachments {
            if let Some(label) = file_labels.get(&file.id) {
                path_labels.insert(file.file_path.clone(), label.clone());
            }
        }

        let has_attachments = !msg.attachments.is_empty() && !is_quote;
        let siblings = self
            .message_siblings
            .get(&msg.id)
            .cloned()
            .unwrap_or_default();
        let current_idx = if siblings.len() > 1 {
            siblings.iter().position(|id| id == &msg.id).unwrap_or(0)
        } else {
            0
        };
        let is_editing = self.editing_message_id.as_ref() == Some(&msg.id) && !msg.id.is_empty();

        v_flex()
            .w_full()
            .when(is_user, |this| this.items_end())
            .when(!is_user, |this| this.items_start())
            .child(
                v_flex()
                    .when(is_user, |this| {
                        this.w(relative(0.8))
                            .bg(theme.primary.opacity(0.06))
                            .border_1()
                            .border_color(theme.primary)
                    })
                    .when(!is_user, |this| {
                        this.bg(bubble_color.opacity(0.15))
                    })
                    .rounded_md()
                    .px_2()
                    .py_1()
                    .gap_0p5()
                    .when(has_attachments, |this| {
                        this.child(h_flex().flex_wrap().gap_1().children(
                            msg.attachments.iter().map(|fp| {
                                let display_ext =
                                    path_labels.get(fp).cloned().unwrap_or_else(|| {
                                        std::path::Path::new(fp)
                                            .extension()
                                            .and_then(|e| e.to_str())
                                            .unwrap_or("FILE")
                                            .to_uppercase()
                                    });
                                div()
                                    .text_xs()
                                    .bg(if is_user {
                                        theme.primary.opacity(0.1)
                                    } else {
                                        theme.muted
                                    })
                                    .text_color(if is_user {
                                        theme.primary
                                    } else {
                                        theme.muted_foreground
                                    })
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .child(display_ext)
                                    .into_any_element()
                            }),
                        ))
                    })
                    .when_some(reasoning, |this, r| {
                        let is_expanded = self.chat_reasoning_expanded.contains(&msg.created_at);
                        let created_at = msg.created_at;
                        this.child(
                            v_flex()
                                .w_full()
                                .my_1()
                                .child(
                                    h_flex()
                                        .id(gpui::SharedString::from(format!(
                                            "reasoning-toggle-{}",
                                            created_at
                                        )))
                                        .cursor_pointer()
                                        .items_center()
                                        .gap_1()
                                        .on_mouse_down(
                                            gpui::MouseButton::Left,
                                            cx.listener(move |this, _, _, cx| {
                                                if this
                                                    .chat_reasoning_expanded
                                                    .contains(&created_at)
                                                {
                                                    this.chat_reasoning_expanded
                                                        .remove(&created_at);
                                                } else {
                                                    this.chat_reasoning_expanded.insert(created_at);
                                                }
                                                cx.notify();
                                            }),
                                        )
                                        .child(
                                            Icon::new(if is_expanded {
                                                IconName::ChevronDown
                                            } else {
                                                IconName::ChevronRight
                                            })
                                            .size(px(12.0))
                                            .text_color(theme.muted_foreground),
                                        )
                                        .child(
                                            Label::new("思考过程")
                                                .text_xs()
                                                .text_color(theme.muted_foreground),
                                        ),
                                )
                                .when(is_expanded, |this| {
                                    this.child(
                                        h_flex()
                                            .pl_2()
                                            .border_l_1()
                                            .border_color(theme.muted_foreground.opacity(0.3))
                                            .child(
                                                div()
                                                    .w_full()
                                                    .child(
                                                        TextView::markdown(
                                                            gpui::SharedString::from(format!(
                                                                "chat-reasoning-{}",
                                                                created_at
                                                            )),
                                                            gpui::SharedString::from(
                                                                services::pdf::preprocess_math(r),
                                                            ),
                                                        )
                                                        .style(
                                                            TextViewStyle::default()
                                                                .heading_font_size(|level, _| {
                                                                    match level {
                                                                        1 => px(16.),
                                                                        2 => px(15.),
                                                                        3 => px(14.),
                                                                        4 => px(13.),
                                                                        _ => px(12.),
                                                                    }
                                                                }),
                                                        )
                                                        .selectable(true)
                                                        .text_size(px(13.))
                                                        .text_color(
                                                            theme.muted_foreground,
                                                        ),
                                                    ),
                                            ),
                                    )
                                }),
                        )
                    })
                    .child({
                        if is_editing {
                            let input_state = self.editing_input_state.clone().unwrap();
                            let msg_id = msg.id.clone();
                            let parent_id = msg.parent_id.clone();
                            let attachments = msg.attachments.clone();
                            let session_id = self.session_id.clone();

                            div()
                                .relative()
                                .w_full()
                                .min_w(px(160.0))
                                .pr_12() // 腾出右侧深色区域供勾和叉浮动排列
                                .child(Input::new(&input_state).w_full())
                                .child(
                                    h_flex()
                                        .absolute()
                                        .top_1p5()
                                        .right_0()
                                        .gap_2()
                                        .items_center()
                                        .child(
                                            // 勾（确认保存）
                                            div()
                                                .id(gpui::SharedString::from(format!("save-edit-btn-{}", msg_id)))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener({
                                                        let parent_id = parent_id.clone();
                                                        let attachments = attachments.clone();
                                                        let session_id = session_id.clone();
                                                        move |this, _, _window, cx| {
                                                            let new_text = this.editing_input_state.as_ref().unwrap().read(cx).text().to_string();
                                                            if new_text.is_empty() { return; }
                                                            if let Some(ref delegate) = this.delegate {
                                                                let new_msg_id = delegate.add_chat_message_with_parent(
                                                                    &session_id,
                                                                    "user",
                                                                    &new_text,
                                                                    &attachments,
                                                                    None,
                                                                    parent_id.as_deref(),
                                                                );
                                                                this.editing_message_id = None;
                                                                this.editing_input_state = None;
                                                                if let Some(new_id) = new_msg_id {
                                                                    let _ = delegate.switch_active_message(&session_id, &new_id);
                                                                }
                                                                this.chat_messages = delegate.list_chat_messages(&session_id);
                                                                this.reload_siblings();
                                                                this.list_state.reset(this.chat_messages.len());
                                                                this.start_chat_stream(session_id.clone(), cx);
                                                                cx.notify();
                                                            }
                                                        }
                                                    }),
                                                )
                                                .child(
                                                    Icon::new(IconName::Check)
                                                        .size(px(14.0))
                                                        .text_color(theme.foreground) // 让勾图标使用前景色，比白色底上的对比度更统一
                                                )
                                        )
                                        .child(
                                            // 叉（取消编辑）
                                            div()
                                                .id(gpui::SharedString::from(format!("cancel-edit-btn-{}", msg_id)))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.editing_message_id = None;
                                                        this.editing_input_state = None;
                                                        cx.notify();
                                                    }),
                                                )
                                                .child(
                                                    Icon::new(IconName::Close)
                                                        .size(px(14.0))
                                                        .text_color(theme.muted_foreground)
                                                )
                                        )
                                )
                        } else {
                            let theme = theme.clone();
                            let mut display_content = display_content.clone();
                            let mut parsed_quote = None;

                            if display_content.starts_with("> [PDF 引用]:") {
                                if let Some(pos) = display_content.find("\n\n") {
                                    let quote_line = &display_content[..pos];
                                    let quote_text = quote_line.trim_start_matches("> [PDF 引用]:").trim();
                                    parsed_quote = Some(quote_text.to_string());
                                    display_content = display_content[pos + 2..].to_string();
                                } else {
                                    let quote_text = display_content.trim_start_matches("> [PDF 引用]:").trim();
                                    parsed_quote = Some(quote_text.to_string());
                                    display_content = String::new();
                                }
                            }

                            let is_expanded = self.chat_quote_expanded.contains(&msg.created_at);
                            let created_at = msg.created_at;

                            div()
                                .relative()
                                .group("chat-bubble-hover-group")
                                .when_some(parsed_quote.clone(), |this, quote| {
                                    this.child(
                                        v_flex()
                                            .bg(theme.primary.opacity(0.04))
                                            .border_l_2()
                                            .border_color(theme.primary)
                                            .rounded_sm()
                                            .px_2()
                                            .py_1()
                                            .mb_2()
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                gpui::MouseButton::Left,
                                                cx.listener(move |this, _, _, cx| {
                                                    if this.chat_quote_expanded.contains(&created_at) {
                                                        this.chat_quote_expanded.remove(&created_at);
                                                    } else {
                                                        this.chat_quote_expanded.insert(created_at);
                                                    }
                                                    cx.notify();
                                                }),
                                            )
                                            .when(!is_expanded, |v| {
                                                let quote = quote.clone();
                                                v.child(
                                                    h_flex()
                                                        .w_full()
                                                        .items_center()
                                                        .justify_between()
                                                        .gap_2()
                                                        .child(
                                                            h_flex()
                                                                .items_center()
                                                                .gap_1()
                                                                .flex_1()
                                                                .overflow_hidden()
                                                                .child(
                                                                    Label::new("PDF 引用")
                                                                        .text_xs()
                                                                        .font_weight(FontWeight::BOLD)
                                                                        .text_color(theme.primary)
                                                                )
                                                                .child(
                                                                    Label::new(":")
                                                                        .text_xs()
                                                                        .font_weight(FontWeight::BOLD)
                                                                        .text_color(theme.primary)
                                                                )
                                                                .child(
                                                                    Label::new(quote)
                                                                        .text_xs()
                                                                        .text_color(theme.foreground.opacity(0.7))
                                                                        .whitespace_nowrap()
                                                                        .overflow_hidden()
                                                                        .text_ellipsis()
                                                                )
                                                        )
                                                        .child(
                                                            Icon::new(IconName::ChevronRight)
                                                                .size(px(10.0))
                                                                .text_color(theme.primary)
                                                        )
                                                )
                                            })
                                            .when(is_expanded, |v| {
                                                let quote = quote.clone();
                                                v.child(
                                                    h_flex()
                                                        .w_full()
                                                        .items_center()
                                                        .justify_between()
                                                        .child(
                                                            Label::new("PDF 引用")
                                                                .text_xs()
                                                                .font_weight(FontWeight::BOLD)
                                                                .text_color(theme.primary)
                                                        )
                                                        .child(
                                                            Icon::new(IconName::ChevronDown)
                                                                .size(px(10.0))
                                                                .text_color(theme.primary)
                                                        )
                                                )
                                                .child(
                                                    Label::new(quote)
                                                        .text_xs()
                                                        .text_color(theme.foreground.opacity(0.7))
                                                )
                                            })
                                    )
                                })
                                .child({
                                    static CHAT_CONTENT_CACHE: std::sync::LazyLock<
                                        std::sync::Mutex<std::collections::HashMap<String, String>>,
                                    > = std::sync::LazyLock::new(|| {
                                        std::sync::Mutex::new(std::collections::HashMap::new())
                                    });

                                    let cache_key = format!(
                                        "chat-msg-{}-{}-l{}",
                                        msg.role, msg.created_at, display_content.len()
                                    );

                                    let processed_content = {
                                        let mut cache = CHAT_CONTENT_CACHE.lock().unwrap();
                                        if let Some(cached_text) = cache.get(&cache_key) {
                                            cached_text.clone()
                                        } else {
                                            let processed = services::pdf::preprocess_math(&display_content);
                                            cache.insert(cache_key, processed.clone());
                                            processed
                                        }
                                    };

                                    TextView::markdown(
                                        gpui::SharedString::from(format!(
                                            "chat-msg-{}-{}",
                                            msg.role, msg.created_at
                                        )),
                                        gpui::SharedString::from(processed_content),
                                    )
                                    .style(
                                        TextViewStyle::default().heading_font_size(|level, _| match level {
                                            1 => CHAT_BODY_FONT_SIZE + px(8.),
                                            2 => CHAT_BODY_FONT_SIZE + px(6.),
                                            3 => CHAT_BODY_FONT_SIZE + px(4.),
                                            4 => CHAT_BODY_FONT_SIZE + px(2.),
                                            _ => CHAT_BODY_FONT_SIZE + px(1.),
                                        }),
                                    )
                                    .selectable(true)
                                    .text_size(CHAT_BODY_FONT_SIZE)
                                    .text_color(theme.foreground)
                                })
                                // 用户消息的编辑和回退按钮浮在右上角，鼠标 hover 时显示（2/3 尺寸缩放）
                                .when(is_user && !msg.id.is_empty(), |this| {
                                    this.child(
                                        h_flex()
                                            .absolute()
                                            .bottom_0()
                                            .right_0()
                                            .gap_1()
                                            .p_1()
                                            .invisible()
                                            .group_hover("chat-bubble-hover-group", |style| style.visible())
                                            .child(
                                                div()
                                                    .id(gpui::SharedString::from(format!("edit-btn-{}", msg.id)))
                                                    .cursor_pointer()
                                                    .on_mouse_down(
                                                        gpui::MouseButton::Left,
                                                        cx.listener({
                                                            let msg_id = msg.id.clone();
                                                            let msg_content = msg.content.clone();
                                                            move |this, _, window, cx| {
                                                                this.editing_message_id = Some(msg_id.clone());
                                                                let content_clone = msg_content.clone();
                                                                this.editing_input_state = Some(cx.new(|cx| {
                                                                    InputState::new(window, cx).default_value(content_clone)
                                                                }));
                                                                cx.notify();
                                                            }
                                                        }),
                                                    )
                                                    .child(
                                                        Icon::new(IconName::Annotations)
                                                            .size(px(12.0))
                                                            .text_color(theme.primary)
                                                    )
                                            )
                                            .child(
                                                div()
                                                    .id(gpui::SharedString::from(format!("rollback-btn-{}", msg.id)))
                                                    .cursor_pointer()
                                                    .on_mouse_down(
                                                        gpui::MouseButton::Left,
                                                        cx.listener({
                                                            let msg_id = msg.id.clone();
                                                            let msg_content = msg.content.clone();
                                                            let msg_attachments = msg.attachments.clone();
                                                            let session_id = self.session_id.clone();
                                                            move |this, _, window, cx| {
                                                                // 1. 将文本装填到底部对话输入框
                                                                if let Some(ref input_state) = this.chat_input_state {
                                                                    input_state.update(cx, |s, cx| {
                                                                        s.set_value(&msg_content, window, cx);
                                                                    });
                                                                }

                                                                // 2. 恢复当时选中的文件附件状态
                                                                this.chat_selected_attachments.clear();
                                                                if !msg_attachments.is_empty() {
                                                                    let current_literature_attachments = this
                                                                        .delegate
                                                                        .as_ref()
                                                                        .map(|d| d.current_literature_attachments())
                                                                        .unwrap_or_default();
                                                                    for fp in &msg_attachments {
                                                                        if let Some(att) = current_literature_attachments.iter().find(|a| &a.file_path == fp) {
                                                                            this.chat_selected_attachments.push(att.clone());
                                                                        }
                                                                    }
                                                                    this.chat_show_attachment_picker = true;
                                                                }

                                                                // 3. 执行删除操作
                                                                if let Some(ref delegate) = this.delegate {
                                                                    let _ = delegate.truncate_chat_messages_after(&session_id, &msg_id);
                                                                    this.chat_messages = delegate.list_chat_messages(&session_id);
                                                                    this.reload_siblings();
                                                                    this.list_state.reset(this.chat_messages.len());
                                                                    cx.notify();
                                                                }
                                                            }
                                                        }),
                                                    )
                                                    .child(
                                                        Icon::new(IconName::RotateCw)
                                                            .size(px(12.0))
                                                            .text_color(theme.primary)
                                                    )
                                            )
                                    )
                                })
                        }
                    }),
            )
            // 版本切换指示器（兄弟节点分页器），当且仅当有多版本且不处于编辑状态时在气泡下方显示
            .when(siblings.len() > 1 && !is_editing, |this| {
                this.child(
                    h_flex()
                        .items_center()
                        .gap_0p5()
                        .mt_1()
                        .child(
                            Button::new(gpui::SharedString::from(format!("prev-ver-{}", msg.id)))
                                .ghost()
                                .icon(IconName::ChevronLeft)
                                .compact()
                                .disabled(current_idx == 0)
                                .on_click(cx.listener({
                                    let siblings = siblings.clone();
                                    let session_id = self.session_id.clone();
                                    move |this, _, _, cx| {
                                        if current_idx > 0 {
                                            let target_msg_id = &siblings[current_idx - 1];
                                            if let Some(ref delegate) = this.delegate
                                                && let Ok(leaf_id) = delegate.find_deepest_leaf(target_msg_id) {
                                                    let _ = delegate.switch_active_message(&session_id, &leaf_id);
                                                    this.chat_messages = delegate.list_chat_messages(&session_id);
                                                    this.reload_siblings();
                                                    this.list_state.reset(this.chat_messages.len());
                                                    cx.notify();
                                                }
                                        }
                                    }
                                }))
                        )
                        .child(
                            Label::new(format!("{}/{}", current_idx + 1, siblings.len()))
                                .text_xs()
                                .text_color(theme.muted_foreground)
                        )
                        .child(
                            Button::new(gpui::SharedString::from(format!("next-ver-{}", msg.id)))
                                .ghost()
                                .icon(IconName::ChevronRight)
                                .compact()
                                .disabled(current_idx == siblings.len() - 1)
                                .on_click(cx.listener({
                                    let siblings = siblings.clone();
                                    let session_id = self.session_id.clone();
                                    move |this, _, _, cx| {
                                        if current_idx < siblings.len() - 1 {
                                            let target_msg_id = &siblings[current_idx + 1];
                                            if let Some(ref delegate) = this.delegate
                                                && let Ok(leaf_id) = delegate.find_deepest_leaf(target_msg_id) {
                                                    let _ = delegate.switch_active_message(&session_id, &leaf_id);
                                                    this.chat_messages = delegate.list_chat_messages(&session_id);
                                                    this.reload_siblings();
                                                    this.list_state.reset(this.chat_messages.len());
                                                    cx.notify();
                                                }
                                        }
                                    }
                                }))
                        )
                )
            })
    }
}

impl gpui::Render for ChatSessionView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;

        let backend_select = self.parent_handle.upgrade().map(|p| {
            p.update(cx, |parent, cx| {
                parent.get_or_create_chat_backend_select(window, cx)
            })
        });

        self.cached_attachments = self
            .delegate
            .as_ref()
            .map(|d| d.current_literature_attachments())
            .unwrap_or_default();

        if self.chat_input_state.is_none() {
            let entity = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder(i18n::t(I18nKey::ChatInputPlaceholder, self.language))
                    .auto_grow(1, 3)
            });
            self.chat_input_state = Some(entity);
        }

        if self.chat_input_state.is_some()
            && self.chat_input_sub.is_none()
            && let Some(entity) = &self.chat_input_state
        {
            let sub = cx.subscribe(entity, |this, _, event: &InputEvent, cx| {
                if let InputEvent::PressEnter { secondary, .. } = event {
                    if *secondary {
                        return;
                    }
                    // Enter without Shift → send
                    let text = this
                        .chat_input_state
                        .as_ref()
                        .map(|e| e.read(cx).text().to_string())
                        .unwrap_or_default();
                    // Strip trailing \n (multi-line mode inserts \n before PressEnter)
                    let trimmed = text.trim_end_matches('\n').to_string();
                    if trimmed.is_empty() {
                        return;
                    }
                    let session_id = this.session_id.clone();
                    let parent_id = this.chat_messages.last().map(|m| m.id.clone());
                    let mut msg_id = String::new();
                    let final_input = if let Some(ref quote) = this.pending_quote {
                        format!("> [PDF 引用]: {}\n\n{}", quote.trim(), trimmed)
                    } else {
                        trimmed
                    };
                    let attach_paths: Vec<String> = this
                        .chat_selected_attachments
                        .iter()
                        .map(|a| a.file_path.clone())
                        .collect();
                    if let Some(ref delegate) = this.delegate
                        && let Some(id) = delegate.add_chat_message_with_parent(
                            &session_id,
                            "user",
                            &final_input,
                            &attach_paths,
                            None,
                            parent_id.as_deref(),
                        )
                    {
                        msg_id = id;
                    }
                    let now = chrono::Utc::now().timestamp();
                    this.chat_messages.push(models::chat::ChatMessage {
                        id: msg_id,
                        session_id: session_id.clone(),
                        role: "user".to_string(),
                        content: final_input,
                        reasoning: None,
                        attachments: attach_paths,
                        created_at: now,
                        parent_id,
                    });
                    this.reload_siblings();
                    this.pending_quote = None; // 清空挂载的引用
                    this.chat_input_state = None;
                    this.chat_input_sub = None;
                    this.start_chat_stream(session_id, cx);
                    cx.notify();
                }
            });
            self.chat_input_sub = Some(sub);
        }

        v_flex()
            .size_full()
            .child(
                // ── 顶部栏 ──
                h_flex()
                    .w_full()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new("chat-back")
                            .ghost()
                            .icon(IconName::ChevronLeft)
                            .compact()
                            .on_click({
                                let parent = self.parent_handle.clone();
                                cx.listener(move |_this, _, _, cx| {
                                    if let Some(parent) = parent.upgrade() {
                                        parent.update(cx, |parent, cx| {
                                            parent.active_chat_session_id = None;
                                            parent.chat_session_view = None;
                                            cx.notify();
                                        });
                                    }
                                })
                            }),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            Label::new(self.session_title.clone())
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis(),
                        ),
                    )
                    .child(
                        Button::new("chat-edit-prompt")
                            .ghost()
                            .icon(IconName::Annotations)
                            .compact()
                            .on_click({
                                let parent = self.parent_handle.clone();
                                let sid = self.session_id.clone();
                                let title = self.session_title.clone();
                                let prompt = self.system_prompt.clone();
                                let lang = self.language;
                                cx.listener(move |this, _, _window, cx| {
                                    let delegate = this.delegate.clone();
                                    let parent = parent.clone();
                                    let title = title.clone();
                                    let prompt = prompt.clone();
                                    let sid = sid.clone();
                                    let size_val = size(px(480.0), px(400.0));
                                    let bounds = Bounds::centered(None, size_val, cx);
                                    let _ = cx.open_window(
                                        WindowOptions {
                                            window_bounds: Some(WindowBounds::Windowed(bounds)),
                                            titlebar: Some(TitlebarOptions {
                                                title: None,
                                                appears_transparent: true,
                                                traffic_light_position: Some(Point::new(
                                                    px(9.0),
                                                    px(9.0),
                                                )),
                                            }),
                                            is_resizable: false,
                                            is_minimizable: false,
                                            kind: WindowKind::Floating,
                                            ..Default::default()
                                        },
                                        move |window, cx| {
                                            let dialog = cx.new(|cx| {
                                                EditChatSessionDialog::new(
                                                    title.clone(),
                                                    prompt.clone(),
                                                    lang,
                                                    window,
                                                    cx,
                                                    move |result, _window, cx| {
                                                        if let Some((new_title, new_prompt)) =
                                                            result
                                                        {
                                                            if let Some(ref delegate) = delegate {
                                                                delegate.update_chat_session(
                                                                    &sid,
                                                                    Some(&new_title),
                                                                    Some(&new_prompt),
                                                                );
                                                            }
                                                            if let Some(parent) = parent.upgrade() {
                                                                parent.update(cx, |parent, cx| {
                                                                    if let Some(s) = parent
                                                                        .chat_sessions
                                                                        .iter_mut()
                                                                        .find(|s| s.id == sid)
                                                                    {
                                                                        s.title = new_title.clone();
                                                                        s.system_prompt =
                                                                            new_prompt.clone();
                                                                    }
                                                                    if let Some(ref entity) =
                                                                        parent.chat_session_view
                                                                    {
                                                                        entity.update(
                                                                            cx,
                                                                            |chat, cx| {
                                                                                chat.session_title =
                                                                                    new_title;
                                                                                chat.system_prompt =
                                                                                    new_prompt;
                                                                                cx.notify();
                                                                            },
                                                                        );
                                                                    }
                                                                    _window.remove_window();
                                                                });
                                                            }
                                                        } else {
                                                            _window.remove_window();
                                                        }
                                                    },
                                                )
                                            });

                                            cx.new(|cx| Root::new(dialog, window, cx))
                                        },
                                    );
                                })
                            }),
                    ),
            )
            .child(
                // ── 消息区域 ──
                v_flex()
                    .flex_grow(1.0)
                    .h_0()
                    .relative()
                    .px_2()
                    .py_2()
                    .child(
                        if self.chat_messages.is_empty() && !self.is_chat_streaming {
                            v_flex()
                                .size_full()
                                .items_center()
                                .justify_center()
                                .child(
                                    Label::new(i18n::t(
                                        I18nKey::ChatInputPlaceholder,
                                        self.language,
                                    ))
                                    .text_xs()
                                    .text_color(muted),
                                )
                                .into_any_element()
                        } else {
                            let view = cx.entity().downgrade();
                            let theme = theme.clone();
                            list(self.list_state.clone(), move |ix, window, cx| {
                                let result = view.update(cx, |this, cx| {
                                    if ix < this.chat_messages.len() {
                                        this.render_chat_bubble(
                                            &this.chat_messages[ix],
                                            &this.cached_attachments,
                                            &theme,
                                            window,
                                            cx,
                                        )
                                        .into_any_element()
                                    } else if let Some(ref sv) = this.streaming_bubble_view {
                                        sv.clone().into_any_element()
                                    } else {
                                        div().into_any_element()
                                    }
                                });
                                result.unwrap_or_else(|_| div().into_any_element())
                            })
                            .size_full()
                            .into_any_element()
                        },
                    ),
            )
            .child(
                // ── 输入栏 ──
                {
                    let mut input_section = v_flex()
                        .w_full()
                        .px_2()
                        .py_1p5()
                        .border_t_1()
                        .border_color(theme.border);

                    let attachments = &self.cached_attachments;

                    let file_labels = models::Attachment::compute_labels(attachments);

                    if self.chat_show_attachment_picker {
                        let selected_ids: Vec<String> = self
                            .chat_selected_attachments
                            .iter()
                            .map(|a| a.id.clone())
                            .collect();

                        let mut badges = Vec::new();

                        for att in attachments {
                            let is_selected = selected_ids.contains(&att.id);
                            let display_ext =
                                file_labels.get(&att.id).cloned().unwrap_or_else(|| {
                                    std::path::Path::new(&att.file_name)
                                        .extension()
                                        .and_then(|e| e.to_str())
                                        .unwrap_or("FILE")
                                        .to_uppercase()
                                });
                            let att_id = att.id.clone();
                            let file_path = att.file_path.clone();
                            let file_name = att.file_name.clone();
                            let is_main = att.is_main;

                            let badge = h_flex()
                                .items_center()
                                .gap_0p5()
                                .text_xs()
                                .bg(if is_main {
                                    theme.primary.opacity(0.1)
                                } else {
                                    theme.muted
                                })
                                .text_color(if is_main {
                                    theme.primary
                                } else {
                                    theme.muted_foreground
                                })
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .when(is_selected, |s| s.border_2().border_color(theme.primary))
                                .when(is_main && !is_selected, |s| s.font_weight(FontWeight::BOLD))
                                .cursor_pointer()
                                .on_mouse_down(
                                    gpui::MouseButton::Left,
                                    cx.listener(move |this, _, _, _cx| {
                                        if is_selected {
                                            this.chat_selected_attachments
                                                .retain(|a| a.id != att_id);
                                        } else {
                                            this.chat_selected_attachments.push(
                                                models::Attachment {
                                                    id: att_id.clone(),
                                                    literature_id: String::new(),
                                                    file_path: file_path.clone(),
                                                    file_name: file_name.clone(),
                                                    file_size: 0,
                                                    mime_type: None,
                                                    etag: None,
                                                    hash: None,
                                                    is_main,
                                                    is_dirty: false,
                                                    is_deleted: false,
                                                    version: 0,
                                                    created_at: 0,
                                                    updated_at: 0,
                                                },
                                            );
                                        }
                                        _cx.notify();
                                    }),
                                )
                                .child(display_ext);

                            badges.push(badge.into_any_element());
                        }

                        if !badges.is_empty() {
                            let picker = div()
                                .w_full()
                                .mb_1()
                                .p_1()
                                .bg(theme.muted.opacity(0.06))
                                .rounded_md()
                                .border_1()
                                .border_color(theme.border)
                                .child(div().flex().flex_wrap().gap_2().children(badges));
                            input_section = input_section.child(picker);
                        }
                    }

                    if let Some(ref text) = self.pending_quote {
                        let text_clone = text.clone();
                        let quote_card = h_flex()
                            .relative()
                            .w_full()
                            .bg(theme.primary.opacity(0.04))
                            .border_l_3()
                            .border_color(theme.primary)
                            .rounded_md()
                            .px_2()
                            .py_1()
                            .mb_1()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1()
                                    .flex_1()
                                    .overflow_hidden()
                                    .child(
                                        Label::new(i18n::t(I18nKey::QuoteLabel, self.language))
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(theme.primary),
                                    )
                                    .child(
                                        Label::new(":")
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(theme.primary),
                                    )
                                    .child(
                                        Label::new(text_clone)
                                            .text_xs()
                                            .text_color(theme.foreground.opacity(0.8))
                                            .whitespace_nowrap()
                                            .overflow_hidden()
                                            .text_ellipsis(),
                                    ),
                            )
                            .child(
                                div()
                                    .id(gpui::SharedString::from("cancel-pending-quote"))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            this.pending_quote = None;
                                            cx.notify();
                                        }),
                                    )
                                    .child(
                                        Icon::new(IconName::Close)
                                            .size(px(12.0))
                                            .text_color(theme.primary),
                                    ),
                            );
                        input_section = input_section.child(quote_card);
                    }

                    input_section
                        // 1. 上排操作栏：模型选择、思考模式、附件按钮、以及引用的 + 按钮
                        .child({
                            let parent = self.parent_handle.clone();
                            let has_selection = parent
                                .upgrade()
                                .and_then(|p| {
                                    p.read(cx).selected_text.as_ref().map(|t| !t.is_empty())
                                })
                                .unwrap_or(false);

                            h_flex()
                                .w_full()
                                .h(px(24.0))
                                .items_center()
                                .gap_1()
                                .mb_1()
                                .when_some(backend_select, |this, sel| {
                                    this.child(
                                        div()
                                            .flex_1()
                                            .child(gpui_component::select::Select::new(&sel)),
                                    )
                                })
                                .child(
                                    Button::new("chat-attach")
                                        .ghost()
                                        .icon(IconName::Attachment)
                                        .compact()
                                        .disabled(self.is_chat_streaming)
                                        .on_click(cx.listener(|this, _, _window, cx| {
                                            this.chat_show_attachment_picker =
                                                !this.chat_show_attachment_picker;
                                            cx.notify();
                                        })),
                                )
                                .when(has_selection, |row| {
                                    let parent = parent.clone();
                                    row.child(
                                        Button::new("chat-send-selection")
                                            .ghost()
                                            .icon(IconName::ZoomIn)
                                            .compact()
                                            .text_xs()
                                            .text_color(theme.primary)
                                            .on_click({
                                                let parent = parent.clone();
                                                cx.listener(move |this, _, _window, cx| {
                                                    if this.is_chat_streaming {
                                                        return;
                                                    }
                                                    let text = parent
                                                        .upgrade()
                                                        .and_then(|p| {
                                                            p.read(cx).selected_text.clone()
                                                        })
                                                        .unwrap_or_default();
                                                    if text.is_empty() {
                                                        return;
                                                    }
                                                    this.pending_quote = Some(text);
                                                    cx.notify();
                                                })
                                            }),
                                    )
                                })
                        })
                        // 2. 中下排：全宽输入框（仅回车发送）
                        .child(
                            div()
                                .w_full()
                                .when_some(self.chat_input_state.as_ref(), |this, e| {
                                    this.child(Textarea::new(e).w_full())
                                }),
                        )
                },
            )
    }
}
