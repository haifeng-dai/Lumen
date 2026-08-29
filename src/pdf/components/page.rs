use super::super::PdfReaderView;
use crate::pdf::helpers;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, InteractiveElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, ScrollWheelEvent, Styled, Window, div, img, px,
};
use gpui_component::{ActiveTheme, v_flex};
use log::debug;
use services::pdf::TextPageData;
use std::sync::Arc;

fn thumbnail_response_in_keep_range(
    page: usize,
    keep_range: (usize, usize),
    total_pages: usize,
) -> bool {
    total_pages == 0 || (page >= keep_range.0 && page <= keep_range.1)
}

#[cfg(test)]
mod thumbnail_response_tests {
    use super::thumbnail_response_in_keep_range;

    #[test]
    fn layout_union_keeps_old_in_flight_response() {
        assert!(thumbnail_response_in_keep_range(15, (9, 16), 30));
    }

    #[test]
    fn stable_eviction_drops_response_outside_keep_range() {
        assert!(!thumbnail_response_in_keep_range(15, (9, 13), 30));
    }

    #[test]
    fn empty_document_has_no_invalid_response() {
        assert!(thumbnail_response_in_keep_range(99, (0, 0), 0));
    }
}

impl PdfReaderView {
    /// 缓存主页面渲染图。raw_image 存入 raw_page_images 供选区和 Pin 裁剪用，
    /// 同时转为 ImageSource 存入 page_images 供 GPUI 渲染。
    pub(crate) fn cache_page_image(
        &mut self,
        page: u16,
        raw_image: image::RgbaImage,
        cx: &mut Context<Self>,
    ) {
        debug!(
            "page: 缓存第 {} 页图片, 分辨率 {}x{}",
            page,
            raw_image.width(),
            raw_image.height()
        );
        let raw_arc = Arc::new(raw_image.clone());
        let img_src = helpers::make_image_source(raw_image);

        if let Some(slot) = self.raw_page_images.get_mut(page as usize) {
            *slot = Some(raw_arc);
        }
        if let Some(slot) = self.page_images.get_mut(page as usize) {
            if let Some(gpui::ImageSource::Render(render_img)) = slot.take() {
                self.pending_drop_images.push(render_img);
            }
            *slot = Some(img_src);
        }
        cx.notify();
    }

    /// 缓存缩略图渲染图。只存 ImageSource，不存 raw 数据（缩略图无需选区/Pin 裁剪）。
    pub(crate) fn cache_thumbnail_image(
        &mut self,
        page: u16,
        image: image::RgbaImage,
        cx: &mut Context<Self>,
    ) {
        debug!(
            "page: 缓存第 {} 页缩略图, 分辨率 {}x{}",
            page,
            image.width(),
            image.height()
        );
        let img_src = helpers::make_image_source(image);
        if let Some(slot) = self.thumbnail_images.get_mut(page as usize) {
            if let Some(gpui::ImageSource::Render(render_img)) = slot.take() {
                self.pending_drop_images.push(render_img);
            }
            *slot = Some(img_src);
        }
        cx.notify();
    }

    pub(crate) fn on_page_rendered(
        &mut self,
        page: u16,
        image: image::RgbaImage,
        cx: &mut Context<Self>,
    ) {
        // 丢弃已离开可见范围的过期响应
        let page_usize = page as usize;
        let buffer = 1;
        if self.total_pages > 0
            && (page_usize + buffer < self.visible_page_first
                || page_usize > self.visible_page_last + buffer)
        {
            debug!(
                "page: 丢弃过期渲染响应 page={}, visible=[{}, {}]",
                page, self.visible_page_first, self.visible_page_last
            );
            self.page_render_requests_pending.remove(&page);
            return;
        }
        self.page_render_requests_pending.remove(&page);
        self.cache_page_image(page, image, cx);
    }

    pub(crate) fn on_thumbnail_rendered(
        &mut self,
        page: u16,
        image: image::RgbaImage,
        cx: &mut Context<Self>,
    ) {
        // 丢弃已离开可见范围的过期响应
        let page_usize = page as usize;
        let was_pending = self.thumb_render_requests_pending.remove(&page);
        if !was_pending {
            // 文档页结构变化会清理 pending；清理后的旧响应不得写入新索引。
            return;
        }
        let (keep_first, keep_last) = self.thumbnail_response_keep_range;
        if !thumbnail_response_in_keep_range(page_usize, (keep_first, keep_last), self.total_pages)
        {
            return;
        }
        self.cache_thumbnail_image(page, image, cx);
    }

    pub(crate) fn on_text_extracted(
        &mut self,
        page: u16,
        data: TextPageData,
        cx: &mut Context<Self>,
    ) {
        if let Some(slot) = self.page_text_data.get_mut(page as usize) {
            *slot = Some(Arc::new(data));
        }
        self.find_char_cache.remove(&page);

        // 写入 search_text_storage 并触发增量搜索
        if let Some(ref mut storage) = self.search_text_storage {
            if (page as usize) < storage.len() {
                storage[page as usize] = self
                    .page_text_data
                    .get(page as usize)
                    .and_then(|d| d.clone());
            }

            if let Some(ref state) = self.search_state
                && !state.query.is_empty()
            {
                let query_lower = state.query.to_lowercase();
                let query_chars: Vec<char> = query_lower.chars().collect();
                if !query_chars.is_empty() {
                    let new_matches = self.search_page_single(page, &query_chars);
                    self.merge_search_results(page, new_matches, cx);
                }
            }
        }

        cx.notify();
    }

