use gpui::prelude::*;
use gpui::{Context, FontWeight, Window, div, rems};
use gpui_component::{
    ActiveTheme,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState, Textarea, TextareaState},
    label::Label,
    v_flex,
};
use i18n::{I18nKey, Language, t};

pub type EditChatCallback = Box<
    dyn FnOnce(Option<(String, String)>, &mut Window, &mut Context<EditChatSessionDialog>)
        + Send
        + 'static,
>;

pub struct EditChatSessionDialog {
    title_input: gpui::Entity<InputState>,
    prompt_input: gpui::Entity<TextareaState>,
    language: Language,
    on_close: Option<EditChatCallback>,
}

impl EditChatSessionDialog {
    pub fn new(
        title: String,
        prompt: String,
        language: Language,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_close: impl FnOnce(Option<(String, String)>, &mut Window, &mut Context<Self>)
        + Send
        + 'static,
    ) -> Self {
        let title_input = cx.new(|cx| InputState::new(window, cx).default_value(title));
        let prompt_input = cx.new(|cx| TextareaState::new(window, cx).default_value(prompt));

        Self {
            title_input,
            prompt_input,
            language,
            on_close: Some(Box::new(on_close)),
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.title_input.read(cx).text().to_string();
        let prompt = self.prompt_input.read(cx).text().to_string();
        if let Some(cb) = self.on_close.take() {
            cb(Some((title, prompt)), window, cx);
        }
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(cb) = self.on_close.take() {
            cb(None, window, cx);
        }
    }
}

impl gpui::Render for EditChatSessionDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let theme = cx.theme().clone();
        let lang = self.language;

        v_flex()
            .size_full()
            .bg(theme.background)
            .when(cfg!(target_os = "macos"), |this| this.pt(rems(2.5)))
            .child(
                // ── 标题区 ──
                h_flex()
                    .w_full()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        Label::new(t(I18nKey::EditSystemPrompt, lang))
                            .text_lg()
                            .font_weight(FontWeight::BOLD),
                    ),
            )
            .child(
                // ── 表单区 ──
                v_flex()
                    .flex_grow(1.0)
                    .gap_4()
                    .px_4()
                    .py_4()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                Label::new(t(I18nKey::AiBackendName, lang))
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM),
                            )
                            .child(Input::new(&self.title_input)),
                    )
                    .child(
                        v_flex()
                            .flex_grow(1.0)
                            .gap_1()
                            .child(
                                Label::new(t(I18nKey::DefaultSystemPrompt, lang))
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM),
                            )
                            .child(
                                div()
                                    .flex_grow(1.0)
                                    .child(Textarea::new(&self.prompt_input).h_full()),
                            ),
                    ),
            )
            .child(
                // ── 按钮区 ──
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        Button::new("edit-chat-cancel")
                            .child(t(I18nKey::Cancel, lang))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel(window, cx);
                            })),
                    )
                    .child(
                        Button::new("edit-chat-save")
                            .child(t(I18nKey::Save, lang))
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save(window, cx);
                            })),
                    ),
            )
    }
}
