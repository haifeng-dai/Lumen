use gpui::prelude::*;
use gpui::{App, Context, SharedString, Window, div, rems};
use gpui_component::{
    ActiveTheme, IndexPath,
    label::Label,
    list::{ListDelegate, ListItem, ListState},
    v_flex,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PageColorMode {
    White,
    Sepia,
    EyeProtect,
}

impl PageColorMode {
    pub fn bg_color(&self) -> gpui::Hsla {
        match self {
            Self::White => gpui::white(),
            Self::Sepia => gpui::rgb(0xF4ECD8).into(),
            Self::EyeProtect => gpui::rgb(0xCCE8CF).into(),
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum WorkerState {
    Loading,
    Running,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftSidebarTab {
    Thumbnails,
    Outline,
    Annotations,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightSidebarTab {
    Translation,
    Notes,
    Chat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranslationResult {
    pub original: String,
    pub translated: Option<String>,
    pub is_loading: bool,
    pub error: Option<String>,
}

pub const SIDEBAR_MIN_RATIO: f32 = 0.1;
pub const SIDEBAR_MAX_RATIO: f32 = 0.4;
pub const DEFAULT_LEFT_SIDEBAR_WIDTH: f32 = 240.0;
pub const DEFAULT_RIGHT_SIDEBAR_WIDTH: f32 = 400.0;

pub const TOOLBAR_HEIGHT_REMS: f32 = 2.0;

// ── 页面布局 ──────────────────────────────────────────
/// display_w 计算公式的基准宽度（rem）。display_w = BASE * zoom * rem_size
pub const PAGE_BASE_WIDTH_REMS: f32 = 45.0;
/// 自动适配宽度时减去的留白/滚动条宽度（逻辑像素）
pub const AUTO_FIT_PADDING_PX: f32 = 48.0;

// ── 渲染缩放量化 ──────────────────────────────────────
/// 渲染缩放等级桶。渲染时取 ≥ 当前显示缩放的最接近桶值。
pub const RENDER_ZOOM_BUCKETS: &[f32] = &[0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0];

// ── 阅读状态保存防抖 ──────────────────────────────────
/// 滚动/缩放等交互后，延迟落盘的去抖间隔（毫秒）。
/// 连续交互期间只标脏，静默达到该时长后才统一写一次，
/// 避免每帧同步 SQLite I/O 造成的 UI 卡顿。
pub const STATE_SAVE_DEBOUNCE_MS: u64 = 1000;

/// 将显示缩放量化为渲染缩放：取 ≥ zoom 的最小桶值
pub fn quantize_render_zoom(zoom: f32) -> f32 {
    RENDER_ZOOM_BUCKETS
        .iter()
        .copied()
        .find(|&level| level >= zoom)
        .unwrap_or(*RENDER_ZOOM_BUCKETS.last().unwrap())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub page_index: u16,
    pub start_char: usize,
    pub end_char: usize,
}

#[derive(Debug, Clone)]
pub struct SearchState {
    pub query: String,
    pub results: Vec<SearchMatch>,
    pub active_match_idx: Option<usize>,
}

impl SearchState {
    pub fn total_matches(&self) -> usize {
        self.results.len()
    }

    pub fn active_match(&self) -> Option<&SearchMatch> {
        self.active_match_idx.and_then(|i| self.results.get(i))
    }
}

// ── 搜索结果显示（gpui-component List） ───────────────

/// 预计算的搜索结果显示数据
#[derive(Clone)]
pub struct SearchResultDisplay {
    pub title: String,
    pub context: SharedString,
}

/// 搜索结果的 ListDelegate，用于 gpui-component List 虚拟滚动
pub struct SearchResultsDelegate {
    pub items: Vec<SearchResultDisplay>,
    pub active_match_idx: Option<usize>,
    pub selected_idx: Option<IndexPath>,
}

impl ListDelegate for SearchResultsDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.items.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let item = &self.items[ix.row];
        if item.context.is_empty() {
            return None;
        }
        let is_active = self.active_match_idx == Some(ix.row);
        let selected = Some(ix) == self.selected_idx;
        let theme = cx.theme();

        Some(
            ListItem::new(ix)
                .selected(selected || is_active)
                .py_2()
                .px_3()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    v_flex()
                        .gap_y_0p5()
                        .child(Label::new(item.title.clone()).text_sm())
                        .child(
                            div().h(rems(1.25)).overflow_hidden().child(
                                Label::new(item.context.clone())
                                    .text_xs()
                                    .text_color(theme.muted_foreground),
                            ),
                        ),
                ),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected_idx = ix;
        cx.notify();
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
    }

    fn cancel(&mut self, _window: &mut Window, _cx: &mut Context<ListState<Self>>) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationEngineItem {
    pub value: String,
    pub label: String,
}

impl gpui_component::select::SelectItem for TranslationEngineItem {
    type Value = String;

    fn title(&self) -> SharedString {
        self.label.clone().into()
    }

    fn value(&self) -> &String {
        &self.value
    }
}

// ── 全局 UI 状态（GPUI Global，仅视图层使用） ──────────

/// PDF 阅读器全局 UI 状态，作为 GPUI 全局单例保存。
/// 属于视图层状态，因此从引擎 crate 移出，定义在此处。
#[derive(Clone, Debug)]
pub struct GlobalPdfUiState {
    pub zoom_level: f32,
    pub fit_to_width: bool,
    pub is_left_sidebar_open: bool,
    pub is_right_sidebar_open: bool,
    pub left_sidebar_width: f32,
    pub right_sidebar_width: f32,
    pub auto_translate: bool,
}

impl gpui::Global for GlobalPdfUiState {}

// ── AI 后端下拉项（本地 newtype，承接引擎的 AiBackendItem） ──

/// 包装引擎的 [services::pdf::AiBackendItem]，为 gpui-component 下拉实现 [gpui_component::select::SelectItem]。
/// 由于 `AiBackendItem` 定义在 `services::pdf` 模块，无法在二进制侧直接为其 impl 外部 trait（orphan rule），故使用 newtype。
#[derive(Clone)]
pub struct AiBackendSelectItem(pub services::pdf::AiBackendItem);

impl gpui_component::select::SelectItem for AiBackendSelectItem {
    type Value = String;

    fn title(&self) -> SharedString {
        self.0.name.clone().into()
    }

    fn value(&self) -> &String {
        &self.0.name
    }
}
