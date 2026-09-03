use crate::RUNTIME;
use crate::app_state::theme::surface;
use crate::ui::views::literature::{FolderDragInfo, LiteratureDragInfo};
use crate::ui::{
    components::muted_input,
    views::main_window::{Cancel, ContextMenuType},
};
use components::IconName;
use gpui::prelude::*;
use services::sync::{DatabaseSyncStatus, FileSyncStatus, FileSyncSummaryView, SyncRunOutcome};
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
        let (sync_status, file_status) = if let Ok(state) = self.app.sync_state.lock() {
            (
                state.database_sync_status.clone(),
                state.file_sync_status.clone(),
            )
        } else {
            (DatabaseSyncStatus::Idle, FileSyncStatus::Idle)
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
        let database_sync_tooltip = match &sync_status {
            DatabaseSyncStatus::Idle => None,
            DatabaseSyncStatus::Syncing => Some(I18nKey::DatabaseSyncInProgress),
            DatabaseSyncStatus::NeedsRemoteInitialization => {
                Some(I18nKey::DatabaseSyncNeedsInitialization)
            }
            DatabaseSyncStatus::NeedsRemoteAdoption => Some(I18nKey::DatabaseSyncNeedsAdoption),
            DatabaseSyncStatus::IdentityMismatch => Some(I18nKey::DatabaseSyncIdentityMismatch),
            DatabaseSyncStatus::Conflict => Some(I18nKey::DatabaseSyncConflict),
            DatabaseSyncStatus::PartialFailure => Some(I18nKey::DatabaseSyncPartialFailure),
            DatabaseSyncStatus::Error(_) => Some(I18nKey::DatabaseSyncError),
        };

        // 最近一次文件同步摘要（经 services 只读接口，UI 不直接访问 database）
        let file_sync_tooltip: Option<SharedString> = match self.app.file_sync_summary() {
            Ok(Some(view)) => Some(format_file_sync_summary(&view, lang).into()),
            Ok(None) => None,
            // 记录损坏：显示安全的不可用状态，不降级为成功
            Err(_) => Some(SharedString::from(t(
                I18nKey::FileSyncSummaryUnavailable,
                lang,
            ))),
        };

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
                                                                let has_children = entry.has_children;
                                                                move |this: &mut Self,
                                                                      event:
                                                                      &gpui::ClickEvent,
                                                                      _,
                                                                      cx| {
                                                                    if event
                                                                        .click_count()
                                                                        == 2
                                                                    {
                                                                        if has_children {
                                                                            this
                                                                                .toggle_folder_expansion(
                                                                                    folder_id
                                                                                        .clone(),
                                                                                );
                                                                        }
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
                                                                                    .id(SharedString::from(
                                                                                        format!(
                                                                                            "folder-item-chevron-{}",
                                                                                            folder_id
                                                                                        ),
                                                                                    ))
                                                                                    .w(rems(0.75))
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .justify_center()
                                                                                    .cursor_pointer()
                                                                                    .on_click(
                                                                                        cx.listener({
                                                                                            let folder_id = folder_id.clone();
                                                                                            let has_children = entry.has_children;
                                                                                            move |this: &mut Self, _, _, cx| {
                                                                                                if has_children {
                                                                                                    cx.stop_propagation();
                                                                                                    this.toggle_folder_expansion(folder_id.clone());
                                                                                                    cx.notify();
                                                                                                }
                                                                                            }
                                                                                        }),
                                                                                    )
                                                                                    .children(chevron.map(|c| {
                                                                                        Icon::new(c)
                                                                                            .xsmall()
                                                                                            .text_color(
                                                                                                if is_selected { theme.primary_foreground } else { theme.muted_foreground },
                                                                                            )
                                                                                    }))
                                                                            )
                                                                            .child(
                                                                                div()
                                                                                    .id(SharedString::from(
                                                                                        format!(
                                                                                            "folder-item-icon-{}",
                                                                                            folder_id
                                                                                        ),
                                                                                    ))
                                                                                    .flex()
                                                                                    .items_center()
                                                                                    .justify_center()
                                                                                    .when(entry.has_children, |s| {
                                                                                        s.cursor_pointer()
                                                                                    })
                                                                                    .on_click(
                                                                                        cx.listener({
                                                                                            let folder_id = folder_id.clone();
                                                                                            let has_children = entry.has_children;
                                                                                            move |this: &mut Self, _, _, cx| {
                                                                                                if has_children {
                                                                                                    cx.stop_propagation();
                                                                                                    this.toggle_folder_expansion(folder_id.clone());
                                                                                                    cx.notify();
                                                                                                }
                                                                                            }
                                                                                        }),
                                                                                    )
                                                                                    .child(
                                                                                        Icon::new(icon)
                                                                                            .small()
                                                                                            .text_color(
                                                                                                if is_selected { theme.primary_foreground } else { theme.foreground },
                                                                                            ),
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
                            DatabaseSyncStatus::Idle => Icon::new(IconName::Check)
                                .small()
                                .text_color(theme.muted_foreground),
                            DatabaseSyncStatus::Syncing => Icon::new(IconName::LoaderCircle)
                                .small()
                                .text_color(theme.primary),
                            DatabaseSyncStatus::NeedsRemoteInitialization
                            | DatabaseSyncStatus::NeedsRemoteAdoption
                            | DatabaseSyncStatus::IdentityMismatch
                            | DatabaseSyncStatus::Conflict
                            | DatabaseSyncStatus::PartialFailure => Icon::new(IconName::TriangleAlert)
                                .small()
                                .text_color(theme.warning),
                            DatabaseSyncStatus::Error(_) => {
                                Icon::new(IconName::CircleX).small().text_color(theme.red_light)
                            }
                        };
                        div().relative().child(
                            Button::new("btn-sync-status")
                                .child(icon)
                                .ghost()
                                .xsmall()
                                .when_some(database_sync_tooltip, |this, key| {
                                    this.tooltip(t(key, lang))
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    match &sync_status {
                                        DatabaseSyncStatus::Idle | DatabaseSyncStatus::Error(_) => {
                                            // 点击时直接触发重试，不显示旧的错误信息
                                            let app = this.app.clone();
                                            RUNTIME.spawn(async move {
                                                app.sync_service.force_sync().await;
                                            });
                                        }
                                        status => {
                                            let parent = this.parent_view.clone();
                                            if let Some(parent) = parent.upgrade() {
                                                parent.update(cx, |window, cx| {
                                                    window.open_database_sync_action(status.clone(), cx);
                                                });
                                            }
                                        }
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        Button::new("btn-sync-attachments")
                            .child(match &file_status {
                                FileSyncStatus::Syncing => Icon::new(IconName::LoaderCircle)
                                    .small()
                                    .text_color(theme.primary),
                                FileSyncStatus::Error(_) => Icon::new(IconName::TriangleAlert)
                                    .small()
                                    .text_color(theme.red_light),
                                _ => Icon::new(IconName::Cloud).small().text_color(theme.muted_foreground),
                            })
                            .ghost()
                            .xsmall()
                            .when_some(file_sync_tooltip, |this, tip| this.tooltip(tip))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let FileSyncStatus::Syncing = file_status {
                                    return;
                                }

                                let app = this.app.clone();
                                let parent = this.parent_view.clone();
                                let lang = this.app.current_language();
                                let handle = window.window_handle();
                                cx.spawn(async move |_, cx| {
                                    let preflight = app.file_library_preflight().await;
                                    match preflight {
                                        services::sync::FileLibraryPreflight::InitializationRequired
                                        | services::sync::FileLibraryPreflight::IdentityMismatch { .. }
                                        | services::sync::FileLibraryPreflight::UnidentifiedRemote
                                        | services::sync::FileLibraryPreflight::Error(_) => {
                                            if let Some(parent) = parent.upgrade() {
                                                parent.update(cx, |window, cx| {
                                                    window.open_file_library_action(preflight, cx);
                                                });
                                            }
                                        }
                                        _ => {
                                            // 手动触发：锁冲突必须让用户知道请求未启动
                                            if app.perform_file_only_sync().await
                                                == SyncRunOutcome::SkippedBusy
                                            {
                                                let _ = cx.update_window(handle, |_, _, cx| {
                                                    crate::ui::notification::show_notification(
                                                        crate::ui::notification::NotificationType::Warning,
                                                        t(I18nKey::SyncSkippedBusy, lang),
                                                        cx,
                                                    );
                                                });
                                            }
                                        }
                                    }
                                    anyhow::Ok(())
                                })
                                .detach();
                                cx.notify();
                            })),
                    ),
            )
    }
}

