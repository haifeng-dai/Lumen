use crate::app_state::data::DataStore;
use crate::app_state::ui::UiState;
use crate::ui::views::main_window::{Cancel, MainWindow};
use components::{ArticleCard, IconName};
use gpui::prelude::*;
use gpui::{
    AnyElement, AnyWindowHandle, AppContext, AsyncApp, Entity, FocusHandle, KeyBinding,
    ListAlignment, ListState, MouseButton, SharedString, WeakEntity, Window, actions, div, px,
    rems,
};
use gpui_component::{ActiveTheme, Icon, Theme};
use models::FeedItem;
use parser::normalize::author_full_name;
use services::app::MainApp;
use services::query::data::get_feed_items;
use std::{cell::RefCell, rc::Rc, sync::Arc};

actions!(subscription_list, [SelectAll, DeleteSelected]);

/// 订阅项视图模型
#[derive(Clone)]
pub struct FeedItemViewModel {
    pub item: Arc<FeedItem>,
    pub authors_text: SharedString,
    pub meta_text: SharedString,
}

/// 订阅列表视图
pub struct SubscriptionListView {
    app: Arc<MainApp>,
    data_store: Entity<DataStore>,
    parent_view: WeakEntity<MainWindow>,
    visible_subscriptions: Vec<FeedItemViewModel>,
    /// 上一次点击的订阅项ID，用于范围选择
    last_selected_id: Option<String>,
    /// 焦点句柄
    focus_handle: FocusHandle,

    /// 自动已读定时器ID (用于取消之前的定时器)
    auto_read_timer_id: Rc<RefCell<u64>>,
    /// 列表状态，用于虚拟列表渲染
    list_state: ListState,
}

impl SubscriptionListView {
    pub fn new(app: Arc<MainApp>, data_store: Entity<DataStore>, cx: &mut Context<Self>) -> Self {
        let visible_subscriptions: Vec<FeedItemViewModel> = {
            let ds = data_store.read(cx);
            let ui = cx.global::<UiState>();
            get_feed_items(&ds.feed_items, &ui.selected_feed_id)
                .iter()
                .map(|item| {
                    let all_authors = item
                        .authors
                        .iter()
                        .map(author_full_name)
                        .collect::<Vec<_>>()
                        .join(", ");

                    let mut meta_parts = Vec::new();
                    if let Some(ref journal) = item.journal
                        && !journal.is_empty()
                    {
                        meta_parts.push(journal.clone());
                    } else if let Some(ref publisher) = item.publisher
                        && !publisher.is_empty()
                    {
                        meta_parts.push(publisher.clone());
                    }

                    if let Some(year) = item.year {
                        meta_parts.push(year.to_string());
                    } else if let Some(ref pub_at) = item.published_at
                        && !pub_at.is_empty()
                    {
                        meta_parts.push(pub_at.clone());
                    }

                    let meta_line = meta_parts.join(" · ");

                    FeedItemViewModel {
                        item: (*item).clone(),
                        authors_text: SharedString::from(all_authors),
                        meta_text: SharedString::from(meta_line),
                    }
                })
                .collect()
        };

        let len = visible_subscriptions.len();
        let list_state = ListState::new(len, ListAlignment::Top, px(100.0));

        Self {
            app,
            data_store,
            parent_view: WeakEntity::new_invalid(),
            visible_subscriptions,
            last_selected_id: None,
            focus_handle: cx.focus_handle(),
            auto_read_timer_id: Rc::new(RefCell::new(0)),
            list_state,
        }
    }

    /// 注册 Action 处理
    pub fn register_actions(&self, cx: &mut Context<Self>) {
        cx.bind_keys([
            KeyBinding::new("cmd-a", SelectAll, Some("SubscriptionList")),
            KeyBinding::new("backspace", DeleteSelected, Some("SubscriptionList")),
            KeyBinding::new("delete", DeleteSelected, Some("SubscriptionList")),
        ]);
    }

    /// 全选
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        if self.visible_subscriptions.is_empty() {
            return;
        }

