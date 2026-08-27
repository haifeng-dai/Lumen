use crate::app_state::data::DataStore;
use crate::ui::{components::FetchMode, views::main_window::Cancel};
use components::IconName;
use components::add_drag_behavior;
use gpui::{
    AppContext, DismissEvent, Entity, EventEmitter, MouseButton, Pixels, Window, WindowControlArea,
    div, prelude::*, rems,
};
#[cfg(not(windows))]
use gpui_component::InteractiveElementExt;
use gpui_component::{
    ActiveTheme, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{PopupMenu, PopupMenuItem},
};
use i18n::{I18nKey, t};
use services::{
    app::MainApp,
    query::data::{AppViewMode, SortField, SortOrder},
};
use std::sync::Arc;

/// 工具栏事件
#[derive(Clone)]
pub enum ToolbarEvent {
    /// 搜索文本改变
    Search(String),
    /// 打开手动添加对话框
    OpenManualAdd,
    /// 打开抓取对话框
    OpenFetch(FetchMode),
    /// 运行重复项检测
    RunDuplicateDetection,
    /// 排序方式改变
    SortChanged(SortField, SortOrder),
    /// 打开设置
    OpenSettings,
    /// 打开添加订阅对话框（订阅视图下的加号按钮，与右键菜单行为一致）
    AddSubscription,
}

impl EventEmitter<ToolbarEvent> for ToolbarView {}

/// 顶部工具栏视图
pub struct ToolbarView {
    /// 应用控制器
    pub app: Arc<MainApp>,
    pub data_store: Entity<DataStore>,
    /// 搜索输入状态
    search_input: Entity<InputState>,
    /// 排序菜单（PopupMenu 实现）
    sort_menu: Option<Entity<PopupMenu>>,
    /// 添加文献菜单（PopupMenu 实现）
    pub add_menu: Option<Entity<PopupMenu>>,
}

