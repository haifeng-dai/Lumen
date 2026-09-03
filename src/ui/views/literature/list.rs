// ── 1. 标准库导入 ──
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// ── 2. 第三方与框架库导入 ──
use gpui::prelude::*;
use gpui::{
    AnyElement, App, AppContext, Entity, FocusHandle, Hsla, KeyBinding, ListAlignment, ListState,
    MouseButton, MouseDownEvent, SharedString, WeakEntity, Window, actions, div, px, rems,
};
use gpui_component::{ActiveTheme, Icon, Theme, h_flex, v_flex};
use log::{debug, info, warn};

// ── 3. 工作区依赖库导入 (Workspace Crates) ──
use i18n::{I18nKey, Language, LiteratureTypeExt, t};
use models::{Literature, ReadingStatus, Tag};
use parser::normalize::author_full_name;

// ── 4. 本地模块导入 (Crate Internals) ──
use crate::app_state::data::DataStore;
use crate::app_state::ui::UiState;
use crate::ui::views::literature::LiteratureDragInfo;
use crate::ui::views::main_window::{ContextMenuType, MainWindow};
use components::{ArticleCard, IconName};
use services::app::MainApp;
use services::query::data::{get_folder_literatures, search_literatures as search_literatures_fn};

/// Pre-computed view model for a single literature item.
/// This struct holds all data needed to render a row, enabling lock-free rendering.
#[derive(Clone)]
struct LiteratureItemViewModel {
    /// The full literature data
    literature: Arc<Literature>,
    /// Whether this item is currently selected
    is_selected: bool,
    /// Pre-computed tag colors as (name, Hsla) pairs
    tag_colors: Vec<(String, Hsla)>,
    /// Literature IDs to drag (either just this one, or all selected)
    drag_ids: Vec<String>,
    /// Pre-computed author full names
    authors_text: gpui::SharedString,
    /// Pre-computed meta details line
    meta_text: gpui::SharedString,
}

impl LiteratureItemViewModel {
    /// Build view models for all visible literatures from pre-fetched data.
    fn build_all(
        literatures: &[Arc<Literature>],
        tags: &[(Arc<Tag>, usize)],
        language: Language,
        visible_literatures: &[String],
        selected_ids: &HashSet<String>,
        fallback_color: Hsla,
    ) -> Vec<LiteratureItemViewModel> {
        // Pre-compute tag color map once
        let tag_color_map: HashMap<String, Hsla> = tags
            .iter()
            .map(|(tag, _)| {
                let color = Self::parse_hex_color(&tag.color, fallback_color);
                (tag.name.clone(), color)
            })
            .collect();

        // Build a lookup map for quick literature access
        let lit_map: HashMap<&str, Arc<Literature>> = literatures
            .iter()
            .map(|l| (l.id.as_str(), l.clone()))
            .collect();

        visible_literatures
            .iter()
            .filter_map(|lit_id| {
                let literature = lit_map.get(lit_id.as_str())?.clone();
                let is_selected = selected_ids.contains(&literature.id);

                // Pre-compute tag colors for this literature
                let tag_colors: Vec<(String, Hsla)> = literature
                    .tags
                    .iter()
                    .map(|tag_name| {
                        let color = tag_color_map
                            .get(tag_name)
                            .copied()
                            .unwrap_or(fallback_color);
                        (tag_name.clone(), color)
                    })
                    .collect();

                // Compute drag IDs: if selected, drag all selected; otherwise just this one
                let drag_ids = if is_selected {
                    selected_ids.iter().cloned().collect()
                } else {
                    vec![literature.id.clone()]
                };

                // Pre-compute authors text
                let authors_text = literature
                    .authors
                    .iter()
                    .map(author_full_name)
                    .collect::<Vec<_>>()
                    .join(", ");

                // Pre-compute meta details line
                let type_name = t(literature.literature_type.i18n_key(), language);
                let journal = literature
                    .publication
                    .as_ref()
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                let year = literature.year.map(|y| y.to_string()).unwrap_or_default();
                let volume = literature.volume.clone().unwrap_or_default();
                let issue = literature.issue.clone().unwrap_or_default();
                let pages = literature.pages.clone().unwrap_or_default();

                let mut meta_parts = Vec::new();
                if !volume.is_empty() {
                    meta_parts.push(format!("vol. {volume}"));
                }
                if !issue.is_empty() {
                    meta_parts.push(format!("no. {issue}"));
                }
                if !pages.is_empty() {
                    meta_parts.push(format!("pp. {pages}"));
                }
                let meta_line = meta_parts.join(" | ");

                let mut meta_text_parts = Vec::new();
                if !year.is_empty() {
                    meta_text_parts.push(year);
                }
                if !journal.is_empty() {
                    meta_text_parts.push(journal);
                }
                meta_text_parts.push(type_name.to_string());
                if !meta_line.is_empty() {
                    meta_text_parts.push(meta_line);
                }
                let meta_text = meta_text_parts.join(" | ");

                Some(LiteratureItemViewModel {
                    literature,
                    is_selected,
                    tag_colors,
                    drag_ids,
                    authors_text: gpui::SharedString::from(authors_text),
                    meta_text: gpui::SharedString::from(meta_text),
                })
            })
            .collect()
    }

