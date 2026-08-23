use gpui::{App, Entity, ParentElement, SharedString, Styled, div};
use gpui_component::{
    ActiveTheme, Theme,
    input::{Input, InputState, Textarea, TextareaState},
    v_flex,
};

/// 通过 Input builder 构建静音输入框（底层 API）。
/// 当需要额外链式调用（如 `.h()`、`.w()`）时使用此版本。
pub fn muted_input_raw(input: Input, theme: &Theme) -> gpui::Div {
    div()
        .bg(theme.muted)
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .child(input.appearance(false))
}

/// 通过 Textarea builder 构建静音文本域（底层 API）。
pub fn muted_textarea_raw(textarea: Textarea, theme: &Theme) -> gpui::Div {
    div()
        .bg(theme.muted)
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .child(textarea.appearance(false))
}

/// 通过 InputState 实体构建静音输入框：灰色背景 + 圆角 + 边框。
pub fn muted_input(input: &Entity<InputState>, theme: &Theme) -> gpui::Div {
    muted_input_raw(Input::new(input), theme)
}

/// 通过 TextareaState 实体构建静音文本域：灰色背景 + 圆角 + 边框。
pub fn muted_textarea(textarea: &Entity<TextareaState>, theme: &Theme) -> gpui::Div {
    muted_textarea_raw(Textarea::new(textarea), theme)
}

/// 密码输入框：在静音输入框基础上加掩码切换按钮。
pub fn password_input(input: &Entity<InputState>, theme: &Theme) -> gpui::Div {
    muted_input_raw(Input::new(input).mask_toggle(), theme)
}

/// 带标签的输入框：标签在上，静音输入框在下。
pub fn labeled_input(
    label: impl Into<SharedString>,
    input: &Entity<InputState>,
    cx: &mut App,
) -> gpui::Div {
    let theme = cx.theme();
    v_flex()
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(label.into()),
        )
        .child(muted_input(input, theme))
}
