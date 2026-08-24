use crate::app_state::config::ConfigStore;
use crate::app_state::data::{DataStore, DataStoreEvent};
use crate::ui::notification::{NotificationType, show_notification};
use crate::ui::{
    apply_theme,
    components::ToastOverlay,
    views::{
        literature::{LiteratureDetailView, LiteratureListView, LiteraturePanel},
        subscription::{SubscriptionDetailView, SubscriptionListView, SubscriptionPanel},
        toolbar::ToolbarView,
    },
};
use gpui::{AppContext, Entity, KeyBinding, ReadGlobal, Window, px};
use services::notify::RefreshMsg;
use services::{app::MainApp, sync::SyncStatus};
use std::sync::Arc;

use super::*;

impl super::MainWindow {
    pub fn new(
        app: Arc<MainApp>,
        data_store: gpui::Entity<DataStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let self_handle: gpui::AnyWindowHandle = window.window_handle();
        // 同步初始主题
        let config = ConfigStore::global(cx).inner.clone();
        let (theme_mode, theme_style, ui_scale) = (
            config.ui.theme_mode.clone(),
            config.ui.theme_style.clone(),
            config.ui.ui_scale,
        );
        apply_theme(&theme_mode, &theme_style, ui_scale, cx);

        // UiState Global 已在 main.rs 中初始化，这里跳过
        let this_weak = cx.entity().downgrade();

        let literature_panel =
            cx.new(|_| LiteraturePanel::new(app.clone(), data_store.clone(), this_weak.clone()));

        let subscription_panel =
            cx.new(|_| SubscriptionPanel::new(app.clone(), data_store.clone(), this_weak.clone()));

        let literature_list =
            cx.new(|cx_inner| LiteratureListView::new(app.clone(), data_store.clone(), cx_inner));
        literature_list.update(cx, |this, cx| {
            this.register_actions(cx);
            this.set_parent_view(this_weak.clone());
        });

        // 绑定全局或局部快捷键
        let literature_detail =
            cx.new(|_| LiteratureDetailView::new(app.clone(), data_store.clone()));
        literature_detail.update(cx, |this, _| this.set_parent_view(this_weak.clone()));

        let subscription_list =
            cx.new(|cx_inner| SubscriptionListView::new(app.clone(), data_store.clone(), cx_inner));
        subscription_list.update(cx, |this, cx| {
            this.register_actions(cx);
            this.set_parent_view(this_weak.clone());
        });

        let subscription_detail =
            cx.new(|_cx| SubscriptionDetailView::new(app.clone(), data_store.clone()));

        let toolbar_view =
            cx.new(|cx| ToolbarView::new(app.clone(), data_store.clone(), window, cx));

        // 监听全局主题变化
        cx.observe_global::<gpui_component::Theme>(|_, cx| {
            cx.notify();
        })
        .detach();

        // 监听配置变更（主题/语言/缩放等）
        cx.observe_global::<ConfigStore>(|_, cx| {
            cx.notify();
        })
        .detach();

        // 订阅 DataStore 领域事件
        let data_store_entity = data_store.clone();
        cx.subscribe(
            &data_store_entity,
            |this, _entity: Entity<DataStore>, event: &DataStoreEvent, cx| match event {
                DataStoreEvent::DataChanged => {
                    this.literature_panel.update(cx, |_, cx| cx.notify());
                    this.subscription_panel.update(cx, |_, cx| cx.notify());
                    this.literature_list.update(cx, |panel, cx| {
                        panel.refresh_visible_literatures(cx);
                        cx.notify();
                    });
                    this.literature_detail.update(cx, |view, cx| {
                        view.reload_notes(cx);
                        cx.notify();
                    });
                    this.subscription_list.update(cx, |panel, cx| {
                        panel.refresh_visible_feed_items(cx);
                        cx.notify();
                    });
                    this.subscription_detail.update(cx, |_, cx| cx.notify());
                    // 通知所有处于激活/载入状态的 PDF 标签页重新加载笔记与会话
                    if let Some(ref weak_ctrl) = this.pdf_window_controller
                        && let Some(controller) = weak_ctrl.upgrade()
                    {
                        controller.update(cx, |ctrl, cx| {
                            ctrl.reload_all_pdf_tabs(cx);
                        });
                    }
                }
            },
        )
        .detach();

        // 广播通道（桥接 MainApp 非 GPUI 上下文 → 所有窗口）
        let (tx, _) = tokio::sync::broadcast::channel::<RefreshMsg>(32);
        let mut rx = tx.subscribe();
        *app.refresh_tx.lock().unwrap() = Some(tx);
        let data_store_for_spawn = data_store.clone();
        let this_weak: gpui::WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(move |_: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let mut cx = cx.clone();
            let this_weak = this_weak.clone();
            async move {
                loop {
                    match rx.recv().await {
                        Ok(RefreshMsg::DataChanged) => {
                            cx.update(|cx| {
                                data_store_for_spawn.update(cx, |store, cx| {
                                    if let Err(e) = store.refresh_from_db(cx) {
                                        log::error!("DataStore: bridge refresh_from_db 失败: {e}");
                                    }
                                });
                            });
                        }
                        Ok(RefreshMsg::UiChanged) => {
                            let _ = this_weak.update(&mut cx, |this, cx| {
                                cx.notify();
                                // 把后台同步错误桥接为 Toast 弹窗（带去重，避免 UiChanged 反复广播时重复弹）
                                let (meta, attach) = {
                                    let st = this.app.sync_state.lock().unwrap();
                                    (st.sync_status.clone(), st.attachment_sync_status.clone())
                                };
                                match &meta {
                                    SyncStatus::Error(msg)
                                        if this.last_metadata_error.as_deref()
                                            != Some(msg.as_str()) =>
                                    {
                                        show_notification(NotificationType::Error, msg.clone(), cx);
                                        this.last_metadata_error = Some(msg.clone());
                                    }
                                    SyncStatus::Error(_) => {}
                                    _ => this.last_metadata_error = None,
                                }
                                match &attach {
                                    SyncStatus::Error(msg)
                                        if this.last_attach_error.as_deref()
                                            != Some(msg.as_str()) =>
                                    {
                                        show_notification(NotificationType::Error, msg.clone(), cx);
                                        this.last_attach_error = Some(msg.clone());
                                    }
                                    SyncStatus::Error(_) => {}
                                    _ => this.last_attach_error = None,
                                }
                            });
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            log::warn!("RefreshMsg 通道滞后 {n} 条消息");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        })
        .detach();

        // 注册全局快捷键
        cx.bind_keys([KeyBinding::new("escape", Cancel, None)]);

        let (saved_left, saved_right) = if let Ok(state) = app.local_state.read() {
            (state.left_sidebar_width, state.right_sidebar_width)
        } else {
            (None, None)
        };

        let mut main_window = Self {
            app,
            data_store,
            literature_panel,
            subscription_panel,
            literature_list,
            literature_detail,
            subscription_list,
            subscription_detail,
            toolbar_view: toolbar_view.clone(),
            left_width: saved_left.map_or(window.rem_size() * 15.0, |v| {
                px((v as f32).clamp(150.0, 450.0))
            }),
            right_width: saved_right.map_or(window.rem_size() * 25.0, |v| {
                px((v as f32).clamp(150.0, 450.0))
            }),
            current_window_width: window.rem_size() * 75.0,
            current_window_height: window.rem_size() * 50.0,
            loading_modal: None,
            context_menu: None,
            active_popup_count: 0,
            pdf_window_controller: None,
            pdf_window_handle: None,
            tag_selector: None,
            pending_imports: Vec::new(),
            pending_compares: Vec::new(),
            pending_selectors: Vec::new(),
            bounds_subscription: None,
            close_subscription: None,
            self_handle,
            fetch_dialog: None,
            subscription_dialog: None,
            duplicate_dialog: None,
            last_metadata_error: None,
            last_attach_error: None,
            toast_overlay: cx.new(|cx| ToastOverlay::new(window, cx)),
        };

        // 处理工具栏事件
        main_window.handle_toolbar_events(&toolbar_view, window, cx);

        main_window
    }
}
