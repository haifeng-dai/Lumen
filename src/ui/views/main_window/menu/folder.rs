use super::super::MainWindow;
use components::IconName;
use gpui::WeakEntity;
use gpui::prelude::*;
use gpui_component::{
    ActiveTheme, Icon,
    menu::{PopupMenu, PopupMenuItem},
};
use i18n::{I18nKey, Language, t};

use super::danger_menu_item;

pub(super) fn build_folder_menu(
    menu: PopupMenu,
    target_id: Option<String>,
    this_weak: WeakEntity<MainWindow>,
    lang: Language,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let mut menu = menu;
    if target_id.as_deref() == Some("trash") {
        let this_weak_clone = this_weak.clone();
        menu = menu.item(
            danger_menu_item(
                cx.theme().danger,
                t(I18nKey::EmptyTrash, lang),
                IconName::Trash,
            )
            .on_click(move |_, _window, cx| {
                if let Some(this) = this_weak_clone.upgrade() {
                    this.update(cx, |this, cx| {
                        this.handle_empty_trash(cx);
                        this.close_menus(cx);
                    });
                }
            }),
        );
    } else {
        // 1. 新建子文件夹
        let this_weak_clone = this_weak.clone();
        let target_id_clone = target_id.clone();
        menu = menu.item(
            PopupMenuItem::new(t(I18nKey::NewFolder, lang))
                .icon(Icon::new(IconName::Plus))
                .on_click(move |_, window, cx| {
                    if let Some(this) = this_weak_clone.upgrade() {
                        this.update(cx, |this, cx| {
                            this.literature_panel.update(cx, |panel, cx| {
                                panel.add_folder(target_id_clone.clone(), window, cx);
                            });
                            this.close_menus(cx);
                        });
                    }
                }),
        );

        // 2. 文件夹重命名与删除 (仅限非系统默认文件夹)
        if let Some(fid) = target_id {
            let is_system = fid == "all" || fid == "uncategorized" || fid == "trash";
            if !is_system {
                let this_weak_clone = this_weak.clone();
                let fid_rename = fid.clone();
                menu = menu.item(
                    PopupMenuItem::new(t(I18nKey::Rename, lang))
                        .icon(Icon::new(IconName::Edit))
                        .on_click(move |_, window, cx| {
                            if let Some(this) = this_weak_clone.upgrade() {
                                this.update(cx, |this, cx| {
                                    this.literature_panel.update(cx, |panel, cx| {
                                        panel.start_rename(fid_rename.clone(), false, window, cx);
                                    });
                                    this.close_menus(cx);
                                });
                            }
                        }),
                );

                let this_weak_clone = this_weak.clone();
                let fid_delete = fid.clone();
                menu = menu.item(
                    danger_menu_item(cx.theme().danger, t(I18nKey::Delete, lang), IconName::Trash)
                        .on_click(move |_, _window, cx| {
                            if let Some(this) = this_weak_clone.upgrade() {
                                this.update(cx, |this, cx| {
                                    this.literature_panel.update(cx, |panel, cx| {
                                        panel.delete_folder(fid_delete.clone(), cx);
                                    });
                                    this.close_menus(cx);
                                });
                            }
                        }),
                );
            }
        }
    }
    menu
}
