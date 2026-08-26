use gpui::prelude::*;
use gpui::{Context, ListAlignment, ListState, px};
use services::pdf::{AnnotationState, PdfInitialState, PdfReaderDelegate, PdfService};

use i18n::Language;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use super::*;

impl super::PdfReaderView {
    pub fn drain_images_to_drop(&mut self) -> Vec<Arc<gpui::RenderImage>> {
        let mut images = Vec::new();
        // 收集并清除主页面纹理
        for opt in self.page_images.drain(..) {
            if let Some(gpui::ImageSource::Render(r)) = opt {
                images.push(r);
            }
        }
        self.raw_page_images.clear();

        // 收集并清除缩略图纹理
        for opt in self.thumbnail_images.drain(..) {
            if let Some(gpui::ImageSource::Render(r)) = opt {
                images.push(r);
            }
        }

        // 收集并清除 Pin 裁剪图纹理
        for mut pin in std::mem::take(&mut self.pins) {
            if let Some(gpui::ImageSource::Render(r)) = pin.image_source.take() {
                images.push(r);
            }
        }
        images
    }

    pub fn new(
        service: Arc<PdfService>,
        delegate: Option<Arc<dyn PdfReaderDelegate>>,
        document_id: String,
        document_path: PathBuf,
        _cx: &mut Context<Self>,
    ) -> Self {
        let list_state = ListState::new(0, ListAlignment::Top, px(1000.0));
        let thumbnail_list_state = ListState::new(0, ListAlignment::Top, px(600.0));

        let initial_state = if let Some(ref d) = delegate {
            d.get_initial_state(document_id.clone())
        } else {
            PdfInitialState::default()
        };

        let global_ui = if _cx.has_global::<GlobalPdfUiState>() {
            Some(_cx.global::<GlobalPdfUiState>().clone())
        } else {
            None
        };

        let use_zoom = global_ui
            .as_ref()
            .map(|g| g.zoom_level)
            .unwrap_or(initial_state.zoom_level);
        let use_fit_to_width = global_ui
            .as_ref()
            .map(|g| g.fit_to_width)
            .unwrap_or(initial_state.fit_to_width);
        let use_is_left_sidebar_open = global_ui
            .as_ref()
            .map(|g| g.is_left_sidebar_open)
            .unwrap_or(initial_state.is_left_sidebar_open);
        let use_is_right_sidebar_open = global_ui
            .as_ref()
            .map(|g| g.is_right_sidebar_open)
            .unwrap_or(initial_state.is_right_sidebar_open);
        let use_left_sidebar_width = global_ui
            .as_ref()
            .map(|g| g.left_sidebar_width)
            .unwrap_or(initial_state.left_sidebar_width);
        let use_right_sidebar_width = global_ui
            .as_ref()
            .map(|g| g.right_sidebar_width)
            .unwrap_or(initial_state.right_sidebar_width);
        let use_auto_translate = global_ui
            .as_ref()
            .map(|g| g.auto_translate)
            .unwrap_or(initial_state.auto_translate);

        let language = delegate
            .as_ref()
            .map(|d| d.current_language())
            .unwrap_or(Language::ZhCn);

        let initial_page_color_mode = if let Some(d) = &delegate {
            match d.get_page_color_mode().as_str() {
                "sepia" => PageColorMode::Sepia,
                "eyeprotect" => PageColorMode::EyeProtect,
                _ => PageColorMode::White,
            }
        } else {
            PageColorMode::White
        };

        let view_instance = Self {
            pdf_service: service,
            delegate,
            document_id: document_id.clone(),
            document_path,
            document_title: document_id,
            current_page: initial_state.page_index,
            current_offset_y: initial_state.offset_y,
            total_pages: 0,
            page_sizes: Vec::new(),

            zoom_level: if use_zoom > 0.1 { use_zoom } else { 1.0 },
            render_zoom: if use_zoom > 0.1 {
                quantize_render_zoom(use_zoom)
            } else {
                1.0
            },
            list_state,
            worker_state: WorkerState::Loading,
            last_rem_size: 16.0,
            window_scale_factor: 1.0,
            last_render_scale_factor: 0.0,
            last_zoom_level: if use_zoom > 0.1 { use_zoom } else { 1.0 },
            fit_to_width_mode: use_fit_to_width,
            initial_state: initial_state.clone(),
            is_restoring: true,

            language,

            // 页面数据初始化（文档加载后会重置）
            page_images: Vec::new(),
            raw_page_images: Vec::new(),
            page_text_data: Vec::new(),
            page_link_data: Vec::new(),
            thumbnail_images: Vec::new(),
            pending_drop_images: Vec::new(),
            thumbnail_text_data: Vec::new(),
            thumbnail_text_requests_pending: HashSet::new(),

            // 页面可见性管理
            visible_page_first: 0,
            visible_page_last: 0,
            page_render_requests_pending: HashSet::new(),
            // 缩略图可见性管理
            visible_thumb_first: 0,
            visible_thumb_last: 0,
            thumb_render_requests_pending: HashSet::new(),
            find_char_cache: HashMap::new(),

            is_mouse_down: false,
            mouse_down_pos: None,
            is_selecting: false,
            is_panning: false,
            offset_x: 0.0,
            selection_start: None,
            selection_end: None,
            selected_text: None,
            rect_in_progress: None,
            rect_start_pos: None,

            is_dragging_scrollbar: false,
            drag_offset: 0.0,
            is_dragging_thumbnail_scrollbar: false,
            thumbnail_drag_offset: 0.0,

            is_left_sidebar_open: use_is_left_sidebar_open,
            active_left_sidebar_tab: LeftSidebarTab::Thumbnails,
            thumbnail_list_state,
            selected_thumbnails: HashSet::new(),
            last_anchor_page: None,
            is_right_sidebar_open: use_is_right_sidebar_open,
            active_right_sidebar_tab: RightSidebarTab::Translation,
            translation_result: None,
            translation_original_expanded: initial_state.translation_original_expanded,
            translation_font_size: initial_state.translation_font_size,
            auto_translate: use_auto_translate,
            append_translation_mode: false,
            copied_translation_original: false,
            copied_translation_result: false,

            preferred_left_sidebar_width: if use_left_sidebar_width > 0.0 {
                use_left_sidebar_width
            } else {
                DEFAULT_LEFT_SIDEBAR_WIDTH
            },
            preferred_right_sidebar_width: if use_right_sidebar_width > 0.0 {
                use_right_sidebar_width
            } else {
                DEFAULT_RIGHT_SIDEBAR_WIDTH
            },
            left_sidebar_width: px(if use_left_sidebar_width > 0.0 {
                use_left_sidebar_width
            } else {
                DEFAULT_LEFT_SIDEBAR_WIDTH
            }),
            right_sidebar_width: px(if use_right_sidebar_width > 0.0 {
                use_right_sidebar_width
            } else {
                DEFAULT_RIGHT_SIDEBAR_WIDTH
            }),
            last_content_width: 0.0,
            programmatic_scroll: false,
            state_save_dirty: false,
            state_save_scheduled: false,
            annotation_state: AnnotationState::default(),
            annotation_version: 0,
            last_composited_version: 0,

            focus_handle: _cx.focus_handle(),
            has_focused: false,

            outlines: None,
            expanded_outlines: std::collections::HashSet::new(),
            note_input_state: None,
            note_input_sub: None,
            overlay_button_clicked: false,

            editing_note_sidebar_id: None,
            editing_note_sidebar_input: None,
            editing_note_sidebar_sub: None,

            search_state: None,
            search_input_state: None,
            search_input_sub: None,
            search_list_state: None,
            search_list_sub: None,
            search_text_storage: None,
            search_content_height: 0.0,

            notes_cache: Vec::new(),
            editing_note_index: None,
            edit_note_title: None,
            edit_note_content: None,
            summary_task: None,
            is_generating_summary: false,
            last_ai_summary_note_id: None,
            expanded_notes: std::collections::HashSet::new(),

            chat_sessions: Vec::new(),
            active_chat_session_id: None,
            chat_creating: false,
            chat_create_title: None,
            chat_create_prompt: None,
            chat_session_view: None,

            pins: Vec::new(),
            active_pin_id: None,
            dragging_pin: None,
            resizing_pin: None,
            pin_context_menu: None,
            annotation_context_menu: None,
            thumbnail_context_menu: None,
            annotation_toolbar_menu: None,
            annotation_drag: None,
            page_color_mode: initial_page_color_mode,
            zoom_changed: false,
            tab_bar_offset_px: 0.0,
            hide_toolbar: false,
            hide_sidebars: false,
        };

        if !_cx.has_global::<GlobalPdfUiState>() {
            _cx.set_global(GlobalPdfUiState {
                zoom_level: use_zoom,
                fit_to_width: use_fit_to_width,
                is_left_sidebar_open: use_is_left_sidebar_open,
                is_right_sidebar_open: use_is_right_sidebar_open,
                left_sidebar_width: use_left_sidebar_width,
                right_sidebar_width: use_right_sidebar_width,
                auto_translate: use_auto_translate,
            });
        }

        _cx.observe_global::<GlobalPdfUiState>(|this, cx| {
            let global = cx.global::<GlobalPdfUiState>();
            let mut changed = false;

            if (this.zoom_level - global.zoom_level).abs() > 0.001 {
                this.zoom_level = global.zoom_level;
                let new_render_zoom = quantize_render_zoom(this.zoom_level);
                if (this.render_zoom - new_render_zoom).abs() > f32::EPSILON {
                    this.render_zoom = new_render_zoom;
                    this.find_char_cache.clear();
                    this.page_render_requests_pending.clear();
                    this.zoom_changed = true;
                }
                this.search_state = None;
                this.programmatic_scroll = true;
                changed = true;
            }
            if this.fit_to_width_mode != global.fit_to_width {
                this.fit_to_width_mode = global.fit_to_width;
                changed = true;
            }
            if this.is_left_sidebar_open != global.is_left_sidebar_open {
                this.is_left_sidebar_open = global.is_left_sidebar_open;
                changed = true;
            }
            if this.is_right_sidebar_open != global.is_right_sidebar_open {
                this.is_right_sidebar_open = global.is_right_sidebar_open;
                changed = true;
            }
            if (f32::from(this.left_sidebar_width) - global.left_sidebar_width).abs() > 0.1 {
                this.left_sidebar_width = px(global.left_sidebar_width);
                this.preferred_left_sidebar_width = global.left_sidebar_width;
                changed = true;
            }
            if (f32::from(this.right_sidebar_width) - global.right_sidebar_width).abs() > 0.1 {
                this.right_sidebar_width = px(global.right_sidebar_width);
                this.preferred_right_sidebar_width = global.right_sidebar_width;
                changed = true;
            }
            if this.auto_translate != global.auto_translate {
                this.auto_translate = global.auto_translate;
                changed = true;
            }

            if changed {
                cx.notify();
            }
        })
        .detach();

        _cx.observe_global::<crate::app_state::config::ConfigStore>(|_, cx| {
            cx.notify();
        })
        .detach();

        view_instance
    }

    pub fn set_tab_bar_offset_px(&mut self, px: f32) {
        self.tab_bar_offset_px = px;
    }

    pub fn set_document_title(&mut self, title: String) {
        self.document_title = title;
    }

    pub fn set_simple_mode(&mut self, simple: bool) {
        self.hide_toolbar = simple;
        self.hide_sidebars = simple;
        self.is_left_sidebar_open = false;
        self.is_right_sidebar_open = false;
    }

    pub(crate) fn set_page_color_mode(&mut self, mode: PageColorMode, cx: &mut Context<Self>) {
        if self.page_color_mode == mode {
            return;
        }
        self.page_color_mode = mode;

        if let Some(d) = &self.delegate {
            let mode_str = match self.page_color_mode {
                PageColorMode::White => "white",
                PageColorMode::Sepia => "sepia",
                PageColorMode::EyeProtect => "eyeprotect",
            };
            d.set_page_color_mode(mode_str.to_string());
        }

        // 背景色通过 div bg() 自动生效，无需清缓存或重渲染
        cx.notify();
    }

    // ─── 缩略图多选 ─────────────────────────────────────────
}
