use self::text_format::clean_translation_text;
use gpui::prelude::*;
use gpui::{AsyncApp, Context, WeakEntity, Window};
use gpui_component::select::SelectEvent;
use services::pdf::PdfReaderDelegate;

use i18n::{I18nKey, Language};
use log::{error, info};
use std::sync::Arc;

use super::*;

impl super::PdfReaderView {
    pub fn translate_text(&mut self, text: String, force: bool, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }

        let formatted = clean_translation_text(&text);

        info!(
            "PdfReaderView: 开始翻译文本, 强制={}, 长度={}",
            force,
            formatted.len()
        );
        self.translation_result = Some(TranslationResult {
            original: formatted.clone(),
            translated: None,
            is_loading: true,
            error: None,
        });
        cx.notify();

        if let Some(delegate) = self.delegate.clone() {
            cx.spawn(move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result: anyhow::Result<String> = delegate.translate(formatted, force).await;
                    let _ = this.update(&mut cx, |this, cx| {
                        if let Some(ref mut res) = this.translation_result {
                            match result {
                                Ok(translated) => {
                                    info!("PdfReaderView: 翻译完成, 长度={}", translated.len());
                                    res.translated = Some(translated);
                                    res.is_loading = false;
                                }
                                Err(e) => {
                                    error!("PdfReaderView: 翻译失败: {}", e);
                                    res.error = Some(e.to_string());
                                    res.is_loading = false;
                                }
                            }
                        }
                        cx.notify();
                    });
                }
            })
            .detach();
        }
    }

    pub fn change_translation_font_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.translation_font_size = (self.translation_font_size + delta).clamp(8.0, 32.0);
        if let Some(delegate) = &self.delegate {
            delegate.set_translation_font_size(self.translation_font_size);
        }
        cx.notify();
    }

    /// 从 ConfigStore observer 更新语言（观察者模式入口）
    pub fn set_language(&mut self, language: Language, cx: &mut Context<Self>) {
        self.language = language;
        // 如果 SelectState 已存在，需要重新生成或更新其选项的语言（为了简单，我们清空它，下次获取时会重新用新语言创建）
        self.engine_select = None;
        cx.notify();
    }

    pub(crate) fn get_or_create_engine_select(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<gpui_component::select::SelectState<Vec<TranslationEngineItem>>> {
        let current_engine = self
            .delegate
            .as_ref()
            .map(|d| d.current_translation_engine_id())
            .unwrap_or_default();

        if let Some(select) = &self.engine_select {
            select.update(cx, |state, cx| {
                if state.selected_value() != Some(&current_engine) {
                    state.set_selected_value(&current_engine, window, cx);
                }
            });
            return select.clone();
        }

        let engines = self
            .delegate
            .as_ref()
            .map(|d| d.get_translation_engines())
            .unwrap_or_default();

        let engine_items: Vec<TranslationEngineItem> = engines
            .into_iter()
            .map(|id| {
                let label = match id.as_str() {
                    "google_free" => i18n::t(I18nKey::EngineGoogleFree, self.language).to_string(),
                    "bing_free" => i18n::t(I18nKey::EngineBingFree, self.language).to_string(),
                    "google" => i18n::t(I18nKey::EngineGoogleCloud, self.language).to_string(),
                    "niutrans" => i18n::t(I18nKey::EngineNiuTrans, self.language).to_string(),
                    "baidu" => i18n::t(I18nKey::EngineBaidu, self.language).to_string(),
                    "youdao" => i18n::t(I18nKey::EngineYoudao, self.language).to_string(),
                    "deepl_free" => i18n::t(I18nKey::EngineDeeplFree, self.language).to_string(),
                    "deepl_pro" => i18n::t(I18nKey::EngineDeeplPro, self.language).to_string(),
                    "ai" => i18n::t(I18nKey::EngineAi, self.language).to_string(),
                    _ => id.clone(),
                };
                TranslationEngineItem { value: id, label }
            })
            .collect();

        let select = cx.new(|cx| {
            let mut state =
                gpui_component::select::SelectState::new(engine_items, None, window, cx);
            state.set_selected_value(&current_engine, window, cx);
            state
        });

        cx.subscribe(&select, |this, _, event, cx| {
            if let SelectEvent::Confirm(Some(engine_id)) = event
                && let Some(delegate) = &this.delegate
            {
                delegate.set_translation_engine(engine_id.clone());
                cx.update_global::<crate::app_state::config::ConfigStore, _>(|store, _cx| {
                    store.inner.translation.engine = engine_id.clone();
                });
            }
        })
        .detach();

        self.engine_select = Some(select.clone());
        select
    }

    pub(crate) fn get_or_create_chat_backend_select(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<gpui_component::select::SelectState<Vec<AiBackendSelectItem>>> {
        let current_name = self
            .delegate
            .as_ref()
            .and_then(|d| d.get_active_chat_backend());

        let new_backends = self
            .delegate
            .as_ref()
            .map(|d| d.list_ai_backends())
            .unwrap_or_default();

        let need_recreate = match &self.chat_backend_select {
            None => true,
            Some(_) => self.cached_ai_backends != new_backends,
        };

        if !need_recreate && let Some(select) = &self.chat_backend_select {
            select.update(cx, |state, cx| {
                if state.selected_value() != current_name.as_ref()
                    && let Some(ref name) = current_name
                {
                    state.set_selected_value(name, window, cx);
                }
            });
            return select.clone();
        }

        self.cached_ai_backends = new_backends.clone();
        let items: Vec<AiBackendSelectItem> =
            new_backends.into_iter().map(AiBackendSelectItem).collect();

        let select = cx.new(|cx| {
            let mut state = gpui_component::select::SelectState::new(items, None, window, cx);
            if let Some(ref name) = current_name {
                state.set_selected_value(name, window, cx);
            }
            state
        });

        cx.subscribe(&select, |this, _, event, _cx| {
            if let SelectEvent::Confirm(Some(name)) = event
                && let Some(delegate) = &this.delegate
            {
                delegate.set_active_chat_backend(name.to_string());
            }
        })
        .detach();

        self.chat_backend_select = Some(select.clone());
        select
    }

    pub fn delegate(&self) -> Option<&Arc<dyn PdfReaderDelegate>> {
        self.delegate.as_ref()
    }

    pub fn document_id(&self) -> &str {
        &self.document_id
    }

    pub fn set_notes_cache(&mut self, notes: Vec<models::LiteratureNote>) {
        self.notes_cache = notes;
    }

    pub fn reload_notes(&mut self, cx: &mut Context<Self>) {
        if let Some(delegate) = &self.delegate {
            let lit_id = self
                .document_id
                .split("::")
                .next()
                .unwrap_or(&self.document_id);
            let notes = delegate.list_notes(lit_id);
            let has_generating = self.is_generating_summary;
            let mut merged_notes = notes;
            if has_generating
                && let Some(gen_note) = self
                    .notes_cache
                    .iter()
                    .find(|n| n.id == "ai_generating_note")
                    .cloned()
            {
                merged_notes.push(gen_note);
            }
            self.notes_cache = merged_notes;
        }
        cx.notify();
    }

    pub fn reload_chat_sessions(&mut self, cx: &mut Context<Self>) {
        if let Some(delegate) = &self.delegate {
            let lit_id = self
                .document_id
                .split("::")
                .next()
                .unwrap_or(&self.document_id);
            self.chat_sessions = delegate.list_chat_sessions(lit_id);
        }
        cx.notify();
    }
}