        UiState::update(cx, |state| {
            state.selected_feed_item_ids.clear();
            for vm in &self.visible_subscriptions {
                state.selected_feed_item_ids.insert(vm.item.id.clone());
            }
        });
    }

    /// 删除选中
    pub fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<_> = {
            let ui = cx.global::<crate::app_state::ui::UiState>();
            ui.selected_feed_item_ids.iter().cloned().collect()
        };
        let _ = self.app.delete_selected_feed_items(ids);
        cx.notify();
    }

    pub fn set_parent_view(&mut self, parent: WeakEntity<MainWindow>) {
        self.parent_view = parent;
    }

    pub fn refresh_visible_feed_items(&mut self, cx: &mut Context<Self>) {
        let ds = self.data_store.read(cx);
        let ui = cx.global::<UiState>();
        self.visible_subscriptions = get_feed_items(&ds.feed_items, &ui.selected_feed_id)
            .iter()
            .map(|item| {
                let all_authors = item
                    .authors
                    .iter()
                    .map(author_full_name)
                    .collect::<Vec<_>>()
                    .join(", ");

                let mut meta_parts = Vec::new();
                if let Some(ref journal) = item.journal
                    && !journal.is_empty()
                {
                    meta_parts.push(journal.clone());
                } else if let Some(ref publisher) = item.publisher
                    && !publisher.is_empty()
                {
                    meta_parts.push(publisher.clone());
                }

                if let Some(year) = item.year {
                    meta_parts.push(year.to_string());
                } else if let Some(ref pub_at) = item.published_at
                    && !pub_at.is_empty()
                {
                    meta_parts.push(pub_at.clone());
                }

                let meta_line = meta_parts.join(" · ");

                FeedItemViewModel {
                    item: (*item).clone(),
                    authors_text: SharedString::from(all_authors),
                    meta_text: SharedString::from(meta_line),
                }
            })
            .collect();
        self.list_state.reset(self.visible_subscriptions.len());
    }

    pub fn select_feed_item(
        &mut self,
        sub_id: String,
        window: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        self.last_selected_id = Some(sub_id.clone());
        let parent = self.parent_view.clone();
        let _ = parent.update(cx, |mw, mw_cx| mw.select_feed_item(sub_id.clone(), mw_cx));

        // 检查该条目是否未读，如果是则启动自动已读定时器
        let is_unread = self
            .data_store
            .read(cx)
            .feed_items
            .iter()
            .find(|s| s.id == sub_id)
            .is_some_and(|s| !s.is_read);

        if is_unread {
            self.start_auto_read_timer(sub_id, window, cx);
        } else {
            // 如果已读，取消之前的定时器
            *self.auto_read_timer_id.borrow_mut() += 1;
        }
    }

    /// 启动自动已读定时器 (3秒后自动设置为已读)
    fn start_auto_read_timer(
        &mut self,
        sub_id: String,
        window: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        // 递增定时器ID以取消之前的定时器
        *self.auto_read_timer_id.borrow_mut() += 1;
        let timer_id = *self.auto_read_timer_id.borrow();
        let timer_id_ref = self.auto_read_timer_id.clone();
        let app = self.app.clone();
        let data_store = self.data_store.clone();

        cx.spawn(move |view: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                // 等待3秒
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(3))
                    .await;

                // 检查定时器是否仍然有效 (没有被新的选中操作取消)
                if *timer_id_ref.borrow() != timer_id {
                    return;
                }

                // 检查该条目是否未读
                let should_mark_read = cx.update(|cx| {
                    data_store
                        .read(cx)
                        .feed_items
                        .iter()
                        .find(|s| s.id == sub_id)
                        .is_some_and(|s| !s.is_read)
                });

                if should_mark_read {
                    // 更新数据库和内存中的已读状态
                    let _ = app
                        .feed_service
                        .update_feed_item_read_status(&app.db, &sub_id, true);

                    // 通知UI更新
                    let _ = cx.update_window(window, |_, _, cx| {
                        let _ = view.update(cx, |_this, cx| {
                            cx.notify();
                        });
                    });
                }
            }
        })
        .detach();
    }

    pub fn toggle_feed_item_selection(&mut self, sub_id: String, cx: &mut Context<Self>) {
        self.last_selected_id = Some(sub_id.clone());
        let parent = self.parent_view.clone();
        let _ = parent.update(cx, |mw, mw_cx| mw.toggle_feed_item_selection(sub_id, mw_cx));
    }

    /// 批量选择 (Shift)
    pub fn range_select_feed_item(
        &mut self,
        sub_id: String,
        window: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        let start_id = if let Some(id) = &self.last_selected_id {
            id.clone()
        } else {
            self.select_feed_item(sub_id, window, cx);
            return;
        };

        let start_idx = self
            .visible_subscriptions
            .iter()
            .position(|vm| vm.item.id == start_id);
        let end_idx = self
            .visible_subscriptions
            .iter()
            .position(|vm| vm.item.id == sub_id);

        if let (Some(s), Some(e)) = (start_idx, end_idx) {
            let (min, max) = if s < e { (s, e) } else { (e, s) };
            let ids: Vec<String> = (min..=max)
                .map(|i| self.visible_subscriptions[i].item.id.clone())
                .collect();
            UiState::update(cx, |state| {
                state.selected_feed_item_ids.clear();
                for id in ids {
                    state.selected_feed_item_ids.insert(id);
                }
            });
        } else {
            self.select_feed_item(sub_id, window, cx);
        }
    }

    /// 渲染单个列表项（由 `ListState` 调用）
    fn render_item(&self, ix: usize, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if ix >= self.visible_subscriptions.len() {
            return div().into_any_element();
        }

        let vm = &self.visible_subscriptions[ix];
        let is_selected = cx
            .global::<UiState>()
            .selected_feed_item_ids
            .contains(&vm.item.id);

        let theme = cx.theme().clone();
        let view = cx.entity().clone();
        let focus_handle = self.focus_handle.clone();

        Self::render_subscription_item(vm, view, is_selected, theme, focus_handle)
            .into_any_element()
    }
}

