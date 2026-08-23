use crate::ui::{
    components::{DetailRow, LinkRow},
    views::main_window::{self},
};
use gpui::prelude::*;
use gpui::{FontWeight, div, rems};
use gpui_component::{Theme, h_flex, label::Label, v_flex};
use i18n::Language;
use log::info;
use models::ReadingStatus;

use super::BadgeData;

impl super::LiteratureDetailView {
    fn render_reading_status_switcher(
        &self,
        current_status: ReadingStatus,
        lit_id: &str,
        theme: &Theme,
        lang: Language,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app = self.app.clone();
        let lit_id = lit_id.to_string();

        let unread_label = match lang {
            Language::ZhCn => "未读",
            _ => "Unread",
        };
        let to_read_label = match lang {
            Language::ZhCn => "将读",
            _ => "To Read",
        };
        let reading_label = match lang {
            Language::ZhCn => "正读",
            _ => "Reading",
        };
        let read_label = match lang {
            Language::ZhCn => "已读",
            _ => "Read",
        };

        h_flex().gap_2().children(
            [
                (
                    ReadingStatus::Unread,
                    "Unread",
                    theme.muted_foreground,
                    unread_label,
                ),
                (
                    ReadingStatus::ToRead,
                    "ToRead",
                    gpui::rgb(0xeab308).into(),
                    to_read_label,
                ),
                (
                    ReadingStatus::Reading,
                    "Reading",
                    gpui::rgb(0x22c55e).into(),
                    reading_label,
                ),
                (
                    ReadingStatus::Read,
                    "Read",
                    gpui::rgb(0xef4444).into(),
                    read_label,
                ),
            ]
            .into_iter()
            .enumerate()
            .map(|(idx, (status, _key, color, label))| {
                let is_active = current_status == status;
                let status_clone = status;
                let lit_id_clone = lit_id.clone();
                let app_clone = app.clone();

                div()
                    .id(("reading-status", idx))
                    .flex()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .on_click(cx.listener(move |_this, _event, _window, cx| {
                        info!(
                            "详情: 阅读状态切换 id={}, status={:?}",
                            lit_id_clone, status_clone
                        );
                        let _ = app_clone
                            .literature_service
                            .update_literature_reading_status(
                                &app_clone.db,
                                &lit_id_clone,
                                status_clone,
                            );
                        app_clone.notify_data_changed();
                        cx.notify();
                    }))
                    .child(
                        div()
                            .w(rems(0.75))
                            .h(rems(0.75))
                            .rounded_full()
                            .border_1()
                            .border_color(if is_active {
                                color
                            } else {
                                theme.muted_foreground
                            })
                            .bg(if is_active {
                                color
                            } else {
                                gpui::transparent_black()
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(if is_active {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .child(label),
                    )
            }),
        )
    }

    pub(super) fn render_title_section(
        &self,
        title: &str,
        reading_status: ReadingStatus,
        lit_id: &str,
        theme: &Theme,
        lang: Language,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("lit-title-wrapper")
            .on_click({
                let title = title.to_string();
                move |event, _, cx| {
                    if event.click_count() == 2 {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(title.clone()));
                    }
                }
            })
            .child(
                v_flex()
                    .group("row_group")
                    .items_start()
                    .gap_1()
                    .child(
                        Label::new(title.to_string())
                            .text_base()
                            .font_weight(FontWeight::BOLD)
                            .line_clamp(10),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .child(self.render_reading_status_switcher(
                                reading_status,
                                lit_id,
                                theme,
                                lang,
                                cx,
                            ))
                            .child(crate::ui::components::detail_widgets::render_copy_button(
                                "copy-title",
                                self.copied_field.as_ref() == Some(&"title".to_string()),
                                theme,
                                cx.listener({
                                    let title = title.to_string();
                                    move |this, _, window, cx| {
                                        this.copy_text(
                                            title.clone(),
                                            "title".to_string(),
                                            window.window_handle(),
                                            cx,
                                        );
                                    }
                                }),
                            )),
                    ),
            )
    }

    pub(super) fn render_badge(&self, data: &BadgeData) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .px_1()
            .py_0p5()
            .bg(data.bg)
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(data.fg)
                    .line_height(rems(0.625))
                    .child(data.text.clone()),
            )
    }

    pub(super) fn render_field_row(
        &self,
        label: &str,
        value: &str,
        field_id: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        DetailRow::new(
            label.to_string(),
            value.to_string(),
            self.copied_field.as_ref() == Some(&field_id.to_string()),
            cx.listener({
                let val = value.to_string();
                let field_id = field_id.to_string();
                move |this, _, window, cx| {
                    this.copy_text(val.clone(), field_id.clone(), window.window_handle(), cx);
                }
            }),
        )
        .render(theme)
    }

    pub(super) fn render_link_row(
        &self,
        label: &str,
        value: &str,
        url: &str,
        field_id: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        LinkRow::new(
            label.to_string(),
            value.to_string(),
            self.copied_field.as_ref() == Some(&field_id.to_string()),
            cx.listener({
                let val = value.to_string();
                let field_id = field_id.to_string();
                move |this, _, window, cx| {
                    this.copy_text(val.clone(), field_id.clone(), window.window_handle(), cx);
                }
            }),
            cx.listener({
                let url = url.to_string();
                move |_, _, _, _| {
                    main_window::utils::open_url(&url);
                }
            }),
        )
        .render(theme)
    }
}
