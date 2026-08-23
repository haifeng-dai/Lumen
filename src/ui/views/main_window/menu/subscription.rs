use super::super::MainWindow;
use crate::ui::notification::show_notification;
use components::IconName;
use gpui::prelude::*;
use gpui::{AsyncApp, WeakEntity, Window};
use gpui_component::notification::NotificationType;
use gpui_component::{
    ActiveTheme, Icon,
    menu::{PopupMenu, PopupMenuItem},
};
use i18n::{I18nKey, Language, t, tf};
use log::error;
use services::feed::SubscriptionRefreshResult;
use std::collections::HashSet;
use std::sync::Arc;

use super::{
    FolderChildrenMap, FolderNameMap, FolderSelectClosure, build_folder_level, danger_menu_item,
};

pub(super) fn build_subscription_menu(
    menu: PopupMenu,
    target_id: Option<String>,
    this_weak: WeakEntity<MainWindow>,
    lang: Language,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let mut menu = menu;
    // 1. 订阅编辑与删除（新建订阅入口已移至顶部工具栏/菜单，此处不再重复）
    if let Some(sid) = target_id {
        // 2.1 立即更新订阅
        let this_weak_clone = this_weak.clone();
        let sid_update = sid.clone();
        menu = menu.item(
            PopupMenuItem::new(t(I18nKey::UpdateSubscription, lang))
                .icon(Icon::new(IconName::RotateCw))
                .on_click(move |_, _window, cx| {
                    if let Some(this) = this_weak_clone.upgrade() {
                        let app = this.read(cx).app.clone();
                        if let Err(e) = app.refresh_feed(&sid_update) {
                            error!("手动刷新订阅失败: {e}");
                        }
                        this.update(cx, |this, cx| {
                            this.close_menus(cx);
                        });
                    }
                }),
        );

        // 2.2 编辑
        let this_weak_clone = this_weak.clone();
        let sid_edit = sid.clone();
        menu = menu.item(
            PopupMenuItem::new(t(I18nKey::Edit, lang))
                .icon(Icon::new(IconName::Edit))
                .on_click(move |_, window, cx| {
                    if let Some(this) = this_weak_clone.upgrade() {
                        this.update(cx, |this, cx| {
                            this.open_edit_subscription_modal(sid_edit.clone(), window, cx);
                            this.close_menus(cx);
                        });
                    }
                }),
        );

        // 2.3 删除
        let this_weak_clone = this_weak.clone();
        let sid_delete = sid.clone();
        menu = menu.item(
            danger_menu_item(cx.theme().danger, t(I18nKey::Delete, lang), IconName::Trash)
                .on_click(move |_, _window, cx| {
                    if let Some(this) = this_weak_clone.upgrade() {
                        let app = this.read(cx).app.clone();
                        let _ = app.delete_feed(&sid_delete);
                        this.update(&mut *cx, |this, cx| {
                            this.close_menus(cx);
                        });
                    }
                }),
        );
    }
    menu
}

