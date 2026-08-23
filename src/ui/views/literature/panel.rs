use super::panel_render::{FolderTreeEntry, StaticItemProps};
use crate::app_state::data::DataStore;
use crate::ui::views::main_window::{ContextMenuType, MainWindow};
use gpui::prelude::*;
use services::app::MainApp;

use gpui::{
    AppContext, Entity, MouseButton, MouseDownEvent, SharedString, UniformListScrollHandle,
    WeakEntity, Window, div, rems,
};
use gpui_component::input::InputEvent;
use gpui_component::{Theme, h_flex, input::InputState};
use i18n::{I18nKey, t};
use log::{debug, info, warn};
use models::{Folder, Tag};
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

pub struct LiteraturePanel {
    pub(crate) app: Arc<MainApp>,
    pub(crate) data_store: Entity<DataStore>,
    /// 正在重命名的文件夹ID和输入框状态
    pub(crate) renaming: Option<(String, Entity<InputState>)>,
    /// 正在重命名的标签ID和输入框状态
    pub(crate) tag_renaming: Option<(String, Entity<InputState>)>,
    /// 父视图引用，用于调用 `MainWindow` 的菜单
    pub(crate) parent_view: WeakEntity<MainWindow>,
    /// 文件夹树虚拟滚动控制
    pub(crate) folder_list_scroll_handle: UniformListScrollHandle,
}

impl LiteraturePanel {
    pub fn new(
        app: Arc<MainApp>,
        data_store: Entity<DataStore>,
        parent_view: WeakEntity<MainWindow>,
    ) -> Self {
        debug!("侧栏面板: 初始化");

        Self {
            app,
            data_store,
            renaming: None,
            tag_renaming: None,
            parent_view,
            folder_list_scroll_handle: UniformListScrollHandle::new(),
        }
    }

    pub fn select_folder(&mut self, folder_id: String, cx: &mut Context<Self>) {
        debug!("侧栏: 选中文件夹 '{}'", folder_id);
        let parent = self.parent_view.clone();
        let _ = parent.update(cx, |mw, mw_cx| mw.select_folder(folder_id, mw_cx));
    }

    pub fn select_tag(&mut self, tag_id: String, cx: &mut Context<Self>) {
        debug!("侧栏: 选中标签 '{}'", tag_id);
        let parent = self.parent_view.clone();
        let _ = parent.update(cx, |mw, mw_cx| mw.select_tag(tag_id, mw_cx));
    }

    pub(crate) fn toggle_folder_expansion(&mut self, folder_id: String) {
        if let Ok(mut state) = self.app.local_state.write() {
            if state.expanded_folder_ids.contains(&folder_id) {
                state.expanded_folder_ids.remove(&folder_id);
            } else {
                state.expanded_folder_ids.insert(folder_id);
            }
        }
    }