/// 文件同步摘要的紧凑单行展示：最近时间 + 主要计数（恒显）
/// + 扩展计数（仅非零时出现）。纯函数，便于测试。
fn format_file_sync_summary(view: &FileSyncSummaryView, lang: i18n::Language) -> String {
    use chrono::TimeZone;
    let time = chrono::Local
        .timestamp_opt(view.updated_at, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "--".to_string());
    let mut parts = vec![format!("{} {}", t(I18nKey::FileSyncLastRun, lang), time)];
    for (key, value) in [
        (I18nKey::SyncUploaded, view.uploaded),
        (I18nKey::SyncDownloaded, view.downloaded),
        (I18nKey::SyncDeleted, view.deleted),
        (I18nKey::SyncFailures, view.failures),
    ] {
        parts.push(format!("{} {value}", t(key, lang)));
    }
    for (key, value) in [
        (I18nKey::SyncWaiting, view.waiting),
        (I18nKey::SyncPendingDownload, view.pending_download),
        (I18nKey::SyncConflicts, view.conflicts),
        (I18nKey::SyncUnknownDivergence, view.unknown_divergence),
        (I18nKey::SyncUnrecoverable, view.unrecoverable_missing),
    ] {
        if value > 0 {
            parts.push(format!("{} {value}", t(key, lang)));
        }
    }
    parts.join(" · ")
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

#[cfg(test)]
mod tests {
    use super::*;
    use i18n::Language;

    fn summary_view(
        uploaded: usize,
        downloaded: usize,
        deleted: usize,
        failures: usize,
        waiting: usize,
        pending_download: usize,
        conflicts: usize,
        unknown_divergence: usize,
        unrecoverable_missing: usize,
        updated_at: i64,
    ) -> FileSyncSummaryView {
        FileSyncSummaryView {
            uploaded,
            downloaded,
            deleted,
            skipped: 0,
            waiting,
            pending_download,
            unrecoverable_missing,
            conflicts,
            unknown_divergence,
            failures,
            state: FileSyncStatus::Complete,
            run_id: "run-1".to_string(),
            file_library_id: None,
            updated_at,
        }
    }

    #[test]
    fn file_sync_summary_always_shows_primary_counts() {
        let view = summary_view(2, 1, 0, 1, 0, 0, 0, 0, 0, 1_700_000_000);
        let out = format_file_sync_summary(&view, Language::En);
        assert!(out.starts_with("Last file sync"), "got: {out}");
        assert!(out.contains("Uploaded 2"), "got: {out}");
        assert!(out.contains("Downloaded 1"), "got: {out}");
        assert!(out.contains("Deleted 0"), "got: {out}");
        assert!(out.contains("Failed 1"), "got: {out}");
        assert!(!out.contains("Waiting"), "got: {out}");
        assert!(!out.contains("Pending download"), "got: {out}");
        assert!(!out.contains("Version Conflicts"), "got: {out}");
        assert!(!out.contains("Divergence"), "got: {out}");
        assert!(!out.contains("Unrecoverable"), "got: {out}");
    }

    #[test]
    fn file_sync_summary_shows_extended_counts_only_when_nonzero() {
        let view = summary_view(1, 0, 0, 0, 1, 2, 3, 4, 5, 1_700_000_000);
        let out = format_file_sync_summary(&view, Language::En);
        assert!(out.contains("Waiting 1"), "got: {out}");
        assert!(out.contains("Pending download 2"), "got: {out}");
        assert!(out.contains("Version Conflicts 3"), "got: {out}");
        assert!(out.contains("Divergence 4"), "got: {out}");
        assert!(out.contains("Unrecoverable 5"), "got: {out}");
    }
}