    /// Parse a hex color string (e.g., "#FF0000") into an Hsla color.
    fn parse_hex_color(color_str: &str, fallback: Hsla) -> Hsla {
        if color_str.starts_with('#') && color_str.len() >= 7 {
            let r = u8::from_str_radix(&color_str[1..3], 16).unwrap_or(128);
            let g = u8::from_str_radix(&color_str[3..5], 16).unwrap_or(128);
            let b = u8::from_str_radix(&color_str[5..7], 16).unwrap_or(128);
            gpui::rgb(u32::from_be_bytes([0, r, g, b])).into()
        } else {
            fallback
        }
    }
}

actions!(literature_list, [SelectAll, DeleteSelected]);

/// 中间文献列表视图
pub struct LiteratureListView {
    /// 应用控制器
    app: Arc<MainApp>,
    data_store: Entity<DataStore>,
    /// 搜索文本
    search_text: String,
    /// 文献列表（可能是搜索结果或当前文件夹的文献）
    visible_literatures: Vec<String>, // 存储文献ID
    /// Pre-computed view models for lock-free rendering
    view_models: Vec<LiteratureItemViewModel>,
    /// 父视图弱引用，用于触发全局菜单
    parent_view: Option<WeakEntity<MainWindow>>,
    /// 上一次点击的文献ID，用于范围选择
    last_selected_id: Option<String>,
    /// 焦点句柄
    focus_handle: FocusHandle,
    /// 列表状态，用于虚拟列表渲染
    list_state: ListState,
}

impl LiteratureListView {
    /// 创建新的文献列表视图
    pub fn new(app: Arc<MainApp>, data_store: Entity<DataStore>, cx: &mut Context<Self>) -> Self {
        let (visible_literatures, _selected_ids, view_models) = {
            let ds = data_store.read(cx);
            let ui = cx.global::<UiState>();
            let visible: Vec<String> = get_folder_literatures(
                &ds.literatures,
                &ds.tags,
                &ui.selected_folder_id,
                &ui.selected_tag_id,
                ui.sort_field,
                ui.sort_order,
            )
            .iter()
            .map(|lit| lit.id.clone())
            .collect();
            let sel = ui.selected_literature_ids.clone();
            let lang = app.current_language();
            let vms = LiteratureItemViewModel::build_all(
                &ds.literatures,
                &ds.tags,
                lang,
                &visible,
                &sel,
                cx.theme().muted_foreground,
            );
            (visible, sel, vms)
        };
        let len = visible_literatures.len();
        debug!("文献列表: 初始化 ({} 篇可见)", len);
        let list_state = ListState::new(len, ListAlignment::Top, px(100.0));

        Self {
            app,
            data_store,
            search_text: String::new(),
            visible_literatures,
            view_models,
            parent_view: None,
            last_selected_id: None,
            focus_handle: cx.focus_handle(),
            list_state,
        }
    }

    /// Rebuild view models from current data.
    fn update_view_models(&mut self, cx: &mut Context<Self>) {
        let selected_ids: HashSet<String> = cx
            .global::<UiState>()
            .selected_literature_ids
            .iter()
            .cloned()
            .collect();
        let ds = self.data_store.read(cx);
        let lang = self.app.current_language();
        self.view_models = LiteratureItemViewModel::build_all(
            &ds.literatures,
            &ds.tags,
            lang,
            &self.visible_literatures,
            &selected_ids,
            cx.theme().muted_foreground,
        );
        debug!("文献列表: 重建视图模型 ({} 个)", self.view_models.len());
    }

    /// 注册 Action 处理
    pub fn register_actions(&self, cx: &mut Context<Self>) {
        cx.bind_keys([
            KeyBinding::new("cmd-a", SelectAll, Some("LiteratureList")),
            KeyBinding::new("backspace", DeleteSelected, Some("LiteratureList")),
            KeyBinding::new("delete", DeleteSelected, Some("LiteratureList")),
        ]);
    }

