use crate::app_state::config::ConfigStore;
use crate::app_state::theme::surface;
use components::IconName;
use components::{muted_input, selector};
use gpui::prelude::*;
use gpui::{AppContext, Entity, SharedString, div};
use gpui_component::{
    ActiveTheme, Icon, h_flex,
    input::{InputEvent, InputState},
    label::Label,
    setting::{SettingGroup, SettingItem, SettingPage},
};
use i18n::{I18nKey, t};
use services::app::MainApp;
use std::sync::Arc;
use translate;

use super::{SettingsWindow, config_str, lang};

impl SettingsWindow {
    pub(super) fn translation_page(
        &self,
        app: Arc<MainApp>,
        cx: &mut Context<Self>,
    ) -> SettingPage {
        let surface = surface(cx);
        let l = lang(cx);
        let engines: Vec<(SharedString, SharedString)> = [
            ("google_free", "Google (Free)"),
            ("bing_free", "Bing (Free)"),
            ("google", "Google"),
            ("niutrans", "NiuTrans"),
            ("baidu", "Baidu"),
            ("youdao", "Youdao"),
            ("deepl_free", "DeepL (Free)"),
            ("deepl_pro", "DeepL (Pro)"),
            ("ai", "AI"),
        ]
        .iter()
        .map(|(v, l)| ((*v).into(), (*l).into()))
        .collect();

        let target_lang_options: Vec<(SharedString, SharedString)> = [
            ("zh-CN", "简体中文"),
            ("zh-TW", "繁體中文"),
            ("en", "English"),
            ("ja", "日本語"),
            ("ko", "한국어"),
            ("fr", "Français"),
            ("de", "Deutsch"),
            ("es", "Español"),
            ("ru", "Русский"),
            ("pt", "Português"),
            ("it", "Italiano"),
            ("nl", "Nederlands"),
            ("ar", "العربية"),
            ("th", "ไทย"),
            ("vi", "Tiếng Việt"),
        ]
        .iter()
        .map(|(v, l)| ((*v).into(), (*l).into()))
        .collect();

        // Conditional API key input helper
        let api_key_field =
            |key_id: &'static str, label: &'static str, key_s: &'static str| -> SettingItem {
                let app = app.clone();
                SettingItem::render(move |_, window, cx| {
                    let engine = cx.global::<ConfigStore>().translation.engine.clone();
                    let show = translate::ENGINES
                        .iter()
                        .any(|e| e.id == engine && e.requires_keys.contains(&key_s));
                    if !show {
                        return div().into_any_element();
                    }
                    struct KeyState {
                        input: Entity<InputState>,
                        _sub: gpui::Subscription,
                    }
                    let val: SharedString = app
                        .local_state
                        .read()
                        .unwrap()
                        .translation_keys
                        .get(key_s)
                        .cloned()
                        .unwrap_or_default()
                        .into();
                    let state = window.use_keyed_state::<KeyState>(
                        format!("tkey-{key_id}"),
                        cx,
                        |window, cx| {
                            let input = cx.new(|cx| {
                                InputState::new(window, cx).default_value(val).masked(true)
                            });
                            let app = app.clone();
                            let k = key_s.to_string();
                            let _sub = cx.subscribe(&input, {
                                move |_, emitter, event: &InputEvent, cx| {
                                    if let InputEvent::Change = event {
                                        let v = emitter.read(cx).value();
                                        let mut state = app.local_state.write().unwrap();
                                        state.translation_keys.insert(k.clone(), v.to_string());
                                    }
                                }
                            });
                            KeyState { input, _sub }
                        },
                    );
                    let theme = cx.theme();
                    muted_input(&state.read(cx).input, theme)
                        .child(
                            Label::new(label)
                                .text_xs()
                                .text_color(theme.muted_foreground),
                        )
                        .into_any_element()
                })
            };