impl Render for SubscriptionListView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity().clone();
        let theme = cx.theme().clone();

        div()
            .relative() // 重要:设置为relative以便菜单使用absolute定位
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .bg(theme.background)
                    .track_focus(&self.focus_handle)
                    .key_context("SubscriptionList")
                    .on_action(cx.listener(|this: &mut Self, _: &SelectAll, _, cx| {
                        this.select_all(cx);
                    }))
                    .on_action(cx.listener(|this: &mut Self, _: &DeleteSelected, _, cx| {
                        this.delete_selected(cx);
                    }))
                    .on_action(cx.listener(|_, _: &Cancel, _, cx| {
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            window.focus(&this.focus_handle, cx);
                        }),
                    )
                    .child(
                        gpui::list(self.list_state.clone(), move |ix, window, cx| {
                            view.update(cx, |this, cx| this.render_item(ix, window, cx))
                        })
                        .size_full()
                        .flex_grow(1.0)
                        .into_any_element(),
                    ),
            )
    }
}

impl SubscriptionListView {
    fn render_subscription_item(
        vm: &FeedItemViewModel,
        view: Entity<Self>,
        is_selected: bool,
        theme: Theme,
        focus_handle: FocusHandle,
    ) -> impl IntoElement {
        let item = &vm.item;
        let is_unread = !item.is_read;
        let item_id: SharedString = item.id.clone().into();
        let title = item.title.clone();
        let all_authors = vm.authors_text.clone();
        let meta_text = vm.meta_text.clone();

        let view_click = view.clone();
        let view_right_click = view.clone();
        let item_id_right = item.id.clone();
        let focus_handle_right = focus_handle.clone();
        let focus_handle_click = focus_handle.clone();

        div()
            .w_full()
            .on_mouse_down(
                MouseButton::Right,
                move |event, window: &mut Window, app| {
                    let id = item_id_right.clone();
                    let focus_handle = focus_handle_right.clone();
                    let position = event.position;

                    let handle = window.window_handle();
                    view_right_click.update(app, move |this, cx| {
                        window.focus(&focus_handle, cx);
                        // 如果右键点击的项未被选中，则选中它（单选）
                        if !is_selected {
                            this.select_feed_item(id.clone(), handle, cx);
                        }

                        // 调用MainWindow的show_context_menu
                        if let Some(parent) = this.parent_view.upgrade() {
                            parent.update(cx, |p, cx| {
                                p.show_context_menu(
                                    position,
                                    crate::ui::views::main_window::ContextMenuType::SubscriptionItem(id),
                                    window,
                                    cx,
                                );
                            });
                        }
                        cx.notify();
                    });
                },
            )
            .child({
                let mut card = ArticleCard::new(item_id.clone())
                    .title(title)
                    .authors(all_authors)
                    .meta(meta_text)
                    .selected(is_selected)
                    .bold(is_unread)
                    .status_pill(if is_unread {
                        Some(gpui::rgb(0xef4444).into())
                    } else {
                        None
                    });

                if item.is_added_to_library {
                    card = card.top_right(
                        Icon::new(IconName::Check)
                            .size(rems(0.75))
                            .flex_none()
                            .text_color(if is_selected {
                                theme.primary_foreground
                            } else {
                                theme.success
                            }),
                    );
                }

                let id = item_id.to_string();
                let focus_handle_click = focus_handle_click.clone();
                card.on_click(move |event, window, app| {
                    let id = id.clone();
                    let focus_handle = focus_handle_click.clone();
                    let handle = window.window_handle();
                    view_click.update(app, move |this, cx| {
                        window.focus(&focus_handle, cx);
                        let cmd = event.modifiers().platform;
                        let shift = event.modifiers().shift;

                        if cmd {
                            this.toggle_feed_item_selection(id, cx);
                        } else if shift {
                            this.range_select_feed_item(id, handle, cx);
                        } else {
                            this.select_feed_item(id, handle, cx);
                        }
                    });
                })
            })
    }
}
