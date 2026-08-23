use crate::app_state::ui::UiState;
use crate::ui::views::toolbar::{ToolbarEvent, ToolbarView};
use gpui::{AppContext, AsyncApp, Entity, Window, prelude::*};
use services::query::data::{SortField, SortOrder};

impl super::MainWindow {
    pub(crate) fn handle_toolbar_events(
        &mut self,
        toolbar_view: &Entity<ToolbarView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this_weak = cx.entity().downgrade();
        let window_handle = window.window_handle();

        cx.subscribe(toolbar_view, move |_, _, event, cx| {
            let event = event.clone();
            let this_weak = this_weak.clone();
            cx.spawn(move |_, cx: &mut AsyncApp| {
                let mut cx = cx.clone();
                let event = event.clone();
                let this_weak = this_weak.clone();
                async move {
                    match event {
                        ToolbarEvent::Search(query) => {
                            let _ = this_weak.update(&mut cx, |this, cx| {
                                this.literature_list.update(cx, |list, cx| {
                                    list.set_search_text(query, cx);
                                });
                            });
                        }
                        ToolbarEvent::OpenManualAdd => {
                            let _ = cx.update_window(window_handle, |_, _window, cx| {
                                if let Some(this) = this_weak.upgrade() {
                                    this.update(cx, |this, cx| {
                                        this.open_manual_add_modal(cx);
                                    });
                                }
                            });
                        }
                        ToolbarEvent::OpenFetch(mode) => {
                            let _ = cx.update_window(window_handle, |_, window, cx| {
                                if let Some(this) = this_weak.upgrade() {
                                    this.update(cx, |this, cx| {
                                        this.open_fetch_modal(mode, window, cx);
                                    });
                                }
                            });
                        }
                        ToolbarEvent::RunDuplicateDetection => {
                            let _ = cx.update_window(window_handle, |_, window, cx| {
                                if let Some(this) = this_weak.upgrade() {
                                    this.update(cx, |this, cx| {
                                        this.run_duplicate_detection(window, cx);
                                    });
                                }
                            });
                        }
                        ToolbarEvent::SortChanged(field, order) => {
                            let _ = this_weak.update(&mut cx, |this, cx| {
                                UiState::update(cx, |state| {
                                    state.sort_field = field;
                                    state.sort_order = order;
                                });

                                if let Ok(mut state) = this.app.local_state.write() {
                                    state.sort_field = Some(match field {
                                        SortField::Title => "Title".to_string(),
                                        SortField::Author => "Author".to_string(),
                                        SortField::Year => "Year".to_string(),
                                        SortField::Journal => "Journal".to_string(),
                                    });
                                    state.sort_asc = matches!(order, SortOrder::Ascending);
                                }

                                this.app.notify_ui_changed();

                                this.literature_list.update(cx, |list, cx| {
                                    list.refresh_visible_literatures(cx);
                                });
                            });
                        }
                        ToolbarEvent::OpenSettings => {
                            let _ = cx.update_window(window_handle, |_, _window, cx| {
                                if let Some(this) = this_weak.upgrade() {
                                    this.update(cx, |this, cx| {
                                        this.open_settings_modal(cx, None);
                                    });
                                }
                            });
                        }
                        ToolbarEvent::AddSubscription => {
                            let _ = cx.update_window(window_handle, |_, window, cx| {
                                if let Some(this) = this_weak.upgrade() {
                                    this.update(cx, |this, cx| {
                                        this.open_add_subscription_modal(window, cx);
                                    });
                                }
                            });
                        }
                    }
                }
            })
            .detach();
        })
        .detach();
    }
}