pub(super) fn build_subscription_all_menu(
    menu: PopupMenu,
    this_weak: WeakEntity<MainWindow>,
    lang: Language,
    _cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let mut menu = menu;
    let this_weak_clone = this_weak.clone();
    menu = menu.item(
        PopupMenuItem::new(t(I18nKey::UpdateAllSubscriptions, lang))
            .icon(Icon::new(IconName::RotateCw))
            .on_click(move |_, _window, cx| {
                if let Some(this) = this_weak_clone.upgrade() {
                    let app = this.read(cx).app.clone();
                    match app.refresh_all_subscriptions() {
                        Ok(rx) => {
                            cx.spawn(async move |cx: &mut AsyncApp| {
                                let mut rx = rx;
                                while let Some(r) = rx.recv().await {
                                    cx.update(|app| match r {
                                        SubscriptionRefreshResult::Ok { name } => {
                                            show_notification(
                                                NotificationType::Success,
                                                tf(
                                                    I18nKey::SubscriptionUpdated,
                                                    lang,
                                                    &[name.as_str()],
                                                ),
                                                app,
                                            );
                                        }
                                        SubscriptionRefreshResult::Err { name, error } => {
                                            show_notification(
                                                NotificationType::Error,
                                                tf(
                                                    I18nKey::SubscriptionUpdateFailed,
                                                    lang,
                                                    &[name.as_str(), error.as_str()],
                                                ),
                                                app,
                                            );
                                        }
                                    });
                                }
                            })
                            .detach();
                        }
                        Err(e) => {
                            error!("手动刷新所有订阅失败: {e}");
                        }
                    }
                    this.update(cx, |this, cx| {
                        this.close_menus(cx);
                    });
                }
            }),
    );
    menu
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_subscription_item_menu(
    menu: PopupMenu,
    sub_id: String,
    sub_item_state: Option<(bool, bool)>,
    sub_name_map: FolderNameMap,
    sub_children_map: FolderChildrenMap,
    window: &mut Window,
    this_weak: WeakEntity<MainWindow>,
    lang: Language,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let mut menu = menu;
    let (is_read, is_added) = sub_item_state.unwrap_or((false, false));

    // 1. 添加到文献库（二级菜单：未分类 + 各文件夹）
    if !is_added {
        let this_weak_clone = this_weak.clone();
        let sub_id_add = sub_id.clone();

        let add_library_submenu = PopupMenu::build(window, cx, move |mut m, window, cx| {
            // 1.1 文献库（原未分类）：仅加入文献库，不指定文件夹
            let this_weak_uncat = this_weak_clone.clone();
            let sub_id_uncat = sub_id_add.clone();
            m = m.item(PopupMenuItem::new(t(I18nKey::Library, lang)).on_click(
                move |_, _window, cx| {
                    if let Some(this) = this_weak_uncat.upgrade() {
                        let id = sub_id_uncat.clone();
                        let app = this.read(cx).app.clone();
                        if let Err(e) = app.add_feed_item_to_library(&id) {
                            error!("添加到文献库失败: {e}");
                        }
                        this.update(cx, |this, cx| this.close_menus(cx));
                    }
                },
            ));
            // 1.2 各自定义文件夹（多级树，不限层级）
            let on_select: FolderSelectClosure = Arc::new({
                let this_weak_tree = this_weak_clone.clone();
                let sub_id_tree = sub_id_add.clone();
                move |folder_id, _window, cx| {
                    if let Some(this) = this_weak_tree.upgrade() {
                        let id = sub_id_tree.clone();
                        let app = this.read(cx).app.clone();
                        match app.add_feed_item_to_library(&id) {
                            Ok(lit_id) => {
                                let _ = app.add_literature_to_folder(&lit_id, folder_id);
                            }
                            Err(e) => {
                                error!("添加到文献库失败: {e}");
                            }
                        }
                        this.update(cx, |this, cx| this.close_menus(cx));
                    }
                }
            });
            m = build_folder_level(
                m,
                None,
                &sub_name_map,
                &sub_children_map,
                &on_select,
                window,
                cx,
            );
            m
        });

        menu = menu.item(
            PopupMenuItem::submenu(t(I18nKey::AddToLibrary, lang), add_library_submenu)
                .icon(Icon::new(IconName::Plus)),
        );
    }

    // 2. 标记已读/未读
    let this_weak_clone = this_weak.clone();
    let sub_id_read = sub_id.clone();
    let label = if is_read {
        t(I18nKey::MarkAsUnread, lang)
    } else {
        t(I18nKey::MarkAsRead, lang)
    };
    menu = menu.item(
        PopupMenuItem::new(label)
            .icon(Icon::new(IconName::Bell))
            .on_click(move |_, _window, cx| {
                if let Some(this) = this_weak_clone.upgrade() {
                    let id = sub_id_read.clone();
                    let app = this.read(cx).app.clone();
                    let mut hs = HashSet::new();
                    hs.insert(id.clone());
                    if let Err(e) = app.smart_toggle_feed_items_read(&id, !is_read, &hs) {
                        error!("更新已读状态失败: {e}");
                    }
                    this.update(cx, |this, cx| {
                        this.close_menus(cx);
                    });
                }
            }),
    );
    menu
}