    /// 计算当前视口落在哪些页面（first, last），含上下各 1 页缓冲。
    /// 在 render() 中由 refresh_page_visibility 调用。
    pub(crate) fn calculate_visible_range(&self, window: &Window) -> (usize, usize) {
        if self.total_pages == 0 {
            return (0, 0);
        }

        let scroll_top = self.list_state.logical_scroll_top();
        let rem_px = f32::from(window.rem_size());
        let toolbar_height = super::super::types::TOOLBAR_HEIGHT_REMS * rem_px;
        let tab_bar_h = self.tab_bar_offset_px;
        let view_height = f32::from(window.viewport_size().height) - tab_bar_h - toolbar_height;

        // 计算 viewport_top_abs：视口顶部在全局坐标中的绝对 Y
        let mut viewport_top_abs = 0.0;
        for i in 0..scroll_top.item_ix {
            viewport_top_abs +=
                super::super::helpers::page_height(&self.page_sizes, i, self.zoom_level, rem_px);
        }
        viewport_top_abs += f32::from(scroll_top.offset_in_item);

        // 遍历页面找到 first_visible 和 last_visible
        let mut acc = 0.0;
        let mut first = self.total_pages - 1;
        let mut last = self.total_pages - 1;

        for i in 0..self.total_pages {
            let h =
                super::super::helpers::page_height(&self.page_sizes, i, self.zoom_level, rem_px);
            if acc + h > viewport_top_abs && first == self.total_pages - 1 {
                first = i;
            }
            if acc + h > viewport_top_abs + view_height {
                last = i;
                break;
            }
            acc += h;
        }

        (first, last)
    }

    /// 淘汰可见范围 [keep_first-1, keep_last+1] 之外的页面数据，释放内存。
    pub(crate) fn evict_distant_pages(
        &mut self,
        keep_first: usize,
        keep_last: usize,
        window: &mut Window,
    ) {
        let range_start = keep_first.saturating_sub(1);
        let range_end = (keep_last + 1).min(self.total_pages.saturating_sub(1));

        for i in 0..self.total_pages {
            if i >= range_start && i <= range_end {
                continue;
            }
            if self.page_images[i].is_some()
                || self.raw_page_images[i].is_some()
                || self.page_text_data[i].is_some()
                || self.page_link_data[i].is_some()
            {
                if let Some(gpui::ImageSource::Render(render_img)) = self.page_images[i].take()
                    && let Err(e) = window.drop_image(render_img)
                {
                    log::error!("drop_image failed: {e}");
                }
                self.raw_page_images[i] = None;
                self.page_text_data[i] = None;
                self.page_link_data[i] = None;
            }
        }
    }

    /// 对可见范围 [first-1, last+1] 内尚未渲染的页面发送渲染/文本/链接请求。
    pub(crate) fn schedule_page_renders(
        &mut self,
        first: usize,
        last: usize,
        cx: &mut Context<Self>,
    ) {
        if self.worker_state != super::super::types::WorkerState::Running {
            return;
        }

        let range_end = (last + 1).min(self.total_pages.saturating_sub(1));
        let scale = self.render_zoom * self.window_scale_factor * 1.2;
        let rem_px = self.last_rem_size;

        // 视区内的页面先发（上到下），缓冲页后发
        for page in first..=last.min(self.total_pages.saturating_sub(1)) {
            self.ensure_page_data_loaded(page, scale, rem_px, cx);
        }
        // 缓冲：上一页
        if first > 0 {
            self.ensure_page_data_loaded(first - 1, scale, rem_px, cx);
        }
        // 缓冲：下一页
        if range_end > last {
            self.ensure_page_data_loaded(range_end, scale, rem_px, cx);
        }
    }

    /// 对单个页面：若图像/文本/链接缺失则发送对应请求。
    fn ensure_page_data_loaded(
        &mut self,
        page: usize,
        scale: f32,
        rem_px: f32,
        cx: &mut Context<Self>,
    ) {
        let page_u16 = page as u16;

        // 图像：无图或缩放刚变 → 重新发送渲染请求
        if (self.page_images[page].is_none() || self.zoom_changed)
            && !self.page_render_requests_pending.contains(&page_u16)
        {
            self.page_render_requests_pending.insert(page_u16);
            self.pdf_service.send_render(page_u16, scale, 0);
        }

        // 文本
        let (display_w, display_h) = super::super::helpers::page_display_size(
            &self.page_sizes,
            page,
            self.zoom_level,
            rem_px,
        );
        self.load_page_text_with_size(page_u16, display_w, cx);
        self.load_page_links_with_size(page_u16, display_w, display_h, cx);
    }

