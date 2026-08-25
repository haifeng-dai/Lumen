use components::IconName;
use components::{muted_textarea, selector};
use gpui::prelude::*;
use gpui::{AppContext, Entity, SharedString, div};
use gpui_component::{
    ActiveTheme, Icon, h_flex,
    input::{InputEvent, TextareaState},
    setting::{SettingGroup, SettingItem, SettingPage},
};
use i18n::{I18nKey, t};
use services::app::MainApp;
use std::sync::Arc;
use translate;

use super::{SettingsWindow, lang};

impl SettingsWindow {
    pub(super) fn ai_chat_page(&self, app: Arc<MainApp>, cx: &mut Context<Self>) -> SettingPage {
        let l = lang(cx);
        let (ai_sel_opts, ai_sel_active): (Vec<(SharedString, SharedString)>, SharedString) = {
            let keys = app.local_state.read().unwrap();
            let json = keys
                .translation_keys
                .get("ai.entries")
                .cloned()
                .unwrap_or_default();
            let active = keys
                .translation_keys
                .get("chat.active")
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

        // System prompt textarea
        let sys_prompt = {
            let app = app.clone();
            SettingItem::render(move |_, window, cx| {
                struct PromptState {
                    input: Entity<TextareaState>,
                    _sub: gpui::Subscription,
                }
                let val: SharedString = app
                    .local_state
                    .read()
                    .unwrap()
                    .translation_keys
                    .get("chat.default_system_prompt")
                    .cloned()
                    .unwrap_or_default()
                    .into();
                let state =
                    window.use_keyed_state::<PromptState>("chat-sys-prompt", cx, |window, cx| {
                        let input = cx.new(|cx| TextareaState::new(window, cx).default_value(val));
                        let app = app.clone();
                        let _sub = cx.subscribe(&input, {
                            move |_, emitter, event: &InputEvent, cx| {
                                if let InputEvent::Change = event {
                                    let v = emitter.read(cx).value();
                                    let mut state = app.local_state.write().unwrap();
                                    state
                                        .translation_keys
                                        .insert("chat.default_system_prompt".into(), v.to_string());
                                }
                            }
                        });
                        PromptState { input, _sub }
                    });
                let theme = cx.theme();
                let input = &state.read(cx).input;
                muted_textarea(input, theme).into_any_element()
            })
        };

        let mut base = SettingPage::new(t(I18nKey::AiChatSettingsTab, l))
            .icon(Icon::new(IconName::BookOpen))
            .group(
                SettingGroup::new()
                    .title(t(I18nKey::DefaultSystemPrompt, l))
                    .item(sys_prompt),
            );

        if !ai_sel_opts.is_empty() {
            base = base.group(
                SettingGroup::new()
                    .title(t(I18nKey::BackendSelection, l))
                    .item(SettingItem::render({
                        let ai_sel_opts = ai_sel_opts.clone();
                        let ai_sel_active = ai_sel_active.clone();
                        let app = app.clone();
                        move |_, _, cx| {
                            let l = lang(cx);
                            let current = ai_sel_active.clone();
                            let app_clone = app.clone();
                            h_flex()
                                .justify_between()
                                .items_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .child(t(I18nKey::ActiveBackend, l)),
                                )
                                .child(selector(
                                    "chat-ai-active-select",
                                    ai_sel_opts.clone(),
                                    current,
                                    false,
                                    move |v, _, _cx| {
                                        let mut state = app_clone.local_state.write().unwrap();
                                        state
                                            .translation_keys
                                            .insert("chat.active".into(), v.to_string());
                                        let _ = app_clone.local_state_manager.save_all(&state);
                                        app_clone.notify_ui_changed();
                                    },
                                ))
                        }
                    })),
            );
        }

        base
    }
}
