use super::PdfReaderView;
use super::helpers;
use super::types::{
    AUTO_FIT_PADDING_PX, GlobalPdfUiState, SearchMatch, SearchResultDisplay, SearchState,
    TOOLBAR_HEIGHT_REMS, quantize_render_zoom,
};
use crate::pdf::PAGE_BASE_WIDTH_REMS;
use gpui::{Context, ListOffset, Pixels, Window, px, rems};
use i18n::I18nKey;
use services::pdf::TextPageData;
use std::sync::Arc;

// ── 缩放常量 ────────────────────────────────────────────
pub const ZOOM_STEP: f32 = 0.1;
pub const ZOOM_MIN: f32 = 0.1;
pub const ZOOM_MAX: f32 = 5.0;

impl PdfReaderView {
    pub(crate) fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        self.zoom_level = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        self.request_state_save(cx);
        let new_render_zoom = quantize_render_zoom(self.zoom_level);
        if (self.render_zoom - new_render_zoom).abs() > f32::EPSILON {
            self.render_zoom = new_render_zoom;
            // 旧图继续显示（等新渲染到达后原地替换），只清 pending 以便重新调度
            self.find_char_cache.clear();
            self.page_render_requests_pending.clear();
            self.zoom_changed = true;
        }

        self.search_state = None;
        self.end_selection(cx);
        self.programmatic_scroll = true;
        cx.notify();
    }

    pub(crate) fn zoom_in(&mut self, cx: &mut Context<Self>) {
        self.fit_to_width_mode = false;
        self.set_zoom(self.zoom_level + ZOOM_STEP, cx);
    }

    pub(crate) fn zoom_out(&mut self, cx: &mut Context<Self>) {
        self.fit_to_width_mode = false;
        self.set_zoom(self.zoom_level - ZOOM_STEP, cx);
    }

    pub(crate) fn reset_zoom(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.fit_to_width_mode = true;
        self.offset_x = 0.0;
        self.apply_auto_fit(window, cx);
    }

    /// 计算 fit-to-width 模式下的目标缩放值。
    /// 含边栏宽度扣除和 48px 留白。返回 None 表示视口过窄。
    fn compute_fit_to_width_zoom(&self, window: &Window) -> Option<f32> {
        let rem_size = f32::from(window.rem_size());
        let viewport_width = f32::from(window.viewport_size().width);

        let mut available_width = viewport_width;
        if self.is_left_sidebar_open {
            available_width -= f32::from(self.left_sidebar_width);
        }
        if self.is_right_sidebar_open {
            available_width -= f32::from(self.right_sidebar_width);
        }

        available_width -= AUTO_FIT_PADDING_PX;

        if available_width > 0.0 {
            Some(available_width / (PAGE_BASE_WIDTH_REMS * rem_size))
        } else {
            None
        }
    }

    pub(crate) fn apply_auto_fit(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !self.fit_to_width_mode {
            return;
        }
        if let Some(zoom) = self.compute_fit_to_width_zoom(window) {
            self.set_zoom(zoom, cx);
        }
    }

    pub(crate) fn scroll_to_page(
        &mut self,
        page_index: u16,
        offset_in_item: Pixels,
        cx: &mut Context<Self>,
    ) {
        if page_index as usize >= self.total_pages {
            return;
        }

        if self.current_page != page_index {
            self.clear_thumbnail_selection(cx);
        }
        self.current_page = page_index;
        self.programmatic_scroll = true;
        // 同步主视图位置
        self.list_state.scroll_to(ListOffset {
            item_ix: page_index as usize,
            offset_in_item,
        });

        if self.is_left_sidebar_open {
            self.thumbnail_list_state
                .scroll_to_reveal_item(page_index as usize);
        }

        cx.notify();
    }

    /// 根据注释的字符范围取中点计算滚动位置，使注释显示在视口中间。
    /// 跨页注释时计算全跨页范围内（start_page ~ end_page）的视觉中点。
    /// 返回 (目标页码, 该页内的 Y 偏移)。
    pub(crate) fn annotation_scroll_offset(
        &mut self,
        start_page: u16,
        start_char: usize,
        end_page: u16,
        end_char: usize,
        content_height_px: f32,
    ) -> (u16, Pixels) {
        let rem_size_px = self.last_rem_size;
        let current_display_w = PAGE_BASE_WIDTH_REMS * self.zoom_level * rem_size_px;

        // 将单页内 Y 坐标缩放到当前显示尺寸
        let scale_y = |data: &services::pdf::TextPageData, y: f32| -> f32 {
            if (data.display_w - current_display_w).abs() > 0.001 {
                y * current_display_w / data.display_w
            } else {
                y
            }
        };

        let page_height_for =
            |ix: usize| helpers::page_height(&self.page_sizes, ix, self.zoom_level, rem_size_px);

        // 累计 start_page 之前所有页的总高度（作为全局坐标系偏移基准）
        let page_offset_for = |ix: u16| -> f32 {
            let mut acc = 0.0;
            for i in 0..ix as usize {
                acc += page_height_for(i);
            }
            acc
        };

        let start_y = self
            .page_text_data
            .get(start_page as usize)
            .and_then(|d| d.as_ref())
            .and_then(|data| data.chars.get(start_char).map(|ch| scale_y(data, ch.y)))
            .unwrap_or(0.0);

        let end_abs_y = self
            .page_text_data
            .get(end_page as usize)
            .and_then(|d| d.as_ref())
            .and_then(|data| {
                let end_idx = end_char.min(data.chars.len().saturating_sub(1));
                data.chars
                    .get(end_idx)
                    .map(|ch| page_offset_for(end_page) + scale_y(data, ch.y + ch.height))
            })
            .unwrap_or_else(|| page_offset_for(end_page) + page_height_for(end_page as usize));

        let global_top = page_offset_for(start_page) + start_y;
        let global_bottom = end_abs_y;
        let global_mid = (global_top + global_bottom) / 2.0;

        // 目标：让 global_mid 出现在视口正中
        let target_scroll = (global_mid - content_height_px / 2.0).max(0.0);

        // 换算回 (target_page, offset_in_item)
        let mut acc = 0.0;
        for i in 0..self.total_pages {
            let ph = page_height_for(i);
            if acc + ph > target_scroll {
                return (i as u16, px(target_scroll - acc));
            }
            acc += ph;
        }
        let last = self.total_pages.saturating_sub(1);
        (last as u16, px(page_height_for(last)))
    }

    pub(crate) fn scroll_to_position(
        &mut self,
        mouse_y: Pixels,
        window_height: Pixels,
        rem_size: Pixels,
        cx: &mut Context<Self>,
    ) {
        let toolbar_height = rems(TOOLBAR_HEIGHT_REMS).to_pixels(rem_size);
        let tab_bar_h = self.tab_bar_offset_px;
        let view_height = f32::from(window_height) - tab_bar_h - f32::from(toolbar_height);
        let view_height_px = view_height;
        let rem_size_px = f32::from(rem_size);

        // 计算总高度（考虑多尺寸页面）
        let mut total_height_px = 0.0;
        let mut heights = Vec::with_capacity(self.total_pages);
        for i in 0..self.total_pages {
            let page_h = helpers::page_height(&self.page_sizes, i, self.zoom_level, rem_size_px);
            heights.push(page_h);
            total_height_px += page_h;
        }

        let scrollable_height_px = (total_height_px - view_height_px).max(0.0);
        let thumb_height_pct =
            (view_height_px / total_height_px.max(view_height_px)).clamp(0.05, 1.0);
        let thumb_height_px = view_height_px * thumb_height_pct;
        let track_height_px = view_height_px - thumb_height_px;

        let mouse_y_px = f32::from(mouse_y) - tab_bar_h - f32::from(toolbar_height);
        let target_thumb_top = (mouse_y_px - self.drag_offset).clamp(0.0, track_height_px);

        let scroll_ratio = if track_height_px > 0.0 {
            target_thumb_top / track_height_px
        } else {
            0.0
        };

        let target_scroll_px = scroll_ratio * scrollable_height_px;

        // 根据累积高度寻找目标页码
        let mut current_sum = 0.0;
        let mut target_page_ix = 0;
        let mut offset_in_item = 0.0;

        for (i, &h) in heights.iter().enumerate() {
            if current_sum + h > target_scroll_px {
                target_page_ix = i;
                offset_in_item = target_scroll_px - current_sum;
                break;
            }
            current_sum += h;
            if i == self.total_pages - 1 {
                target_page_ix = i;
                offset_in_item = (target_scroll_px - (current_sum - h)).max(0.0);
            }
        }

        if self.current_page != target_page_ix as u16 {
            self.clear_thumbnail_selection(cx);
        }
        self.current_page = target_page_ix as u16;
        self.list_state.scroll_to(ListOffset {
            item_ix: target_page_ix,
            offset_in_item: px(offset_in_item),
        });

        if self.is_left_sidebar_open {
            self.thumbnail_list_state
                .scroll_to_reveal_item(target_page_ix);
        }

        cx.notify();
    }

    pub(crate) fn scroll_thumbnails_to_position(
        &mut self,
        mouse_y: Pixels,
        sidebar_height: f32,
        cx: &mut Context<Self>,
    ) {
        let item_height_px = self.get_thumbnail_item_height();
        let total_height_px = self.total_pages as f32 * item_height_px;
        let scrollable_height_px = (total_height_px - sidebar_height).max(0.0);

        let thumb_height_pct =
            (sidebar_height / total_height_px.max(sidebar_height)).clamp(0.05, 1.0);
        let thumb_height_px = sidebar_height * thumb_height_pct;
        let track_height_px = sidebar_height - thumb_height_px;

        let mouse_y_px = f32::from(mouse_y) - self.tab_bar_offset_px; // 减去 Tab 栏高度
        let target_thumb_top =
            (mouse_y_px - self.thumbnail_drag_offset).clamp(0.0, track_height_px);

        let scroll_ratio = if track_height_px > 0.0 {
            target_thumb_top / track_height_px
        } else {
            0.0
        };

        let target_scroll_px = scroll_ratio * scrollable_height_px;
        let target_item_ix = (target_scroll_px / item_height_px).floor() as usize;
        let target_item_ix = target_item_ix.min(self.total_pages.saturating_sub(1));
        let offset_in_item = target_scroll_px - (target_item_ix as f32 * item_height_px);

        self.thumbnail_list_state.scroll_to(ListOffset {
            item_ix: target_item_ix,
            offset_in_item: px(offset_in_item),
        });
        cx.notify();
    }

    /// 请求保存阅读状态（防抖）：交互期间只标脏，由唯一的防抖定时器
    /// 在静默 `STATE_SAVE_DEBOUNCE_MS` 后统一落盘，避免每帧同步 SQLite I/O。
    /// 视图销毁前的强一致保存请直接用 `save_current_state(None)`。
    pub(crate) fn request_state_save(&mut self, cx: &mut Context<Self>) {
        self.state_save_dirty = true;
        if self.state_save_scheduled {
            return;
        }
        self.state_save_scheduled = true;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor
                .timer(std::time::Duration::from_millis(
                    crate::pdf::types::STATE_SAVE_DEBOUNCE_MS,
                ))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.state_save_scheduled = false;
                if !this.state_save_dirty {
                    return;
                }
                this.state_save_dirty = false;
                this.save_current_state(Some(cx));
            });
        })
        .detach();
    }

    pub(crate) fn save_current_state(&self, cx: Option<&mut Context<Self>>) {
        if let Some(ref d) = self.delegate {
            d.save_state(
                self.document_id.clone(),
                self.current_page,
                self.zoom_level,
                self.current_offset_y,
                self.fit_to_width_mode,
                self.is_left_sidebar_open,
                self.is_right_sidebar_open,
                self.preferred_left_sidebar_width,
                self.preferred_right_sidebar_width,
                self.auto_translate,
            );
        }
        if let Some(cx) = cx {
            cx.set_global(GlobalPdfUiState {
                zoom_level: self.zoom_level,
                fit_to_width: self.fit_to_width_mode,
                is_left_sidebar_open: self.is_left_sidebar_open,
                is_right_sidebar_open: self.is_right_sidebar_open,
                left_sidebar_width: self.preferred_left_sidebar_width,
                right_sidebar_width: self.preferred_right_sidebar_width,
                auto_translate: self.auto_translate,
            });
        }
    }

    pub(crate) fn perform_search(&mut self, query: &str, cx: &mut Context<Self>) {
        if query.is_empty() {
            self.search_state = None;
            if let Some(ref list_state) = self.search_list_state {
                list_state.update(cx, |ls, _| {
                    ls.delegate_mut().items.clear();
                    ls.delegate_mut().active_match_idx = None;
                });
            }
            cx.notify();
            return;
        }

        let query_lower = query.to_lowercase();
        let query_chars: Vec<char> = query_lower.chars().collect();
        if query_chars.is_empty() {
            cx.notify();
            return;
        }

        // 创建搜索文本存储（首次搜索时）
        self.ensure_search_text_storage();

        // 在 storage 中搜索
        let mut results: Vec<SearchMatch> = Vec::new();
        if self.search_text_storage.is_some() {
            for page in 0..self.total_pages as u16 {
                results.extend(self.search_page_single(page, &query_chars));
            }
        }

        let active_idx = if results.is_empty() { None } else { Some(0) };

        // 预计算显示数据
        let display_items = self.build_search_display_items(&results);

        self.search_state = if results.is_empty() {
            Some(SearchState {
                query: query.to_string(),
                results: Vec::new(),
                active_match_idx: None,
            })
        } else {
            Some(SearchState {
                query: query.to_string(),
                results,
                active_match_idx: active_idx,
            })
        };

        // 更新列表 delegate
        if let Some(ref list_state) = self.search_list_state {
            list_state.update(cx, |ls, _| {
                ls.delegate_mut().items = display_items;
                ls.delegate_mut().active_match_idx = active_idx;
            });
        }

        cx.notify();
    }

    /// 如果 search_text_storage 为 None，从 page_text_data 重建
    pub(crate) fn ensure_search_text_storage(&mut self) -> bool {
        if self.search_text_storage.is_some() || self.total_pages == 0 {
            return false;
        }
        let mut storage: Vec<Option<Arc<TextPageData>>> = vec![None; self.total_pages];
        for page in 0..self.total_pages as u16 {
            if let Some(data) = self
                .page_text_data
                .get(page as usize)
                .and_then(|d| d.as_ref())
            {
                storage[page as usize] = Some(Arc::clone(data));
            }
        }
        self.search_text_storage = Some(storage);
        // 所有页面的文本数据在文档加载时已一次性请求，无需再发送请求
        true
    }

    /// 对 search_text_storage 中已有的页面重新执行搜索，更新 state + 列表
    pub(crate) fn re_run_search_from_storage(&mut self, cx: &mut Context<Self>) {
        let Some(ref state) = self.search_state else {
            return;
        };
        if state.query.is_empty() {
            return;
        }

        let query_lower = state.query.to_lowercase();
        let query_chars: Vec<char> = query_lower.chars().collect();
        if query_chars.is_empty() {
            return;
        }

        let mut results: Vec<SearchMatch> = Vec::new();
        for page in 0..self.total_pages as u16 {
            results.extend(self.search_page_single(page, &query_chars));
        }

        let active_idx = if results.is_empty() { None } else { Some(0) };

        let display_items = self.build_search_display_items(&results);

        if let Some(ref mut s) = self.search_state {
            s.results = results;
            s.active_match_idx = active_idx;
        }

        if let Some(ref list_state) = self.search_list_state {
            list_state.update(cx, |ls, _| {
                ls.delegate_mut().items = display_items;
                ls.delegate_mut().active_match_idx = active_idx;
            });
        }
    }

    /// 对单页文本搜索，从 storage 中取数据
    pub(crate) fn search_page_single(&self, page: u16, query_chars: &[char]) -> Vec<SearchMatch> {
        let Some(ref storage) = self.search_text_storage else {
            return Vec::new();
        };
        let Some(data) = storage.get(page as usize).and_then(|d| d.as_ref()) else {
            return Vec::new();
        };

        let chars: Vec<char> = data.chars.iter().map(|c| c.char).collect();
        let char_count = chars.len();
        if char_count < query_chars.len() {
            return Vec::new();
        }

        let mut results = Vec::new();
        let mut search_start = 0usize;
        while search_start + query_chars.len() <= char_count {
            let mut found = true;
            for qi in 0..query_chars.len() {
                if chars[search_start + qi]
                    .to_lowercase()
                    .next()
                    .unwrap_or('\0')
                    != query_chars[qi]
                {
                    found = false;
                    break;
                }
            }
            if found {
                results.push(SearchMatch {
                    page_index: page,
                    start_char: search_start,
                    end_char: search_start + query_chars.len(),
                });
                search_start += 1;
            } else {
                search_start += 1;
            }
        }
        results
    }

    /// 将单页搜索的结果合入当前 search_state 并刷新显示数据 + delegate
    pub(crate) fn merge_search_results(
        &mut self,
        page: u16,
        new_matches: Vec<SearchMatch>,
        cx: &mut Context<Self>,
    ) {
        if new_matches.is_empty() {
            return;
        }

        let new_idx;
        let cloned_results;

        // 第一阶段：变更 search_state
        {
            let Some(ref mut state) = self.search_state else {
                return;
            };

            state.results.retain(|m| m.page_index != page);
            state.results.extend(new_matches);
            state.results.sort_by_key(|m| (m.page_index, m.start_char));

            let current_active = state.active_match().cloned();
            let empty = state.results.is_empty();

            new_idx = if let Some(ref active) = current_active {
                state.results.iter().position(|m| m == active)
            } else {
                None
            }
            .or(if empty { None } else { Some(0) });

            state.active_match_idx = new_idx;
            cloned_results = state.results.clone();
        } // 释放 state 的可变借用

        // 第二阶段：计算显示数据
        let display_items = self.build_search_display_items(&cloned_results);

        if let Some(ref list_state) = self.search_list_state {
            list_state.update(cx, |ls, _| {
                ls.delegate_mut().items = display_items;
                ls.delegate_mut().active_match_idx = new_idx;
            });
        }

        cx.notify();
    }

    fn build_search_display_items(&mut self, results: &[SearchMatch]) -> Vec<SearchResultDisplay> {
        let lang = self.language;
        results
            .iter()
            .map(|m| {
                let title = i18n::tf(
                    I18nKey::SinglePage,
                    lang,
                    &[&(m.page_index + 1).to_string()],
                );
                SearchResultDisplay {
                    title,
                    context: self.search_context_text(m).into(),
                }
            })
            .collect()
    }
}
