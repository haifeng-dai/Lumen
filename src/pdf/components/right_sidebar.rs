use crate::pdf::PdfReaderView;
use crate::pdf::components::chat_session_view::ChatSessionView;
use crate::pdf::types::{RightSidebarTab, TOOLBAR_HEIGHT_REMS};
use components::IconName;
use gpui::prelude::*;
use gpui::{ClipboardItem, Context, WeakEntity, Window, div, px, rems};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::scroll::ScrollableElement;
use gpui_component::select::Select;
use gpui_component::{ActiveTheme, Icon, Selectable, h_flex, label::Label, v_flex};
use i18n::I18nKey;
use log::debug;

impl PdfReaderView {
    pub(crate) fn render_right_sidebar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();

        v_flex()
            .w(self.right_sidebar_width)
            .flex_shrink_0()
            .overflow_hidden()
            .h_full()
            .bg(theme.sidebar)
            .child(
                h_flex()
                    .w_full()
                    .h(rems(TOOLBAR_HEIGHT_REMS))
                    .px_2()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        Button::new("right-tab-translation")
                            .ghost()
                            .icon(IconName::Translate)
                            .h(rems(1.5))
                            .w(rems(1.5))
                            .when(
                                self.active_right_sidebar_tab == RightSidebarTab::Translation,
                                |b| b.selected(true),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_right_sidebar_tab = RightSidebarTab::Translation;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("right-tab-notes")
                            .ghost()
                            .icon(IconName::FileText)
                            .h(rems(1.5))
                            .w(rems(1.5))
                            .when(
                                self.active_right_sidebar_tab == RightSidebarTab::Notes,
                                |b| b.selected(true),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_right_sidebar_tab = RightSidebarTab::Notes;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("right-tab-chat")
                            .ghost()
                            .icon(IconName::MessageSquare)
                            .h(rems(1.5))
                            .w(rems(1.5))
                            .when(
                                self.active_right_sidebar_tab == RightSidebarTab::Chat,
                                |b| b.selected(true),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_right_sidebar_tab = RightSidebarTab::Chat;
                                cx.notify();
                            })),
                    ),
            )
            .child(v_flex().flex_grow(1.0).h_0().w_full().child({
                let element: gpui::AnyElement = match self.active_right_sidebar_tab {
                    RightSidebarTab::Translation => v_flex()
                        .size_full()
                        .child(self.render_translation_content(window, cx))
                        .child(self.render_translation_bottom_bar(cx))
                        .into_any_element(),
                    RightSidebarTab::Notes => {
                        self.render_notes_content(window, cx).into_any_element()
                    }
                    RightSidebarTab::Chat => {
                        self.render_chat_content(window, cx).into_any_element()
                    }
                };
                element
            }))
    }

    fn render_translation_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let select_state = self.get_or_create_engine_select(window, cx);
        let theme = cx.theme();
        let theme_foreground = theme.foreground;
        let theme_muted_foreground = theme.muted_foreground;
        let result = &self.translation_result;

        let original_text = result
            .as_ref()
            .map(|r| r.original.clone())
            .unwrap_or_default();
        let original_for_copy = original_text.clone();
        let is_placeholder = original_text.is_empty();
        let original_display: gpui::SharedString = if is_placeholder {
            i18n::t(I18nKey::SelectTextToTranslate, self.language).into()
        } else {
            original_text.into()
        };

        let translated_text = result.as_ref().and_then(|r| r.translated.clone());

        v_flex()
            .w_full()
            .p_3()
            .gap_2()
            .h_full()
            .relative() // 重要：为绝对定位的菜单提供容器
            .child(
                // ── 原文区域（可折叠）──
                v_flex()
                    .w_full()
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .rounded_md()
                    .when(self.translation_original_expanded, |this| {
                        this.flex_grow(1.0).h_0()
                    })
                    .child(
                        h_flex()
                            .px_2()
                            .py_1()
                            .bg(theme.accent.opacity(0.05))
                            .items_center()
                            .justify_between()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .cursor_pointer()
                                    .on_mouse_up(
                                        gpui::MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.translation_original_expanded =
                                                !this.translation_original_expanded;
                                            if let Some(ref delegate) = this.delegate {
                                                delegate.set_translation_original_expanded(
                                                    this.translation_original_expanded,
                                                );
                                            }
                                            cx.notify();
                                        }),
                                    )
                                    .child(
                                        Icon::new(if self.translation_original_expanded {
                                            IconName::ChevronDown
                                        } else {
                                            IconName::ChevronRight
                                        })
                                        .text_color(theme.muted_foreground),
                                    )
                                    .child(
                                        Label::new(i18n::t(
                                            I18nKey::OriginalSection,
                                            self.language,
                                        ))
                                        .text_sm()
                                        .text_color(theme.foreground),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(div().w(px(140.0)).child(Select::new(&select_state)))
                                    .when(!original_for_copy.is_empty(), |this| {
                                        this.child(
                                            Button::new("append-translation-toggle")
                                                .ghost()
                                                .icon(IconName::Plus)
                                                .h(rems(1.5))
                                                .w(rems(1.5))
                                                .text_color(if self.append_translation_mode {
                                                    theme.primary
                                                } else {
                                                    theme.muted_foreground
                                                })
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.append_translation_mode =
                                                        !this.append_translation_mode;
                                                    cx.notify();
                                                })),
                                        )
                                        .child({
                                            let copied_original = self.copied_translation_original;
                                            Button::new("copy-original")
                                                .ghost()
                                                .icon(if copied_original {
                                                    IconName::Check
                                                } else {
                                                    IconName::Copy
                                                })
                                                .h(rems(1.5))
                                                .w(rems(1.5))
                                                .when(copied_original, |btn| {
                                                    btn.text_color(theme.primary)
                                                })
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        cx.write_to_clipboard(
                                                            ClipboardItem::new_string(
                                                                original_for_copy.clone(),
                                                            ),
                                                        );
                                                        this.copied_translation_original = true;
                                                        cx.notify();

                                                        let window_handle = window.window_handle();
                                                        cx.spawn(move |view: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                                                            let mut cx = cx.clone();
                                                            async move {
                                                                cx.background_executor()
                                                                    .timer(std::time::Duration::from_millis(1500))
                                                                    .await;
                                                                let _ = cx.update_window(
                                                                    window_handle,
                                                                    |_, _, cx| {
                                                                        if let Some(view) = view.upgrade() {
                                                                            view.update(cx, |this, cx| {
                                                                                this.copied_translation_original = false;
                                                                                cx.notify();
                                                                            });
                                                                        }
                                                                    },
                                                                );
                                                            }
                                                        })
                                                        .detach();
                                                    },
                                                ))
                                        })
                                    }),
                            ),
                    )
                    .when(self.translation_original_expanded, |this| {
                        this.child(
                            v_flex()
                                .flex_grow(1.0)
                                .h_0()
                                .w_full()
                                .overflow_y_scrollbar()
                                .px_2()
                                .py_2()
                                .child(
                                    Label::new(original_display.clone())
                                        .w_full()
                                        .text_size(px(self.translation_font_size))
                                        .when(is_placeholder, |l| {
                                            l.text_color(theme.muted_foreground)
                                        }),
                                ),
                        )
                    }),
            )
            .child(
                // ── 译文区域（始终撑满剩余空间）──
                v_flex()
                    .w_full()
                    .flex_grow(1.0)
                    .h_0()
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .rounded_md()
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .px_2()
                            .py_1()
                            .bg(theme.accent.opacity(0.05))
                            .items_center()
                            .justify_between()
                            .child(
                                Label::new(i18n::t(I18nKey::TranslationSection, self.language))
                                    .text_sm()
                                    .text_color(theme.foreground),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        Button::new("auto-translate-toggle")
                                            .ghost()
                                            .icon(IconName::FastForward)
                                            .h(rems(1.5))
                                            .w(rems(1.5))
                                            .text_color(if self.auto_translate {
                                                theme.primary
                                            } else {
                                                theme.muted_foreground
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.auto_translate = !this.auto_translate;
                                                this.request_state_save(cx);
                                                cx.notify();
                                            })),
                                    )
                                    .when_some(self.translation_result.clone(), |this, res| {
                                        this.child(
                                            Button::new("retry-translation")
                                                .ghost()
                                                .icon(IconName::RotateCw)
                                                .h(rems(1.5))
                                                .w(rems(1.5))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.translate_text(
                                                        res.original.clone(),
                                                        true,
                                                        cx,
                                                    );
                                                })),
                                        )
                                    })
                                    .when_some(translated_text.clone(), |this, text| {
                                        let copied_result = self.copied_translation_result;
                                        this.child(
                                            Button::new("copy-translated")
                                                .ghost()
                                                .icon(if copied_result {
                                                    IconName::Check
                                                } else {
                                                    IconName::Copy
                                                })
                                                .h(rems(1.5))
                                                .w(rems(1.5))
                                                .when(copied_result, |btn| {
                                                    btn.text_color(theme.primary)
                                                })
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        cx.write_to_clipboard(
                                                            ClipboardItem::new_string(text.clone()),
                                                        );
                                                        this.copied_translation_result = true;
                                                        cx.notify();

                                                        let window_handle = window.window_handle();
                                                        cx.spawn(move |view: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                                                            let mut cx = cx.clone();
                                                            async move {
                                                                cx.background_executor()
                                                                    .timer(std::time::Duration::from_millis(1500))
                                                                    .await;
                                                                let _ = cx.update_window(
                                                                    window_handle,
                                                                    |_, _, cx| {
                                                                        if let Some(view) = view.upgrade() {
                                                                            view.update(cx, |this, cx| {
                                                                                this.copied_translation_result = false;
                                                                                cx.notify();
                                                                            });
                                                                        }
                                                                    },
                                                                );
                                                            }
                                                        })
                                                        .detach();
                                                    },
                                                )),
                                        )
                                    }),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_grow(1.0)
                            .h_0()
                            .w_full()
                            .overflow_y_scrollbar()
                            .px_2()
                            .py_2()
                            .child(match result {
                                Some(res) if res.is_loading => {
                                    Label::new(i18n::t(I18nKey::Translating, self.language))
                                        .w_full()
                                        .text_size(px(self.translation_font_size))
                                        .text_color(theme_muted_foreground)
                                        .into_any_element()
                                }
                                Some(res) if res.error.is_some() => {
                                    Label::new(res.error.clone().unwrap())
                                        .w_full()
                                        .text_size(px(self.translation_font_size))
                                        .text_color(gpui::red())
                                        .into_any_element()
                                }
                                Some(res) => match &res.translated {
                                    Some(t) => crate::ui::components::render_math_markdown(
                                        "pdf-translation-content",
                                        t,
                                        px(self.translation_font_size),
                                        Some(theme_foreground),
                                        None,
                                        cx,
                                    )
                                    .into_any_element(),
                                    None => Label::new(i18n::t(
                                        I18nKey::TranslationPending,
                                        self.language,
                                    ))
                                    .w_full()
                                    .text_size(px(self.translation_font_size))
                                    .text_color(theme_muted_foreground)
                                    .into_any_element(),
                                },
                                None => Label::new(i18n::t(
                                    I18nKey::SelectTextToTranslate,
                                    self.language,
                                ))
                                .w_full()
                                .text_size(px(self.translation_font_size))
                                .text_color(theme_muted_foreground)
                                .into_any_element(),
                            }),
                    ),
            )
    }

    fn render_notes_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;

        // 首次渲染时加载笔记
        if self.notes_cache.is_empty()
            && let Some(delegate) = &self.delegate
        {
            let lit_id = self
                .document_id
                .split("::")
                .next()
                .unwrap_or(&self.document_id);
            self.notes_cache = delegate.list_notes(lit_id);
        }

        if let Some(index) = self.editing_note_index
            && index >= self.notes_cache.len()
        {
            self.editing_note_index = None;
            self.edit_note_title = None;
            self.edit_note_content = None;
        }

        if let Some(index) = self.editing_note_index {
            // 确保输入框状态在新建/编辑时都被正确初始化
            if self.edit_note_title.is_none() || self.edit_note_content.is_none() {
                let note = &self.notes_cache[index];
                let title = note.title.clone();
                let content = note.content.clone();

                let entity = cx.new(|cx| {
                    gpui_component::input::InputState::new(window, cx).placeholder("输入标题...")
                });
                entity.update(cx, |s, cx| {
                    s.set_value(&title, window, cx);
                });
                self.edit_note_title = Some(entity);

                let entity2 = cx.new(|cx| {
                    gpui_component::input::TextareaState::new(window, cx)
                        .placeholder("输入内容 (支持 Markdown)...")
                });
                entity2.update(cx, |s, cx| {
                    s.set_value(&content, window, cx);
                });
                self.edit_note_content = Some(entity2);
            }

            let note = &self.notes_cache[index];
            let note_id = note.id.clone();

            return v_flex()
                .size_full()
                .p_3()
                .gap_3()
                .child(
                    // ── 顶部栏：包含标题和操作按钮 ──
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .child(
                            Label::new("编辑笔记")
                                .text_sm()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(muted),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new(gpui::SharedString::from(format!(
                                        "note-cancel-{index}"
                                    )))
                                    .ghost()
                                    .icon(IconName::Close)
                                    .compact()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if this.notes_cache.get(index).map(|n| n.id.as_str())
                                            == Some("temp_new_note")
                                        {
                                            this.notes_cache.remove(index);
                                        }
                                        this.editing_note_index = None;
                                        this.edit_note_title = None;
                                        this.edit_note_content = None;
                                        cx.notify();
                                    })),
                                )
                                .child(
                                    Button::new(gpui::SharedString::from(format!(
                                        "note-save-{index}"
                                    )))
                                    .ghost()
                                    .icon(IconName::Check)
                                    .compact()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let new_title = this
                                            .edit_note_title
                                            .as_ref()
                                            .map(|e| e.read(cx).text().to_string());
                                        let new_content = this
                                            .edit_note_content
                                            .as_ref()
                                            .map(|e| e.read(cx).text().to_string());

                                        let mut final_note_id = note_id.clone();
                                        let is_temp = note_id == "temp_new_note";

                                        if is_temp && let Some(delegate) = &this.delegate {
                                            let lit_id = this
                                                .document_id
                                                .split("::")
                                                .next()
                                                .unwrap_or(&this.document_id)
                                                .to_string();
                                            let default_title = new_title
                                                .clone()
                                                .unwrap_or_else(|| "未命名笔记".to_string());
                                            if let Some(real_id) =
                                                delegate.create_note(&lit_id, &default_title)
                                            {
                                                final_note_id = real_id;
                                            }
                                        }

                                        if let Some(delegate) = &this.delegate {
                                            delegate.update_note(
                                                &final_note_id,
                                                new_title.as_deref(),
                                                new_content.as_deref(),
                                            );
                                        }
                                        if let Some(note) = this.notes_cache.get_mut(index) {
                                            note.id = final_note_id;
                                            if let Some(ref t) = new_title {
                                                note.title = t.clone();
                                            }
                                            if let Some(ref c) = new_content {
                                                note.content = c.clone();
                                            }
                                        }
                                        this.editing_note_index = None;
                                        this.edit_note_title = None;
                                        this.edit_note_content = None;
                                        cx.notify();
                                    })),
                                ),
                        ),
                )
                .when_some(self.edit_note_title.as_ref(), |this, e| {
                    this.child(gpui_component::input::Input::new(e).w_full())
                })
                .child(
                    // ── 内容输入框，通过 div 容器包裹撑满剩余纵向空间 ──
                    div().w_full().flex_grow(1.0).h_0().when_some(
                        self.edit_note_content.as_ref(),
                        |this, e| {
                            this.child(gpui_component::input::Textarea::new(e).w_full().h_full())
                        },
                    ),
                )
                .into_any_element();
        }

        v_flex()
            .size_full()
            .child(
                // ── 笔记顶部 Header ──
                h_flex()
                    .w_full()
                    .px_3()
                    .py_2()
                    .items_center()
                    .justify_between()
                    .child(
                        Label::new(i18n::t(I18nKey::Notes, self.language))
                            .text_sm()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(muted),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                Button::new("ai-summary-btn")
                                    .ghost()
                                    .icon(IconName::Star)
                                    .h(rems(1.5))
                                    .w(rems(1.5))
                                    .text_color(if self.is_generating_summary {
                                        theme.primary
                                    } else {
                                        theme.muted_foreground
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if this.is_generating_summary {
                                            return;
                                        }
                                        this.generate_ai_summary(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("add-note")
                                    .ghost()
                                    .icon(IconName::ZoomIn)
                                    .h(rems(1.5))
                                    .w(rems(1.5))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let lit_id = this
                                            .document_id
                                            .split("::")
                                            .next()
                                            .unwrap_or(&this.document_id)
                                            .to_string();
                                        let title = "".to_string();
                                        let note = models::LiteratureNote {
                                            id: "temp_new_note".to_string(),
                                            literature_id: lit_id,
                                            title,
                                            content: String::new(),
                                            sort_order: this.notes_cache.len() as i32,
                                            created_at: chrono::Utc::now().timestamp(),
                                            updated_at: chrono::Utc::now().timestamp(),
                                            is_deleted: false,
                                            is_dirty: false,
                                            version: 1,
                                        };
                                        this.notes_cache.push(note);
                                        this.editing_note_index = Some(this.notes_cache.len() - 1);
                                        this.edit_note_title = None;
                                        this.edit_note_content = None;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .flex_grow(1.0)
                    .h_0()
                    .overflow_y_scrollbar()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .children({
                        let mut cards: Vec<gpui::AnyElement> = Vec::new();
                        let notes_snapshot = self.notes_cache.clone();
                        for (i, note) in notes_snapshot.iter().enumerate() {
                            let card = self.render_note_card(i, note, window, cx);
                            cards.push(card.into_any_element());
                        }
                        if self.notes_cache.is_empty() {
                            let empty_lang = self.language;
                            cards.push(
                                v_flex()
                                    .size_full()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        Label::new(i18n::t(I18nKey::NoNotes, empty_lang))
                                            .text_xs()
                                            .text_color(muted),
                                    )
                                    .into_any_element(),
                            );
                        }
                        cards
                    }),
            )
            .into_any_element()
    }

    fn render_note_card(
        &mut self,
        index: usize,
        note: &models::LiteratureNote,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        let is_expanded = self.expanded_notes.contains(&note.id);

        let this_weak = cx.entity().downgrade();
        let et = note.title.clone();
        let ec = note.content.clone();
        let note_id_edit = note.id.clone();
        let on_edit = move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
            let _ = this_weak.update(cx, |this, cx| {
                if let Some(current_idx) =
                    this.notes_cache.iter().position(|n| n.id == note_id_edit)
                {
                    this.editing_note_index = Some(current_idx);
                    let entity = cx.new(|cx| {
                        gpui_component::input::InputState::new(window, cx).placeholder("标题")
                    });
                    entity.update(cx, |s, cx| {
                        s.set_value(&et, window, cx);
                    });
                    this.edit_note_title = Some(entity);
                    let entity2 =
                        cx.new(|cx| gpui_component::input::TextareaState::new(window, cx));
                    entity2.update(cx, |s, cx| {
                        s.set_value(&ec, window, cx);
                    });
                    this.edit_note_content = Some(entity2);
                    cx.notify();
                }
            });
        };

        let this_weak = cx.entity().downgrade();
        let note_id_del = note.id.clone();
        let on_delete = move |_: &gpui::ClickEvent, _window: &mut Window, cx: &mut gpui::App| {
            let _ = this_weak.update(cx, |this, cx| {
                if let Some(delegate) = &this.delegate {
                    delegate.delete_note(&note_id_del);
                }
                this.notes_cache.retain(|n| n.id != note_id_del);
                cx.notify();
            });
        };

        let this_weak = cx.entity().downgrade();
        let note_id_exp = note.id.clone();
        let on_toggle_expand =
            move |_: &gpui::ClickEvent, _window: &mut Window, cx: &mut gpui::App| {
                let _ = this_weak.update(cx, |this, cx| {
                    if this.expanded_notes.contains(&note_id_exp) {
                        this.expanded_notes.remove(&note_id_exp);
                    } else {
                        this.expanded_notes.insert(note_id_exp.clone());
                    }
                    cx.notify();
                });
            };

        render_shared_note_card(
            index,
            note,
            is_expanded,
            theme,
            window,
            cx,
            on_edit,
            on_delete,
            on_toggle_expand,
        )
    }

    fn render_translation_bottom_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        h_flex()
            .w_full()
            .h_10()
            .px_3()
            .items_center()
            .justify_center()
            .child(
                h_flex()
                    .bg(theme.muted.opacity(0.2))
                    .rounded_md()
                    .items_center()
                    .child(
                        Button::new("font-size-decrease")
                            .ghost()
                            .icon(IconName::ZoomOut)
                            .h(rems(1.5))
                            .w(rems(1.5))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.change_translation_font_size(-1.0, cx);
                            })),
                    )
                    .child(
                        div().px_1().min_w(px(24.0)).child(
                            Label::new(format!("{}", self.translation_font_size as i32))
                                .text_xs()
                                .text_center()
                                .text_color(theme.muted_foreground),
                        ),
                    )
                    .child(
                        Button::new("font-size-increase")
                            .ghost()
                            .icon(IconName::ZoomIn)
                            .h(rems(1.5))
                            .w(rems(1.5))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.change_translation_font_size(1.0, cx);
                            })),
                    ),
            )
    }

    // ── AI 对话 ─────────────────────────────────────────

    fn render_chat_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        let _muted = theme.muted_foreground;

        if self.chat_sessions.is_empty()
            && let Some(delegate) = &self.delegate
        {
            let lit_id = self
                .document_id
                .split("::")
                .next()
                .unwrap_or(&self.document_id);
            debug!("[Chat] render_chat_content: loading sessions from DB, lit_id={lit_id}");
            self.chat_sessions = delegate.list_chat_sessions(lit_id);
            debug!(
                "[Chat] render_chat_content: loaded {} sessions",
                self.chat_sessions.len()
            );
        }

        debug!(
            "[Chat] render_chat_content: sessions={}, creating={}, active={:?}",
            self.chat_sessions.len(),
            self.chat_creating,
            self.active_chat_session_id,
        );

        let content: gpui::AnyElement = if self.chat_creating {
            self.render_chat_create_form(window, cx).into_any_element()
        } else if let Some(session_id) = &self.active_chat_session_id.clone() {
            if self.chat_session_view.is_none()
                && let Some(delegate) = &self.delegate
            {
                let messages = delegate.list_chat_messages(session_id);
                let session = self
                    .chat_sessions
                    .iter()
                    .find(|s| s.id == *session_id)
                    .cloned();
                if let Some(s) = session {
                    let parent_handle = cx.entity().downgrade();
                    let entity = cx.new(|cx| {
                        ChatSessionView::new(
                            self.delegate.clone(),
                            self.language,
                            session_id.clone(),
                            s.title.clone(),
                            s.system_prompt.clone(),
                            messages,
                            parent_handle,
                            cx,
                        )
                    });
                    self.chat_session_view = Some(entity);
                }
            }
            if let Some(ref view) = self.chat_session_view {
                view.clone().into_any_element()
            } else {
                self.render_chat_session_list(window, cx).into_any_element()
            }
        } else {
            self.render_chat_session_list(window, cx).into_any_element()
        };

        v_flex().size_full().child(content)
    }

    fn render_chat_create_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;

        if self.chat_create_title.is_none() {
            let entity = cx.new(|cx| {
                gpui_component::input::InputState::new(window, cx).placeholder("对话标题")
            });
            entity.update(cx, |s, cx| {
                s.set_value(
                    i18n::t(I18nKey::DefaultChatTitle, self.language),
                    window,
                    cx,
                );
            });
            self.chat_create_title = Some(entity);

            let entity2 = cx.new(|cx| {
                gpui_component::input::TextareaState::new(window, cx)
                    .placeholder("系统提示词 (可选)")
            });
            let lang_name = self.language.name();
            let prompt = format!(
                "你是一个精通学术文献分析的 AI 助手。你可以围绕用户上传或选中的论文回答各种问题，包括但不限于：论文核心方法、实验设计、结果解读、公式推导、相关工作对比等。\n\n回复规范：\n1. 使用{lang_name}回答\n2. 使用清晰易读的 Markdown 格式\n3. 数学符号和公式使用 LaTeX 语法：单行公式用 $$ 包裹，行内公式用 $ 包裹\n4. 当引用文献中的具体内容时，标明引用来源\n5. 回答应当简洁、有条理、有深度，避免空泛的客套话"
            );
            entity2.update(cx, |s, cx| {
                s.set_value(prompt, window, cx);
            });
            self.chat_create_prompt = Some(entity2);
        }

        v_flex()
            .size_full()
            .p_3()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        Label::new(i18n::t(I18nKey::NewChat, self.language))
                            .text_sm()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(muted),
                    )
                    .child(
                        h_flex().gap_2().child(
                            Button::new("chat-create-cancel")
                                .ghost()
                                .icon(IconName::Close)
                                .h(rems(1.5))
                                .w(rems(1.5))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.chat_creating = false;
                                    this.chat_create_title = None;
                                    this.chat_create_prompt = None;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("chat-create-confirm")
                                .ghost()
                                .icon(IconName::Check)
                                .h(rems(1.5))
                                .w(rems(1.5))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let title = this
                                        .chat_create_title
                                        .as_ref()
                                        .map(|e| e.read(cx).text().to_string())
                                        .unwrap_or_default();
                                    let prompt = this
                                        .chat_create_prompt
                                        .as_ref()
                                        .map(|e| e.read(cx).text().to_string())
                                        .unwrap_or_default();
                                    let lit_id = this
                                        .document_id
                                        .split("::")
                                        .next()
                                        .unwrap_or(&this.document_id)
                                        .to_string();
                                    debug!("[Chat] create confirm: title={title:?}, lit_id={lit_id}, prompt_len={}", prompt.len());
                                    if let Some(delegate) = &this.delegate {
                                        if let Some(id) =
                                            delegate.create_chat_session(&lit_id, &title, &prompt)
                                        {
                                            debug!("[Chat] create confirm: OK, session_id={id}");
                                            this.active_chat_session_id = Some(id.clone());
                                            let session = models::chat::ChatSession {
                                                id,
                                                literature_id: lit_id,
                                                title,
                                                system_prompt: prompt,
                                                created_at: chrono::Utc::now().timestamp(),
                                                updated_at: chrono::Utc::now().timestamp(),
                                                compressed_summary: String::new(),
                                            };
                                            this.chat_sessions.insert(0, session);
                                            debug!("[Chat] create confirm: inserted into chat_sessions, len={}", this.chat_sessions.len());
                                        } else {
                                            debug!("[Chat] create confirm: delegate returned None (create failed)");
                                        }
                                    } else {
                                        debug!("[Chat] create confirm: no delegate");
                                    }
                                    this.chat_creating = false;
                                    this.chat_create_title = None;
                                    this.chat_create_prompt = None;
                                    this.chat_session_view = None;
                                    cx.notify();
                                })),
                        ),
                    ),
            )
            .when_some(self.chat_create_title.as_ref(), |this, e| {
                this.child(gpui_component::input::Input::new(e).w_full())
            })
            .child(
                div().w_full().flex_grow(1.0).h_0().when_some(
                    self.chat_create_prompt.as_ref(),
                    |this, e| {
                        this.child(
                            gpui_component::input::Textarea::new(e).w_full().h_full(),
                        )
                    },
                ),
            )
    }

    fn render_chat_session_list(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;

        v_flex()
            .size_full()
            .child(
                h_flex()
                    .w_full()
                    .px_3()
                    .py_2()
                    .items_center()
                    .justify_between()
                    .child(
                        Label::new(i18n::t(I18nKey::Chat, self.language))
                            .text_sm()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(muted),
                    )
                    .child(
                        Button::new("new-chat")
                            .ghost()
                            .icon(IconName::ZoomIn)
                            .h(rems(1.5))
                            .w(rems(1.5))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.chat_creating = true;
                                this.chat_create_title = None;
                                this.chat_create_prompt = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .flex_grow(1.0)
                    .h_0()
                    .overflow_y_scrollbar()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .children({
                        let mut cards: Vec<gpui::AnyElement> = Vec::new();
                        let sessions = self.chat_sessions.clone();
                        debug!(
                            "[Chat] render_chat_session_list: {} sessions",
                            sessions.len()
                        );
                        for (i, session) in sessions.iter().enumerate() {
                            let sid = session.id.clone();
                            let title = session.title.clone();
                            let local_time =
                                chrono::DateTime::from_timestamp(session.updated_at, 0)
                                    .map(|dt| dt.with_timezone(&chrono::Local))
                                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                                    .unwrap_or_default();
                            let card = v_flex()
                                .w_full()
                                .group("chat-session-card")
                                .bg(theme.background)
                                .border_1()
                                .border_color(theme.border)
                                .rounded_md()
                                .overflow_hidden()
                                .hover(|s| s.border_color(theme.accent))
                                .cursor_pointer()
                                .on_mouse_up(
                                    gpui::MouseButton::Left,
                                    cx.listener({
                                        let sid = sid.clone();
                                        move |this, _, _, cx| {
                                            debug!("[Chat] select session: sid={sid}");
                                            this.active_chat_session_id = Some(sid.clone());
                                            this.chat_session_view = None;
                                            cx.notify();
                                        }
                                    }),
                                )
                                .child(
                                    h_flex()
                                        .w_full()
                                        .px_2()
                                        .py_1p5()
                                        .justify_between()
                                        .items_center()
                                        .child(
                                            div().flex_1().min_w_0().child(
                                                Label::new(title)
                                                    .text_xs()
                                                    .whitespace_nowrap()
                                                    .overflow_hidden()
                                                    .text_ellipsis(),
                                            ),
                                        )
                                        .child(
                                            h_flex()
                                                .gap_0()
                                                .opacity(0.0)
                                                .group_hover("chat-session-card", |s| {
                                                    s.opacity(1.0)
                                                })
                                                .on_mouse_up(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(move |_, _, _, cx| {
                                                        cx.stop_propagation();
                                                    }),
                                                )
                                                .child(
                                                    Button::new(gpui::SharedString::from(format!(
                                                        "chat-delete-{i}"
                                                    )))
                                                    .ghost()
                                                    .icon(IconName::Close)
                                                    .compact()
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        if let Some(delegate) = &this.delegate {
                                                            delegate.delete_chat_session(&sid);
                                                        }
                                                        this.chat_sessions.retain(|s| s.id != sid);
                                                        cx.notify();
                                                    })),
                                                ),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .px_2()
                                        .py_0p5()
                                        .justify_end()
                                        .child(Label::new(local_time).text_xs().text_color(muted)),
                                );
                            cards.push(card.into_any_element());
                        }
                        if self.chat_sessions.is_empty() {
                            cards.push(
                                v_flex()
                                    .size_full()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        Label::new(i18n::t(I18nKey::NoChatSessions, self.language))
                                            .text_xs()
                                            .text_color(muted),
                                    )
                                    .into_any_element(),
                            );
                        }
                        cards
                    }),
            )
    }

    fn generate_ai_summary(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let lit_id = self
            .document_id
            .split("::")
            .next()
            .unwrap_or(&self.document_id)
            .to_string();

        // 删除上一次的 AI 总结
        if let Some(last_id) = self.last_ai_summary_note_id.take() {
            if let Some(delegate) = &self.delegate {
                delegate.delete_note(&last_id);
            }
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

        let delegate = match &self.delegate {
            Some(d) => d.clone(),
            None => return,
        };

        let task = cx.spawn(|this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let result: Result<String, String> = async {
                    let attachments = delegate.current_literature_attachments();
                    let mut pdf_path = None;
                    for att in &attachments {
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
                        pdf_text = Some(services::pdf::extract_text_from_pdf(&path).map_err(|e| format!("PDF 文本提取失败: {:?}", e))?);
                    }

                    let _ = this.update(&mut cx, |this, cx| {
                        if let Some(n) = this.notes_cache.iter_mut().find(|n| n.id == "ai_generating_note") {
                            n.content = "正在发起 AI 总结生成...\n\n".to_string();
                        }
                        cx.notify();
                    });

                    let mut prompt_content = format!("文献 ID: {}\n", lit_id);
                    if let Some(text) = pdf_text {
                        prompt_content.push_str(&format!("\n正文全文:\n{}", text));
                    }

                    let messages = vec![
                        models::chat::ChatMessage {
                            id: uuid::Uuid::new_v4().to_string(),
                            session_id: "ai_summary".to_string(),
                            role: "user".to_string(),
                            content: prompt_content,
                            reasoning: None,
                            attachments: Vec::new(),
                            created_at: chrono::Utc::now().timestamp(),
                            parent_id: None,
                        }
                    ];

                    let system_prompt = "你是一个精通学术论文分析的 AI 助手。请针对用户给出的文献，写一份详细且条理清晰的学术总结。总结必须包含：1. 研究背景与动机（作者为什么要研究这个问题）；2. 核心方法与模型（作者是如何实现和解决这个问题的，包含哪些技术核心）；3. 关键实验结果（核心数据、结论等）；4. 主要结论与学术贡献。请用中文回答，并以清晰易读的 Markdown 格式输出。注意：必须直接输出 Markdown 纯文本，严禁在最外层使用 ```markdown ... ``` 或 ``` ... ``` 这样的代码块标记包裹整篇回答。所有数学符号、希腊字母、公式等使用 LaTeX 语法书写，公式必须且只能使用 $$ 包裹（例如 $$a^2 + b^2 = c^2$$，不要使用 \\(...\\) 或 \\[...\\] 等包裹方式），同时请避免输出复杂或多行的公式，尽量使用简单、单行的公式形式。".to_string();

                    let mut rx = delegate.chat_stream("ai_summary".to_string(), messages, system_prompt).await.map_err(|e| format!("AI 服务请求失败: {}", e))?;

                    let mut full_output = String::new();
                    while let Some(chunk) = rx.recv().await {
                        match chunk {
                            models::chat::ChatResponseChunk::Content(text) => {
                                log::info!(
                                    "[AI Summary Chunk] Content: len={}, preview={:?}",
                                    text.len(),
                                    &text[..text.len().min(80)]
                                );
                                let display_output = {
                                    full_output.push_str(&text);
                                    full_output.clone()
                                };
                                let _ = this.update(&mut cx, |this, cx| {
                                    if let Some(n) = this.notes_cache.iter_mut().find(|n| n.id == "ai_generating_note") {
                                        n.content = display_output;
                                    }
                                    cx.notify();
                                });
                            }
                            other => {
                                log::info!("[AI Summary Chunk] Other variant: {:?}", other);
                            }
                        }
                    }

                    if full_output.trim().is_empty() {
                        return Err("AI 服务返回了空内容".to_string());
                    }

                    log::info!(
                        "[AI Summary Final] total_len={}, starts_with_code_block={}, ends_with_code_block={}, preview_end={:?}",
                        full_output.len(),
                        full_output.trim().starts_with("```"),
                        full_output.trim().ends_with("```"),
                        full_output.chars().rev().take(200).collect::<String>()
                    );

                    let note_id = delegate.create_note(&lit_id, "AI 总结").ok_or_else(|| "创建文献笔记失败".to_string())?;
                    let ok = delegate.update_note(&note_id, Some("AI 总结"), Some(&full_output));
                    if !ok {
                        return Err("保存笔记内容失败".to_string());
                    }
                    let _ = this.update(&mut cx, |this, _cx| {
                        this.last_ai_summary_note_id = Some(note_id);
                    });
                    if !ok {
                        return Err("保存笔记内容失败".to_string());
                    }

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
                            log::error!("AI 总结生成失败: {}", err_msg);
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

#[allow(clippy::too_many_arguments)]
pub fn render_shared_note_card<V: 'static>(
    _index: usize,
    note: &models::LiteratureNote,
    is_expanded: bool,
    theme: gpui_component::Theme,
    _window: &mut Window,
    cx: &mut Context<V>,
    on_edit: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    on_delete: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    on_toggle_expand: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let font_size = px(12.0);
    let border_color = theme.border;
    let accent_color = theme.accent;
    let muted_color = theme.muted;
    let muted_foreground = theme.muted_foreground;
    let note_id = note.id.clone();
    let is_long = note.content.len() > 100 || note.content.contains('\n');

    let local_time = chrono::DateTime::from_timestamp(note.updated_at, 0)
        .map(|dt| dt.with_timezone(&chrono::Local))
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();

    let card_group_name = format!("note-card-{}", note_id);

    div()
        .w_full()
        .group(card_group_name.clone())
        .bg(theme.muted.opacity(0.10))
        .border_1()
        .border_color(border_color)
        .rounded_md()
        .overflow_hidden()
        .hover(|s| s.border_color(accent_color))
        .child(
            // ── 标题栏 ──
            h_flex()
                .w_full()
                .bg(muted_color.opacity(0.20))
                .px_2()
                .py_0p5()
                .border_b_1()
                .border_color(border_color)
                .justify_between()
                .items_center()
                .child(
                    div()
                        .id(gpui::SharedString::from(format!(
                            "note-edit-container-{}",
                            note_id
                        )))
                        .cursor_pointer()
                        .on_click(on_edit)
                        .hover(|s| s.bg(muted_color.opacity(0.2)))
                        .rounded_sm()
                        .p_0p5()
                        .child(
                            Icon::new(IconName::Annotations)
                                .size(gpui::rems(0.7))
                                .text_color(muted_foreground),
                        ),
                )
                .child(
                    div()
                        .id(gpui::SharedString::from(format!(
                            "note-title-toggle-{}",
                            note_id
                        )))
                        .flex_1()
                        .min_w_0()
                        .ml_1p5()
                        .cursor_pointer()
                        .on_click(on_toggle_expand)
                        .child(
                            Label::new(note.title.clone())
                                .text_size(px(12.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis(),
                        ),
                )
                .child(
                    div()
                        .id(gpui::SharedString::from(format!(
                            "note-delete-container-{}",
                            note_id
                        )))
                        .cursor_pointer()
                        .on_click(on_delete)
                        .hover(|s| s.bg(muted_color.opacity(0.2)))
                        .rounded_sm()
                        .p_0p5()
                        .child(
                            Icon::new(IconName::Close)
                                .size(gpui::rems(0.7))
                                .text_color(muted_foreground),
                        ),
                ),
        )
        .child(
            // ── 内容及时间戳区域 ──
            {
                let mut content_container = v_flex().gap_0p5();
                if is_expanded {
                    content_container =
                        content_container.child(crate::ui::components::render_math_markdown(
                            format!("note-content-{}", note_id),
                            &note.content,
                            px(14.0),
                            Some(theme.foreground),
                            Some(
                                gpui_component::text::TextViewStyle::default().heading_font_size(
                                    move |level, _| match level {
                                        1 => px(16.),
                                        2 => px(15.),
                                        3 => px(14.),
                                        4 => px(13.),
                                        _ => px(12.),
                                    },
                                ),
                            ),
                            cx,
                        ));
                } else {
                    content_container = content_container.child(
                        Label::new(note.content.clone())
                            .text_size(font_size)
                            .text_color(theme.foreground),
                    );
                }

                let mut click_wrapper = div()
                    .id(gpui::SharedString::from(format!(
                        "d-note-body-click-{}",
                        note_id
                    )))
                    .child(content_container);

                if is_long && !is_expanded {
                    click_wrapper = click_wrapper.max_h(px(42.0)).overflow_hidden();
                }

                v_flex().p_1().gap_0p5().child(click_wrapper).child(
                    h_flex().w_full().justify_end().items_center().child(
                        Label::new(local_time)
                            .text_size(px(9.0))
                            .text_color(muted_foreground),
                    ),
                )
            },
        )
}
