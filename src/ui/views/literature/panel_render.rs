use crate::RUNTIME;
use crate::app_state::theme::surface;
use crate::ui::views::literature::{FolderDragInfo, LiteratureDragInfo};
use crate::ui::{
    components::muted_input,
    views::main_window::{Cancel, ContextMenuType},
};
use components::IconName;
use gpui::prelude::*;
use services::sync::SyncStatus;
use std::ops::Range;

use gpui::{
    AnyElement, AppContext, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, Point, SharedString,
    Window, div, px, rems, uniform_list,
};
use gpui_component::{
    ActiveTheme, Icon, Sizable, Theme,
    button::{Button, ButtonVariants},
    h_flex,
};
use i18n::{I18nKey, t};
use log::{debug, error, info, warn};
use models::Folder;
use std::rc::Rc;
use std::sync::Arc;

use super::panel::LiteraturePanel;

impl Render for LiteraturePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let surface = surface(cx);
        let ui = cx.global::<crate::app_state::ui::UiState>();
        let (sync_status, attachment_sync_status) = if let Ok(state) = self.app.sync_state.lock() {
            (
                state.sync_status.clone(),
                state.attachment_sync_status.clone(),
            )
        } else {
            (SyncStatus::Idle, SyncStatus::Idle)
        };
        let (folders, mut tags) = {
            let ds = self.data_store.read(cx);
            (ds.folders.clone(), ds.tags.clone())
        };
        debug!(
            "[LiteraturePanel::render] 从 DataStore 读取的计数: all={}, uncategorized={}, trash={}",
            folders
                .iter()
                .find(|f| f.id == "all")
                .map_or(0, |f| f.literature_count),
            folders
                .iter()
                .find(|f| f.id == "uncategorized")
                .map_or(0, |f| f.literature_count),
            folders
                .iter()
                .find(|f| f.id == "trash")
                .map_or(0, |f| f.literature_count)
        );
        let (selected_folder_id, selected_tag_id) =
            (ui.selected_folder_id.clone(), ui.selected_tag_id.clone());
        let lang = self.app.current_language();

        // 按名称排序标签
        tags.sort_by_key(|a| a.0.name.to_lowercase());

        let parent_view = self.parent_view.clone();
        let theme = cx.theme().clone();

        div()
            .flex()
            .flex_col()
            .w_full()
            .flex_grow(1.0)
            .overflow_hidden()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .relative()
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                if let Some((rid, _)) = this.renaming.take() {
                    let is_new = {
                        let data = this.data_store.read(cx);
                        data.folders
                            .iter()
                            .find(|f| f.id == rid)
                            .is_some_and(|f| f.name.is_empty())
                    };
                    if is_new {
                        let _ = this.app.delete_folder(&rid);
                    }
                    cx.notify();
                }
                this.tag_renaming = None;
            }))
            .child({
                let all_count = folders
                    .iter()
                    .find(|f| f.id == "all")
                    .map_or(0, |f| f.literature_count);
                let uncategorized_count = folders
                    .iter()
                    .find(|f| f.id == "uncategorized")
                    .map_or(0, |f| f.literature_count);
                let trash_count = folders
                    .iter()
                    .find(|f| f.id == "trash")
                    .map_or(0, |f| f.literature_count);

                div()
                    .flex()
                    .flex_col()
                    .flex_grow(1.0)
                    .min_h_0()
                    .child(self.render_static_item(
                        StaticItemProps {
                            icon_builder: Box::new(|color| {
                                Icon::new(IconName::BookOpen)
                                    .small()
                                    .text_color(color)
                                    .into_any_element()
                            }),
                            text: t(I18nKey::AllLiterature, lang).to_string(),
                            count: all_count.to_string(),
                            is_selected: selected_folder_id.as_ref() == Some(&"all".to_string()),
                            id: "all".to_string(),
                            theme: theme.clone(),
                        },
                        cx,
                    ))
                    .child(self.render_static_item(
                        StaticItemProps {
                            icon_builder: Box::new(|color| {
                                Icon::new(IconName::File)
                                    .size(rems(1.0))
                                    .text_color(color)
                                    .into_any_element()
                            }),
                            text: t(I18nKey::Uncategorized, lang).to_string(),
                            count: uncategorized_count.to_string(),
                            is_selected: selected_folder_id.as_ref()
                                == Some(&"uncategorized".to_string()),
                            id: "uncategorized".to_string(),
                            theme: theme.clone(),
                        },
                        cx,
                    ))
                    .child(self.render_static_item(
                        StaticItemProps {
                            icon_builder: Box::new(|color| {
                                Icon::new(IconName::Trash)
                                    .size(rems(1.0))
                                    .text_color(color)
                                    .into_any_element()
                            }),
                            text: t(I18nKey::Trash, lang).to_string(),
                            count: trash_count.to_string(),
                            is_selected: selected_folder_id.as_ref() == Some(&"trash".to_string()),
                            id: "trash".to_string(),
                            theme: theme.clone(),
                        },
                        cx,
                    ))
                    .child(div().h(rems(0.0625)).bg(theme.border).my_2().mx_4())
                    // 1. 文件夹列表 (flex_1, 占用上方剩余空间)
                    .child({
                        let parent = parent_view.clone();
                        div()
                            .id("folder-list")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            // 拖放支持：移动到根目录
                            .on_drop(cx.listener(|this, drag_info: &FolderDragInfo, _, cx| {
                                let source_folder_id = &drag_info.folder_id;

                                // 检查是否已经在根目录
                                let is_already_root = {
                                     let data = this.data_store.read(cx);
                                     data.folders.iter()
                                         .find(|f| f.id == *source_folder_id)
                                         .is_some_and(|f| f.parent_id.is_none())
                                };

                                if is_already_root {
                                    return;
                                }

                                info!("移动文件夹 {source_folder_id} -> Root");
                                 let _ = this.app.move_folder(
                                     source_folder_id,
                                     None
                                 );
                                 cx.notify();
                            }))
                            // 拖拽悬停样式 (全局区域)
                            .drag_over::<FolderDragInfo>({
                                move |style, _, _, _| {
                                    style
                                        .bg(surface.selected_faint)
                                }
                            })
                            .on_mouse_down(MouseButton::Right, move |event: &MouseDownEvent, window, cx| {
                                // 空白区域触发“新建文件夹”菜单
                                if let Some(mw) = parent.upgrade() {
                                    mw.update(cx, |mw, cx| {
                                        mw.show_context_menu(
                                            event.position,
                                            ContextMenuType::Folder(None),
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            })
                            .child({
                                // 获取展开状态
                                let expanded_ids = self
                                    .app
                                    .local_state
                                    .read()
                                    .map(|s| s.expanded_folder_ids.clone())
                                    .unwrap_or_default();
                                // 拍平成平面列表
                                let entries = Rc::new(self.flatten_folders(
                                    &folders,
                                    None,
                                    0,
                                    &expanded_ids,
                                ));
                                let entry_count = entries.len();
                                let entries_clone = entries.clone();
                                let selected_id = selected_folder_id.clone();

                                uniform_list("folder-tree", entry_count, {
                                    cx.processor(move |this, visible_range: Range<usize>, _window, cx| {
                                        let mut items = Vec::with_capacity(
                                            visible_range.len(),
                                        );
                                        let theme = cx.theme().clone();
                                        for ix in visible_range {
                                            let entry = &entries_clone[ix];
                                            let is_selected = selected_id.as_ref()
                                                == Some(&entry.folder.id);
                                            let is_renaming = this
                                                .renaming
                                                .as_ref()
                                                .is_some_and(|(rid, _)| {
                                                    rid == &entry.folder.id
                                                });

                                            let item: AnyElement = if is_renaming {
                                                let (rid, input_state) = this
                                                    .renaming
                                                    .as_ref()
                                                    .unwrap();
                                                let rid = rid.clone();
                                                let input_state = input_state.clone();
                                                div()
                                                    .px_3()
                                                    .pl(rems(
                                                        1.0 * entry.depth as f32 + 1.25,
                                                    ))
                                                    .py_0p5()
                                                    .on_key_down(cx.listener(
                                                        move |this,
                                                              event: &KeyDownEvent,
                                                              _,
                                                              cx| {
                                                            if event.keystroke.key
                                                                == "escape"
                                                            {
                                                                let is_new = {
                                                                    let data = this
                                                                        .data_store
                                                                        .read(cx);
                                                                    data.folders
                                                                        .iter()
                                                                        .find(|f| {
                                                                            f.id == rid
                                                                        })
                                                                        .is_some_and(
                                                                            |f| {
                                                                                f.name
                                                                                    .is_empty()
                                                                            },
                                                                        )
                                                                };
                                                                if is_new {
                                                                    let _ = this
                                                                        .app
                                                                        .delete_folder(
                                                                            &rid,
                                                                        );
                                                                }
                                                                this.renaming = None;
                                                                cx.notify();
                                                            }
                                                        },
                                                    ))
                                                    .child(muted_input(&input_state, &theme))
                                                    .into_any_element()
                                            } else {
                                                let folder_id = entry.folder.id.clone();
                                                let folder_id_right = folder_id.clone();
                                                let folder_id_drop = folder_id.clone();
                                                let folder_id_folder_drop =
                                                    folder_id.clone();
                                                let folder_id_drag = folder_id.clone();
                                                let folder_name_drag =
                                                    entry.folder.name.clone();

                                                let icon = if entry.is_expanded {
                                                    IconName::FolderOpen
                                                } else {
                                                    IconName::Folder
                                                };
                                                let chevron = if entry.has_children {
                                                    if entry.is_expanded {
                                                        Some(IconName::ChevronDown)
                                                    } else {
                                                        Some(IconName::ChevronRight)
                                                    }
                                                } else {
                                                    None
                                                };
                                                let parent_mw = this.parent_view.clone();

                                                div()
                                                    .id(SharedString::from(format!(
                                                        "folder-item-wrapper-{}",
                                                        folder_id
                                                    )))
                                                    .on_drop(cx.listener({
                                                        let folder_id =
                                                            folder_id_drop.clone();
                                                        move |this,
                                                              drag_info:
                                                              &LiteratureDragInfo,
                                                              _,
                                                              cx| {
                                                            info!(
                                                                "拖放文献到文件夹: {} 篇文献 -> {}",
                                                                drag_info.count(),
                                                                folder_id
                                                            );
                                                            for lit_id in
                                                                &drag_info
                                                                    .literature_ids
                                                            {
                                                                if let Err(e) = this
                                                                    .app
                                                                    .add_literature_to_folder(
                                                                        lit_id,
                                                                        &folder_id,
                                                                    )
                                                                {
                                                                    error!(
                                                                        "添加文献到文件夹失败: {e}"
                                                                    );
                                                                }
                                                            }
                                                            cx.notify();
                                                        }
                                                    }))
                                                    .on_drop(cx.listener({
                                                        let target_folder_id =
                                                            folder_id_folder_drop;
                                                        move |this,
                                                              drag_info:
                                                              &FolderDragInfo,
                                                              _,
                                                              cx| {
                                                            let source_folder_id =
                                                                &drag_info.folder_id;
                                                            if source_folder_id
                                                                == &target_folder_id
                                                            {
                                                                return;
                                                            }
                                                            let data = this
                                                                .data_store
                                                                .read(cx);
                                                            let is_descendant = {
                                                                let mut current_id =
                                                                    Some(
                                                                        target_folder_id
                                                                            .clone(),
                                                                    );
                                                                let mut found = false;
                                                                while let Some(
                                                                    cid,
                                                                ) = current_id
                                                                {
                                                                    if cid
                                                                        == *source_folder_id
                                                                    {
                                                                        found = true;
                                                                        break;
                                                                    }
                                                                    if let Some(
                                                                        folder,
                                                                    ) = data
                                                                        .folders
                                                                        .iter()
                                                                        .find(|f| {
                                                                            f.id == cid
                                                                        })
                                                                    {
                                                                        current_id =
                                                                            folder
                                                                                .parent_id
                                                                                .clone();
                                                                    } else {
                                                                        break;
                                                                    }
                                                                }
                                                                found
                                                            };
                                                            if is_descendant {
                                                                warn!(
                                                                    "无法移动文件夹: 目标是源的子文件夹"
                                                                );
                                                                return;
                                                            }
                                                            let is_already_parent = data
                                                                .folders
                                                                .iter()
                                                                .find(|f| {
                                                                    f.id
                                                                        == *source_folder_id
                                                                })
                                                                .is_some_and(|f| {
                                                                    f.parent_id.as_ref()
                                                                        == Some(
                                                                            &target_folder_id,
                                                                        )
                                                                });
                                                            if is_already_parent {
                                                                return;
                                                            }
                                                            info!(
                                                                "移动文件夹 {source_folder_id} -> {target_folder_id}"
                                                            );
                                                            let _ = this
                                                                .app
                                                                .move_folder(
                                                                    source_folder_id,
                                                                    Some(
                                                                        target_folder_id
                                                                            .clone(),
                                                                    ),
                                                                );
                                                            cx.notify();
                                                        }
                                                    }))
                                                    .drag_over::<LiteratureDragInfo>({
                                                        let theme = theme.clone();
                                                        move |style, _, _, _| {
                                                            style
                                                                .bg(surface.selected_hover)
                                                                .border_1()
                                                                .border_color(
                                                                    theme.primary,
                                                                )
                                                                .rounded_md()
                                                        }
                                                    })
                                                    .drag_over::<FolderDragInfo>({
                                                        let theme = theme.clone();
                                                        move |style, _, _, _| {
                                                            style
                                                                .bg(surface.selected_hover)
                                                                .border_1()
                                                                .border_color(
                                                                    theme.primary,
                                                                )
                                                                .rounded_md()
                                                        }
                                                    })
                                                    .on_mouse_down(
                                                        MouseButton::Right,
                                                        cx.listener({
                                                            let folder_id =
                                                                folder_id_right.clone();
                                                            let parent_mw =
                                                                parent_mw.clone();
                                                            move |this,
                                                                  event:
                                                                  &MouseDownEvent,
                                                                  window,
                                                                  cx| {
                                                                cx.stop_propagation();
                                                                this.select_folder(
                                                                    folder_id.clone(),
                                                                    cx,
                                                                );
                                                                if let Some(mw) =
                                                                    parent_mw.upgrade()
                                                                {
                                                                    mw.update(
                                                                        cx,
                                                                        |mw, cx| {
                                                                            mw
                                                                                .show_context_menu(
                                                                                    event
                                                                                        .position,
                                                                                    ContextMenuType::Folder(
                                                                                        Some(
                                                                                            folder_id.clone(),
                                                                                        ),
                                                                                    ),
                                                                                    window,
                                                                                    cx,
                                                                                );
                                                                        },
                                                                    );
                                                                }
                                                            }
                                                        }),
                                                    )
                                                    .child(
                                                        div()
                                                            .id(SharedString::from(
                                                                format!(
                                                                    "folder-item-inner-{}",
                                                                    folder_id
                                                                ),
                                                            ))
                                                            .on_drag(
                                                                FolderDragInfo::new(
                                                                    folder_id_drag,
                                                                    folder_name_drag,
                                                                ),
                                                                |drag_info,
                                                                 _point,
                                                                 _window,
                                                                 cx| {
                                                                    cx.new(|_| {
                                                                        drag_info
                                                                            .clone()
                                                                            .with_position(
                                                                                Point::new(
                                                                                    px(
                                                                                        0.0,
                                                                                    ),
                                                                                    px(
                                                                                        0.0,
                                                                                    ),
                                                                                ),
                                                                            )
                                                                    })
                                                                },
                                                            )
                                                            .flex()
                                                            .items_center()
                                                            .px_3()
                                                            .py_0p5()
                                                            .pl(rems(
                                                                1.0 * entry.depth as f32
                                                                    + 0.25,
                                                            ))
                                                            .mx_2()
                                                            .rounded_md()
                                                            .when(
                                                                is_selected,
                                                                |s| {
                                                                     s.bg(
                                                                         theme.primary,
                                                                     )
                                                                    .text_color(
                                                                        theme.primary_foreground,
                                                                    )
                                                                },
                                                            )
                                                            .when(
                                                                !is_selected,
                                                                |s| {
                                                                    s.hover(|s| {
                                                                        s.bg(theme.primary.opacity(0.15))
                                                                    })
                                                                },
                                                            )
                                                            .on_click(cx.listener({
                                                                let folder_id =
                                                                    folder_id.clone();
                                                                move |this: &mut Self,
                                                                      event:
                                                                      &gpui::ClickEvent,
                                                                      _,
                                                                      cx| {
                                                                    if event
                                                                        .click_count()
                                                                        == 2
                                                                    {
                                                                        this
                                                                            .toggle_folder_expansion(
                                                                                folder_id
                                                                                    .clone(),
                                                                            );
                                                                    } else {
                                                                        this
                                                                            .select_folder(
                                                                                folder_id
                                                                                    .clone(),
                                                                                cx,
                                                                            );
                                                                    }
                                                                    cx.notify();
                                                                }
                                                            }))
                                                            .child(
                                                                h_flex()
                                                                    .w_full()
                                                                    .justify_between()
                                                                    .child(
                                                                        h_flex()
                                                                            .gap_1()
                                                                            .child(
                                                                                div()
                                                                                    .id(
                                                                                        SharedString::from(
                                                                                            format!(
                                                                                                "folder-item-chevron-{}",
                                                                                                folder_id
                                                                                            ),
                                                                                        ),
                                                                                    )
                                                                                    .w(
                                                                                        rems(
                                                                                            0.75,
                                                                                        ),
                                                                                    )
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .justify_center()
                                                                                    .cursor_pointer()
                                                                                    .on_click(
                                                                                        cx
                                                                                            .listener(
                                                                                        {
                                                                                            let folder_id =
                                                                                                folder_id
                                                                                                    .clone(
                                                                                                );
                                                                                            move |this: &mut Self,
                                                                                                  _,
                                                                                                  _,
                                                                                                  cx| {
                                                                                                cx.stop_propagation(
                                                                                                );
                                                                                                this
                                                                                                    .toggle_folder_expansion(
                                                                                                        folder_id
                                                                                                            .clone(
                                                                                                        ),
                                                                                                    );
                                                                                                cx.notify(
                                                                                                );
                                                                                            }
                                                                                        },
                                                                                        ),
                                                                                    )
                                                                                    .children(
                                                                                        chevron
                                                                                            .map(
                                                                                            |c| {
                                                                                                Icon::new(
                                                                                                    c,
                                                                                                )
                                                                                                .xsmall()
                                                                                                .text_color(
                                                                                                    if is_selected { theme.primary_foreground } else { theme.muted_foreground },
                                                                                                )
                                                                                            },
                                                                                        ),
                                                                                    ),
                                                                            )
                                                                            .child(
                                                                                Icon::new(
                                                                                    icon,
                                                                                )
                                                                                .small()
                                                                                .text_color(
                                                                                    if is_selected { theme.primary_foreground } else { theme.foreground },
                                                                                ),
                                                                            )
                                                                            .child(
                                                                                div()
                                                                                    .text_sm()
                                                                                    .text_color(
                                                                                        if is_selected { theme.primary_foreground } else { theme.foreground },
                                                                                    )
                                                                                    .child(
                                                                                        entry
                                                                                            .folder
                                                                                            .name
                                                                                            .clone(),
                                                                                    ),
                                                                            ),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .text_xs()
                                                                            .text_color(
                                                                                if is_selected { theme.primary_foreground } else { theme.muted_foreground },
                                                                            )
                                                                            .child(
                                                                                entry
                                                                                    .folder
                                                                                    .literature_count
                                                                                    .to_string(),
                                                                            ),
                                                                    ),
                                                            ),
                                                    )
                                                    .into_any_element()
                                            };
                                            items.push(item);
                                        }
                                        items
                                    })
                                })
                                .flex_grow_1()
                                .size_full()
                                .track_scroll(&self.folder_list_scroll_handle)
                            })
                    })
                    // 2. 标签容器
                    .child(
                        div()
                            .id("tag-container")
                            .flex()
                            .flex_col()
                            .flex_shrink_0()
                            .max_h(rems(12.5))
                            .overflow_y_scroll()
                            .border_t_1()
                            .border_color(surface.border_faint)
                            .bg(theme.sidebar)
                            .child(
                                div()
                                    .id("tag-scroll-list")
                                    .flex()
                                    .flex_row()
                                    .flex_wrap()
                                    .gap_x_2()
                                    .gap_y_1()
                                    .p_3()
                                    .children(tags.iter().map(|(tag, _count)| {
                                            if let Some((rid, input_state)) = &self.tag_renaming
                                                && rid == &tag.id
                                            {
                                                let rid = rid.clone();
                                                return div()
                                                        .w_full()
                                                        .mb_1()
                                                        .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                                                             if event.keystroke.key == "escape" {
                                                                 let is_new = {
                                                                     let data = this.data_store.read(cx);
                                                                     data.tags
                                                                         .iter()
                                                                         .find(|(t, _)| t.id == rid)
                                                                         .is_some_and(|(t, _)| t.name.is_empty())
                                                                 };

                                                                if is_new {
                                                                    let _ = this.app.tag_service.delete_tag(&this.app.db, || this.app.notify_data_changed(), &rid);
                                                                }
                                                                this.tag_renaming = None;
                                                                cx.notify();
                                                            }
                                                        }))
                                                        .child(muted_input(input_state, &theme))
                                                        .into_any_element();
                                            }
                                            self.render_tag_item(tag, selected_tag_id.as_ref(), &theme, cx).into_any_element()
                                        }))
                                )
                    )
            })
            .child(
                h_flex()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(surface.border_faint)
                    .child({
                        let icon = match &sync_status {
                            SyncStatus::Idle => Icon::new(IconName::Check)
                                .small()
                                .text_color(theme.muted_foreground),
                            SyncStatus::Syncing => Icon::new(IconName::LoaderCircle)
                                .small()
                                .text_color(theme.primary),
                            SyncStatus::Error(_) => {
                                Icon::new(IconName::CircleX).small().text_color(theme.red_light)
                            }
                            SyncStatus::Conflict(_) => Icon::new(IconName::TriangleAlert)
                                .small()
                                .text_color(theme.warning),
                        };
                        div().relative().child(
                            Button::new("btn-sync-status")
                                .child(icon)
                                .ghost()
                                .xsmall()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    match &sync_status {
                                        SyncStatus::Idle | SyncStatus::Error(_) => {
                                            // 点击时直接触发重试，不显示旧的错误信息
                                            let app = this.app.clone();
                                            RUNTIME.spawn(async move {
                                                app.sync_service.force_sync().await;
                                            });
                                        }
                                        SyncStatus::Conflict(lits) => {
                                            // 获取本地对应的文献，构成对比组
                                            let mut groups = Vec::new();
                                            {
                                                    for remote_lit in lits {
                                                    if let Ok(Some(local_lit)) =
                                                        this.app.db.get_literature(&remote_lit.id)
                                                    {
                                                        groups.push(vec![
                                                            local_lit,
                                                            remote_lit.clone(),
                                                        ]);
                                                    } else {
                                                        groups.push(vec![remote_lit.clone()]);
                                                    }
                                                }
                                            }
                                            if let Ok(mut state) = this.app.sync_state.lock() {
                                                state.sync_conflict_groups = Some(groups);
                                            }
                                            // 发射 Action 让 MainWindow 捕获并打开冲突列表
                                            cx.dispatch_action(
                                                &crate::ui::views::main_window::HandleSyncConflicts,
                                            );
                                        }
                                        SyncStatus::Syncing => {}
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        Button::new("btn-sync-attachments")
                            .child(match &attachment_sync_status {
                                SyncStatus::Syncing => Icon::new(IconName::LoaderCircle)
                                    .small()
                                    .text_color(theme.primary),
                                SyncStatus::Error(_) => Icon::new(IconName::TriangleAlert)
                                    .small()
                                    .text_color(theme.red_light),
                                _ => Icon::new(IconName::Cloud).small().text_color(theme.muted_foreground),
                            })
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let SyncStatus::Syncing = attachment_sync_status {
                                    return;
                                }

                                // 点击直接重试，不显示旧错误
                                let app = this.app.clone();
                                RUNTIME.spawn(async move {
                                    app.perform_attachments_sync();
                                });
                                    cx.notify();
                            })),
                    ),
            )
    }
}

pub(crate) struct FolderTreeEntry {
    pub(crate) folder: Arc<Folder>,
    pub(crate) depth: usize,
    pub(crate) is_expanded: bool,
    pub(crate) has_children: bool,
}
pub(crate) struct StaticItemProps {
    pub(crate) icon_builder: Box<dyn Fn(Hsla) -> AnyElement>,
    pub(crate) text: String,
    pub(crate) count: String,
    pub(crate) is_selected: bool,
    pub(crate) id: String,
    pub(crate) theme: Theme,
}
