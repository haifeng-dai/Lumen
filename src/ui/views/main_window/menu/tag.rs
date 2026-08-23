use super::super::MainWindow;
use components::IconName;
use gpui::WeakEntity;
use gpui::prelude::*;
use gpui::{SharedString, div, rems};
use gpui_component::{
    ActiveTheme, Colorize, Icon, h_flex,
    menu::{PopupMenu, PopupMenuItem},
    v_flex,
};
use i18n::{I18nKey, Language, t};

use super::danger_menu_item;

pub(super) fn build_tag_menu(
    menu: PopupMenu,
    target_id: Option<String>,
    this_weak: WeakEntity<MainWindow>,
    lang: Language,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let mut menu = menu;
    // 标签重命名与删除
    if let Some(tid) = target_id {
        let this_weak_clone = this_weak.clone();
        let tid_rename = tid.clone();
        menu = menu.item(
            PopupMenuItem::new(t(I18nKey::Rename, lang))
                .icon(Icon::new(IconName::Edit))
                .on_click(move |_, window, cx| {
                    if let Some(this) = this_weak_clone.upgrade() {
                        this.update(cx, |this, cx| {
                            this.literature_panel.update(cx, |panel, cx| {
                                panel.start_tag_rename(tid_rename.clone(), false, window, cx);
                            });
                            this.close_menus(cx);
                        });
                    }
                }),
        );

        // 颜色选择
        menu = menu.separator();
        let this_weak_clone = this_weak.clone();
        let tid_color = tid.clone();
        menu = menu.item(PopupMenuItem::element(move |_window, cx| {
            let this_weak_inner = this_weak_clone.clone();
            let tid_inner = tid_color.clone();

            let (tag_name, current_color) = if let Some(this) = this_weak_inner.upgrade() {
                this.update(cx, |this, cx| {
                    let data = this.data_store.read(cx);
                    data.tags
                        .iter()
                        .find(|(t, _)| t.id == tid_inner)
                        .map(|(t, _)| (t.name.clone(), t.color.clone()))
                        .unwrap_or_default()
                })
            } else {
                (String::new(), String::new())
            };

            let tag_colors_ref = models::tag::TAG_COLORS;
            let tag_colors: Vec<&str> = tag_colors_ref.iter().map(|(_, hex)| *hex).collect();

            let this_weak_inner2 = this_weak_inner.clone();
            let tid_inner2 = tid_inner.clone();
            let tag_name_inner2 = tag_name.clone();
            let active_border_color = cx.theme().foreground;

            v_flex().mx(gpui::px(-8.0)).px_2().py_1().gap_1().children(
                tag_colors
                    .chunks(5)
                    .map(move |chunk| {
                        let chunk = chunk.to_vec();
                        let current_color = current_color.clone();
                        let tag_name = tag_name_inner2.clone();
                        let tid = tid_inner2.clone();
                        let this_weak = this_weak_inner2.clone();
                        let active_border = active_border_color;

                        h_flex().w_full().justify_around().gap_1().py_1().children(
                            chunk
                                .iter()
                                .map(move |&color_hex| {
                                    let color_hex = color_hex.to_string();
                                    let is_active = current_color == color_hex;
                                    let color_hex_clone = color_hex.clone();
                                    let tid_clone = tid.clone();
                                    let tag_name_clone = tag_name.clone();
                                    let this_weak_click = this_weak.clone();

                                    let color =
                                        gpui::Hsla::parse_hex(&color_hex).unwrap_or(gpui::red());

                                    div()
                                        .id(SharedString::from(format!("color-{}", color_hex)))
                                        .size(rems(1.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded_full()
                                        .cursor_pointer()
                                        .when(is_active, |this| {
                                            this.border_2().border_color(active_border)
                                        })
                                        .child(div().size(rems(0.6)).rounded_full().bg(color))
                                        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                            cx.stop_propagation();
                                            if let Some(this) = this_weak_click.upgrade() {
                                                this.update(cx, |this, cx| {
                                                    let _ = this.app.tag_service.update_tag(
                                                        &this.app.db,
                                                        || this.app.notify_data_changed(),
                                                        &tid_clone,
                                                        &tag_name_clone,
                                                        &color_hex_clone,
                                                    );
                                                    this.close_menus(cx);
                                                });
                                            }
                                        })
                                })
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        }));
        menu = menu.separator();

        let this_weak_clone = this_weak.clone();
        let tid_delete = tid.clone();
        menu = menu.item(
            danger_menu_item(cx.theme().danger, t(I18nKey::Delete, lang), IconName::Trash)
                .on_click(move |_, _window, cx| {
                    if let Some(this) = this_weak_clone.upgrade() {
                        let app = this.read(cx).app.clone();
                        let id = tid_delete.clone();
                        let _ =
                            app.tag_service
                                .delete_tag(&app.db, || app.notify_data_changed(), &id);
                        this.update(cx, |this, cx| {
                            this.close_menus(cx);
                        });
                    }
                }),
        );
    }
    menu
}
