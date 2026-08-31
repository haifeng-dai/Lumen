use crate::app_state::ui::UiState;
use crate::ui::actions::{
    AddSourceArxiv, AddSourceBibtex, AddSourceDblp, AddSourceDoi, AddSourceManual,
    AddSourceOpenalex, AddSubscription, DuplicateSearch, EmptyTrash,
};
use crate::ui::dialogs::FetchMode;
use crate::ui::views::settings::SettingsTab;
use components::{add_drag_behavior, make_window_controls};
use gpui::{DragMoveEvent, FontWeight, MouseButton, Window, div, rems};
use gpui_component::{ActiveTheme, h_flex, v_flex};
use i18n::{I18nKey, t};
use services::query::data::AppViewMode;

use super::*;

impl MainWindow {
    fn render_sidebar_tab_bar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let ui = cx.global::<UiState>();
        let view_mode = ui.view_mode;
        let lang = self.app.current_language();

        h_flex()
            .px_5()
            .pt(rems(0.5))
            .pb_3()
            .gap_4()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .child(
                div()
                    .id("tab-library")
                    .cursor_pointer()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(if view_mode == AppViewMode::Library {
                        cx.theme().sidebar_foreground
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(t(I18nKey::Library, lang))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_view_mode(AppViewMode::Library, cx);
                    })),
            )
            .child(
                div()
                    .id("tab-subscription")
                    .cursor_pointer()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(if view_mode == AppViewMode::Subscription {
                        cx.theme().sidebar_foreground
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(t(I18nKey::Subscription, lang))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_view_mode(AppViewMode::Subscription, cx);
                    })),
            )
            .child({
                let spacer = div().id("sidebar-tab-drag-area").h_full().flex_grow(1.0);
                add_drag_behavior(spacer, window, cx)
            })
    }

    fn render_main_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let ui_state = cx.global::<UiState>();
        let view_mode = ui_state.view_mode;
        let has_selected_id = if view_mode == AppViewMode::Library {
            !ui_state.selected_literature_ids.is_empty()
        } else {
            !ui_state.selected_feed_item_ids.is_empty()
        };
        let window_width = self.current_window_width;
        let left_width = self.left_width.clamp(
            window_width * SIDEBAR_MIN_RATIO,
            window_width * SIDEBAR_MAX_RATIO,
        );
        let right_width = if has_selected_id {
            self.right_width.clamp(
                window_width * SIDEBAR_MIN_RATIO,
                window_width * SIDEBAR_MAX_RATIO,
            )
        } else {
            self.right_width
        };

        div()
            .flex()
            .flex_row()
            .flex_grow(1.0)
            .h_0()
            .relative()
            // 1. 左侧边栏
            .child({
                let sidebar = div()
                    .flex()
                    .flex_col()
                    .h_full()
                    .w(left_width)
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .relative();

                #[cfg(target_os = "macos")]
                let sidebar = {
                    let drag_area = div()
                        .id("sidebar-macos-drag-area")
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .h(rems(1.75));
                    sidebar.child(add_drag_behavior(drag_area, window, cx))
                };

                let sidebar = sidebar.when(cfg!(target_os = "macos"), |this| {
                    this.bg(cx.theme().sidebar).pt(rems(1.5))
                });

                sidebar
                    .child(self.render_sidebar_tab_bar(window, cx))
                    .child(if view_mode == AppViewMode::Library {
                        self.literature_panel.clone().into_any_element()
                    } else {
                        self.subscription_panel.clone().into_any_element()
                    })
            })
            // 2. 主区域 — Column 2 (中间列表和其顶部的工具栏)
            .child(
                v_flex()
                    .flex_grow(1.0)
                    .h_full()
                    .relative()
                    .child(
                        self.toolbar_view
                            .update(cx, |tb, cx| tb.render_bar(window, cx)),
                    )
                    .child(div().flex_grow(1.0).h_0().w_full().overflow_hidden().child(
                        if view_mode == AppViewMode::Library {
                            self.literature_list.clone().into_any_element()
                        } else {
                            self.subscription_list.clone().into_any_element()
                        },
                    ))
                    .children(
                        self.toolbar_view
                            .update(cx, |tb, cx| tb.render_dropdowns(cx)),
                    ),
            )
            // 3. 右侧详细面板 — Column 3 (仅在有选中项时显示)
            .when(has_selected_id, |this: gpui::Div| {
                let detail_content = if view_mode == AppViewMode::Library {
                    self.literature_detail.clone().into_any_element()
                } else {
                    self.subscription_detail.clone().into_any_element()
                };

                // 窗口控件
                let win_ctrl_bar = h_flex()
                    .w_full()
                    .h(rems(2.5))
                    .flex_shrink_0()
                    .items_center()
                    .justify_end()
                    .border_b_1()
                    .border_color(cx.theme().background)
                    .pr(rems(1.0))
                    .child(make_window_controls(window, cx));

                this.child(
                    div()
                        .h_full()
                        .w(right_width)
                        .border_l_1()
                        .border_color(cx.theme().border)
                        .child(
                            v_flex()
                                .h_full()
                                .when(cfg!(not(target_os = "macos")), |this| {
                                    this.child(win_ctrl_bar)
                                })
                                .child(div().flex_grow(1.0).h_0().child(detail_content)),
                        ),
                )
            })
            .when(has_selected_id, |this: gpui::Div| {
                this.child(layout::render_right_resizer(right_width, cx))
            })
            // 4. 左侧调节条
            .child(layout::render_left_resizer(left_width, cx))
    }
}

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.current_window_width = window.bounds().size.width;
        self.current_window_height = window.bounds().size.height;

        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .on_drag_move::<DraggedSidebar>(cx.listener(
                |this, event: &DragMoveEvent<DraggedSidebar>, window, cx| {
                    use components::Side;
                    match event.drag(cx).0 {
                        Side::Left => {
                            this.left_width = event
                                .event
                                .position
                                .x
                                .max(window.rem_size() * 9.375)
                                .min(window.rem_size() * 28.125);
                        }
                        Side::Right => {
                            let window_width = this.current_window_width;
                            this.right_width = (window_width - event.event.position.x)
                                .max(window.rem_size() * 9.375)
                                .min(window.rem_size() * 28.125);
                        }
                    }
                    if let Ok(mut state) = this.app.local_state.write() {
                        state.left_sidebar_width = Some(f64::from(f32::from(this.left_width)));
                        state.right_sidebar_width = Some(f64::from(f32::from(this.right_width)));
                    }
                    cx.notify();
                },
            ))
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                this.loading_modal = None;
                this.context_menu = None;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowAbout, _window, cx| {
                this.open_settings_modal(cx, Some(SettingsTab::About));
            }))
            .on_action(cx.listener(|this, _: &ShowSettings, _window, cx| {
                this.open_settings_modal(cx, None);
            }))
            // ── 文献库 / 订阅 上下文菜单 action ──
            .on_action(cx.listener(|this, _: &AddSourceManual, _window, cx| {
                this.open_manual_add_modal(cx);
            }))
            .on_action(cx.listener(|this, _: &AddSourceBibtex, window, cx| {
                this.open_fetch_modal(FetchMode::BibTeX, window, cx);
            }))
            .on_action(cx.listener(|this, _: &AddSourceDoi, window, cx| {
                this.open_fetch_modal(FetchMode::Doi, window, cx);
            }))
            .on_action(cx.listener(|this, _: &AddSourceArxiv, window, cx| {
                this.open_fetch_modal(FetchMode::ArXiv, window, cx);
            }))
            .on_action(cx.listener(|this, _: &AddSourceDblp, window, cx| {
                this.open_fetch_modal(FetchMode::Dblp, window, cx);
            }))
            .on_action(cx.listener(|this, _: &AddSourceOpenalex, window, cx| {
                this.open_fetch_modal(FetchMode::OpenAlex, window, cx);
            }))
            .on_action(cx.listener(|this, _: &DuplicateSearch, window, cx| {
                this.run_duplicate_detection(window, cx);
            }))
            // 添加订阅：以应用内 dialog 弹窗打开（替代原先的独立 OS 窗口）
            .on_action(cx.listener(|this, _: &AddSubscription, window, cx| {
                this.open_add_subscription_modal(window, cx);
            }))
            .on_action(cx.listener(|this, _: &EmptyTrash, _window, cx| {
                this.handle_empty_trash(cx);
            }))
            // 直接渲染主内容区
            .child(self.render_main_content(window, cx).into_any_element())
            // 3. 菜单遮罩
            .children((self.context_menu.is_some()).then(|| {
                div()
                    .absolute()
                    .size_full()
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.context_menu = None;
                            cx.notify();
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, _, _, cx| {
                            this.context_menu = None;
                            cx.notify();
                        }),
                    )
            }))
            // 4. 模态框浮层
            .child(self.toast_overlay.clone())
            .children(
                self.loading_modal
                    .as_ref()
                    .map(|message: &String| modals::render_loading_modal(message.clone(), cx)),
            )
            .children(modals::render_tag_selector(self, window, cx))
            .children(self.render_global_context_menu(cx))
            .children({
                if self.active_popup_count > 0 {
                    log::debug!(
                        "MODAL_DEBUG: render occluding overlay (popup_count={})",
                        self.active_popup_count
                    );
                    Some(div().absolute().size_full().occlude())
                } else {
                    None
                }
            })
            .children(gpui_component::Root::render_dialog_layer(window, cx))
    }
}
