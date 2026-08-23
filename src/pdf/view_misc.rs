use ::components::{Side, render_resize_handle};
use gpui::prelude::*;
use gpui::{
    Context, Entity, MouseMoveEvent, MouseUpEvent, Pixels, Point, Window, deferred, div, px, rems,
};
use gpui_component::ActiveTheme;
use gpui_component::menu::PopupMenu;

use super::*;

impl super::PdfReaderView {
    pub(crate) fn apply_horizontal_scroll(&mut self, dx: f32, window: &Window) {
        if dx == 0.0 {
            return;
        }
        let rem_size_px = f32::from(window.rem_size());
        let display_width_px = PAGE_BASE_WIDTH_REMS * self.zoom_level * rem_size_px;
        let mut available_width = f32::from(window.viewport_size().width);
        if self.is_left_sidebar_open {
            available_width -= f32::from(self.left_sidebar_width);
        }
        if self.is_right_sidebar_open {
            available_width -= f32::from(self.right_sidebar_width);
        }

        if !self.fit_to_width_mode && display_width_px > available_width {
            let max_offset = (display_width_px - available_width) / 2.0;
            self.offset_x = (self.offset_x + dx).clamp(-max_offset, max_offset);
        } else {
            self.offset_x = 0.0;
        }
    }

    pub(crate) fn render_sidebar_resizer(
        &self,
        is_left: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (side, offset) = if is_left {
            (Side::Left, self.left_sidebar_width)
        } else {
            (Side::Right, self.right_sidebar_width)
        };

        // 可见细线，内嵌在热区（绝对定位）内部并居中：
        // 热区宽 rems(0.375)，细线宽 rems(0.125)，居中偏移 = (0.375-0.125)/2 = rems(0.125)。
        let line = div()
            .absolute()
            .top_0()
            .h_full()
            .w(rems(0.125))
            .bg(cx.theme().border)
            .left(rems(0.125));

        // 直接返回热区（作为 relative 容器的直接子元素），不再用 0 宽包裹层。
        // 否则右侧 right(offset) 会相对 0 宽包裹层计算而跑到窗口外不可见。
        // deferred 仅延迟绘制到顶层，布局仍属于当前树，故定位上下文仍是整宽容器。
        deferred(
            render_resize_handle(side, offset)
                .id(if is_left {
                    "pdf-left-resizer"
                } else {
                    "pdf-right-resizer"
                })
                .child(line)
                .on_drag(DraggedSidebar(is_left), |drag, _, _, cx| {
                    cx.new(|_| drag.clone())
                }),
        )
    }

    pub fn is_content_interacting(&self) -> bool {
        self.is_mouse_down
            || self.annotation_drag.is_some()
            || self.dragging_pin.is_some()
            || self.resizing_pin.is_some()
    }

    pub fn handle_global_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_content_interacting() {
            self.handle_content_mouse_move(event, window, cx);
        } else if self.is_dragging_scrollbar || self.is_dragging_thumbnail_scrollbar {
            self.handle_root_mouse_move(event, window, cx);
        }
    }

    pub fn handle_global_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_content_interacting() {
            self.handle_content_mouse_up(event.position, window, cx);
            self.is_panning = false;
        } else if self.is_dragging_scrollbar || self.is_dragging_thumbnail_scrollbar {
            self.handle_root_mouse_up(cx);
        }
    }

    pub(crate) fn render_menu_overlay(
        &self,
        pos: Point<Pixels>,
        menu: Entity<PopupMenu>,
        window: &Window,
        menu_w: f32,
        menu_h: f32,
    ) -> impl IntoElement {
        let local_x = f32::from(pos.x).max(0.0);
        let local_y = f32::from(pos.y);
        let h_flex_w = f32::from(window.viewport_size().width);
        let h_flex_h = f32::from(window.viewport_size().height) - self.tab_bar_offset_px;

        let clamp_x = local_x.clamp(0.0, (h_flex_w - menu_w).max(0.0));
        let clamp_y = local_y.min((h_flex_h - menu_h).max(0.0));

        div()
            .absolute()
            .left(px(clamp_x))
            .top(px(clamp_y))
            .cursor_default()
            .child(menu)
    }
}