    pub fn start_rename(
        &mut self,
        id: String,
        is_new: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current_name = if is_new {
            String::new()
        } else {
            let data = self.data_store.read(cx);
            data.folders
                .iter()
                .find(|f| f.id == id)
                .map(|f| f.name.clone())
                .unwrap_or_default()
        };

        let lang = self.app.current_language();
        let input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t(I18nKey::FolderNamePlaceholder, lang))
                .default_value(current_name)
        });

        input_state.update(cx, |state: &mut InputState, cx| {
            state.focus(window, cx);
        });

        cx.subscribe(&input_state, {
            let folder_id = id.clone();
            move |this, input_state: Entity<InputState>, event, cx| {
                if let InputEvent::PressEnter { .. } | InputEvent::Blur = event {
                    let new_name = input_state.read(cx).text().to_string();
                    let new_name = new_name.trim();

                    if new_name.is_empty() {
                        if is_new {
                            let _ = this.app.delete_folder(&folder_id);
                        }
                    } else {
                        let _ = this.app.rename_folder(&folder_id, new_name.to_string());
                    }
                    this.renaming = None;
                    cx.notify();
                }
            }
        })
        .detach();

        self.renaming = Some((id, input_state));
        cx.notify();
    }

    pub fn add_folder(
        &mut self,
        parent_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pid) = &parent_id
            && let Ok(mut state) = self.app.local_state.write()
        {
            state.expanded_folder_ids.insert(pid.clone());
        }

        let new_id = Uuid::new_v4().to_string();
        info!("侧栏: 新建文件夹 (parent={:?}, id={})", parent_id, new_id);
        let _ = self.app.add_folder(parent_id, Some(new_id.clone()));
        self.start_rename(new_id, true, window, cx);
        cx.notify();
    }

    pub fn delete_folder(&mut self, id: String, cx: &mut Context<Self>) {
        info!("侧栏: 删除文件夹 (id={})", id);
        let _ = self.app.delete_folder(&id);
        cx.notify();
    }

    pub fn start_tag_rename(
        &mut self,
        id: String,
        is_new: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        info!("UI: 启动标签重命名交互 (ID: {id})");
        let (current_name, current_color) = {
            let data = self.data_store.read(cx);
            let fallback_color = data
                .tags
                .first()
                .map(|(t, _)| t.color.clone())
                .unwrap_or_default();
            data.tags
                .iter()
                .find(|(t, _)| t.id == id)
                .map(|(t, _)| (t.name.clone(), t.color.clone()))
                .unwrap_or_else(|| {
                    warn!("UI: 在内存中未找到待重命名的标签 (ID: {id})");
                    (String::new(), fallback_color)
                })
        };

        let lang = self.app.current_language();
        let input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t(I18nKey::TagNamePlaceholder, lang))
                .default_value(current_name)
        });

        input_state.update(cx, |state: &mut InputState, cx| {
            state.focus(window, cx);
        });

        cx.subscribe(&input_state, {
            let tag_id = id.clone();
            let color = current_color;
            let app = self.app.clone();
            move |this, input_state: Entity<InputState>, event, cx| {
                if let InputEvent::PressEnter { .. } | InputEvent::Blur = event {
                    let new_name = input_state.read(cx).text().to_string();
                    let new_name = new_name.trim();

                    if !new_name.is_empty() {
                        info!("UI: 提交标签重命名 (ID: {tag_id}, NewName: {new_name})");
                        let _ = app.tag_service.update_tag(
                            &app.db,
                            || app.notify_data_changed(),
                            &tag_id,
                            new_name,
                            &color,
                        );
                    } else if is_new {
                        info!("UI: 新标签名称为空，执行删除 (ID: {tag_id})");
                        let _ = app.tag_service.delete_tag(
                            &app.db,
                            || app.notify_data_changed(),
                            &tag_id,
                        );
                    } else {
                        info!("UI: 标签名称为空，取消重命名");
                    }
                    this.tag_renaming = None;
                    cx.notify();
                }
            }
        })
        .detach();

        self.tag_renaming = Some((id, input_state));
        cx.notify();
    }

    pub fn delete_tag(&mut self, id: String, cx: &mut Context<Self>) {
        info!("UI: 用户请求删除标签 (ID: {id})");
        let _ =
            self.app
                .tag_service
                .delete_tag(&self.app.db, || self.app.notify_data_changed(), &id);
        cx.notify();
    }

    pub(crate) fn render_static_item(
        &self,
        props: StaticItemProps,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id_str = props.id.clone();
        let color = if props.is_selected {
            props.theme.primary_foreground
        } else {
            props.theme.foreground
        };

        div()
            .id(SharedString::from(format!("static-item-{}", props.id)))
            .py_1()
            .px_3()
            .mx_2()
            .flex()
            .items_center()
            .rounded_md()
            .when(props.is_selected, |s| {
                s.bg(props.theme.primary)
                    .text_color(props.theme.primary_foreground)
            })
            .when(!props.is_selected, |s| {
                s.hover(|s| s.bg(props.theme.primary.opacity(0.15)))
            })
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let id = id_str.clone();
                    let parent = self.parent_view.clone();
                    move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        // 1. 先选中
                        this.select_folder(id.clone(), cx);
                        // 2. 如果是回收站，显示菜单
                        if id == "trash"
                            && let Some(mw) = parent.upgrade()
                        {
                            mw.update(cx, |mw, cx| {
                                mw.show_context_menu(
                                    event.position,
                                    ContextMenuType::Folder(Some("trash".to_string())),
                                    window,
                                    cx,
                                );
                            });
                        }
                    }
                }),
            )
            .on_click(cx.listener(move |this: &mut Self, _, _, cx| {
                this.select_folder(id_str.clone(), cx);
                cx.notify();
            }))
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .child(
                        h_flex().gap_2().child((props.icon_builder)(color)).child(
                            div()
                                .text_sm()
                                .text_color(if props.is_selected {
                                    props.theme.primary_foreground
                                } else {
                                    props.theme.foreground
                                })
                                .child(props.text),
                        ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(if props.is_selected {
                                props.theme.primary_foreground
                            } else {
                                props.theme.muted_foreground
                            })
                            .child(props.count),
                    ),
            )
    }

    pub(crate) fn flatten_folders(
        &self,
        folders: &[Arc<Folder>],
        parent_id: Option<String>,
        depth: usize,
        expanded_ids: &HashSet<String>,
    ) -> Vec<FolderTreeEntry> {
        let mut entries = Vec::new();
        let mut children: Vec<_> = folders
            .iter()
            .filter(|f| {
                f.id != "all"
                    && f.id != "uncategorized"
                    && f.id != "trash"
                    && f.parent_id == parent_id
            })
            .collect();
        children.sort_by_key(|a| a.name.to_lowercase());

        for folder in children {
            let id = folder.id.clone();
            let has_children = folders.iter().any(|f| f.parent_id == Some(id.clone()));
            let is_expanded = expanded_ids.contains(&id);
            entries.push(FolderTreeEntry {
                folder: folder.clone(),
                depth,
                is_expanded,
                has_children,
            });
            if is_expanded {
                entries.extend(self.flatten_folders(folders, Some(id), depth + 1, expanded_ids));
            }
        }
        entries
    }
    pub(crate) fn render_tag_item(
        &self,
        tag: &Tag,
        selected_id: Option<&String>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_selected = selected_id == Some(&tag.id);
        let tag_id = tag.id.clone();
        let tag_id_right = tag.id.clone();

        // 解析十六进制颜色
        let color = if tag.color.starts_with('#') && tag.color.len() >= 7 {
            let r = u8::from_str_radix(&tag.color[1..3], 16).unwrap_or(0);
            let g = u8::from_str_radix(&tag.color[3..5], 16).unwrap_or(0);
            let b = u8::from_str_radix(&tag.color[5..7], 16).unwrap_or(0);
            gpui::rgb(u32::from_be_bytes([0, r, g, b])).into()
        } else {
            theme.primary
        };

        let parent = self.parent_view.clone();

        div()
            .id(SharedString::from(format!("tag-item-{}", tag.id)))
            .px_1p5()
            .py_0p5()
            .rounded_sm()
            .flex()
            .items_center()
            .gap_1p5()
            .cursor_pointer()
            .when(is_selected, |s| {
                s.bg(theme.primary).text_color(theme.primary_foreground)
            })
            .when(!is_selected, |s| {
                s.hover(|s| s.bg(theme.primary.opacity(0.15)))
            })
            .on_mouse_down(MouseButton::Right, {
                let tag_id = tag_id_right.clone();
                move |event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    if let Some(mw) = parent.upgrade() {
                        mw.update(cx, |mw, cx| {
                            mw.show_context_menu(
                                event.position,
                                ContextMenuType::Tag(Some(tag_id.clone())),
                                window,
                                cx,
                            );
                        });
                    }
                }
            })
            .on_click(cx.listener({
                let tag_id = tag_id.clone();
                move |this, _, _, cx| {
                    this.select_tag(tag_id.clone(), cx);
                    cx.notify();
                }
            }))
            .child(
                div()
                    .size(rems(0.4375))
                    .rounded_full()
                    .bg(color)
                    .flex_shrink_0(),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(if is_selected {
                        theme.primary_foreground
                    } else {
                        theme.foreground
                    })
                    .child(tag.name.clone()),
            )
    }
}
