use crate::pdf::{PAGE_BASE_WIDTH_REMS, PdfReaderView, TOOLBAR_HEIGHT_REMS, helpers};
use components::IconName;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, MouseButton, PathPromptOptions, Pixels, SharedString,
    WeakEntity, Window, div, px,
};
use gpui_component::input::{InputEvent, Textarea, TextareaState};
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::{ActiveTheme, Icon, h_flex, v_flex};
use i18n::I18nKey;
use models::AnnotationColor;
use services::pdf::annotation::ToolbarAnnotationKind;

/// 标注选取器的模式：浮出工具栏 vs 右键菜单
#[derive(Clone)]
enum AnnotationPickerMode {
    Create,
    Edit {
        ann_id: String,
        current_color: AnnotationColor,
        current_kind: services::pdf::AnnotationKind,
    },
}

impl PdfReaderView {
    fn find_annotation(&self, id: &str) -> Option<services::pdf::Annotation> {
        for annotations in self.annotation_state.annotations.values() {
            for ann in annotations {
                if !ann.is_deleted && ann.id == id {
                    return Some(ann.clone());
                }
            }
        }
        None
    }

    fn find_annotation_mut(&mut self, id: &str) -> Option<&mut services::pdf::Annotation> {
        for annotations in self.annotation_state.annotations.values_mut() {
            for ann in annotations.iter_mut() {
                if !ann.is_deleted && ann.id == id {
                    return Some(ann);
                }
            }
        }
        None
    }

    fn update_and_save(&mut self, id: &str, f: impl FnOnce(&mut services::pdf::Annotation)) {
        let cloned = match self.find_annotation_mut(id) {
            Some(ann) => {
                f(ann);
                ann.clone()
            }
            None => return,
        };
        if let Some(delegate) = &self.delegate {
            delegate.save_annotation(&cloned);
        }
    }