impl ToolbarView {
    pub fn new(
        app: Arc<MainApp>,
        data_store: Entity<DataStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let lang = app.current_language();
        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t(I18nKey::SearchBoxPlaceholder, lang))
        });

        // 订阅搜索输入事件
        cx.subscribe(
            &search_input,
            |_, input_state: Entity<InputState>, event, cx| {
                if let InputEvent::Change = event {
                    let query = input_state.read(cx).text().to_string();
                    cx.emit(ToolbarEvent::Search(query));
                }
            },
        )
        .detach();

        Self {
            app,
            data_store,
            search_input,
            sort_menu: None,
            add_menu: None,
        }
    }

    fn toggle_sort_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sort_menu.is_some() {
            self.sort_menu = None;
            cx.notify();
            return;
        }

        let lang = self.app.current_language();
        let view_weak = cx.entity().downgrade();

        let ui = cx.global::<crate::app_state::ui::UiState>();
        let (current_field, current_order) = (ui.sort_field, ui.sort_order);
        let _ = ui;

        let menu = PopupMenu::build(window, cx, move |mut menu, window, _cx| {
            let width: Pixels = rems(6.25).to_pixels(window.rem_size());
            menu = menu.min_w(width);

            menu = menu.label(t(I18nKey::SortBy, lang));
            menu = menu.item(
                PopupMenuItem::new(t(I18nKey::SortByTitle, lang))
                    .checked(current_field == SortField::Title)
                    .on_click({
                        let view_weak = view_weak.clone();
                        move |_, _, cx| {
                            if let Some(view) = view_weak.upgrade() {
                                view.update(cx, |this, cx| {
                                    cx.emit(ToolbarEvent::SortChanged(
                                        SortField::Title,
                                        current_order,
                                    ));
                                    this.sort_menu = None;
                                    cx.notify();
                                });
                            }
                        }
                    }),
            );
            menu = menu.item(
                PopupMenuItem::new(t(I18nKey::SortByAuthor, lang))
                    .checked(current_field == SortField::Author)
                    .on_click({
                        let view_weak = view_weak.clone();
                        move |_, _, cx| {
                            if let Some(view) = view_weak.upgrade() {
                                view.update(cx, |this, cx| {
                                    cx.emit(ToolbarEvent::SortChanged(
                                        SortField::Author,
                                        current_order,
                                    ));
                                    this.sort_menu = None;
                                    cx.notify();
                                });
                            }
                        }
                    }),
            );
            menu = menu.item(
                PopupMenuItem::new(t(I18nKey::SortByYear, lang))
                    .checked(current_field == SortField::Year)
                    .on_click({
                        let view_weak = view_weak.clone();
                        move |_, _, cx| {
                            if let Some(view) = view_weak.upgrade() {
                                view.update(cx, |this, cx| {
                                    cx.emit(ToolbarEvent::SortChanged(
                                        SortField::Year,
                                        current_order,
                                    ));
                                    this.sort_menu = None;
                                    cx.notify();
                                });
                            }
                        }
                    }),
            );
            menu = menu.item(
                PopupMenuItem::new(t(I18nKey::SortByJournal, lang))
                    .checked(current_field == SortField::Journal)
                    .on_click({
                        let view_weak = view_weak.clone();
                        move |_, _, cx| {
                            if let Some(view) = view_weak.upgrade() {
                                view.update(cx, |this, cx| {
                                    cx.emit(ToolbarEvent::SortChanged(
                                        SortField::Journal,
                                        current_order,
                                    ));
                                    this.sort_menu = None;
                                    cx.notify();
                                });
                            }
                        }
                    }),
            );
            menu = menu.separator();
            menu = menu.item(
                PopupMenuItem::new(t(I18nKey::SortAscending, lang))
                    .checked(current_order == SortOrder::Ascending)
                    .on_click({
                        let view_weak = view_weak.clone();
                        move |_, _, cx| {
                            if let Some(view) = view_weak.upgrade() {
                                view.update(cx, |this, cx| {
                                    cx.emit(ToolbarEvent::SortChanged(
                                        current_field,
                                        SortOrder::Ascending,
                                    ));
                                    this.sort_menu = None;
                                    cx.notify();
                                });
                            }
                        }
                    }),
            );
            menu = menu.item(
                PopupMenuItem::new(t(I18nKey::SortDescending, lang))
                    .checked(current_order == SortOrder::Descending)
                    .on_click({
                        let view_weak = view_weak.clone();
                        move |_, _, cx| {
                            if let Some(view) = view_weak.upgrade() {
                                view.update(cx, |this, cx| {
                                    cx.emit(ToolbarEvent::SortChanged(
                                        current_field,
                                        SortOrder::Descending,
                                    ));
                                    this.sort_menu = None;
                                    cx.notify();
                                });
                            }
                        }
                    }),
            );
            menu
        });

        cx.subscribe(&menu, |this: &mut ToolbarView, _, _: &DismissEvent, cx| {
            this.sort_menu = None;
            cx.notify();
        })
        .detach();

        self.sort_menu = Some(menu);
        cx.notify();
    }

    fn toggle_add_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.add_menu.is_some() {
            self.add_menu = None;
            cx.notify();
            return;
        }

        let lang = self.app.current_language();
        let view_weak = cx.entity().downgrade();

        let menu = PopupMenu::build(window, cx, move |mut menu, window, _cx| {
            let width: Pixels = rems(7.5).to_pixels(window.rem_size());
            menu = menu.min_w(width);

            menu = menu.item(PopupMenuItem::new(t(I18nKey::ManualAdd, lang)).on_click({
                let view_weak = view_weak.clone();
                move |_, _, cx| {
                    if let Some(view) = view_weak.upgrade() {
                        view.update(cx, |_, cx| {
                            cx.emit(ToolbarEvent::OpenManualAdd);
                        });
                    }
                }
            }));
            menu = menu.separator();
            menu = menu.item(PopupMenuItem::new("BibTeX").on_click({
                let view_weak = view_weak.clone();
                move |_, _, cx| {
                    if let Some(view) = view_weak.upgrade() {
                        view.update(cx, |_, cx| {
                            cx.emit(ToolbarEvent::OpenFetch(FetchMode::BibTeX));
                        });
                    }
                }
            }));
            menu = menu.item(PopupMenuItem::new("DOI").on_click({
                let view_weak = view_weak.clone();
                move |_, _, cx| {
                    if let Some(view) = view_weak.upgrade() {
                        view.update(cx, |_, cx| {
                            cx.emit(ToolbarEvent::OpenFetch(FetchMode::Doi));
                        });
                    }
                }
            }));
            menu = menu.item(PopupMenuItem::new("ArXiv").on_click({
                let view_weak = view_weak.clone();
                move |_, _, cx| {
                    if let Some(view) = view_weak.upgrade() {
                        view.update(cx, |_, cx| {
                            cx.emit(ToolbarEvent::OpenFetch(FetchMode::ArXiv));
                        });
                    }
                }
            }));
            menu = menu.item(PopupMenuItem::new("DBLP").on_click({
                let view_weak = view_weak.clone();
                move |_, _, cx| {
                    if let Some(view) = view_weak.upgrade() {
                        view.update(cx, |_, cx| {
                            cx.emit(ToolbarEvent::OpenFetch(FetchMode::Dblp));
                        });
                    }
                }
            }));
            menu = menu.item(PopupMenuItem::new("OpenAlex").on_click({
                let view_weak = view_weak.clone();
                move |_, _, cx| {
                    if let Some(view) = view_weak.upgrade() {
                        view.update(cx, |_, cx| {
                            cx.emit(ToolbarEvent::OpenFetch(FetchMode::OpenAlex));
                        });
                    }
                }
            }));
            menu
        });

        cx.subscribe(&menu, |this: &mut ToolbarView, _, _: &DismissEvent, cx| {
            this.add_menu = None;
            cx.notify();
        })
        .detach();

        self.add_menu = Some(menu);
        cx.notify();
    }

    fn render_sort_menu(&self, _cx: &mut Context<Self>) -> Option<impl IntoElement> {
        self.sort_menu.as_ref().map(|menu| {
            div()
                .absolute()
                .top(rems(2.25))
                .right(rems(8.5))
                .child(menu.clone())
        })
    }

    fn render_add_menu(&self, _cx: &mut Context<Self>) -> Option<impl IntoElement> {
        self.add_menu.as_ref().map(|menu| {
            div()
                .absolute()
                .top(rems(2.25))
                .right(rems(3.5))
                .child(menu.clone())
        })
    }

    /// 工具栏横条（不含下拉菜单）
    pub fn render_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let (view_mode, has_selected_id) = {
            let ui = cx.global::<crate::app_state::ui::UiState>();
            let has_selected_id = if ui.view_mode == AppViewMode::Library {
                !ui.selected_literature_ids.is_empty()
            } else {
                !ui.selected_feed_item_ids.is_empty()
            };
            (ui.view_mode, has_selected_id)
        };

        div()
            .id("toolbar")
            .w_full()
            .h(rems(2.5))
            .flex_shrink_0()
            .on_action(cx.listener(|_, _: &Cancel, _, cx| {
                cx.notify();
            }))
            .child(
                h_flex()
                    .size_full()
                    .bg(cx.theme().background)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .px_4()
                    .items_center()
                    .child(
                        h_flex().w(rems(18.75)).child(
                            div()
                                .id("search-input-wrapper")
                                .flex_grow(1.0)
                                .h_full()
                                .bg(cx.theme().background)
                                .child(
                                    Input::new(&self.search_input)
                                        .w_full()
                                        .bg(cx.theme().background),
                                ),
                        ),
                    )
                    .child({
                        let spacer = div().id("toolbar-spacer").flex_grow(1.0).h_full();
                        #[cfg(not(windows))]
                        let spacer = spacer.on_double_click(|_, window, _| window.zoom_window());
                        add_drag_behavior(spacer, window, cx)
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .when(view_mode == AppViewMode::Library, |this| {
                                this.child(
                                    Button::new("sort-trigger")
                                        .icon(IconName::ArrowUpDown)
                                        .ghost()
                                        .h(rems(1.5))
                                        .w(rems(1.5))
                                        .selected(self.sort_menu.is_some())
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.toggle_sort_menu(window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("find-duplicates-trigger")
                                        .icon(IconName::Clear)
                                        .ghost()
                                        .h(rems(1.5))
                                        .w(rems(1.5))
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .on_click(cx.listener(|_this, _, _, cx| {
                                            cx.emit(ToolbarEvent::RunDuplicateDetection);
                                        })),
                                )
                            })
                            .child(
                                Button::new("add-literature")
                                    .icon(IconName::Add)
                                    .ghost()
                                    .h(rems(1.5))
                                    .w(rems(1.5))
                                    .selected(self.add_menu.is_some())
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if view_mode == AppViewMode::Subscription {
                                            cx.emit(ToolbarEvent::AddSubscription);
                                        } else {
                                            this.toggle_add_menu(window, cx);
                                        }
                                    })),
                            )
                            .child(
                                Button::new("open-settings")
                                    .icon(IconName::Settings)
                                    .ghost()
                                    .h(rems(1.5))
                                    .w(rems(1.5))
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.emit(ToolbarEvent::OpenSettings);
                                    })),
                            )
                            // 窗口控件（详情栏关闭时显示，macOS 使用原生红绿灯）
                            .when(!has_selected_id && cfg!(not(target_os = "macos")), |this| {
                                let theme = cx.theme().clone();
                                this.child(
                                    h_flex()
                                        .gap_0()
                                        .items_center()
                                        .child(
                                            div()
                                                .id("win-minimize")
                                                .flex()
                                                .w(rems(1.5))
                                                .h(rems(1.5))
                                                .flex_shrink_0()
                                                .justify_center()
                                                .items_center()
                                                .text_color(theme.foreground)
                                                .hover(|style| {
                                                    style
                                                        .bg(theme.secondary_hover)
                                                        .text_color(theme.secondary_foreground)
                                                })
                                                .when(cfg!(windows), |this| {
                                                    this.window_control_area(WindowControlArea::Min)
                                                })
                                                .when(cfg!(not(windows)), |this| {
                                                    this.on_mouse_down(
                                                        MouseButton::Left,
                                                        |_, window, cx| {
                                                            if cfg!(target_os = "linux") {
                                                                window.prevent_default();
                                                            }
                                                            cx.stop_propagation();
                                                        },
                                                    )
                                                    .on_click(|_, window, _| {
                                                        window.minimize_window()
                                                    })
                                                })
                                                .child(
                                                    gpui_component::Icon::new(
                                                        gpui_component::IconName::WindowMinimize,
                                                    )
                                                    .small(),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .id("win-maximize")
                                                .flex()
                                                .w(rems(1.5))
                                                .h(rems(1.5))
                                                .flex_shrink_0()
                                                .justify_center()
                                                .items_center()
                                                .text_color(theme.foreground)
                                                .hover(|style| {
                                                    style
                                                        .bg(theme.secondary_hover)
                                                        .text_color(theme.secondary_foreground)
                                                })
                                                .when(cfg!(windows), |this| {
                                                    this.window_control_area(WindowControlArea::Max)
                                                })
                                                .when(cfg!(not(windows)), |this| {
                                                    this.on_mouse_down(
                                                        MouseButton::Left,
                                                        |_, window, cx| {
                                                            if cfg!(target_os = "linux") {
                                                                window.prevent_default();
                                                            }
                                                            cx.stop_propagation();
                                                        },
                                                    )
                                                    .on_click(|_, window, _| window.zoom_window())
                                                })
                                                .child(
                                                    gpui_component::Icon::new(
                                                        if window.is_maximized() {
                                                            gpui_component::IconName::WindowRestore
                                                        } else {
                                                            gpui_component::IconName::WindowMaximize
                                                        },
                                                    )
                                                    .small(),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .id("win-close")
                                                .flex()
                                                .w(rems(1.5))
                                                .h(rems(1.5))
                                                .flex_shrink_0()
                                                .justify_center()
                                                .items_center()
                                                .hover(|style| {
                                                    style
                                                        .bg(theme.danger)
                                                        .text_color(theme.danger_foreground)
                                                })
                                                .when(cfg!(windows), |this| {
                                                    this.window_control_area(
                                                        WindowControlArea::Close,
                                                    )
                                                })
                                                .when(cfg!(not(windows)), |this| {
                                                    this.on_mouse_down(
                                                        MouseButton::Left,
                                                        |_, window, cx| {
                                                            if cfg!(target_os = "linux") {
                                                                window.prevent_default();
                                                            }
                                                            cx.stop_propagation();
                                                        },
                                                    )
                                                    .on_click(|_, window, _| window.remove_window())
                                                })
                                                .child(
                                                    gpui_component::Icon::new(
                                                        gpui_component::IconName::WindowClose,
                                                    )
                                                    .small(),
                                                ),
                                        ),
                                )
                            })
                            .when(has_selected_id, |this| {
                                this.child(
                                    Button::new("close-details-panel")
                                        .icon(IconName::PanelRight)
                                        .ghost()
                                        .h(rems(1.5))
                                        .w(rems(1.5))
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .on_click(cx.listener(|_, _, _, cx| {
                                            crate::app_state::ui::UiState::update(cx, |state| {
                                                state.selected_literature_ids.clear();
                                                state.selected_feed_item_ids.clear();
                                            });
                                        })),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }

    /// 下拉菜单（需由父级在内容区之后渲染，确保覆盖内容区）
    pub fn render_dropdowns(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let mut children = Vec::new();
        if let Some(menu) = self.render_sort_menu(cx) {
            children.push(menu.into_any_element());
        }
        if let Some(menu) = self.render_add_menu(cx) {
            children.push(menu.into_any_element());
        }
        children
    }
}

impl Render for ToolbarView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut children: Vec<gpui::AnyElement> =
            vec![self.render_bar(window, cx).into_any_element()];
        children.extend(self.render_dropdowns(cx));
        div().relative().w_full().h(rems(2.5)).children(children)
    }
}
