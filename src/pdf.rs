use gpui::{FocusHandle, ListState};
use gpui_component::menu::PopupMenu;
use services::pdf::{
    AnnotationState, PdfInitialState, PdfReaderDelegate, PdfService, TextPageData,
};

use i18n::Language;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

mod actions;
pub(crate) mod components;
pub(crate) mod helpers;
pub(crate) use components::pip;
mod init;
mod render;
mod selection;
mod text_format;
mod thumbnails;
mod translation;
pub(crate) mod types;
mod view_misc;
mod workers;

pub(crate) use render::DraggedSidebar;
pub use types::*;

pub struct PdfReaderView {
    pub(crate) pdf_service: Arc<PdfService>,
    pub(crate) delegate: Option<Arc<dyn PdfReaderDelegate>>,
    pub(crate) current_page: u16,
    pub(crate) current_offset_y: f32,
    pub(crate) total_pages: usize,
    pub(crate) page_sizes: Vec<(f32, f32)>,

    // 状态
    pub(crate) zoom_level: f32,
    pub(crate) render_zoom: f32,
    pub(crate) window_scale_factor: f32,
    pub(crate) last_render_scale_factor: f32,
    pub(crate) list_state: ListState,
    pub(crate) worker_state: WorkerState,
    pub(crate) initial_state: PdfInitialState,
    pub(crate) is_restoring: bool,
    pub(crate) last_rem_size: f32,
    pub(crate) last_zoom_level: f32,
    pub(crate) fit_to_width_mode: bool,

    // 页面数据（一次性加载所有页面）
    pub(crate) page_images: Vec<Option<gpui::ImageSource>>,
    pub(crate) raw_page_images: Vec<Option<Arc<image::RgbaImage>>>,
    pub(crate) page_text_data: Vec<Option<Arc<services::pdf::TextPageData>>>,
    pub(crate) page_link_data: Vec<Option<Arc<services::pdf::LinkPageData>>>,
    pub(crate) thumbnail_images: Vec<Option<gpui::ImageSource>>,
    pub(crate) pending_drop_images: Vec<Arc<gpui::RenderImage>>,
    /// 缩略图专用的文字数据（250px 分辨率下），延迟加载
    pub(crate) thumbnail_text_data: Vec<Option<Arc<services::pdf::TextPageData>>>,
    /// 已发出缩略图文字请求的页面集合，用于去重
    pub(crate) thumbnail_text_requests_pending: HashSet<u16>,
    // 字符位置查找缓存（Y 分桶索引，避免全量 O(n) 扫描）
    pub(crate) find_char_cache: HashMap<u16, Vec<Vec<usize>>>,

    // 程序化滚动（缩放/恢复等），跳过当前帧的页面跟踪覆写
    pub(crate) programmatic_scroll: bool,

    // 阅读状态保存防抖：交互只标脏，防抖定时器静默到期后统一落盘
    pub(crate) state_save_dirty: bool,
    pub(crate) state_save_scheduled: bool,

    // 选择状态
    pub(crate) is_mouse_down: bool,
    pub(crate) mouse_down_pos: Option<gpui::Point<gpui::Pixels>>,
    pub(crate) is_selecting: bool,
    pub(crate) is_panning: bool,
    pub(crate) offset_x: f32,
    pub(crate) selection_start: Option<(u16, usize)>, // (page_index, char_index)
    pub(crate) selection_end: Option<(u16, usize)>,   // (page_index, char_index)
    pub(crate) selected_text: Option<String>,
    pub(crate) rect_in_progress: Option<(u16, gpui::Bounds<f32>)>, // (page_index, bounds_in_pdf_coords)
    pub(crate) rect_start_pos: Option<(u16, f32, f32)>,            // (page_index, start_x, start_y)

    // 交互
    pub(crate) is_dragging_scrollbar: bool,
    pub(crate) drag_offset: f32,
    pub(crate) is_dragging_thumbnail_scrollbar: bool,
    pub(crate) thumbnail_drag_offset: f32,

    // 左侧边栏状态
    pub(crate) is_left_sidebar_open: bool,
    pub(crate) active_left_sidebar_tab: LeftSidebarTab,
    pub(crate) thumbnail_list_state: ListState,
    // 缩略图多选状态
    pub(crate) selected_thumbnails: HashSet<u16>,
    pub(crate) last_anchor_page: Option<u16>,
    // 右侧边栏状态
    pub(crate) is_right_sidebar_open: bool,
    pub(crate) active_right_sidebar_tab: RightSidebarTab,
    pub(crate) translation_result: Option<TranslationResult>,
    pub(crate) engine_select:
        Option<gpui::Entity<gpui_component::select::SelectState<Vec<TranslationEngineItem>>>>,
    pub(crate) translation_original_expanded: bool,
    pub(crate) translation_font_size: f32,
    pub(crate) auto_translate: bool,
    pub(crate) append_translation_mode: bool,
    pub(crate) copied_translation_original: bool,
    pub(crate) copied_translation_result: bool,

    pub(crate) document_id: String,
    pub(crate) document_path: PathBuf,
    // 侧边栏宽度与拖拽状态
    pub(crate) left_sidebar_width: gpui::Pixels,
    pub(crate) right_sidebar_width: gpui::Pixels,
    pub(crate) preferred_left_sidebar_width: f32,
    pub(crate) preferred_right_sidebar_width: f32,
    pub(crate) last_content_width: f32,

    pub(crate) annotation_state: AnnotationState,
    pub(crate) annotation_version: u64,
    pub(crate) last_composited_version: u64,

    pub(crate) focus_handle: FocusHandle,
    pub(crate) has_focused: bool,