    /// 统一入口：计算可见范围 → 淘汰远页 → 调度渲染请求。
    pub(crate) fn refresh_page_visibility(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (first, last) = self.calculate_visible_range(window);
        if first == self.visible_page_first && last == self.visible_page_last && !self.zoom_changed
        {
            return;
        }
        self.visible_page_first = first;
        self.visible_page_last = last;
        self.evict_distant_pages(first, last, window);
        self.schedule_page_renders(first, last, cx);
        self.zoom_changed = false;
    }

    pub(crate) fn render_list_item(
        &mut self,
        index: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let page_index = index as u16;
        let zoom = self.zoom_level;
        let rem_size = window.rem_size();

        // 1. 基础逻辑尺寸 (Logical Pixels)
        let rem_px = f32::from(rem_size);
        let (display_width_px, display_height_px) =
            helpers::page_display_size(&self.page_sizes, index, zoom, rem_px);

        // 文本/链接数据懒刷新（display_w 匹配则跳过，不匹配自动重请求）
        self.load_page_text_with_size(page_index, display_width_px, cx);
        self.load_page_links_with_size(page_index, display_width_px, display_height_px, cx);

        if self.annotation_version != self.last_composited_version {
            // 注释版本变化时，需要重新合成所有已渲染的页面
            // 这里简化处理，直接通知刷新
            self.last_composited_version = self.annotation_version;
        }

        let content = {
            if let Some(Some(img_src)) = self.page_images.get(page_index as usize) {
                img(img_src.clone())
                    .id(("pdf-page-img", page_index as usize))
                    .w(px(display_width_px))
                    .h(px(display_height_px))
                    .into_any_element()
            } else {
                // 尚未渲染：纯底色，无占位符
                div()
                    .w(px(display_width_px))
                    .h(px(display_height_px))
                    .bg(cx.theme().background)
                    .into_any_element()
            }
        };

        // 下方注释层（Highlight + Underline — 在 PDF 图像之下，透明区显底色）
        let below_annotation_overlay = self.render_below_annotation_overlay(page_index, window, cx);

        // 上方注释层（Rectangle + rect_in_progress — 在 PDF 图像之上）
        let above_annotation_overlay = self.render_above_annotation_overlay(page_index, window, cx);

        // 链接层
        let link_overlay = self.render_link_overlay(page_index, window, cx);

        // 选区层
        let selection_highlight = self.render_selection_highlight(page_index, window, cx);

        // 组合渲染：建立坐标沙盒并设定唯一 ID，规避 GPUI 列表元素复用差分渲染时的图像错置 Bug
        v_flex()
            .id(("pdf-page-item", page_index as usize))
            .w_full()
            .overflow_hidden()
            .items_center()
            .child(
                div()
                    .shadow_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(self.page_color_mode.bg_color())
                    .left(px(self.offset_x))
                    .w(px(display_width_px))
                    .h(px(display_height_px))
                    .child(
                        div()
                            .relative()
                            .overflow_hidden()
                            .size_full()
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    if let Some((pid, px_x, px_y)) = this.content_to_page_coords(
                                        event.position.x,
                                        event.position.y,
                                        window,
                                    ) && pid == page_index
                                        && let Some(ann_id) = this.hit_test_annotation(
                                            pid,
                                            f32::from(px_x),
                                            f32::from(px_y),
                                            window,
                                        )
                                    {
                                        this.annotation_state.selected_id = Some(ann_id.clone());
                                        let menu = this.build_annotation_context_menu(
                                            &ann_id, false, window, cx,
                                        );
                                        let local_pos = gpui::point(
                                            event.position.x,
                                            px((f32::from(event.position.y)
                                                - this.tab_bar_offset_px)
                                                .max(0.0)),
                                        );
                                        this.annotation_context_menu = Some((local_pos, menu));
                                        cx.notify();
                                        return;
                                    }
                                    // 未命中任何注释 → 清除
                                    this.annotation_state.selected_id = None;
                                    this.annotation_context_menu = None;
                                    cx.notify();
                                }),
                            )
                            .when_some(below_annotation_overlay, |this, a| this.child(a))
                            .when_some(selection_highlight, |this, hl| this.child(hl))
                            .child(content)
                            .when_some(above_annotation_overlay, |this, a| this.child(a))
                            .when_some(link_overlay, |this, hl| this.child(hl))
                            .when_some(
                                self.render_search_highlight(page_index, window, cx),
                                |this, hl| this.child(hl),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn render_selection_highlight(
        &mut self,
        page_index: u16,
        _window: &Window,
        _cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let mut sp = self.selection_start?;
        let mut ep = self.selection_end?;
        if sp == ep {
            return None;
        }
        if sp.0 > ep.0 || (sp.0 == ep.0 && sp.1 > ep.1) {
            std::mem::swap(&mut sp, &mut ep);
        }

        if page_index < sp.0 || page_index > ep.0 {
            return None;
        }

        let text_data = self.page_text_data.get(page_index as usize)?.as_ref()?;
        let start = if page_index == sp.0 { sp.1 } else { 0 };
        let end = if page_index == ep.0 {
            ep.1
        } else {
            text_data.chars.len().saturating_sub(1)
        };

        let blocks = text_data.merge_char_blocks(start, end);
        if blocks.is_empty() {
            return None;
        }

        let highlights: Vec<_> = blocks
            .iter()
            .map(|&(bx, by, b_max_x, b_max_y)| {
                div()
                    .absolute()
                    .left(px(bx))
                    .top(px(by))
                    .w(px(b_max_x - bx))
                    .h(px((b_max_y - by).max(1.0)))
                    .bg(gpui::rgba(0x90CAF966))
                    .rounded(px(2.0))
                    .into_any_element()
            })
            .collect();

        Some(
            div()
                .absolute()
                .inset_0()
                .children(highlights)
                .into_any_element(),
        )
    }

    pub(crate) fn render_search_highlight(
        &mut self,
        page_index: u16,
        window: &Window,
        _cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let m = self
            .search_state
            .as_ref()
            .and_then(|s| s.active_match())
            .cloned()?;
        if m.page_index != page_index {
            return None;
        }

        let text_data = self
            .search_text_storage
            .as_ref()
            .and_then(|s| s.get(page_index as usize).and_then(|d| d.as_ref()))?;

        // 计算当前最新的显示宽度
        let rem_px = f32::from(window.rem_size());
        let (display_width_px, _) = helpers::page_display_size(
            &self.page_sizes,
            page_index as usize,
            self.zoom_level,
            rem_px,
        );

        // 计算物理坐标与缓存中坐标的比例关系，进行缩放调整
        let scale_ratio = if text_data.display_w > 0.0 {
            display_width_px / text_data.display_w
        } else {
            1.0
        };

        let blocks = text_data.merge_char_blocks(m.start_char, m.end_char.saturating_sub(1));
        if blocks.is_empty() {
            return None;
        }

        let highlights: Vec<_> = blocks
            .iter()
            .map(|&(bx, by, b_max_x, b_max_y)| {
                // 将原始或缓存中的逻辑像素坐标乘以 scale_ratio 换算至当前真实的缩放物理坐标
                let scaled_x = bx * scale_ratio;
                let scaled_y = by * scale_ratio;
                let scaled_w = (b_max_x - bx) * scale_ratio;
                let scaled_h = (b_max_y - by) * scale_ratio;

                div()
                    .absolute()
                    .left(px(scaled_x))
                    .top(px(scaled_y))
                    .w(px(scaled_w))
                    .h(px(scaled_h.max(1.0)))
                    .bg(gpui::rgba(0xf59e0b70))
                    .rounded(px(2.0))
                    .into_any_element()
            })
            .collect();

        Some(
            div()
                .absolute()
                .inset_0()
                .children(highlights)
                .into_any_element(),
        )
    }

    pub(crate) fn collect_annotations_for_page(
        &self,
        page_index: u16,
    ) -> Vec<services::pdf::Annotation> {
        let mut result = Vec::new();
        if let Some(anns) = self.annotation_state.annotations.get(&page_index) {
            for ann in anns {
                if !ann.is_deleted {
                    result.push(ann.clone());
                }
            }
        }
        for anns in self.annotation_state.annotations.values() {
            for ann in anns {
                if !ann.is_deleted
                    && let Some(ref range) = ann.range
                    && range.start_page < page_index
                    && range.end_page_or() >= page_index
                {
                    result.push(ann.clone());
                }
            }
        }
        result
    }

    pub(crate) fn get_annotation_gpui_color(
        color: services::pdf::AnnotationColor,
        kind: &services::pdf::AnnotationKind,
    ) -> gpui::Hsla {
        let alpha: f32 = match kind {
            services::pdf::AnnotationKind::Highlight => 0.376,
            services::pdf::AnnotationKind::Underline => 1.0,
            services::pdf::AnnotationKind::Rectangle { .. } => 1.0,
        };
        let mut hsla = color.to_hsla();
        hsla.a = alpha;
        hsla
    }

    pub(crate) fn create_annotation_element(
        bx: f32,
        by: f32,
        b_max_x: f32,
        b_max_y: f32,
        color: gpui::Hsla,
        kind: &services::pdf::AnnotationKind,
    ) -> AnyElement {
        match kind {
            services::pdf::AnnotationKind::Highlight => div()
                .absolute()
                .left(px(bx))
                .top(px(by))
                .w(px((b_max_x - bx).max(1.0)))
                .h(px((b_max_y - by).max(1.0)))
                .bg(color)
                .rounded(px(2.0))
                .into_any_element(),
            services::pdf::AnnotationKind::Underline => div()
                .absolute()
                .left(px(bx))
                .top(px(b_max_y - 2.0))
                .w(px((b_max_x - bx).max(1.0)))
                .h(px(2.0))
                .bg(color)
                .into_any_element(),
            services::pdf::AnnotationKind::Rectangle { .. } => div()
                .absolute()
                .left(px(bx))
                .top(px(by))
                .w(px((b_max_x - bx).max(1.0)))
                .h(px((b_max_y - by).max(1.0)))
                .border_2()
                .border_color(color)
                .rounded(px(2.0))
                .into_any_element(),
        }
    }

    pub(crate) fn render_below_annotation_overlay(
        &mut self,
        page_index: u16,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let anns = self.collect_annotations_for_page(page_index);
        if anns.is_empty() {
            return None;
        }

        let text_data = self
            .page_text_data
            .get(page_index as usize)
            .and_then(|d| d.as_ref());

        let mut elements: Vec<AnyElement> = Vec::new();

        for ann in &anns {
            let color = Self::get_annotation_gpui_color(ann.color, &ann.kind);
            let is_selected = self
                .annotation_state
                .selected_id
                .as_ref()
                .is_some_and(|id| id == &ann.id);
            match &ann.kind {
                // Highlight 和 Underline 都渲染在 PDF 图像之下
                services::pdf::AnnotationKind::Highlight
                | services::pdf::AnnotationKind::Underline => {
                    if let Some(ref range) = ann.range {
                        if page_index < range.start_page || page_index > range.end_page_or() {
                            continue;
                        }
                        let start = if page_index == range.start_page {
                            range.start_char
                        } else {
                            0
                        };
                        let end = if page_index == range.end_page_or() {
                            range.end_char
                        } else if let Some(td) = text_data {
                            td.chars.len().saturating_sub(1)
                        } else {
                            continue;
                        };
                        if let Some(td) = text_data
                            && start <= end
                            && end < td.chars.len()
                        {
                            let blocks = td.merge_char_blocks(start, end);
                            let is_highlight =
                                matches!(&ann.kind, services::pdf::AnnotationKind::Highlight);
                            // Highlight 和 Underline 都作为 overlay 元素渲染
                            for block in &blocks {
                                elements.push(Self::create_annotation_element(
                                    block.0, block.1, block.2, block.3, color, &ann.kind,
                                ));
                            }
                            // 选中时在所有字符块外围画一个框（Highlight 和 Underline 都保留）
                            if is_selected {
                                let mut border_color = color;
                                if is_highlight {
                                    border_color.a = 1.0;
                                }
                                if let Some((mx, my, mmx, mmy)) = blocks.iter().fold(
                                    None::<(f32, f32, f32, f32)>,
                                    |acc, &(bx, by, b_max_x, b_max_y)| {
                                        Some(match acc {
                                            Some((x0, y0, x1, y1)) => (
                                                x0.min(bx),
                                                y0.min(by),
                                                x1.max(b_max_x),
                                                y1.max(b_max_y),
                                            ),
                                            None => (bx, by, b_max_x, b_max_y),
                                        })
                                    },
                                ) {
                                    elements.push(
                                        div()
                                            .absolute()
                                            .left(px(mx - 4.0))
                                            .top(px(my - 4.0))
                                            .w(px((mmx - mx + 8.0).max(1.0)))
                                            .h(px((mmy - my + 8.0).max(1.0)))
                                            .border_3()
                                            .border_dashed()
                                            .border_color(border_color)
                                            .rounded(px(2.0))
                                            .into_any_element(),
                                    );
                                }

                                // 选中时在首尾字符块绘制文字扩选把手 (TextStart & TextEnd)
                                let ann_id = ann.id.clone();
                                let handle_color = color.opacity(0.85);

                                // 1. 起始位置把手
                                if let Some(first_block) = blocks.first() {
                                    let bx = first_block.0;
                                    let by = first_block.1;
                                    let bh = first_block.3 - first_block.1;

                                    let ann_id_clone = ann_id.clone();
                                    elements.push(
                                        div()
                                            .absolute()
                                            .left(px(bx - 6.0))
                                            .top(px(by - 3.0))
                                            .w(px(12.0))
                                            .h(px(bh + 6.0))
                                            .cursor_col_resize()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                                                    cx.stop_propagation();
                                                    this.is_mouse_down = true;
                                                    this.annotation_drag = Some(crate::pdf::AnnotationDragState {
                                                        annotation_id: ann_id_clone.clone(),
                                                        page: page_index,
                                                        handle: crate::pdf::AnnotationResizeHandle::TextStart,
                                                        start_mouse: event.position,
                                                        start_x: 0.0,
                                                        start_y: 0.0,
                                                        start_w: 0.0,
                                                        start_h: 0.0,
                                                    });
                                                    cx.notify();
                                                }),
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left(px(1.0))
                                                    .top(px(-5.0))
                                                    .w(px(10.0))
                                                    .h(px(10.0))
                                                    .bg(handle_color)
                                                    .rounded_full()
                                                    .border_2()
                                                    .border_color(cx.theme().border)
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left(px(5.0))
                                                    .top(px(3.0))
                                                    .w(px(2.0))
                                                    .h(px(bh))
                                                    .bg(handle_color)
                                            )
                                            .into_any_element(),
                                    );
                                }

                                // 2. 结束位置把手
                                if let Some(last_block) = blocks.last() {
                                    let b_max_x = last_block.2;
                                    let by = last_block.1;
                                    let bh = last_block.3 - last_block.1;

                                    let ann_id_clone = ann_id.clone();
                                    elements.push(
                                        div()
                                            .absolute()
                                            .left(px(b_max_x - 6.0))
                                            .top(px(by - 3.0))
                                            .w(px(12.0))
                                            .h(px(bh + 6.0))
                                            .cursor_col_resize()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                                                    cx.stop_propagation();
                                                    this.is_mouse_down = true;
                                                    this.annotation_drag = Some(crate::pdf::AnnotationDragState {
                                                        annotation_id: ann_id_clone.clone(),
                                                        page: page_index,
                                                        handle: crate::pdf::AnnotationResizeHandle::TextEnd,
                                                        start_mouse: event.position,
                                                        start_x: 0.0,
                                                        start_y: 0.0,
                                                        start_w: 0.0,
                                                        start_h: 0.0,
                                                    });
                                                    cx.notify();
                                                }),
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left(px(1.0))
                                                    .bottom(px(-5.0))
                                                    .w(px(10.0))
                                                    .h(px(10.0))
                                                    .bg(handle_color)
                                                    .rounded_full()
                                                    .border_2()
                                                    .border_color(cx.theme().border)
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left(px(5.0))
                                                    .top(px(3.0))
                                                    .w(px(2.0))
                                                    .h(px(bh))
                                                    .bg(handle_color)
                                            )
                                            .into_any_element(),
                                    );
                                }
                            }
                        }
                    }
                }
                services::pdf::AnnotationKind::Rectangle { .. } => {}
            }
        }

        if elements.is_empty() {
            return None;
        }

        Some(
            div()
                .absolute()
                .inset_0()
                .children(elements)
                .into_any_element(),
        )
    }

    pub(crate) fn render_above_annotation_overlay(
        &mut self,
        page_index: u16,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let anns = self.collect_annotations_for_page(page_index);
        if anns.is_empty() && self.rect_in_progress.is_none() {
            return None;
        }

        let rem_px = f32::from(window.rem_size());
        let (display_width_px, display_height_px) = helpers::page_display_size(
            &self.page_sizes,
            page_index as usize,
            self.zoom_level,
            rem_px,
        );

        let mut elements: Vec<AnyElement> = Vec::new();

        for ann in &anns {
            let color = Self::get_annotation_gpui_color(ann.color, &ann.kind);
            let is_selected = self
                .annotation_state
                .selected_id
                .as_ref()
                .is_some_and(|id| id == &ann.id);
            match &ann.kind {
                services::pdf::AnnotationKind::Rectangle { x, y, w, h } => {
                    let (rx_val, ry_val, rw_val, rh_val) = (*x, *y, *w, *h);
                    let rx_px = rx_val * display_width_px;
                    let ry_px = ry_val * display_height_px;
                    let rw_px = rw_val * display_width_px;
                    let rh_px = rh_val * display_height_px;

                    let mut rect = div()
                        .absolute()
                        .left(px(rx_px))
                        .top(px(ry_px))
                        .w(px((rw_px).max(1.0)))
                        .h(px((rh_px).max(1.0)))
                        .rounded(px(2.0))
                        .border_color(color);
                    rect = rect.border_3();
                    if is_selected {
                        rect = rect.border_dashed();
                    }

                    elements.push(rect.into_any_element());

                    if is_selected {
                        let ann_id = ann.id.clone();

                        // 1. 边缘拖拽感应带 (宽度 8px)
                        let sensor_thickness = 8.0;
                        let half_thickness = sensor_thickness / 2.0;

                        // 上边感应带
                        let ann_id_clone = ann_id.clone();
                        elements.push(
                            div()
                                .absolute()
                                .left(px(rx_px))
                                .top(px(ry_px - half_thickness))
                                .w(px(rw_px))
                                .h(px(sensor_thickness))
                                .cursor_row_resize()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(
                                        move |this, event: &MouseDownEvent, _window, cx| {
                                            cx.stop_propagation();
                                            this.is_mouse_down = true;
                                            this.annotation_drag =
                                                Some(crate::pdf::AnnotationDragState {
                                                    annotation_id: ann_id_clone.clone(),
                                                    page: page_index,
                                                    handle: crate::pdf::AnnotationResizeHandle::Top,
                                                    start_mouse: event.position,
                                                    start_x: rx_val,
                                                    start_y: ry_val,
                                                    start_w: rw_val,
                                                    start_h: rh_val,
                                                });
                                            cx.notify();
                                        },
                                    ),
                                )
                                .into_any_element(),
                        );

                        // 下边感应带
                        let ann_id_clone = ann_id.clone();
                        elements.push(
                            div()
                                .absolute()
                                .left(px(rx_px))
                                .top(px(ry_px + rh_px - half_thickness))
                                .w(px(rw_px))
                                .h(px(sensor_thickness))
                                .cursor_row_resize()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(
                                        move |this, event: &MouseDownEvent, _window, cx| {
                                            cx.stop_propagation();
                                            this.is_mouse_down = true;
                                            this.annotation_drag =
                                                Some(crate::pdf::AnnotationDragState {
                                                    annotation_id: ann_id_clone.clone(),
                                                    page: page_index,
                                                    handle:
                                                        crate::pdf::AnnotationResizeHandle::Bottom,
                                                    start_mouse: event.position,
                                                    start_x: rx_val,
                                                    start_y: ry_val,
                                                    start_w: rw_val,
                                                    start_h: rh_val,
                                                });
                                            cx.notify();
                                        },
                                    ),
                                )
                                .into_any_element(),
                        );

                        // 左边感应带
                        let ann_id_clone = ann_id.clone();
                        elements.push(
                            div()
                                .absolute()
                                .left(px(rx_px - half_thickness))
                                .top(px(ry_px))
                                .w(px(sensor_thickness))
                                .h(px(rh_px))
                                .cursor_col_resize()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(
                                        move |this, event: &MouseDownEvent, _window, cx| {
                                            cx.stop_propagation();
                                            this.is_mouse_down = true;
                                            this.annotation_drag =
                                                Some(crate::pdf::AnnotationDragState {
                                                    annotation_id: ann_id_clone.clone(),
                                                    page: page_index,
                                                    handle:
                                                        crate::pdf::AnnotationResizeHandle::Left,
                                                    start_mouse: event.position,
                                                    start_x: rx_val,
                                                    start_y: ry_val,
                                                    start_w: rw_val,
                                                    start_h: rh_val,
                                                });
                                            cx.notify();
                                        },
                                    ),
                                )
                                .into_any_element(),
                        );

                        // 右边感应带
                        let ann_id_clone = ann_id.clone();
                        elements.push(
                            div()
                                .absolute()
                                .left(px(rx_px + rw_px - half_thickness))
                                .top(px(ry_px))
                                .w(px(sensor_thickness))
                                .h(px(rh_px))
                                .cursor_col_resize()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(
                                        move |this, event: &MouseDownEvent, _window, cx| {
                                            cx.stop_propagation();
                                            this.is_mouse_down = true;
                                            this.annotation_drag =
                                                Some(crate::pdf::AnnotationDragState {
                                                    annotation_id: ann_id_clone.clone(),
                                                    page: page_index,
                                                    handle:
                                                        crate::pdf::AnnotationResizeHandle::Right,
                                                    start_mouse: event.position,
                                                    start_x: rx_val,
                                                    start_y: ry_val,
                                                    start_w: rw_val,
                                                    start_h: rh_val,
                                                });
                                            cx.notify();
                                        },
                                    ),
                                )
                                .into_any_element(),
                        );

                        // 2. 四个角上的小控制点手柄
                        let handle_size = 8.0;
                        let offset = handle_size / 2.0;

                        let corners = [
                            (crate::pdf::AnnotationResizeHandle::TopLeft, rx_px, ry_px),
                            (
                                crate::pdf::AnnotationResizeHandle::TopRight,
                                rx_px + rw_px,
                                ry_px,
                            ),
                            (
                                crate::pdf::AnnotationResizeHandle::BottomLeft,
                                rx_px,
                                ry_px + rh_px,
                            ),
                            (
                                crate::pdf::AnnotationResizeHandle::BottomRight,
                                rx_px + rw_px,
                                ry_px + rh_px,
                            ),
                        ];

                        for (handle, hx, hy) in corners {
                            let ann_id_clone = ann_id.clone();
                            elements.push(
                                div()
                                    .absolute()
                                    .left(px(hx - offset))
                                    .top(px(hy - offset))
                                    .w(px(handle_size))
                                    .h(px(handle_size))
                                    .bg(color)
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .when(
                                        matches!(
                                            handle,
                                            crate::pdf::AnnotationResizeHandle::TopLeft
                                                | crate::pdf::AnnotationResizeHandle::BottomRight
                                        ),
                                        |d| d.cursor_nwse_resize(),
                                    )
                                    .when(
                                        matches!(
                                            handle,
                                            crate::pdf::AnnotationResizeHandle::TopRight
                                                | crate::pdf::AnnotationResizeHandle::BottomLeft
                                        ),
                                        |d| d.cursor_nesw_resize(),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |this, event: &MouseDownEvent, _window, cx| {
                                                cx.stop_propagation();
                                                this.is_mouse_down = true;
                                                this.annotation_drag =
                                                    Some(crate::pdf::AnnotationDragState {
                                                        annotation_id: ann_id_clone.clone(),
                                                        page: page_index,
                                                        handle,
                                                        start_mouse: event.position,
                                                        start_x: rx_val,
                                                        start_y: ry_val,
                                                        start_w: rw_val,
                                                        start_h: rh_val,
                                                    });
                                                cx.notify();
                                            },
                                        ),
                                    )
                                    .into_any_element(),
                            );
                        }
                    }
                }
                services::pdf::AnnotationKind::Highlight
                | services::pdf::AnnotationKind::Underline => {}
            }
        }

        if let Some((pid, ref bounds)) = self.rect_in_progress
            && pid == page_index
        {
            let color: gpui::Hsla = gpui::rgba(0x4285f460).into();
            let bw = bounds.right() - bounds.left();
            let bh = bounds.bottom() - bounds.top();
            elements.push(
                div()
                    .absolute()
                    .left(px(bounds.left()))
                    .top(px(bounds.top()))
                    .w(px(bw.max(1.0)))
                    .h(px(bh.max(1.0)))
                    .border_3()
                    .border_color(color)
                    .rounded(px(2.0))
                    .into_any_element(),
            );
        }

        if elements.is_empty() {
            return None;
        }

        Some(
            div()
                .absolute()
                .inset_0()
                .children(elements)
                .into_any_element(),
        )
    }

    pub(crate) fn load_page_links_with_size(
        &mut self,
        page_index: u16,
        display_width_px: f32,
        display_height_px: f32,
        _cx: &mut Context<Self>,
    ) {
        // 存在且 display_w/h 匹配则跳过
        if self
            .page_link_data
            .get(page_index as usize)
            .is_some_and(|d| {
                d.as_ref().is_some_and(|d| {
                    d.display_w == display_width_px && d.display_h == display_height_px
                })
            })
        {
            return;
        }
        // 缩放后 display_w 不匹配，重新请求
        self.pdf_service
            .send_links(page_index, display_width_px, display_height_px, 0);
    }

    /// 发送主页面文字请求（generation=0）。若已有匹配 display_w 的缓存则跳过。
    pub(crate) fn load_page_text_with_size(
        &mut self,
        page_index: u16,
        display_width_px: f32,
        _cx: &mut Context<Self>,
    ) {
        // 存在且 display_w 匹配则跳过
        if self
            .page_text_data
            .get(page_index as usize)
            .is_some_and(|d| d.as_ref().is_some_and(|d| d.display_w == display_width_px))
        {
            return;
        }
        // 缩放后 display_w 不匹配，清除旧数据并重新请求
        debug!(
            "page: 请求第 {} 页文字数据, display_w={}",
            page_index, display_width_px
        );
        self.page_text_data[page_index as usize] = None;
        self.send_text_request(page_index, display_width_px, 0);
    }

    /// 底层文字请求：根据 page_index 查找 pdf 尺寸，计算 display_h 并发送。
    /// 主页面调用 generation=0，缩略图调用 generation=1。
    pub(crate) fn send_text_request(&mut self, page_index: u16, display_w: f32, generation: u64) {
        let (pdf_w, pdf_h) = self
            .page_sizes
            .get(page_index as usize)
            .copied()
            .unwrap_or((612.0, 792.0));
        let display_h = display_w * (pdf_h / pdf_w);
        self.pdf_service
            .send_text(page_index, display_w, display_h, generation);
    }

    pub(crate) fn render_link_overlay(
        &mut self,
        page_index: u16,
        _window: &Window,
        _cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let link_data = self.page_link_data.get(page_index as usize)?.as_ref()?;

        if link_data.links.is_empty() {
            return None;
        }

        let elements: Vec<AnyElement> = link_data
            .links
            .iter()
            .map(|link| {
                div()
                    .absolute()
                    .left(px(link.left))
                    .top(px(link.top))
                    .w(px((link.right - link.left).max(1.0)))
                    .h(px((link.bottom - link.top).max(1.0)))
                    .cursor_pointer()
                    .into_any_element()
            })
            .collect();

        Some(
            div()
                .absolute()
                .inset_0()
                .children(elements)
                .into_any_element(),
        )
    }

    pub(crate) fn render_main_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view_weak = cx.entity().downgrade();

        div()
            .id("pdf-main-view")
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(cx.theme().muted.opacity(0.3))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, _cx| {
                    if this.overlay_button_clicked {
                        this.overlay_button_clicked = false;
                        return;
                    }
                    this.is_mouse_down = true;
                    this.mouse_down_pos = Some(event.position);

                    if let Some((page_index, start_x, start_y)) =
                        this.content_to_page_coords(event.position.x, event.position.y, window)
                    {
                        this.rect_start_pos =
                            Some((page_index, f32::from(start_x), f32::from(start_y)));
                    } else {
                        this.rect_start_pos = None;
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                this.handle_content_mouse_move(event, window, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.handle_content_mouse_up(event.position, window, cx);
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                if this.is_mouse_down {
                    return;
                }
                match event.delta {
                    gpui::ScrollDelta::Pixels(p) => {
                        this.apply_horizontal_scroll(f32::from(p.x), window);
                        cx.notify();
                    }
                    gpui::ScrollDelta::Lines(l) => {
                        this.apply_horizontal_scroll(l.x * 20.0, window);
                        cx.notify();
                    }
                }
            }))
            .child(
                // 页面列表
                div()
                    .size_full()
                    .child(
                        gpui::list(self.list_state.clone(), move |ix, _win, cx| {
                            view_weak
                                .update(cx, |this, cx| this.render_list_item(ix, _win, cx))
                                .unwrap_or_else(|_| div().into_any_element())
                        })
                        .size_full(),
                    )
                    // 滚动条占位
                    .child(self.render_scrollbar(window, cx)),
            )
            // PiP 图钉（放在 main_view div 内，坐标与页面列表一致）
            .when_some(self.render_pip_pins(window, cx), |this, pins| {
                this.child(pins)
            })
            // 悬浮缩放 HUD 胶囊 (左下角)
            .child(
                div()
                    .absolute()
                    .bottom_4()
                    .left_4()
                    .occlude()
                    .child(self.render_zoom_capsule(cx)),
            )
            // 悬浮页码 HUD 胶囊 (右下角)
            .child(
                div()
                    .absolute()
                    .bottom_4()
                    .right_4() // 保持与左侧 left_4 相同的边距
                    .occlude()
                    .child(self.render_page_capsule(cx)),
            )
    }
}
