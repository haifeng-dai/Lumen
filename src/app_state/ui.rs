use std::collections::HashSet;

use gpui::{App, Global};

use models::AdvancedSearchQuery;
use services::query::data::{AppViewMode, SortField, SortOrder};

/// UI 状态 — 每窗口独立的选中/排序/视图状态。
/// 以 GPUI Global 存储，MainWindow 写入并 `refresh_windows()`，
/// 子视图在 render() 中通过 `cx.global::<UiState>()` 只读访问。
#[derive(Debug, Clone)]
pub struct UiState {
    pub selected_folder_id: Option<String>,
    pub selected_tag_id: Option<String>,
    pub selected_literature_ids: HashSet<String>,
    pub view_mode: AppViewMode,
    pub sort_field: SortField,
    pub sort_order: SortOrder,
    pub selected_feed_id: Option<String>,
    pub selected_feed_item_ids: HashSet<String>,
    pub menu_folder_expanded: HashSet<String>,
    pub advanced_search_query: AdvancedSearchQuery,
    pub copied_formula_id: Option<String>,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            selected_folder_id: Some("all".to_string()),
            selected_tag_id: None,
            selected_literature_ids: HashSet::new(),
            view_mode: AppViewMode::Library,
            sort_field: SortField::default(),
            sort_order: SortOrder::default(),
            selected_feed_id: Some("all_subs".to_string()),
            selected_feed_item_ids: HashSet::new(),
            menu_folder_expanded: HashSet::new(),
            advanced_search_query: AdvancedSearchQuery::default(),
            copied_formula_id: None,
        }
    }

    /// 修改 UiState 并广播刷新所有窗口
    pub fn update(cx: &mut App, f: impl FnOnce(&mut Self)) {
        let mut state = cx.global::<Self>().clone();
        f(&mut state);
        cx.set_global(state);
        cx.refresh_windows();
    }
}

impl Global for UiState {}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}