    // 大纲状态
    pub(crate) outlines: Option<Vec<services::pdf::OutlineItem>>,
    pub(crate) expanded_outlines: std::collections::HashSet<String>,

    // 笔记编辑器
    pub(crate) note_input_state: Option<gpui::Entity<gpui_component::input::TextareaState>>,
    pub(crate) note_input_sub: Option<gpui::Subscription>,
    /// 防止 overlay 按钮点击后 main_content 的 mousedown+mouseup 连锁清除 overlay
    pub(crate) overlay_button_clicked: bool,

    // 左侧栏内联笔记编辑
    pub(crate) editing_note_sidebar_id: Option<String>,
    pub(crate) editing_note_sidebar_input:
        Option<gpui::Entity<gpui_component::input::TextareaState>>,
    pub(crate) editing_note_sidebar_sub: Option<gpui::Subscription>,

    // 当前界面语言
    pub(crate) language: Language,

    // 搜索状态
    pub(crate) search_state: Option<SearchState>,
    pub(crate) search_input_state: Option<gpui::Entity<gpui_component::input::InputState>>,
    pub(crate) search_input_sub: Option<gpui::Subscription>,
    pub(crate) search_list_state:
        Option<gpui::Entity<gpui_component::list::ListState<SearchResultsDelegate>>>,
    pub(crate) search_list_sub: Option<gpui::Subscription>,
    pub(crate) search_text_storage: Option<Vec<Option<Arc<TextPageData>>>>,
    pub(crate) search_content_height: f32,

    // 多笔记卡片
    pub(crate) notes_cache: Vec<models::LiteratureNote>,
    pub(crate) editing_note_index: Option<usize>,
    pub(crate) edit_note_title: Option<gpui::Entity<gpui_component::input::InputState>>,
    pub(crate) edit_note_content: Option<gpui::Entity<gpui_component::input::TextareaState>>,
    pub(crate) summary_task: Option<gpui::Task<()>>,
    pub(crate) is_generating_summary: bool,
    pub(crate) last_ai_summary_note_id: Option<String>,
    pub(crate) expanded_notes: std::collections::HashSet<String>,

    // ─── AI 对话 ─────────────────────────────────────────
    pub(crate) chat_sessions: Vec<models::chat::ChatSession>,
    pub(crate) active_chat_session_id: Option<String>,
    pub(crate) chat_creating: bool,
    pub(crate) chat_create_title: Option<gpui::Entity<gpui_component::input::InputState>>,
    pub(crate) chat_create_prompt: Option<gpui::Entity<gpui_component::input::TextareaState>>,
    pub(crate) chat_session_view:
        Option<gpui::Entity<components::chat_session_view::ChatSessionView>>,
    pub(crate) chat_backend_select:
        Option<gpui::Entity<gpui_component::select::SelectState<Vec<AiBackendSelectItem>>>>,
    pub(crate) cached_ai_backends: Vec<services::pdf::AiBackendItem>,

    // ─── 页面可见性管理 ─────────────────────────────────
    pub(crate) visible_page_first: usize,
    pub(crate) visible_page_last: usize,
    pub(crate) page_render_requests_pending: HashSet<u16>,
    // ─── 缩略图可见性管理 ──────────────────────────────
    pub(crate) visible_thumb_first: usize,
    pub(crate) visible_thumb_last: usize,
    pub(crate) thumb_render_requests_pending: HashSet<u16>,

    // ─── 画中画 (PiP) ────────────────────────────────────
    pub(crate) pins: Vec<pip::PiPPin>,
    #[allow(dead_code)]
    pub(crate) active_pin_id: Option<String>,
    pub(crate) dragging_pin: Option<pip::PiPDragState>,
    pub(crate) resizing_pin: Option<pip::PiPResizeState>,
    /// Pin 右键菜单：(菜单位置, PopupMenu 实体)
    pub(crate) pin_context_menu: Option<(gpui::Point<gpui::Pixels>, gpui::Entity<PopupMenu>)>,
    /// 注释右键菜单：(菜单位置, PopupMenu 实体)
    pub(crate) annotation_context_menu:
        Option<(gpui::Point<gpui::Pixels>, gpui::Entity<PopupMenu>)>,
    /// 缩略图右键菜单：(菜单位置, PopupMenu 实体)
    pub(crate) thumbnail_context_menu: Option<(gpui::Point<gpui::Pixels>, gpui::Entity<PopupMenu>)>,
    /// 浮动工具栏（选中文本后出现的颜色/类型选择菜单）
    pub(crate) annotation_toolbar_menu:
        Option<(gpui::Point<gpui::Pixels>, gpui::Entity<PopupMenu>)>,
    pub(crate) annotation_drag: Option<AnnotationDragState>,
    /// 当前文档标题（论文名），供保存图片等场景使用
    pub(crate) document_title: String,
    pub(crate) page_color_mode: PageColorMode,
    /// 缩放刚变化，需要以新分辨率重新渲染
    pub(crate) zoom_changed: bool,
    /// 主窗口 Tab 栏高度偏移（rems，嵌入时使用）
    pub(crate) tab_bar_offset_px: f32,
    /// 简单预览模式：隐藏工具栏
    pub(crate) hide_toolbar: bool,
    /// 简单预览模式：隐藏侧栏
    pub(crate) hide_sidebars: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationResizeHandle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
    TextStart,
    TextEnd,
}

#[derive(Clone, Debug)]
pub struct AnnotationDragState {
    pub annotation_id: String,
    pub page: u16,
    pub handle: AnnotationResizeHandle,
    pub start_mouse: gpui::Point<gpui::Pixels>,
    pub start_x: f32,
    pub start_y: f32,
    pub start_w: f32,
    pub start_h: f32,
}
