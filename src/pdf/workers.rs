use gpui::{AsyncApp, Context, WeakEntity};
use services::pdf::{Annotation, PdfResponse};

use log::{debug, error, info};
use std::collections::HashMap;
use std::sync::Arc;

use super::render::translate_outlines;
use super::*;

impl super::PdfReaderView {
    pub fn init_workers(
        &mut self,
        response_rx: std::sync::mpsc::Receiver<PdfResponse>,
        cx: &mut Context<Self>,
    ) {
        info!("PDF View: 启动工作线程响应监听...");
        let executor = cx.background_executor().clone();
        cx.spawn(move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let cx = cx.clone();
            let executor = executor.clone();
            async move {
                loop {
                    let mut disconnected = false;
                    loop {
                        match response_rx.try_recv() {
                            Ok(response) => {
                                cx.update(|cx| {
                                    let _ = this.update(cx, |this, cx| match response {
                                        PdfResponse::DocumentLoaded {
                                            doc_id,
                                            page_count,
                                            page_sizes,
                                        } => {
                                            info!(
                                                "PDF View: 文档已加载, ID: {}, 共 {} 页",
                                                doc_id, page_count
                                            );
                                            this.total_pages = page_count;
                                            this.page_sizes = page_sizes;
                                            this.worker_state = WorkerState::Running;
                                            this.link_navigation_history.clear();
                                            this.list_state.reset(page_count);
                                            this.thumbnail_list_state.reset(page_count);
                                            this.is_restoring = true;

                                            // 初始化页面数据 Vec
                                            this.page_images = vec![None; page_count];
                                            this.raw_page_images = vec![None; page_count];
                                            this.page_text_data = vec![None; page_count];
                                            this.page_link_data = vec![None; page_count];
                                            this.thumbnail_images = vec![None; page_count];
                                            this.thumbnail_text_data = vec![None; page_count];
                                            this.thumbnail_text_requests_pending.clear();
                                            this.visible_page_first = usize::MAX;
                                            this.visible_page_last = 0;
                                            this.page_render_requests_pending.clear();
                                            this.visible_thumb_first = usize::MAX;
                                            this.visible_thumb_last = 0;
                                            this.thumb_render_requests_pending.clear();
                                            this.thumbnail_layout_refresh_pending = true;
                                            this.thumbnail_eviction_pending = false;
                                            this.thumbnail_layout_width = -1.0;
                                            this.thumbnail_layout_height = -1.0;
                                            this.thumbnail_response_keep_range = (0, 0);

                                            // 主页面和缩略图渲染由 render() 里的
                                            // refresh_page_visibility / refresh_thumb_visibility 触发
                                            // （DocumentLoaded 时 list_state 已重置，第一帧 render 会自动调度）

                                            // 加载注释
                                            if let Some(delegate) = &this.delegate {
                                                let annotations =
                                                    delegate.load_annotations(&this.document_id);
                                                let mut page_map: HashMap<u16, Vec<Annotation>> =
                                                    HashMap::new();
                                                for ann in annotations {
                                                    page_map.entry(ann.page).or_default().push(ann);
                                                }
                                                this.annotation_state.annotations = page_map;
                                            }

                                            cx.notify();
                                        }
                                        PdfResponse::PageRendered {
                                            page,
                                            generation: _,
                                            image,
                                        } => {
                                            this.on_page_rendered(page, image, cx);
                                        }
                                        PdfResponse::ThumbnailRendered {
                                            page,
                                            generation: _,
                                            image,
                                        } => {
                                            this.on_thumbnail_rendered(page, image, cx);
                                        }
                                        PdfResponse::LinksExtracted {
                                            page,
                                            generation: _,
                                            data,
                                        } => {
                                            if let Some(slot) =
                                                this.page_link_data.get_mut(page as usize)
                                            {
                                                *slot = Some(Arc::new(data));
                                            }
                                            cx.notify();
                                        }
                                        PdfResponse::TextExtracted {
                                            page,
                                            generation,
                                            data,
                                        } => {
                                            if generation == 1 {
                                                // 缩略图文字：存入专用存储，不触发搜索
                                                this.thumbnail_text_requests_pending.remove(&page);
                                                if let Some(slot) =
                                                    this.thumbnail_text_data.get_mut(page as usize)
                                                {
                                                    *slot = Some(Arc::new(data));
                                                }
                                                cx.notify();
                                            } else {
                                                this.on_text_extracted(page, data, cx);
                                            }
                                        }
                                        PdfResponse::PinRendered { pin_id, image } => {
                                            debug!(
                                                "mod: 收到 Pin 渲染结果 pin_id={}, 分辨率 {}x{}",
                                                pin_id,
                                                image.width(),
                                                image.height()
                                            );
                                            if let Some(pin) =
                                                this.pins.iter_mut().find(|p| p.id == pin_id)
                                            {
                                                // 使用 pending_scale 而非当前显示宽度，
                                                // 避免响应到达时用户已继续拖拽导致 rendered_scale 偏差
                                                pin.rendered_scale = pin.pending_scale;
                                                pin.render_pending = false;
                                                pin.raw_image = Some(Arc::new(image.clone()));
                                                pin.image_source =
                                                    Some(helpers::make_image_source(image));
                                                cx.notify();
                                            } else {
                                                debug!(
                                                    "mod: PinRendered 但 pin_id={} 已不存在",
                                                    pin_id
                                                );
                                            }
                                        }
                                        PdfResponse::OutlineExtracted { outlines, .. } => {
                                            this.outlines =
                                                Some(translate_outlines(outlines, this.language));
                                            cx.notify();
                                        }
                                        PdfResponse::DocumentModified {
                                            doc_id: _,
                                            page_count,
                                            page_sizes,
                                            deleted_page,
                                        } => {
                                            log::info!(
                                                "PDF View: 文档已修改, 共 {} 页",
                                                page_count
                                            );
                                            this.total_pages = page_count;
                                            this.page_sizes = page_sizes;
                                            this.link_navigation_history.clear();
                                            this.list_state.reset(page_count);
                                            this.thumbnail_list_state.reset(page_count);

                                            let deleted_idx = deleted_page as usize;
                                            if deleted_idx < this.page_images.len() {
                                                this.page_images.remove(deleted_idx);
                                                this.raw_page_images.remove(deleted_idx);
                                                this.page_text_data.remove(deleted_idx);
                                                this.page_link_data.remove(deleted_idx);
                                                this.thumbnail_images.remove(deleted_idx);
                                                this.thumbnail_text_data.remove(deleted_idx);
                                            }
                                            this.thumbnail_text_requests_pending.clear();

                                            // 强制在下一帧进行可见性重新判定与渲染
                                            this.visible_page_first = usize::MAX;
                                            this.visible_page_last = 0;
                                            this.page_render_requests_pending.clear();
                                            this.visible_thumb_first = usize::MAX;
                                            this.visible_thumb_last = 0;
                                            this.thumb_render_requests_pending.clear();
                                            this.thumbnail_layout_refresh_pending = true;
                                            this.thumbnail_eviction_pending = false;
                                            this.thumbnail_layout_width = -1.0;
                                            this.thumbnail_layout_height = -1.0;
                                            this.thumbnail_response_keep_range = (0, 0);

                                            // 重新定位滚动位置并安全限制页码
                                            let target_page = (this.current_page as usize)
                                                .min(page_count.saturating_sub(1));
                                            this.list_state.scroll_to(gpui::ListOffset {
                                                item_ix: target_page,
                                                offset_in_item: gpui::px(0.0),
                                            });
                                            this.thumbnail_list_state.scroll_to(gpui::ListOffset {
                                                item_ix: target_page,
                                                offset_in_item: gpui::px(0.0),
                                            });
                                            this.current_page = target_page as u16;
                                            this.current_offset_y = 0.0;

                                            // 批注物理平移
                                            this.annotation_state.annotations.remove(&deleted_page);
                                            let mut new_annotations =
                                                std::collections::HashMap::new();
                                            for (page, mut anns) in
                                                this.annotation_state.annotations.drain()
                                            {
                                                if page < deleted_page {
                                                    new_annotations.insert(page, anns);
                                                } else if page > deleted_page {
                                                    for ann in &mut anns {
                                                        ann.page = page - 1;
                                                    }
                                                    new_annotations.insert(page - 1, anns);
                                                }
                                            }
                                            this.annotation_state.annotations = new_annotations;

                                            cx.notify();
                                        }
                                        PdfResponse::FatalError(e) => {
                                            error!("PDF View: 收到致命错误: {}", e);
                                            this.worker_state = WorkerState::Failed(e);
                                            this.is_restoring = false;
                                            cx.notify();
                                        }
                                    });
                                });
                            }
                            Err(std::sync::mpsc::TryRecvError::Empty) => {
                                break;
                            }
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                disconnected = true;
                                break;
                            }
                        }
                    }
                    if disconnected {
                        error!("PDF View: 工作线程通道断开");
                        break;
                    }
                    executor.timer(std::time::Duration::from_millis(16)).await;
                }
            }
        })
        .detach();
    }
}