        SettingPage::new(t(I18nKey::TranslationSettingsTab, l))
                .icon(Icon::new(IconName::Globe))
                .group(
                    SettingGroup::new()
                        .title(t(I18nKey::TranslationSettings, l))
                        .item(SettingItem::render({
                            let engines = engines.clone();
                            let app = app.clone();
                            move |_, _, cx| {
                                let l = lang(cx);
                                let current = config_str(|c| &c.translation.engine)(cx);
                                let app_clone = app.clone();
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child(t(I18nKey::TranslationEngine, l)),
                                    )
                                    .child(selector(
                                        "translation-engine-select",
                                        engines.clone(),
                                        current,
                                        false,
                                        move |v, _, cx| {
                                            cx.update_global::<ConfigStore, _>(|store, _| {
                                                store.inner.translation.engine = v.to_string();
                                            });
                                            let _ = app_clone.update_config(
                                                cx.global::<ConfigStore>().inner.clone(),
                                            );
                                        },
                                    ))
                            }
                        }))
                        .item(SettingItem::render({
                            let target_lang_options = target_lang_options.clone();
                            let app = app.clone();
                            move |_, _, cx| {
                                let l = lang(cx);
                                let current = config_str(|c| &c.translation.target_language)(cx);
                                let app_clone = app.clone();
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child(t(I18nKey::TargetLanguage, l)),
                                    )
                                    .child(selector(
                                        "target-language-select",
                                        target_lang_options.clone(),
                                        current,
                                        false,
                                        move |v, _, cx| {
                                            cx.update_global::<ConfigStore, _>(|store, _| {
                                                store.inner.translation.target_language =
                                                    v.to_string();
                                            });
                                            let _ = app_clone.update_config(
                                                cx.global::<ConfigStore>().inner.clone(),
                                            );
                                        },
                                    ))
                            }
                        }))
                        .item(SettingItem::render({
                            let app = app.clone();
                            move |_, _, cx| {
                                let engine = cx.global::<ConfigStore>().translation.engine.clone();
                                if engine != "ai" {
                                    return div().into_any_element();
                                }
                                let (opts, active): (
                                    Vec<(SharedString, SharedString)>,
                                    SharedString,
                                ) = {
                                    let keys = app.local_state.read().unwrap();
                                    let json = keys
                                        .translation_keys
                                        .get("ai.entries")
                                        .cloned()
                                        .unwrap_or_default();
                                    let active = keys
                                        .translation_keys
                                        .get("ai.active")
                                        .cloned()
                                        .unwrap_or_default();
                                    drop(keys);
                                    let entries: Vec<translate::AiBackendEntry> =
                                        serde_json::from_str(&json).unwrap_or_default();
                                    let opts = entries
                                        .iter()
                                        .map(|e| (e.name.clone().into(), e.name.clone().into()))
                                        .collect();
                                    (opts, active.into())
                                };
                                if opts.is_empty() {
                                    return div().into_any_element();
                                }
                                let l = lang(cx);
                                let app_clone = app.clone();
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        div().text_sm().font_weight(gpui::FontWeight::BOLD).child(
                                            t(I18nKey::AiBackend, l),
                                        ),
                                    )
                                    .child(selector(
                                        "translation-ai-active-select",
                                        opts,
                                        active,
                                        false,
                                        move |v, _, _cx| {
                                            let mut state = app_clone.local_state.write().unwrap();
                                            state
                                                .translation_keys
                                                .insert("ai.active".into(), v.to_string());
                                            let _ = app_clone.local_state_manager.save_all(&state);
                                        },
                                    ))
                                    .into_any_element()
                            }
                        }))
                        .item(api_key_field("google", "Google API Key", "google"))
                        .item(api_key_field("niutrans", "NiuTrans API Key", "niutrans"))
                        .item(api_key_field("baidu", "Baidu API Key", "baidu"))
                        .item(api_key_field("youdao", "Youdao API Key", "youdao"))
                        .item(api_key_field("deepl", "DeepL API Key", "deepl"))
                        .item(SettingItem::render(move |_, _, cx| {
                            let engine = cx.global::<ConfigStore>().translation.engine.clone();
                            let is_free = translate::ENGINES
                                .iter()
                                .any(|e| e.id == engine && e.is_free);
                            if !is_free {
                                return div().into_any_element();
                            }
                            let theme = cx.theme();
                            div()
                                .p_3()
                                .bg(surface.info_bg)
                                .rounded_md()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(t(I18nKey::NoApiKeyRequired, lang(cx)))
                                .into_any_element()
                        })),
                )
    }
}
