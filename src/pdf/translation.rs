use self::text_format::clean_translation_text;
use gpui::{AsyncApp, Context, WeakEntity};
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
        cx.notify();
    }

    pub(crate) fn translation_engine_options(
        &self,
    ) -> Vec<(gpui::SharedString, gpui::SharedString)> {
        let engines = self
            .delegate
            .as_ref()
            .map(|d| d.get_translation_engines())
            .unwrap_or_default();

        engines
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
                (id.into(), label.into())
            })
            .collect()
    }

    pub(crate) fn chat_backend_options(&self) -> Vec<(gpui::SharedString, gpui::SharedString)> {
        self.delegate
            .as_ref()
            .map(|d| {
                d.list_ai_backends()
                    .into_iter()
                    .map(|item| (item.name.clone().into(), item.name.into()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn delegate(&self) -> Option<&Arc<dyn PdfReaderDelegate>> {
        self.delegate.as_ref()
    }

    pub fn document_id(&self) -> &str {
        &self.document_id
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
