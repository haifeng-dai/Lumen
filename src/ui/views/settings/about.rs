use crate::ui::views::main_window::utils::open_url;
use components::IconName;
use gpui::prelude::*;
use gpui::{MouseButton, div, rems};
use gpui_component::{
    ActiveTheme, Icon, h_flex,
    label::Label,
    setting::{SettingGroup, SettingItem, SettingPage},
    v_flex,
};
use i18n::{I18nKey, t};

use super::{SettingsWindow, lang};

impl SettingsWindow {
    pub(super) fn about_page(&self, cx: &mut Context<Self>) -> SettingPage {
        let l = lang(cx);
        SettingPage::new(t(I18nKey::About, l))
            .icon(Icon::new(IconName::Info))
            .group(
                SettingGroup::new().item(SettingItem::render(move |_, _, cx| {
                    let theme = cx.theme();
                    v_flex()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .size_full()
                        .py(rems(4.0))
                        .child(
                            div()
                                .size(rems(5.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(gpui::img("icons/app_icon.png").size(rems(6.0))),
                        )
                        .child(
                            Label::new("Lumen")
                                .text_2xl()
                                .font_weight(gpui::FontWeight::BOLD),
                        )
                        .child(
                            Label::new(env!("CARGO_PKG_VERSION"))
                                .text_sm()
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            Label::new(t(I18nKey::AboutDesc, l))
                                .text_sm()
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            Label::new(t(I18nKey::Copyright, l))
                                .text_sm()
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            div()
                                .cursor_pointer()
                                .on_mouse_down(MouseButton::Left, |_, _, _| {
                                    open_url("https://github.com/LumenLib/Lumen");
                                })
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(Icon::new(IconName::GitHub).size(rems(0.875)))
                                        .child(
                                            Label::new("GitHub")
                                                .text_sm()
                                                .text_color(theme.muted_foreground),
                                        ),
                                ),
                        )
                        .into_any_element()
                })),
            )
    }
}