    pub(crate) fn build_annotation_context_menu(
        &mut self,
        ann_id: &str,
        from_sidebar: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<PopupMenu> {
        let ann = match self.find_annotation(ann_id) {
            Some(a) => a.clone(),
            None => {
                let app: &mut App = cx;
                return PopupMenu::build(window, app, |m, _, _| m);
            }
        };
        let ann_page = ann.page;
        let rect_coords = match &ann.kind {
            services::pdf::AnnotationKind::Rectangle { x, y, w, h } => Some((*x, *y, *w, *h)),
            _ => None,
        };
        let current_color = ann.color;
        let has_note = ann.note.as_ref().is_some_and(|n| !n.is_empty());
        let current_kind = ann.kind.clone();
        let lang = self.language;
        let weak_self = cx.weak_entity();

        let ann_id_build = ann_id.to_string();

        let app: &mut App = cx;
        PopupMenu::build(window, app, move |mut menu, _window, _cx| {
            // ── 颜色圆点 + 类型切换（共享组件） ──
            let picker_items = annotation_picker_items(
                weak_self.clone(),
                AnnotationPickerMode::Edit {
                    ann_id: ann_id_build.clone(),
                    current_color,
                    current_kind: current_kind.clone(),
                },
            );
            for item in picker_items {
                menu = menu.item(item);
            }

            // ── 矩形注释操作 ──
            if let Some((rx, ry, rw, rh)) = rect_coords {
                let page = ann_page;
                let weak_copy = weak_self.clone();
                menu = menu.item(
                    PopupMenuItem::new(i18n::t(I18nKey::CopyAsImage, lang)).on_click(
                        move |_, _window, cx| {
                            if let Some(this) = weak_copy.upgrade() {
                                this.update(cx, |this, cx| {
                                    this.annotation_context_menu = None;
                                    this.copy_rect_image(page, rx, ry, rw, rh);
                                    cx.notify();
                                });
                            }
                        },
                    ),
                );

                let weak_save = weak_self.clone();
                menu = menu.item(
                    PopupMenuItem::new(i18n::t(I18nKey::SaveAsImage, lang)).on_click(
                        move |_, _window, cx| {
                            if let Some(this) = weak_save.upgrade() {
                                this.update(cx, |this, cx| {
                                    this.annotation_context_menu = None;
                                    this.save_rect_image(page, rx, ry, rw, rh, cx);
                                    cx.notify();
                                });
                            }
                        },
                    ),
                );

                let weak_pip = weak_self.clone();
                menu = menu.item(
                    PopupMenuItem::new(i18n::t(I18nKey::CreatePip, lang)).on_click(
                        move |_, window, cx| {
                            if let Some(this) = weak_pip.upgrade() {
                                this.update(cx, |this, cx| {
                                    this.annotation_context_menu = None;
                                    this.create_pip_from_rect(page, rx, ry, rw, rh, window);
                                    cx.notify();
                                });
                            }
                        },
                    ),
                );
                menu = menu.separator();
            }

            // ── 添加/查看备注 ──
            let weak_note = weak_self.clone();
            let aid_note = ann_id_build.clone();
            let has_note_val = has_note;
            let from_sidebar_val = from_sidebar;
            menu = menu.item(
                PopupMenuItem::new(if has_note_val {
                    i18n::t(I18nKey::ViewNote, lang)
                } else {
                    i18n::t(I18nKey::AddNote, lang)
                })
                .on_click(move |_, window, cx| {
                    if let Some(this) = weak_note.upgrade() {
                        this.update(cx, |this, cx| {
                            let id = aid_note.clone();
                            let (existing, saved_clone) = {
                                let entry = this.find_annotation_mut(&id);
                                match entry {
                                    None => (String::new(), None),
                                    Some(ann) => {
                                        let text = ann.note.as_deref().unwrap_or("").to_string();
                                        if ann.note.is_none() {
                                            ann.note = Some(String::new());
                                            ann.updated_at = chrono::Utc::now().timestamp();
                                            (text, Some(ann.clone()))
                                        } else {
                                            (text, None)
                                        }
                                    }
                                }
                            };
                            if let Some(cloned) = saved_clone
                                && let Some(delegate) = &this.delegate
                            {
                                delegate.save_annotation(&cloned);
                            }
                            let input_state = cx.new(|inner_cx| {
                                TextareaState::new(window, inner_cx)
                                    .rows(3)
                                    .auto_grow(2, 10)
                                    .placeholder(i18n::t(I18nKey::NotePlaceholder, this.language))
                                    .default_value(existing)
                            });
                            let menu_pos = this
                                .annotation_context_menu
                                .as_ref()
                                .map(|(p, _)| *p)
                                .unwrap_or(gpui::point(px(0.0), px(0.0)));
                            if from_sidebar_val {
                                let sub = cx.subscribe(
                                    &input_state,
                                    |this: &mut PdfReaderView,
                                     _emitter: gpui::Entity<TextareaState>,
                                     event: &InputEvent,
                                     cx: &mut Context<PdfReaderView>| {
                                        match event {
                                            InputEvent::PressEnter { .. } => {
                                                this.save_sidebar_note(cx);
                                            }
                                            InputEvent::Blur => {
                                                this.save_sidebar_note(cx);
                                            }
                                            _ => {}
                                        }
                                    },
                                );
                                input_state.update(cx, |state, inner_cx| {
                                    state.focus(window, inner_cx);
                                });
                                this.editing_note_sidebar_id = Some(id);
                                this.editing_note_sidebar_input = Some(input_state);
                                this.editing_note_sidebar_sub = Some(sub);
                            } else {
                                let sub = cx.subscribe(
                                    &input_state,
                                    |this: &mut PdfReaderView,
                                     _emitter: gpui::Entity<TextareaState>,
                                     event: &InputEvent,
                                     cx: &mut Context<PdfReaderView>| {
                                        if let InputEvent::PressEnter {
                                                secondary: true, ..
                                            } = event {
                                            this.save_note_and_close(cx);
                                        }
                                    },
                                );
                                input_state.update(cx, |state, inner_cx| {
                                    state.focus(window, inner_cx);
                                });
                                this.note_input_state = Some(input_state);
                                this.note_input_sub = Some(sub);
                                this.annotation_state.note_editor =
                                    Some(services::pdf::NoteEditorState {
                                        annotation_id: id,
                                        position: services::pdf::Point {
                                            x: services::pdf::Pixels(f32::from(menu_pos.x)),
                                            y: services::pdf::Pixels(f32::from(menu_pos.y)),
                                        },
                                    });
                            }
                            this.overlay_button_clicked = true;
                            this.annotation_context_menu = None;
                            cx.notify();
                        });
                    }
                }),
            );

            menu = menu.separator();

            // ── 删除 ──
            let weak_delete = weak_self.clone();
            let aid_delete = ann_id_build.clone();
            let delete_label: SharedString = i18n::t(I18nKey::Delete, lang).into();
            menu = menu.item(
                PopupMenuItem::element(move |_window, cx| {
                    div()
                        .text_color(cx.theme().danger)
                        .child(delete_label.clone())
                })
                .on_click(move |_, _window, cx| {
                    if let Some(this) = weak_delete.upgrade() {
                        this.update(cx, |this, cx| {
                            let id = aid_delete.clone();
                            this.annotation_context_menu = None;
                            this.annotation_state.selected_id = None;
                            if let Some(ann) = this.find_annotation_mut(&id) {
                                ann.is_deleted = true;
                            }
                            if let Some(delegate) = &this.delegate {
                                delegate.delete_annotation(&id);
                            }
                            this.annotation_version += 1;
                            cx.notify();
                        });
                    }
                }),
            );

            menu
        })
    }

    pub(crate) fn render_note_editor(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let _note_state = self.annotation_state.note_editor.as_ref()?;
        let input_state = self.note_input_state.as_ref()?;

        let theme = cx.theme();
        let bg = theme.background;
        let border = theme.border;

        let pos_x = f32::from(_note_state.position.x);
        let pos_y = f32::from(_note_state.position.y);
        let viewport_w = f32::from(window.viewport_size().width);
        let viewport_h = f32::from(window.viewport_size().height) - self.tab_bar_offset_px;

        const POPUP_W: f32 = 300.0;

        let px_pos_x = pos_x.clamp(0.0, (viewport_w - POPUP_W).max(0.0));
        let py_pos_y = if pos_y + 200.0 > viewport_h {
            (pos_y - 200.0).max(0.0)
        } else {
            pos_y
        };

        let outer_id = "note-editor-overlay";
        Some(
            div()
                .absolute()
                .left(px(px_pos_x))
                .top(px(py_pos_y))
                .w(px(POPUP_W))
                .bg(bg)
                .border_1()
                .border_color(border)
                .shadow_lg()
                .rounded_md()
                .p_3()
                .cursor_default()
                .id(outer_id)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _| {
                        this.overlay_button_clicked = true;
                    }),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child(Textarea::new(input_state).w_full())
                        .child(
                            h_flex()
                                .w_full()
                                .justify_end()
                                .gap_2()
                                .child(
                                    div()
                                        .id("note_editor_check")
                                        .cursor_pointer()
                                        .rounded_sm()
                                        .hover(move |s| s.bg(theme.muted))
                                        .on_hover(cx.listener(move |_this, _, _, cx| {
                                            cx.notify();
                                        }))
                                        .child(Icon::new(IconName::Check).size(px(16.0)))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _, _, cx| {
                                                this.save_note_and_close(cx);
                                            }),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("note_editor_close")
                                        .cursor_pointer()
                                        .rounded_sm()
                                        .hover(move |s| s.bg(theme.muted))
                                        .on_hover(cx.listener(move |_this, _, _, cx| {
                                            cx.notify();
                                        }))
                                        .child(Icon::new(IconName::Close).size(px(16.0)))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _, _, cx| {
                                                this.close_note_editor(cx);
                                            }),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    fn save_note_and_close(&mut self, cx: &mut Context<Self>) {
        let text = self
            .note_input_state
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();

        if let Some(editor) = &self.annotation_state.note_editor {
            let id = editor.annotation_id.clone();
            self.update_and_save(&id, |ann| {
                ann.note = Some(text);
                ann.updated_at = chrono::Utc::now().timestamp();
            });
        }

        self.note_input_state = None;
        self.note_input_sub = None;
        self.annotation_state.note_editor = None;
        cx.notify();
    }

    pub(crate) fn close_note_editor(&mut self, cx: &mut Context<Self>) {
        self.note_input_state = None;
        self.note_input_sub = None;
        self.annotation_state.note_editor = None;
        cx.notify();
    }

    pub(crate) fn save_sidebar_note(&mut self, cx: &mut Context<Self>) {
        let id = match self.editing_note_sidebar_id.clone() {
            Some(id) => id,
            None => return,
        };
        let text = self
            .editing_note_sidebar_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        self.update_and_save(&id, |ann| {
            ann.note = Some(text);
            ann.updated_at = chrono::Utc::now().timestamp();
        });
        self.editing_note_sidebar_id = None;
        self.editing_note_sidebar_input = None;
        self.editing_note_sidebar_sub = None;
        cx.notify();
    }

    pub(crate) fn cancel_sidebar_note(&mut self, cx: &mut Context<Self>) {
        self.editing_note_sidebar_id = None;
        self.editing_note_sidebar_input = None;
        self.editing_note_sidebar_sub = None;
        cx.notify();
    }

    /// 获取选区在页面局部坐标系下的包围盒 (min_x, min_y, max_x, max_y)
    fn get_selection_bounds(
        &self,
        state: &services::pdf::AnnotationToolbarState,
    ) -> Option<(f32, f32, f32, f32)> {
        let data = self
            .page_text_data
            .get(state.start_page as usize)?
            .as_ref()?;
        let end_on_page = if state.start_page == state.end_page {
            state.end_char
        } else {
            data.chars.len().saturating_sub(1)
        };
        if state.start_char > end_on_page {
            return None;
        }
        let blocks = data.merge_char_blocks(state.start_char, end_on_page);
        let first = *blocks.first()?;
        let (mut min_x, mut min_y, mut max_x, mut max_y) = first;
        for &(bx, by, bx2, by2) in &blocks[1..] {
            min_x = min_x.min(bx);
            min_y = min_y.min(by);
            max_x = max_x.max(bx2);
            max_y = max_y.max(by2);
        }
        Some((min_x, min_y, max_x, max_y))
    }

    /// 获取指定页面在视口（窗口）坐标系下的左上角原点 (origin_x, origin_y)
    fn get_page_screen_origin(&self, page_index: usize, window: &Window) -> Option<(f32, f32)> {
        let scroll_top = self.list_state.logical_scroll_top();
        if page_index < scroll_top.item_ix {
            return None;
        }

        let rem_size_px = f32::from(window.rem_size());
        let toolbar_h = f32::from(gpui::rems(TOOLBAR_HEIGHT_REMS).to_pixels(window.rem_size()));

        // 计算 Y 坐标：累计前面页面的高度并减去当前页的滚动偏移量
        let mut screen_y = toolbar_h - f32::from(scroll_top.offset_in_item);
        for ix in scroll_top.item_ix..page_index {
            screen_y += helpers::page_height(&self.page_sizes, ix, self.zoom_level, rem_size_px);
        }

        // 计算 X 坐标：剔除侧边栏后的可用居中区域
        let left_w = if self.is_left_sidebar_open {
            f32::from(self.left_sidebar_width)
        } else {
            0.0
        };
        let right_w = if self.is_right_sidebar_open {
            f32::from(self.right_sidebar_width)
        } else {
            0.0
        };
        let available_w = f32::from(window.viewport_size().width) - left_w - right_w;
        let display_w = PAGE_BASE_WIDTH_REMS * self.zoom_level * rem_size_px;
        let screen_x = left_w + (available_w - display_w) / 2.0 + self.offset_x;

        Some((screen_x, screen_y))
    }

    /// 计算工具栏在屏幕（窗口）坐标系中的位置
    pub(crate) fn compute_toolbar_screen_pos(
        &mut self,
        window: &Window,
    ) -> Option<(Pixels, Pixels)> {
        let state = self.annotation_state.toolbar.as_ref()?;
        let (min_x, min_y, max_x, max_y) = self.get_selection_bounds(state)?;
        let (page_x, page_y) = self.get_page_screen_origin(state.start_page as usize, window)?;

        let center_x = page_x + (min_x + max_x) / 2.0;
        let text_top_y = page_y + min_y;
        let text_bottom_y = page_y + max_y;

        const TOOLBAR_W: f32 = 200.0;
        const TOOLBAR_H: f32 = 80.0;

        let viewport_w = f32::from(window.viewport_size().width);
        let viewport_h = f32::from(window.viewport_size().height) - self.tab_bar_offset_px;

        let tool_x = (center_x - TOOLBAR_W / 2.0).clamp(0.0, (viewport_w - TOOLBAR_W).max(0.0));
        let tool_y = if text_bottom_y + 6.0 + TOOLBAR_H > viewport_h {
            text_top_y - TOOLBAR_H - 6.0
        } else {
            text_bottom_y + 6.0
        };

        Some((px(tool_x), px(tool_y)))
    }

    pub(crate) fn close_annotation_toolbar(&mut self, cx: &mut Context<Self>) {
        self.annotation_state.toolbar = None;
        self.annotation_toolbar_menu = None;
        self.selection_start = None;
        self.selection_end = None;
        self.selected_text = None;
        cx.notify();
    }

    pub(crate) fn build_toolbar_popup_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(gpui::Point<gpui::Pixels>, gpui::Entity<PopupMenu>)> {
        let pos = self.compute_toolbar_screen_pos(window)?;
        let point = gpui::Point { x: pos.0, y: pos.1 };
        let items = annotation_picker_items(cx.weak_entity(), AnnotationPickerMode::Create);
        let app: &mut App = cx;
        let menu = PopupMenu::build(window, app, |mut m, _, _| {
            for item in items {
                m = m.item(item);
            }
            m
        });
        Some((point, menu))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_annotation_from_selection(
        &mut self,
        start_page: u16,
        start_char: usize,
        end_page: u16,
        end_char: usize,
        kind: services::pdf::AnnotationKind,
        color: AnnotationColor,
        cx: &mut Context<Self>,
    ) {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        let annotation = services::pdf::Annotation {
            id: id.clone(),
            document_id: self.document_id.clone(),
            page: start_page,
            kind,
            color,
            range: Some(services::pdf::TextRange {
                start_page,
                start_char,
                end_page: if end_page != start_page {
                    Some(end_page)
                } else {
                    None
                },
                end_char,
            }),
            note: None,
            created_at: now,
            updated_at: now,
            version: 1,
            is_deleted: false,
            is_dirty: true,
        };

        self.annotation_state
            .annotations
            .entry(start_page)
            .or_default()
            .push(annotation.clone());

        if let Some(delegate) = &self.delegate {
            delegate.save_annotation(&annotation);
        }
        self.annotation_version += 1;
        cx.notify();
    }

    /// 将矩形标注区域从原始页面图中裁剪并复制到剪贴板。
    fn copy_rect_image(&mut self, page: u16, x: f32, y: f32, w: f32, h: f32) {
        let img = match self.raw_page_images.get(page as usize) {
            Some(Some(raw)) => &**raw,
            _ => return,
        };
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        let cx = (x * iw) as u32;
        let cy = (y * ih) as u32;
        let cw = (w * iw) as u32;
        let ch = (h * ih) as u32;
        let cropped = image::imageops::crop_imm(img, cx, cy, cw, ch).to_image();
        helpers::copy_rgba_to_clipboard(&cropped);
    }

    /// 将矩形标注区域裁剪并另存为 PNG 文件。
    fn save_rect_image(
        &mut self,
        page: u16,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        cx: &mut Context<Self>,
    ) {
        let img = match self.raw_page_images.get(page as usize) {
            Some(Some(raw)) => &**raw,
            _ => return,
        };
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        let crop_x = (x * iw) as u32;
        let crop_y = (y * ih) as u32;
        let crop_w = (w * iw) as u32;
        let crop_h = (h * ih) as u32;
        let cropped = image::imageops::crop_imm(img, crop_x, crop_y, crop_w, crop_h).to_image();
        let name = self.document_title.clone();

        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择保存位置".into()),
        });

        cx.background_executor()
            .spawn(async move {
                if let Ok(Ok(Some(paths))) = receiver.await
                    && let Some(dir) = paths.first()
                {
                    let path = dir.join(format!("{}.png", name));
                    let _ = cropped.save(&path);
                }
            })
            .detach();
    }
}

// ─── 共享函数：标注选取器（颜色圆点 + 类型按钮） ──────────────────────

fn annotation_picker_items(
    weak_self: WeakEntity<PdfReaderView>,
    mode: AnnotationPickerMode,
) -> Vec<PopupMenuItem> {
    let is_text = match &mode {
        AnnotationPickerMode::Create => true,
        AnnotationPickerMode::Edit { current_kind, .. } => matches!(
            current_kind,
            services::pdf::AnnotationKind::Highlight | services::pdf::AnnotationKind::Underline
        ),
    };

    let mut items: Vec<PopupMenuItem> = Vec::new();

    // ── 颜色圆点行 ──
    let mode_dots = mode.clone();
    items.push({
        let w = weak_self.clone();
        PopupMenuItem::element(move |_window, cx| {
            let w = w.clone();
            h_flex()
                .gap_1()
                .px_1()
                .py_1()
                .children(ALL_COLORS.iter().map(|&ac| {
                    let hsla = ac.to_hsla();
                    let dot_hover = cx.theme().primary.opacity(0.08);
                    match mode_dots.clone() {
                        AnnotationPickerMode::Create => {
                            let weak = w.clone();
                            div()
                                .size_5()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .cursor_pointer()
                                .hover(move |s| s.bg(dot_hover))
                                .child(div().size_4().rounded_full().bg(hsla))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    if let Some(this) = weak.upgrade() {
                                        this.update(cx, |this, cx| {
                                            if let Some(ref toolbar) = this.annotation_state.toolbar
                                            {
                                                let kind = match this.annotation_state.toolbar_kind
                                                {
                                                    ToolbarAnnotationKind::Highlight => {
                                                        services::pdf::AnnotationKind::Highlight
                                                    }
                                                    ToolbarAnnotationKind::Underline => {
                                                        services::pdf::AnnotationKind::Underline
                                                    }
                                                };
                                                this.annotation_state.last_highlight_color = ac;
                                                this.create_annotation_from_selection(
                                                    toolbar.start_page,
                                                    toolbar.start_char,
                                                    toolbar.end_page,
                                                    toolbar.end_char,
                                                    kind,
                                                    ac,
                                                    cx,
                                                );
                                            }
                                            this.close_annotation_toolbar(cx);
                                        });
                                    }
                                })
                        }
                        AnnotationPickerMode::Edit {
                            ann_id,
                            current_color,
                            ..
                        } => {
                            let is_active = current_color == ac;
                            let active_hover = cx.theme().primary.opacity(0.15);
                            let weak = w.clone();
                            div()
                                .size_5()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .cursor_pointer()
                                .when(is_active, |this| {
                                    this.border_2().border_color(cx.theme().foreground)
                                })
                                .hover(move |s| {
                                    if is_active {
                                        s.bg(active_hover)
                                    } else {
                                        s.bg(dot_hover)
                                    }
                                })
                                .child(div().size_4().rounded_full().bg(hsla))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    if let Some(this) = weak.upgrade() {
                                        this.update(cx, |this, cx| {
                                            this.update_and_save(&ann_id, |ann| {
                                                ann.color = ac;
                                                ann.updated_at = chrono::Utc::now().timestamp();
                                            });
                                            this.annotation_state.last_highlight_color = ac;
                                            this.annotation_context_menu = None;
                                            this.annotation_version += 1;
                                            cx.notify();
                                        });
                                    }
                                })
                        }
                    }
                }))
        })
        .disabled(true)
    });

    items.push(PopupMenuItem::Separator);

    // ── 类型切换行 ──
    if is_text {
        match mode {
            AnnotationPickerMode::Create => {
                let w = weak_self.clone();
                items.push(
                    PopupMenuItem::element(move |_window, cx| {
                        let w = w.clone();
                        let fg = cx.theme().foreground;
                        let active_bg = cx.theme().primary.opacity(0.15);
                        let accent_bg = cx.theme().tokens.accent;
                        let accent_fg = cx.theme().tokens.accent_foreground;

                        let is_hl_active = w
                            .upgrade()
                            .map(|e| {
                                e.read(cx).annotation_state.toolbar_kind
                                    == ToolbarAnnotationKind::Highlight
                            })
                            .unwrap_or(false);
                        let is_ul_active = w
                            .upgrade()
                            .map(|e| {
                                e.read(cx).annotation_state.toolbar_kind
                                    == ToolbarAnnotationKind::Underline
                            })
                            .unwrap_or(false);

                        h_flex()
                            .w_full()
                            .px_1()
                            .py_1()
                            .gap_2()
                            .text_color(fg)
                            .child(
                                div().flex_1().child(
                                    h_flex()
                                        .id("toolbar_type_btn_Highlight")
                                        .w_full()
                                        .px_2()
                                        .py_1()
                                        .rounded_sm()
                                        .cursor_pointer()
                                        .justify_center()
                                        .items_center()
                                        .when(is_hl_active, |this| this.bg(active_bg))
                                        .hover(move |s| {
                                            if is_hl_active {
                                                s.bg(active_bg)
                                            } else {
                                                s.bg(accent_bg).text_color(accent_fg)
                                            }
                                        })
                                        .child(
                                            div().child(i18n::t(
                                                I18nKey::Highlight,
                                                Default::default(),
                                            )),
                                        )
                                        .on_mouse_down(MouseButton::Left, {
                                            let w2 = w.clone();
                                            move |_, _, cx| {
                                                if let Some(this) = w2.upgrade() {
                                                    this.update(cx, |this, cx| {
                                                        this.annotation_state.toolbar_kind =
                                                            ToolbarAnnotationKind::Highlight;
                                                        this.overlay_button_clicked = true;
                                                        cx.notify();
                                                    });
                                                }
                                            }
                                        }),
                                ),
                            )
                            .child(
                                div().flex_1().child(
                                    h_flex()
                                        .id("toolbar_type_btn_Underline")
                                        .w_full()
                                        .px_2()
                                        .py_1()
                                        .rounded_sm()
                                        .cursor_pointer()
                                        .justify_center()
                                        .items_center()
                                        .when(is_ul_active, |this| this.bg(active_bg))
                                        .hover(move |s| {
                                            if is_ul_active {
                                                s.bg(active_bg)
                                            } else {
                                                s.bg(accent_bg).text_color(accent_fg)
                                            }
                                        })
                                        .child(
                                            div().child(i18n::t(
                                                I18nKey::Underline,
                                                Default::default(),
                                            )),
                                        )
                                        .on_mouse_down(MouseButton::Left, {
                                            let w2 = w.clone();
                                            move |_, _, cx| {
                                                if let Some(this) = w2.upgrade() {
                                                    this.update(cx, |this, cx| {
                                                        this.annotation_state.toolbar_kind =
                                                            ToolbarAnnotationKind::Underline;
                                                        this.overlay_button_clicked = true;
                                                        cx.notify();
                                                    });
                                                }
                                            }
                                        }),
                                ),
                            )
                    })
                    .disabled(true),
                );
            }
            AnnotationPickerMode::Edit {
                ann_id,
                current_kind,
                ..
            } => {
                let w = weak_self.clone();
                let aid = ann_id.clone();
                let cur_kind = current_kind.clone();
                items.push(
                    PopupMenuItem::element(move |_window, cx| {
                        let w = w.clone();
                        let aid = aid.clone();
                        let kind = cur_kind.clone();
                        let fg = cx.theme().foreground;
                        let active_bg = cx.theme().primary.opacity(0.15);
                        let accent_bg = cx.theme().tokens.accent;
                        let accent_fg = cx.theme().tokens.accent_foreground;

                        let is_hl_active = kind == services::pdf::AnnotationKind::Highlight;
                        let is_ul_active = kind == services::pdf::AnnotationKind::Underline;

                        h_flex()
                            .w_full()
                            .px_1()
                            .py_1()
                            .gap_2()
                            .text_color(fg)
                            .child(div().flex_1().child({
                                let w = w.clone();
                                let aid = aid.clone();
                                h_flex()
                                    .w_full()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .justify_center()
                                    .items_center()
                                    .when(is_hl_active, |this| this.bg(active_bg))
                                    .hover(move |s| {
                                        if is_hl_active {
                                            s.bg(active_bg)
                                        } else {
                                            s.bg(accent_bg).text_color(accent_fg)
                                        }
                                    })
                                    .child(
                                        div()
                                            .child(i18n::t(I18nKey::Highlight, Default::default())),
                                    )
                                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        if let Some(this) = w.upgrade() {
                                            this.update(cx, |this, cx| {
                                                this.update_and_save(&aid, |ann| {
                                                    ann.kind =
                                                        services::pdf::AnnotationKind::Highlight;
                                                    ann.updated_at = chrono::Utc::now().timestamp();
                                                });
                                                this.annotation_context_menu = None;
                                                this.annotation_version += 1;
                                                cx.notify();
                                            });
                                        }
                                    })
                            }))
                            .child(div().flex_1().child({
                                let w = w.clone();
                                let aid = aid.clone();
                                h_flex()
                                    .w_full()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .justify_center()
                                    .items_center()
                                    .when(is_ul_active, |this| this.bg(active_bg))
                                    .hover(move |s| {
                                        if is_ul_active {
                                            s.bg(active_bg)
                                        } else {
                                            s.bg(accent_bg).text_color(accent_fg)
                                        }
                                    })
                                    .child(
                                        div()
                                            .child(i18n::t(I18nKey::Underline, Default::default())),
                                    )
                                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        if let Some(this) = w.upgrade() {
                                            this.update(cx, |this, cx| {
                                                this.update_and_save(&aid, |ann| {
                                                    ann.kind =
                                                        services::pdf::AnnotationKind::Underline;
                                                    ann.updated_at = chrono::Utc::now().timestamp();
                                                });
                                                this.annotation_context_menu = None;
                                                this.annotation_version += 1;
                                                cx.notify();
                                            });
                                        }
                                    })
                            }))
                    })
                    .disabled(true),
                );
            }
        }
        items.push(PopupMenuItem::Separator);
    }

    items
}

const ALL_COLORS: [AnnotationColor; 8] = [
    AnnotationColor::Yellow,
    AnnotationColor::Red,
    AnnotationColor::Green,
    AnnotationColor::Blue,
    AnnotationColor::Purple,
    AnnotationColor::Magenta,
    AnnotationColor::Orange,
    AnnotationColor::Gray,
];