    /// 全选
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        if self.visible_literatures.is_empty() {
            return;
        }

        info!("文献列表: 全选 ({} 篇)", self.visible_literatures.len());
        UiState::update(cx, |state| {
            state.selected_literature_ids.clear();
            for id in &self.visible_literatures {
                state.selected_literature_ids.insert(id.clone());
            }
        });
        self.update_view_models(cx);
    }

    /// 删除选中
    pub fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<_> = {
            let ui = cx.global::<crate::app_state::ui::UiState>();
            ui.selected_literature_ids.iter().cloned().collect()
        };
        info!("文献列表: 删除选中 ({} 篇)", ids.len());
        let _ = self.app.delete_selected_literatures(ids);
        cx.notify();
    }

    /// 设置父视图引用
    pub fn set_parent_view(&mut self, parent: WeakEntity<MainWindow>) {
        self.parent_view = Some(parent);
    }

    /// 设置搜索文本并刷新列表
    pub fn set_search_text(&mut self, text: String, cx: &mut Context<Self>) {
        info!("文献列表: 搜索 '{}'", text);
        self.search_text = text;
        self.refresh_visible_literatures(cx);
        cx.notify();
    }

    /// 刷新可见文献列表（数据同步的核心逻辑）
    pub fn refresh_visible_literatures(&mut self, cx: &mut Context<Self>) {
        let ui = cx.global::<UiState>();
        let ds = self.data_store.read(cx);

        let new_literatures: Vec<String> = if self.search_text.is_empty() {
            get_folder_literatures(
                &ds.literatures,
                &ds.tags,
                &ui.selected_folder_id,
                &ui.selected_tag_id,
                ui.sort_field,
                ui.sort_order,
            )
            .iter()
            .map(|lit| lit.id.clone())
            .collect()
        } else {
            let search_results = search_literatures_fn(
                &ds.literatures,
                &ds.folders,
                &ds.tags,
                &ui.selected_folder_id,
                &ui.selected_tag_id,
                ui.sort_field,
                ui.sort_order,
                &ui.advanced_search_query,
                &self.search_text,
            );
            search_results.iter().map(|lit| lit.id.clone()).collect()
        };

        debug!(
            "文献列表: 刷新可见 ({} 篇, 搜索='{}')",
            new_literatures.len(),
            self.search_text
        );

        // 仅当列表内容发生实质变化时才重置 ListState，避免仅仅因为选中状态变化导致滚动条跳回顶部
        if self.visible_literatures != new_literatures {
            self.visible_literatures = new_literatures;
            self.list_state.reset(self.visible_literatures.len());
        }

        self.update_view_models(cx);
    }

    /// 选中文献
    pub fn select_literature(&mut self, lit_id: String, cx: &mut Context<Self>) {
        debug!("文献列表: 选中 '{}'", lit_id);
        self.last_selected_id = Some(lit_id.clone());
        if let Some(parent) = &self.parent_view {
            let _ = parent.update(cx, |mw, mw_cx| mw.select_literature(lit_id, mw_cx));
        }
        self.update_view_models(cx);
    }

    /// 切换选中文献
    pub fn toggle_literature_selection(&mut self, lit_id: String, cx: &mut Context<Self>) {
        debug!("文献列表: 切换选中 '{}'", lit_id);
        self.last_selected_id = Some(lit_id.clone());
        if let Some(parent) = &self.parent_view {
            let _ = parent.update(cx, |mw, mw_cx| {
                mw.toggle_literature_selection(lit_id, mw_cx)
            });
        }
        self.update_view_models(cx);
    }

    /// 添加到选中
    pub fn add_literature_selection(&mut self, lit_id: String, cx: &mut Context<Self>) {
        self.last_selected_id = Some(lit_id.clone());
        if let Some(parent) = &self.parent_view {
            let _ = parent.update(cx, |mw, mw_cx| mw.add_literature_selection(lit_id, mw_cx));
        }
        self.update_view_models(cx);
    }

    /// 批量选择 (Shift)
    pub fn range_select_literature(&mut self, lit_id: String, cx: &mut Context<Self>) {
        let start_id = if let Some(id) = &self.last_selected_id {
            id.clone()
        } else {
            debug!("文献列表: 范围选中无起始点，回退到单选 '{}'", lit_id);
            self.select_literature(lit_id, cx);
            return;
        };

        debug!("文献列表: 范围选中 '{}' -> '{}'", start_id, lit_id);

        let start_idx = self
            .visible_literatures
            .iter()
            .position(|id| id == &start_id);
        let end_idx = self.visible_literatures.iter().position(|id| id == &lit_id);

        if let (Some(s), Some(e)) = (start_idx, end_idx) {
            let (min, max) = if s < e { (s, e) } else { (e, s) };
            let ids: Vec<String> = (min..=max)
                .map(|i| self.visible_literatures[i].clone())
                .collect();
            UiState::update(cx, |state| {
                state.selected_literature_ids.clear();
                for id in ids {
                    state.selected_literature_ids.insert(id);
                }
            });
            self.update_view_models(cx);
        } else {
            self.select_literature(lit_id, cx);
        }
    }

    /// 获取当前选中的文献 ID 集合
    #[must_use]
    pub fn selected_literature_ids(&self) -> HashSet<String> {
        // This is a best-effort stub. Real reads should use cx.global::<UiState>().
        // The method remains for callers lacking cx; returns empty set.
        // TODO: migrate all callers to UiState Global reads
        HashSet::new()
    }

    /// 渲染单个列表项（由 `ListState` 调用）- 使用预计算的 view model，无需加锁
    fn render_item(&self, ix: usize, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let view_model = match self.view_models.get(ix) {
            Some(vm) => vm,
            None => {
                warn!(
                    "文献列表: 渲染越界 (index={}, len={})",
                    ix,
                    self.view_models.len()
                );
                return div().into_any_element();
            }
        };

        let theme = cx.theme().clone();
        let view = cx.entity().clone();
        let focus_handle = self.focus_handle.clone();

        Self::render_literature_item(view_model, view, theme, focus_handle).into_any_element()
    }

    /// 渲染文献列表项 (使用预计算的 view model)
    fn render_literature_item(
        vm: &LiteratureItemViewModel,
        view: Entity<Self>,
        theme: Theme,
        focus_handle: FocusHandle,
    ) -> impl IntoElement {
        let literature = &vm.literature;
        let is_selected = vm.is_selected;
        let lit_id: SharedString = literature.id.clone().into();
        let title = literature.title.clone();
        let all_authors = vm.authors_text.clone();
        let meta_text = vm.meta_text.clone();

        let item_wrapper_id: SharedString = format!("lit-item-wrapper-{}", literature.id).into();

        // 使用预计算的拖拽信息
        let drag_info = LiteratureDragInfo::new(vm.drag_ids.clone());

        // 预计算的标签颜色
        let tag_colors = vm.tag_colors.clone();
        let has_tags = !tag_colors.is_empty();

        div()
            .id(item_wrapper_id)
            .w_full()
            .cursor_grab()
            .on_drag(
                drag_info,
                |info: &LiteratureDragInfo, position, _: &mut Window, cx: &mut App| {
                    cx.new(|_| info.clone().with_position(position))
                },
            )
            .on_mouse_down(MouseButton::Left, {
                let view = view.clone();
                let id = literature.id.clone();
                move |event: &MouseDownEvent, _: &mut Window, cx: &mut App| {
                    if event.click_count == 2 {
                        let id = id.clone();
                        view.update(cx, |this, cx| {
                            // 获取主文件路径
                            let main_file_path = {
                                let ds = this.data_store.read(cx);
                                ds.literatures.iter().find(|l| l.id == id).and_then(|l| {
                                    l.attachments
                                        .iter()
                                        .find(|a| a.is_main)
                                        .map(|a| a.file_path.clone())
                                })
                            };

                            {
                                if let Some(ref path) = main_file_path
                                    && path.to_lowercase().ends_with(".pdf")
                                    && !this.app.should_use_external_viewer(path)
                                {
                                    if let Some(parent) = &this.parent_view
                                        && let Some(parent) = parent.upgrade()
                                    {
                                        let lit = {
                                            let ds = this.data_store.read(cx);
                                            ds.literatures.iter().find(|l| l.id == id).cloned()
                                        };
                                        if let Some(lit) = lit {
                                            parent.update(cx, |mw, cx| {
                                                mw.open_pdf_viewer(lit, cx);
                                            });
                                        }
                                    }
                                    return;
                                }
                            }

                            // fallback：非 PDF 文件或配置外部阅读器打开时，通过统一入口打开
                            let _ = this.app.open_literature_main_file(&id);
                        });
                    }
                }
            })
            .on_mouse_down(MouseButton::Right, {
                let view = view.clone();
                let id = literature.id.clone();
                let is_selected_clone = is_selected;
                move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                    cx.stop_propagation();
                    let id = id.clone();
                    let pos = event.position;
                    view.update(cx, |this, cx| {
                        // 如果右键点击的项未被选中，则选中它（单选）
                        if !is_selected_clone {
                            this.select_literature(id.clone(), cx);
                        }

                        if let Some(parent) = &this.parent_view
                            && let Some(parent) = parent.upgrade()
                        {
                            parent.update(cx, |p, cx| {
                                p.show_context_menu(
                                    pos,
                                    ContextMenuType::Literature(id),
                                    window,
                                    cx,
                                );
                            });
                        }
                    });
                }
            })
            .child({
                let pill_color = match literature.reading_status {
                    ReadingStatus::ToRead => Some(gpui::rgb(0xef4444).into()),
                    ReadingStatus::Reading => Some(gpui::rgb(0x22c55e).into()),
                    ReadingStatus::Read => Some(gpui::rgb(0xeab308).into()),
                    ReadingStatus::Unread => None,
                };

                let mut card = ArticleCard::new(lit_id.clone())
                    .title(title)
                    .authors(all_authors)
                    .meta(meta_text)
                    .selected(is_selected)
                    .status_pill(pill_color);

                if has_tags {
                    card = card.top_right(
                        h_flex().gap_1().children(
                            tag_colors
                                .iter()
                                .map(|(_, color)| div().size(rems(0.5)).rounded_full().bg(*color)),
                        ),
                    );
                }

                let has_main = literature.attachments.iter().any(|a| a.is_main);
                let has_other = literature.attachments.iter().any(|a| !a.is_main);
                if has_main || has_other {
                    card = card.bottom_right(
                        h_flex()
                            .gap_1()
                            .flex_shrink_0()
                            .when(has_main, |this| {
                                this.child(
                                    Icon::new(IconName::FileSolid)
                                        .size(rems(0.75))
                                        .flex_none()
                                        .text_color(if is_selected {
                                            theme.primary_foreground
                                        } else {
                                            theme.foreground
                                        }),
                                )
                            })
                            .when(has_other, |this| {
                                this.child(
                                    Icon::new(IconName::Attachment)
                                        .size(rems(0.75))
                                        .flex_none()
                                        .text_color(if is_selected {
                                            theme.primary_foreground
                                        } else {
                                            theme.foreground
                                        }),
                                )
                            }),
                    );
                }

                let id = lit_id.to_string();
                let view_click = view.clone();
                let focus_handle_click = focus_handle.clone();
                card.on_click(move |event, window, app| {
                    let id = id.clone();
                    let focus_handle = focus_handle_click.clone();
                    view_click.update(app, |this, cx| {
                        window.focus(&focus_handle, cx);
                        let cmd = event.modifiers().platform;
                        let shift = event.modifiers().shift;

                        if cmd {
                            this.toggle_literature_selection(id, cx);
                        } else if shift {
                            this.range_select_literature(id, cx);
                        } else {
                            this.select_literature(id, cx);
                        }
                    });
                })
            })
    }

    /// 渲染空状态
    fn render_empty_state(&self, theme: &Theme) -> impl IntoElement {
        let lang = self.app.current_language();
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_4()
            .child(
                Icon::new(IconName::Inbox)
                    .size(rems(4.0))
                    .text_color(theme.muted_foreground),
            )
            .child(div().text_lg().text_color(theme.muted_foreground).child(
                if self.search_text.is_empty() {
                    t(I18nKey::EmptyFolder, lang)
                } else {
                    t(I18nKey::NoMatchFound, lang)
                },
            ))
    }
}

impl Render for LiteratureListView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_empty = self.visible_literatures.is_empty();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .track_focus(&self.focus_handle)
            .key_context("LiteratureList")
            .on_action(cx.listener(|this: &mut Self, _: &SelectAll, _, cx| {
                this.select_all(cx);
            }))
            .on_action(cx.listener(|this: &mut Self, _: &DeleteSelected, _, cx| {
                this.delete_selected(cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.focus(&this.focus_handle, cx);
                }),
            )
            .child(if is_empty {
                let theme = cx.theme().clone();
                self.render_empty_state(&theme).into_any_element()
            } else {
                let view = cx.entity().downgrade();
                gpui::list(self.list_state.clone(), move |ix, window, cx| {
                    view.update(cx, |this, cx| this.render_item(ix, window, cx))
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .size_full()
                .flex_grow(1.0)
                .into_any_element()
            })
    }
}
