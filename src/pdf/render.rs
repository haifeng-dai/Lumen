use ::components::IconName;
use gpui::prelude::*;
use gpui::{
    App, ClipboardItem, Context, DragMoveEvent, FocusHandle, Focusable, KeyDownEvent, ListOffset,
    MouseButton, Render, Window, div, px, rems,
};
use gpui_component::{ActiveTheme, Icon, button::Button, h_flex, label::Label, v_flex};

use i18n::{I18nKey, Language};
use log::{debug, info};

use super::*;

impl Render for PdfReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 在绘图帧开头，物理释放所有由于图片覆盖等延迟的 GPU 纹理资源
        if !self.pending_drop_images.is_empty() {
            let count = self.pending_drop_images.len();
            for img in self.pending_drop_images.drain(..) {
                if let Err(e) = window.drop_image(img) {
                    log::error!("drop_image failed: {e}");
                }
            }
            debug!(
                "PdfReaderView: 延迟释放队列处理完成，释放了 {} 张覆盖纹理",
                count
            );
        }

        let viewport_width = window.viewport_size().width;
        if viewport_width > px(0.0) {
            self.left_sidebar_width = px(self.preferred_left_sidebar_width).clamp(
                px(f32::from(viewport_width) * SIDEBAR_MIN_RATIO),
                px(f32::from(viewport_width) * SIDEBAR_MAX_RATIO),
            );
            self.right_sidebar_width = px(self.preferred_right_sidebar_width).clamp(
                px(f32::from(viewport_width) * SIDEBAR_MIN_RATIO),
                px(f32::from(viewport_width) * SIDEBAR_MAX_RATIO),
            );
        } else {
            self.left_sidebar_width = px(self.preferred_left_sidebar_width);
            self.right_sidebar_width = px(self.preferred_right_sidebar_width);
        }
        let current_rem_size = f32::from(window.rem_size());
        let current_viewport_width = f32::from(window.viewport_size().width);

        // 如果处于自适应模式且可用内容宽度发生变化，则重新计算缩放
        let mut current_content_width = current_viewport_width;
        if self.is_left_sidebar_open {
            current_content_width -= f32::from(self.left_sidebar_width);
        }
        if self.is_right_sidebar_open {
            current_content_width -= f32::from(self.right_sidebar_width);
        }

        if self.fit_to_width_mode && (current_content_width - self.last_content_width).abs() > 1.0 {
            self.last_content_width = current_content_width;
            self.apply_auto_fit(window, cx);
        }

        let get_page_height = |ix: usize, zoom: f32, rem_size: f32| {
            helpers::page_height(&self.page_sizes, ix, zoom, rem_size)
        };

        // 检测缩放或 DPI 变化，重置列表状态以重新计算高度
        if (self.zoom_level - self.last_zoom_level).abs() > 0.001
            || (current_rem_size - self.last_rem_size).abs() > 0.001
        {
            let saved_page = self.current_page;
            let saved_offset_y = self.current_offset_y;

            self.list_state.reset(self.total_pages);
            self.thumbnail_list_state.reset(self.total_pages);

            let px_offset = saved_offset_y * self.zoom_level * current_rem_size;
            self.list_state.scroll_to(ListOffset {
                item_ix: saved_page as usize,
                offset_in_item: px(px_offset),
            });

            self.thumbnail_list_state.scroll_to(ListOffset {
                item_ix: saved_page as usize,
                offset_in_item: px(0.0),
            });

            self.last_zoom_level = self.zoom_level;
            self.last_rem_size = current_rem_size;
        }

        // 执行初始进度恢复
        if self.is_restoring && self.total_pages > 0 {
            let page_index = self.initial_state.page_index as usize;
            if page_index < self.total_pages {
                let px_offset = self.initial_state.offset_y * self.zoom_level * self.last_rem_size;
                self.list_state.scroll_to(ListOffset {
                    item_ix: page_index,
                    offset_in_item: px(px_offset),
                });

                self.current_page = page_index as u16;
                self.current_offset_y = self.initial_state.offset_y;
                self.thumbnail_list_state.scroll_to(ListOffset {
                    item_ix: page_index,
                    offset_in_item: px(0.0),
                });
            }
            self.is_restoring = false;
        } else if self.total_pages > 0 {
            let scroll_top = self.list_state.logical_scroll_top();
            let toolbar_height = rems(TOOLBAR_HEIGHT_REMS).to_pixels(window.rem_size());
            let tab_bar_h = self.tab_bar_offset_px;
            let view_height =
                f32::from(window.viewport_size().height) - tab_bar_h - f32::from(toolbar_height);
            self.search_content_height = view_height;
            // 计算视窗顶部在全局坐标系中的绝对位置
            let mut viewport_top_abs = 0.0;
            for i in 0..scroll_top.item_ix {
                viewport_top_abs += get_page_height(i, self.zoom_level, self.last_rem_size);
            }
            viewport_top_abs += f32::from(scroll_top.offset_in_item);

            // 寻找顶部落在哪个页面
            let mut accumulated_height = 0.0;
            let mut new_page = 0;
            let mut new_offset_y = 0.0;

            for i in 0..self.total_pages {
                let page_h = get_page_height(i, self.zoom_level, self.last_rem_size);
                if accumulated_height + page_h > viewport_top_abs || i == self.total_pages - 1 {
                    new_page = i as u16;
                    let scaled_offset_to_top = viewport_top_abs - accumulated_height;
                    if self.zoom_level > 0.0 && self.last_rem_size > 0.0 {
                        new_offset_y =
                            scaled_offset_to_top / (self.zoom_level * self.last_rem_size);
                    }
                    break;
                }
                accumulated_height += page_h;
            }

            if !self.programmatic_scroll
                && (self.current_page != new_page
                    || (self.current_offset_y - new_offset_y).abs() > 0.01)
            {
                let page_changed = self.current_page != new_page;
                self.current_page = new_page;
                self.current_offset_y = new_offset_y;
                self.request_state_save(cx);

                if page_changed && self.is_left_sidebar_open {
                    let thumbnail_scroll = self.thumbnail_list_state.logical_scroll_top();
                    // 如果当前页面已经对齐在顶部（容差 1px），则不触发强制对齐，防止拉动侧边栏时产生跳变
                    let already_aligned = thumbnail_scroll.item_ix == new_page as usize
                        && f32::from(thumbnail_scroll.offset_in_item).abs() < 1.0;

                    if !already_aligned {
                        let target_page = new_page as usize;
                        cx.on_next_frame(window, move |this, _win, cx| {
                            this.thumbnail_list_state.scroll_to_reveal_item(target_page);
                            cx.notify();
                        });
                    }
                }
            }
            self.programmatic_scroll = false;
        }

        // 页面可见性管理：计算视口范围、淘汰远页、调度渲染
        if self.worker_state == WorkerState::Running {
            // 先读取 window scale_factor / rem_size，确保 refresh 中使用的缩放比例正确
            self.window_scale_factor = window.scale_factor();
            self.last_rem_size = f32::from(window.rem_size());
            self.refresh_page_visibility(window, cx);
            if self.is_left_sidebar_open {
                self.refresh_thumb_visibility(window, cx);
            }
        }

        if let WorkerState::Failed(ref msg) = self.worker_state {
            let msg = msg.clone();
            return div()
                .size_full()
                .bg(cx.theme().background)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .child(
                    v_flex()
                        .gap_4()
                        .items_center()
                        .child(
                            Icon::new(IconName::Close)
                                .size(px(48.0))
                                .text_color(gpui::red()),
                        )
                        .child(
                            Label::new(i18n::t(I18nKey::PdfEngineError, self.language))
                                .text_color(gpui::red()),
                        )
                        .child(
                            Label::new(msg)
                                .text_sm()
                                .text_color(gpui::red().opacity(0.7)),
                        )
                        .child(
                            Button::new("close_error")
                                .label(i18n::t(I18nKey::CloseWindow, self.language))
                                .on_click(|_, window: &mut Window, _| {
                                    window.remove_window();
                                }),
                        ),
                )
                .into_any_element();
        }

        if self.worker_state == WorkerState::Loading {
            return div()
                .size_full()
                .bg(cx.theme().background)
                .child(
                    h_flex()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .child("Loading Document..."),
                )
                .into_any_element();
        }

        if !self.has_focused && self.worker_state == WorkerState::Running {
            self.has_focused = true;
            let handle = self.focus_handle.clone();
            window.on_next_frame(move |window, cx| {
                window.focus(&handle, cx);
            });
        }

        // 如果 scale_factor 发生变化，清除缓存让 refresh_page_visibility 重新调度
        let scale_factor = window.scale_factor();
        if self.worker_state == WorkerState::Running
            && (self.last_render_scale_factor - scale_factor).abs() > 0.01
        {
            self.last_render_scale_factor = scale_factor;
            debug!(
                "mod: 窗口 scale_factor 变更为 {}, 等待 refresh_page_visibility 重新调度",
                scale_factor
            );
            // 清空页面缓存，触发重新渲染
            for img in self.page_images.iter_mut() {
                *img = None;
            }
            for img in self.raw_page_images.iter_mut() {
                *img = None;
            }
            self.page_render_requests_pending.clear();
            // 强制下一帧 refresh_page_visibility 进入调度路径
            self.visible_page_first = usize::MAX;
            self.visible_page_last = 0;
        }

        // 惰性构建浮动工具栏 PopupMenu（需要 &mut Window，在 theme 之前）
        if self.annotation_state.toolbar.is_some() && self.annotation_toolbar_menu.is_none() {
            self.annotation_toolbar_menu = self.build_toolbar_popup_menu(window, cx);
        }
        // 每帧刷新位置（跟随滚动 + 边界避碰）
        if self.annotation_toolbar_menu.is_some()
            && let Some((x, y)) = self.compute_toolbar_screen_pos(window)
        {
            self.annotation_toolbar_menu.as_mut().unwrap().0 = gpui::Point { x, y };
        }

        let theme = cx.theme();

        v_flex()
            .size_full()
            .relative()
            .bg(theme.background)
            .track_focus(&self.focus_handle)
            .on_mouse_move(
                cx.listener(|this, event, window, cx| {
                    this.handle_root_mouse_move(event, window, cx)
                }),
            )
            .on_drag_move::<DraggedSidebar>(cx.listener(
                |this, event: &DragMoveEvent<DraggedSidebar>, window, cx| {
                    let viewport_width = window.viewport_size().width;
                    let min_width = px(f32::from(viewport_width) * SIDEBAR_MIN_RATIO);
                    let max_width = px(f32::from(viewport_width) * SIDEBAR_MAX_RATIO);

                    if event.drag(cx).0 {
                        // 左侧 resizer
                        let current_right_w = if this.is_right_sidebar_open {
                            this.right_sidebar_width
                        } else {
                            px(0.0)
                        };
                        let available_for_left =
                            (viewport_width - current_right_w - px(300.0)).max(min_width);
                        let final_max = max_width.min(available_for_left);

                        this.left_sidebar_width =
                            event.event.position.x.max(min_width).min(final_max);
                        this.preferred_left_sidebar_width = f32::from(this.left_sidebar_width);
                    } else {
                        // 右侧 resizer
                        let current_left_w = if this.is_left_sidebar_open {
                            this.left_sidebar_width
                        } else {
                            px(0.0)
                        };
                        let available_for_right =
                            (viewport_width - current_left_w - px(300.0)).max(min_width);
                        let final_max = max_width.min(available_for_right);

                        this.right_sidebar_width = (viewport_width - event.event.position.x)
                            .max(min_width)
                            .min(final_max);
                        this.preferred_right_sidebar_width = f32::from(this.right_sidebar_width);
                    }
                    this.request_state_save(cx);
                    cx.notify();
                },
            ))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if event.keystroke.key.as_str() == "c"
                    && (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
                {
                    if let Some(ref text) = this.selected_text
                        && !text.is_empty()
                    {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                    }
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _window, cx| {
                    this.handle_root_mouse_up(cx);
                }),
            )
            .child(
                h_flex()
                    .flex_grow(1.0)
                    .h_0()
                    .relative()
                    .when(self.is_left_sidebar_open && !self.hide_sidebars, |this| {
                        this.child(self.render_left_sidebar(window, cx))
                    })
                    .child(
                        v_flex()
                            .flex_grow(1.0)
                            .h_full()
                            .when(!self.hide_toolbar, |this| {
                                this.child(self.render_toolbar(window, cx))
                            })
                            .child(
                                div()
                                    .flex_grow(1.0)
                                    .h_0()
                                    .w_full()
                                    .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                                        this.annotation_context_menu = None;
                                        this.pin_context_menu = None;
                                        this.thumbnail_context_menu = None;
                                        this.annotation_toolbar_menu = None;
                                        this.annotation_state.toolbar = None;
                                        this.selection_start = None;
                                        this.selection_end = None;
                                        this.selected_text = None;
                                        this.annotation_state.note_editor = None;
                                        this.note_input_state = None;
                                        this.note_input_sub = None;
                                        cx.notify();
                                    }))
                                    .child(self.render_main_content(window, cx)),
                            ),
                    )
                    .when(self.is_right_sidebar_open && !self.hide_sidebars, |this| {
                        this.child(self.render_right_sidebar(window, cx))
                    })
                    .when(self.is_left_sidebar_open && !self.hide_sidebars, |this| {
                        this.child(self.render_sidebar_resizer(true, cx))
                    })
                    .when(self.is_right_sidebar_open && !self.hide_sidebars, |this| {
                        this.child(self.render_sidebar_resizer(false, cx))
                    })
                    // 不再使用遮罩层：改为在阅读区容器上挂 capture_any_mouse_down 捕获阶段 handler
                    .when_some(
                        self.annotation_toolbar_menu.as_ref(),
                        |this, (pos, menu)| {
                            this.child(self.render_menu_overlay(
                                *pos,
                                menu.clone(),
                                window,
                                200.0,
                                80.0,
                            ))
                        },
                    )
                    .when_some(
                        self.annotation_context_menu.as_ref(),
                        |this, (pos, menu)| {
                            this.child(self.render_menu_overlay(
                                *pos,
                                menu.clone(),
                                window,
                                180.0,
                                220.0,
                            ))
                        },
                    )
                    .when_some(self.pin_context_menu.as_ref(), |this, (pos, menu)| {
                        this.child(self.render_menu_overlay(
                            *pos,
                            menu.clone(),
                            window,
                            180.0,
                            160.0,
                        ))
                    })
                    .when_some(self.thumbnail_context_menu.as_ref(), |this, (pos, menu)| {
                        this.child(self.render_menu_overlay(
                            *pos,
                            menu.clone(),
                            window,
                            180.0,
                            40.0,
                        ))
                    })
                    .when_some(self.render_note_editor(window, cx), |this, editor| {
                        this.child(editor)
                    }),
            )
            .when(
                self.dragging_pin.is_some() || self.resizing_pin.is_some(),
                |this| {
                    this.child(
                        div()
                            .absolute()
                            .inset_0()
                            .cursor_default()
                            .on_mouse_move(cx.listener(|this, event, window, cx| {
                                this.handle_pin_mouse_move(event, window, cx);
                            }))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.handle_pin_mouse_up(cx);
                                    cx.notify();
                                }),
                            ),
                    )
                },
            )
            .when(
                self.is_dragging_scrollbar
                    || self.is_dragging_thumbnail_scrollbar
                    || self.is_panning,
                |this| {
                    this.child(
                        div()
                            .absolute()
                            .inset_0()
                            .cursor_default()
                            .on_mouse_move(cx.listener(|this, event, window, cx| {
                                this.handle_root_mouse_move(event, window, cx);
                            }))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.handle_root_mouse_up(cx);
                                }),
                            ),
                    )
                },
            )
            .into_any_element()
    }
}

pub(super) fn translate_outlines(
    items: Vec<services::pdf::OutlineItem>,
    lang: Language,
) -> Vec<services::pdf::OutlineItem> {
    let unnamed = i18n::t(I18nKey::UnnamedBookmark, lang);
    items
        .into_iter()
        .map(|mut item| {
            if item.title == "未命名书签" {
                item.title = unnamed.to_string();
            }
            item.children = translate_outlines(item.children, lang);
            item
        })
        .collect()
}

impl Drop for PdfReaderView {
    fn drop(&mut self) {
        info!("PdfReaderView: 视图销毁, 保存阅读状态");
        self.save_current_state(None);
    }
}

impl Focusable for PdfReaderView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[derive(Clone)]
pub struct DraggedSidebar(pub bool); // true if left

impl Render for DraggedSidebar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}
