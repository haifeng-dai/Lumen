use image::{ImageBuffer, RgbaImage};
use log::{debug, error, info};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;

use crate::pdf::{
    Annotation, AnnotationColor, AnnotationKind, LinkInfo, LinkPageData, LinkTarget, OutlineItem,
    TextChar, TextPageData,
};

fn destination_top(kind: &mupdf::DestinationKind) -> Option<f32> {
    match kind {
        mupdf::DestinationKind::XYZ { top, .. }
        | mupdf::DestinationKind::FitH { top }
        | mupdf::DestinationKind::FitBH { top } => *top,
        mupdf::DestinationKind::FitR { top, .. } => Some(*top),
        mupdf::DestinationKind::Fit
        | mupdf::DestinationKind::FitV { .. }
        | mupdf::DestinationKind::FitBV { .. }
        | mupdf::DestinationKind::FitB => None,
    }
}

fn normalized_destination_y(top: f32, y0: f32, y1: f32) -> Option<f32> {
    if !top.is_finite() || !y0.is_finite() || !y1.is_finite() || y1 <= y0 {
        return None;
    }
    Some(((top - y0) / (y1 - y0)).clamp(0.0, 1.0))
}

#[cfg(test)]
mod destination_tests {
    use super::{destination_top, normalized_destination_y};

    #[test]
    fn extracts_vertical_coordinate_for_supported_kinds() {
        assert_eq!(
            destination_top(&mupdf::DestinationKind::XYZ {
                left: None,
                top: Some(25.0),
                zoom: None,
            }),
            Some(25.0)
        );
        assert_eq!(
            destination_top(&mupdf::DestinationKind::FitH { top: Some(10.0) }),
            Some(10.0)
        );
        assert_eq!(
            destination_top(&mupdf::DestinationKind::FitBH { top: Some(5.0) }),
            Some(5.0)
        );
        assert_eq!(
            destination_top(&mupdf::DestinationKind::FitR {
                left: 0.0,
                bottom: 0.0,
                right: 10.0,
                top: 50.0,
            }),
            Some(50.0)
        );
    }

    #[test]
    fn unsupported_destination_kinds_use_page_start() {
        for kind in [
            mupdf::DestinationKind::Fit,
            mupdf::DestinationKind::FitB,
            mupdf::DestinationKind::FitV { left: Some(1.0) },
            mupdf::DestinationKind::FitBV { left: Some(1.0) },
        ] {
            assert_eq!(destination_top(&kind), None);
        }
    }

    #[test]
    fn normalizes_and_clamps_coordinates_safely() {
        assert_eq!(normalized_destination_y(50.0, 0.0, 100.0), Some(0.5));
        assert_eq!(normalized_destination_y(-10.0, 0.0, 100.0), Some(0.0));
        assert_eq!(normalized_destination_y(110.0, 0.0, 100.0), Some(1.0));
        assert_eq!(normalized_destination_y(1.0, 10.0, 10.0), None);
        assert_eq!(normalized_destination_y(f32::NAN, 0.0, 1.0), None);
    }

    #[test]
    fn external_links_keep_their_url() {
        let target = crate::pdf::LinkTarget::External {
            url: "https://example.com".to_string(),
        };
        assert_eq!(
            target,
            crate::pdf::LinkTarget::External {
                url: "https://example.com".to_string()
            }
        );
    }
}

// ─── Worker Messages ─────────────────────────────────────────

#[derive(Debug)]
pub enum PdfRequest {
    /// 请求加载文档
    OpenDocument {
        doc_id: u32,
        path: PathBuf,
        tx: SyncSender<PdfResponse>,
    },
    /// 请求渲染特定页面
    RenderPage {
        doc_id: u32,
        page: u16,
        scale: f32,
        generation: u64,
    },
    /// 请求渲染缩略图
    RenderThumbnail {
        doc_id: u32,
        page: u16,
        max_size: f32,
        generation: u64,
    },
    /// 请求提取特定页面的链接
    ExtractLinks {
        doc_id: u32,
        page: u16,
        display_w: f32,
        display_h: f32,
        generation: u64,
    },
    /// 请求提取特定页面的文本数据
    ExtractText {
        doc_id: u32,
        page: u16,
        display_w: f32,
        display_h: f32,
        generation: u64,
    },
    /// 渲染 Pin 子区域（使用 DisplayList + bbox 裁剪）
    RenderPin {
        doc_id: u32,
        page: u16,
        pin_id: String,
        /// PDF 坐标 (x0, y0, x1, y1)
        bbox: (f32, f32, f32, f32),
        zoom: f32,
    },
    /// 关闭特定文档
    CloseDocument { doc_id: u32 },
    /// 删除文档中的一页并保存
    DeletePage { doc_id: u32, page: u16 },
    /// 将选中的页导出（复制源文件后删除未选中页）为新 PDF 并保存到 dest_path
    ExtractPages {
        doc_id: u32,
        pages: Vec<u16>,
        dest_path: PathBuf,
    },
    /// 关闭工作线程
    Shutdown,
}

fn is_same_task(a: &PdfRequest, b: &PdfRequest) -> bool {
    match (a, b) {
        (
            PdfRequest::RenderPage {
                doc_id: d1,
                page: p1,
                ..
            },
            PdfRequest::RenderPage {
                doc_id: d2,
                page: p2,
                ..
            },
        ) => d1 == d2 && p1 == p2,
        (
            PdfRequest::ExtractLinks {
                doc_id: d1,
                page: p1,
                ..
            },
            PdfRequest::ExtractLinks {
                doc_id: d2,
                page: p2,
                ..
            },
        ) => d1 == d2 && p1 == p2,
        (
            PdfRequest::ExtractText {
                doc_id: d1,
                page: p1,
                ..
            },
            PdfRequest::ExtractText {
                doc_id: d2,
                page: p2,
                ..
            },
        ) => d1 == d2 && p1 == p2,
        (
            PdfRequest::RenderThumbnail {
                doc_id: d1,
                page: p1,
                ..
            },
            PdfRequest::RenderThumbnail {
                doc_id: d2,
                page: p2,
                ..
            },
        ) => d1 == d2 && p1 == p2,
        (
            PdfRequest::RenderPin {
                doc_id: d1,
                pin_id: p1,
                ..
            },
            PdfRequest::RenderPin {
                doc_id: d2,
                pin_id: p2,
                ..
            },
        ) => d1 == d2 && p1 == p2,
        (
            PdfRequest::DeletePage {
                doc_id: d1,
                page: p1,
            },
            PdfRequest::DeletePage {
                doc_id: d2,
                page: p2,
            },
        ) => d1 == d2 && p1 == p2,
        _ => false,
    }
}
pub struct PdfTaskQueue {
    queue: Mutex<VecDeque<PdfRequest>>,
    condvar: Condvar,
    max_capacity: usize,
}

impl PdfTaskQueue {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            condvar: Condvar::new(),
            max_capacity,
        }
    }

    pub fn push(&self, req: PdfRequest) {
        let mut q = self.queue.lock().unwrap();

        // 去重：移除同类型的旧请求
        q.retain(|existing| !is_same_task(existing, &req));

        // 加入新请求到队尾
        q.push_back(req);

        // 截断：如果超出容量，丢弃队首（最老）的任务
        if q.len() > self.max_capacity {
            // 避免丢弃关键的任务
            let mut remove_idx = None;
            for (i, r) in q.iter().enumerate() {
                match r {
                    PdfRequest::OpenDocument { .. } | PdfRequest::Shutdown => continue,
                    _ => {
                        remove_idx = Some(i);
                        break;
                    }
                }
            }
            if let Some(idx) = remove_idx {
                q.remove(idx);
            }
        }
        self.condvar.notify_one();
    }

    pub fn pop(&self) -> PdfRequest {
        let mut q = self.queue.lock().unwrap();
        loop {
            // 优先级 1：Shutdown 信号
            if let Some(idx) = q.iter().position(|r| matches!(r, PdfRequest::Shutdown)) {
                return q.remove(idx).unwrap();
            }
            // 优先级 2：文档打开、关闭、删除、导出
            if let Some(idx) = q.iter().position(|r| {
                matches!(
                    r,
                    PdfRequest::OpenDocument { .. }
                        | PdfRequest::CloseDocument { .. }
                        | PdfRequest::DeletePage { .. }
                        | PdfRequest::ExtractPages { .. }
                )
            }) {
                return q.remove(idx).unwrap();
            }
            // 优先级 3：主页面渲染 (RenderPage)
            if let Some(idx) = q
                .iter()
                .position(|r| matches!(r, PdfRequest::RenderPage { .. }))
            {
                return q.remove(idx).unwrap();
            }
            // 优先级 4：文本与链接数据准备
            if let Some(idx) = q.iter().position(|r| {
                matches!(
                    r,
                    PdfRequest::ExtractText { .. } | PdfRequest::ExtractLinks { .. }
                )
            }) {
                return q.remove(idx).unwrap();
            }
            // 优先级 5：如果没有高优先级任务，正常处理剩下的任务（如缩略图 RenderThumbnail）
            if let Some(req) = q.pop_front() {
                return req;
            }
            q = self.condvar.wait(q).unwrap();
        }
    }
}

#[derive(Debug)]
pub enum PdfResponse {
    /// 文档加载完成
    DocumentLoaded {
        doc_id: u32,
        page_count: usize,
        page_sizes: Vec<(f32, f32)>,
    },
    /// 页面渲染完成
    PageRendered {
        page: u16,
        generation: u64,
        image: RgbaImage,
    },
    /// 缩略图渲染完成
    ThumbnailRendered {
        page: u16,
        generation: u64,
        image: RgbaImage,
    },
    /// 文本提取完成
    TextExtracted {
        page: u16,
        generation: u64,
        data: TextPageData,
    },
    /// 链接提取完成
    LinksExtracted {
        page: u16,
        generation: u64,
        data: LinkPageData,
    },
    /// Pin 子区域渲染完成
    PinRendered { pin_id: String, image: RgbaImage },
    /// 提取出文档的大纲结构
    OutlineExtracted {
        doc_id: u32,
        outlines: Vec<OutlineItem>,
    },
    /// 页面删除完成，文档已修改
    DocumentModified {
        doc_id: u32,
        page_count: usize,
        page_sizes: Vec<(f32, f32)>,
        deleted_page: u16,
    },
    /// 致命错误（如文档无法打开）
    FatalError(String),
}

// ─── Worker Core ─────────────────────────────────────────────

fn extract_outlines(outlines: &[mupdf::Outline]) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    for outline in outlines {
        let title = outline.title.clone();
        let page_index = outline
            .dest
            .map(|dest| dest.loc.page_number as u16)
            .unwrap_or(0);
        let children = extract_outlines(&outline.down);
        items.push(OutlineItem {
            title,
            page_index,
            children,
        });
    }
    items
}

pub fn get_global_pdf_queue() -> Arc<PdfTaskQueue> {
    static GLOBAL_QUEUE: OnceLock<Arc<PdfTaskQueue>> = OnceLock::new();
    GLOBAL_QUEUE
        .get_or_init(|| {
            let queue = Arc::new(PdfTaskQueue::new(200));
            start_global_worker(Arc::clone(&queue));
            queue
        })
        .clone()
}

fn render_page_to_image_internal(
    pdf_page: &mupdf::Page,
    scale: f32,
) -> Result<image::RgbaImage, String> {
    let bounds = pdf_page
        .bounds()
        .map_err(|e| format!("Failed to get page bounds: {:?}", e))?;

    let matrix = mupdf::Matrix::new(
        scale,
        0.0,
        0.0,
        scale,
        -bounds.x0 * scale,
        -bounds.y0 * scale,
    );

    let pixmap = pdf_page
        .to_pixmap(&matrix, &mupdf::Colorspace::device_bgr(), true, true)
        .map_err(|e| format!("Render error: {:?}", e))?;

    let width = pixmap.width();
    let height = pixmap.height();
    let rgba_bytes = pixmap.samples().to_vec();

    image::ImageBuffer::from_raw(width, height, rgba_bytes)
        .ok_or_else(|| "ImageBuffer creation failed".to_string())
}

fn start_global_worker(queue: Arc<PdfTaskQueue>) {
    thread::spawn(move || {
        info!("PDF Global Worker: 线程已启动");

        let mut documents: HashMap<u32, (mupdf::Document, SyncSender<PdfResponse>, PathBuf)> =
            HashMap::new();
        loop {
            let request = queue.pop();
            match request {
                PdfRequest::Shutdown => {
                    info!("PDF Global Worker: 收到全局关闭信号");
                    break;
                }
                PdfRequest::OpenDocument { doc_id, path, tx } => {
                    info!(
                        "PDF Global Worker: 正在加载文档 {}, ID: {}",
                        path.display(),
                        doc_id
                    );
                    let path_str = match path.to_str() {
                        Some(p) => p,
                        None => {
                            let _ =
                                tx.send(PdfResponse::FatalError("Path is not valid UTF-8".into()));
                            continue;
                        }
                    };
                    match mupdf::Document::open(path_str) {
                        Ok(document) => {
                            let page_count = match document.page_count() {
                                Ok(c) => c as usize,
                                Err(e) => {
                                    let _ = tx.send(PdfResponse::FatalError(format!(
                                        "Failed to get page count: {e:?}"
                                    )));
                                    continue;
                                }
                            };
                            let mut page_sizes = Vec::with_capacity(page_count);
                            for i in 0..page_count {
                                if let Ok(page) = document.load_page(i as i32) {
                                    if let Ok(bounds) = page.bounds() {
                                        page_sizes
                                            .push((bounds.x1 - bounds.x0, bounds.y1 - bounds.y0));
                                    } else {
                                        page_sizes.push((612.0, 792.0));
                                    }
                                } else {
                                    page_sizes.push((612.0, 792.0));
                                }
                            }
                            let _ = tx.send(PdfResponse::DocumentLoaded {
                                doc_id,
                                page_count,
                                page_sizes,
                            });

                            // 提取并发送大纲数据
                            let mut mapped_outlines = Vec::new();
                            if let Ok(outlines) = document.outlines() {
                                mapped_outlines = extract_outlines(&outlines);
                            }
                            let _ = tx.send(PdfResponse::OutlineExtracted {
                                doc_id,
                                outlines: mapped_outlines,
                            });

                            documents.insert(doc_id, (document, tx, path));
                        }
                        Err(e) => {
                            error!("PDF Global Worker: 文档加载失败: {e:?}");
                            let _ = tx.send(PdfResponse::FatalError(format!(
                                "Failed to load PDF: {e:?}"
                            )));
                        }
                    }
                }
                PdfRequest::CloseDocument { doc_id } => {
                    if let Some((document, _tx, _path)) = documents.remove(&doc_id) {
                        info!("PDF Global Worker: 文档 {} 已关闭", doc_id);
                        let ctx = mupdf::context::Context::get();
                        let raw_ctx: *mut mupdf_sys::fz_context = unsafe {
                            *(&ctx as *const mupdf::context::Context
                                as *const *mut mupdf_sys::fz_context)
                        };
                        let doc_ptr: *mut mupdf_sys::fz_document = unsafe {
                            *(&document as *const mupdf::Document
                                as *const *mut mupdf_sys::fz_document)
                        };
                        unsafe {
                            // 物理注销该文档已渲染的所有 Tile 缓存
                            mupdf_sys::fz_drop_drawn_tiles_for_document(raw_ctx, doc_ptr);
                            // 转换并物理清空 PDF 文档内部的资源解析缓存
                            let pdf_doc = mupdf_sys::pdf_specifics(raw_ctx, doc_ptr);
                            if !pdf_doc.is_null() {
                                mupdf_sys::pdf_empty_store(raw_ctx, pdf_doc);
                            }
                        }
                    }
                    if documents.is_empty() {
                        let ctx = mupdf::context::Context::get();
                        let raw_ctx: *mut mupdf_sys::fz_context = unsafe {
                            *(&ctx as *const mupdf::context::Context
                                as *const *mut mupdf_sys::fz_context)
                        };
                        unsafe {
                            mupdf_sys::fz_empty_store(raw_ctx);
                            mupdf_sys::fz_purge_glyph_cache(raw_ctx);
                        }
                        info!(
                            "PDF Global Worker: 所有文档已关闭，MuPDF 全局缓存 (256MB) 已物理清空"
                        );
                    }
                }
                PdfRequest::DeletePage { doc_id, page } => {
                    info!(
                        "PDF Global Worker: 正在删除文档 {} 的第 {} 页",
                        doc_id, page
                    );
                    if let Some((document, tx, path)) = documents.remove(&doc_id) {
                        match mupdf::pdf::PdfDocument::try_from(document) {
                            Ok(mut pdf_doc) => {
                                let page_no = page as i32;
                                if let Err(e) = pdf_doc.delete_page(page_no) {
                                    error!("delete_page 失败: {e:?}");
                                    let _ = tx.send(PdfResponse::FatalError(format!(
                                        "删除页面失败: {e:?}"
                                    )));
                                    continue;
                                }
                                // 保存到临时文件，避免 MuPDF 原地保存时因 Windows 文件锁导致删除原始文件失败
                                let temp_path = path.with_extension("tmp.pdf");
                                let temp_str = match temp_path.to_str() {
                                    Some(s) => s,
                                    None => continue,
                                };
                                if let Err(e) = pdf_doc.save(temp_str) {
                                    error!("保存临时文件失败: {e:?}");
                                    let _ = std::fs::remove_file(&temp_path);
                                    let _ = tx.send(PdfResponse::FatalError(format!(
                                        "保存文档失败: {e:?}"
                                    )));
                                    continue;
                                }
                                drop(pdf_doc);
                                // 替换原始文件
                                let _ = std::fs::remove_file(&path);
                                if let Err(e) = std::fs::rename(&temp_path, &path) {
                                    error!("替换原始文件失败: {e:?}");
                                    let _ = tx.send(PdfResponse::FatalError(format!(
                                        "保存文档失败: {e:?}"
                                    )));
                                    continue;
                                }
                                // 重新打开以更新 Document
                                let path_str = match path.to_str() {
                                    Some(p) => p,
                                    None => continue,
                                };
                                match mupdf::Document::open(path_str) {
                                    Ok(new_doc) => {
                                        let page_count = new_doc.page_count().unwrap_or(0) as usize;
                                        let mut page_sizes = Vec::with_capacity(page_count);
                                        for i in 0..page_count {
                                            if let Ok(p) = new_doc.load_page(i as i32) {
                                                if let Ok(bounds) = p.bounds() {
                                                    page_sizes.push((
                                                        bounds.x1 - bounds.x0,
                                                        bounds.y1 - bounds.y0,
                                                    ));
                                                } else {
                                                    page_sizes.push((612.0, 792.0));
                                                }
                                            } else {
                                                page_sizes.push((612.0, 792.0));
                                            }
                                        }
                                        let _ = tx.send(PdfResponse::DocumentModified {
                                            doc_id,
                                            page_count,
                                            page_sizes,
                                            deleted_page: page,
                                        });
                                        documents.insert(doc_id, (new_doc, tx, path));
                                    }
                                    Err(e) => {
                                        error!("删除页面后重新打开文档失败: {e:?}");
                                        let _ = tx.send(PdfResponse::FatalError(format!(
                                            "重新打开文档失败: {e:?}"
                                        )));
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Document → PdfDocument 转换失败: {e:?}");
                                let _ = tx.send(PdfResponse::FatalError(format!(
                                    "文档不支持编辑: {e:?}"
                                )));
                            }
                        }
                    }
                }
                PdfRequest::ExtractPages {
                    doc_id,
                    pages,
                    dest_path,
                } => {
                    info!(
                        "PDF Global Worker: 正在导出文档 {} 的 {} 页到 {:?}",
                        doc_id,
                        pages.len(),
                        dest_path
                    );
                    // 仅读取源路径，不移除/修改源文档（源文档保持打开）
                    let src_path = match documents.get(&doc_id) {
                        Some((_, _, path)) => path.clone(),
                        None => {
                            error!("ExtractPages: 文档 {} 未打开", doc_id);
                            continue;
                        }
                    };

                    // 复制源文件到目标路径
                    if let Err(e) = std::fs::copy(&src_path, &dest_path) {
                        error!("ExtractPages: 复制文件失败: {e:?}");
                        continue;
                    }

                    let dest_str = match dest_path.to_str() {
                        Some(s) => s.to_string(),
                        None => {
                            let _ = std::fs::remove_file(&dest_path);
                            continue;
                        }
                    };

                    // 打开副本以统计页数并转为可编辑 PdfDocument
                    let opened = match mupdf::Document::open(dest_str.as_str()) {
                        Ok(d) => d,
                        Err(e) => {
                            error!("ExtractPages: 打开副本失败: {e:?}");
                            let _ = std::fs::remove_file(&dest_path);
                            continue;
                        }
                    };
                    let count = opened.page_count().unwrap_or(0);
                    let mut dst = match mupdf::pdf::PdfDocument::try_from(opened) {
                        Ok(p) => p,
                        Err(e) => {
                            error!("ExtractPages: Document → PdfDocument 转换失败: {e:?}");
                            let _ = std::fs::remove_file(&dest_path);
                            continue;
                        }
                    };

                    // 降序删除所有未选中的页（降序避免索引前移错位）
                    let selected: HashSet<u16> = pages.iter().copied().collect();
                    let mut failed = false;
                    for p in (0..count).rev() {
                        let p_u16 = p as u16;
                        if !selected.contains(&p_u16)
                            && let Err(e) = dst.delete_page(p)
                        {
                            error!("ExtractPages: 删除页 {} 失败: {e:?}", p);
                            failed = true;
                            break;
                        }
                    }

                    if failed {
                        let _ = std::fs::remove_file(&dest_path);
                    } else if let Err(e) = dst.save(dest_str.as_str()) {
                        error!("ExtractPages: 保存失败: {e:?}");
                        let _ = std::fs::remove_file(&dest_path);
                    } else {
                        info!(
                            "PDF Global Worker: 导出完成, 共保留 {} 页 -> {:?}",
                            selected.len(),
                            dest_path
                        );
                    }
                }
                PdfRequest::RenderPage {
                    doc_id,
                    page,
                    scale,
                    generation,
                } => {
                    if let Some((document, tx, _)) = documents.get(&doc_id) {
                        let start_time = std::time::Instant::now();
                        debug!("PDF Worker: 开始渲染页面 {}, 代数 {}", page, generation);
                        let pdf_page = match document.load_page(page as i32) {
                            Ok(p) => p,
                            Err(e) => {
                                // 致命错误策略：发送 FatalError
                                let _ = tx.send(PdfResponse::FatalError(format!(
                                    "Failed to get page: {e:?}"
                                )));
                                continue;
                            }
                        };

                        match render_page_to_image_internal(&pdf_page, scale) {
                            Ok(img) => {
                                info!(
                                    "PDF Worker: 页面渲染成功 - 页面 {}, 分辨率 {}x{}, 耗时 {:?}",
                                    page,
                                    img.width(),
                                    img.height(),
                                    start_time.elapsed()
                                );
                                let _ = tx.send(PdfResponse::PageRendered {
                                    page,
                                    generation,
                                    image: img,
                                });
                            }
                            Err(err_msg) => {
                                // 致命错误策略：发送 FatalError
                                error!(
                                    "PDF Worker: 页面渲染失败 - 页面 {}, 错误: {}",
                                    page, err_msg
                                );
                                let _ = tx.send(PdfResponse::FatalError(err_msg));
                            }
                        }
                    }
                }
                PdfRequest::RenderThumbnail {
                    doc_id,
                    page,
                    max_size,
                    generation,
                } => {
                    if let Some((document, tx, _)) = documents.get(&doc_id) {
                        let start_time = std::time::Instant::now();
                        debug!("PDF Worker: 开始渲染缩略图 {}, 代数 {}", page, generation);
                        let pdf_page = match document.load_page(page as i32) {
                            Ok(p) => p,
                            Err(e) => {
                                // 非致命错误策略：仅打印日志，不发 FatalError
                                error!("PDF Worker: 缩略图加载失败 - 页面 {}, 错误: {:?}", page, e);
                                continue;
                            }
                        };

                        let bounds = match pdf_page.bounds() {
                            Ok(b) => b,
                            Err(e) => {
                                // 非致命错误策略
                                error!(
                                    "PDF Worker: 缩略图获取 bounds 失败 - 页面 {}, 错误: {:?}",
                                    page, e
                                );
                                continue;
                            }
                        };

                        let base_w = bounds.x1 - bounds.x0;
                        let base_h = bounds.y1 - bounds.y0;
                        let scale = max_size / base_w.max(base_h);

                        match render_page_to_image_internal(&pdf_page, scale) {
                            Ok(img) => {
                                info!(
                                    "PDF Worker: 缩略图渲染成功 - 页面 {}, 分辨率 {}x{}, 耗时 {:?}",
                                    page,
                                    img.width(),
                                    img.height(),
                                    start_time.elapsed()
                                );
                                let _ = tx.send(PdfResponse::ThumbnailRendered {
                                    page,
                                    generation,
                                    image: img,
                                });
                            }
                            Err(err_msg) => {
                                // 非致命错误策略：仅在日志记录警告
                                error!(
                                    "PDF Worker: 缩略图渲染失败 - 页面 {}, 错误: {}",
                                    page, err_msg
                                );
                            }
                        }
                    }
                }
                PdfRequest::RenderPin {
                    doc_id,
                    page,
                    pin_id,
                    bbox,
                    zoom,
                } => {
                    if let Some((document, tx, _)) = documents.get(&doc_id) {
                        let pdf_page = match document.load_page(page as i32) {
                            Ok(p) => p,
                            Err(e) => {
                                error!("PDF Worker: RenderPin 加载页面失败: {e:?}");
                                continue;
                            }
                        };
                        let dl = match pdf_page.to_display_list(false) {
                            Ok(d) => d,
                            Err(e) => {
                                error!("PDF Worker: RenderPin 创建 DisplayList 失败: {e:?}");
                                continue;
                            }
                        };

                        let (x0, y0, x1, y1) = bbox;
                        let pw = (x1 - x0) * zoom;
                        let ph = (y1 - y0) * zoom;
                        if pw < 1.0 || ph < 1.0 {
                            continue;
                        }

                        let cs = mupdf::Colorspace::device_bgr();
                        let irect = mupdf::IRect::new(0, 0, pw as i32, ph as i32);
                        let mut pixmap = match mupdf::Pixmap::new_with_rect(&cs, irect, true) {
                            Ok(p) => p,
                            Err(e) => {
                                error!("PDF Worker: RenderPin 创建 Pixmap 失败: {e:?}");
                                continue;
                            }
                        };
                        let _ = pixmap.clear();

                        let device = match mupdf::Device::from_pixmap(&pixmap) {
                            Ok(d) => d,
                            Err(e) => {
                                error!("PDF Worker: RenderPin 创建 Device 失败: {e:?}");
                                continue;
                            }
                        };

                        let matrix =
                            mupdf::Matrix::new(zoom, 0.0, 0.0, zoom, -x0 * zoom, -y0 * zoom);
                        let clip_rect = mupdf::Rect::new(0.0, 0.0, pw, ph);

                        if let Err(e) = dl.run(&device, &matrix, clip_rect) {
                            error!("PDF Worker: RenderPin 渲染 DisplayList 失败: {e:?}");
                            continue;
                        }

                        std::mem::drop(device);

                        let width = pixmap.width();
                        let height = pixmap.height();
                        let rgba_bytes = pixmap.samples().to_vec();

                        if let Some(img) = ImageBuffer::from_raw(width, height, rgba_bytes) {
                            debug!(
                                "PDF Worker: RenderPin 成功 - page {}, pin {}, 分辨率 {}x{}",
                                page, pin_id, width, height
                            );
                            let _ = tx.send(PdfResponse::PinRendered { pin_id, image: img });
                        }
                    }
                }
                PdfRequest::ExtractLinks {
                    doc_id,
                    page,
                    display_w,
                    display_h,
                    generation,
                } => {
                    if let Some((document, tx, _)) = documents.get(&doc_id) {
                        debug!("PDF Worker: 开始提取页面 {} 的链接", page);
                        let pdf_page = match document.load_page(page as i32) {
                            Ok(p) => p,
                            Err(e) => {
                                let _ = tx.send(PdfResponse::FatalError(format!(
                                    "Failed to get page: {e:?}"
                                )));
                                continue;
                            }
                        };

                        let bounds = match pdf_page.bounds() {
                            Ok(b) => b,
                            Err(e) => {
                                let _ = tx.send(PdfResponse::FatalError(format!(
                                    "Failed to get page bounds: {e:?}"
                                )));
                                continue;
                            }
                        };

                        let page_width = bounds.x1 - bounds.x0;
                        let page_height = bounds.y1 - bounds.y0;
                        let scale = (display_w / page_width + display_h / page_height) / 2.0;

                        let mut links_data = Vec::new();
                        let mut destination_bounds = HashMap::new();
                        if let Ok(links_iter) = pdf_page.links() {
                            for link in links_iter {
                                let rect = link.bounds;
                                let left = rect.x0 * scale;
                                let top = rect.y0 * scale;
                                let right = rect.x1 * scale;
                                let bottom = rect.y1 * scale;

                                let target = match link.dest {
                                    Some(dest) => {
                                        let page_number = dest.loc.page_number;
                                        match u16::try_from(page_number) {
                                            Ok(target_page) => {
                                                let normalized_y = match destination_top(&dest.kind)
                                                {
                                                    Some(top) => {
                                                        let bounds = destination_bounds
                                                            .entry(target_page)
                                                            .or_insert_with(|| {
                                                                document
                                                                    .load_page(target_page as i32)
                                                                    .and_then(|page| page.bounds())
                                                            });
                                                        match bounds {
                                                            Ok(bounds) => normalized_destination_y(
                                                                top, bounds.y0, bounds.y1,
                                                            ),
                                                            Err(error) => {
                                                                debug!(
                                                                    "PDF Worker: 无法读取内部链接目标页 {} bounds，降级到页首: {:?}",
                                                                    target_page, error
                                                                );
                                                                None
                                                            }
                                                        }
                                                    }
                                                    None => None,
                                                };
                                                LinkTarget::Internal {
                                                    page: target_page,
                                                    normalized_y,
                                                }
                                            }
                                            Err(_) => {
                                                debug!(
                                                    "PDF Worker: 内部链接目标页 {} 超出 u16，降级为外部 URL",
                                                    page_number
                                                );
                                                LinkTarget::External { url: link.uri }
                                            }
                                        }
                                    }
                                    None => LinkTarget::External { url: link.uri },
                                };

                                links_data.push(LinkInfo {
                                    left,
                                    top,
                                    right,
                                    bottom,
                                    target,
                                });
                            }
                        }

                        debug!(
                            "PDF Worker: 链接提取成功 - 页面 {}, 共 {} 个链接",
                            page,
                            links_data.len()
                        );
                        let _ = tx.send(PdfResponse::LinksExtracted {
                            page,
                            generation,
                            data: LinkPageData {
                                links: links_data,
                                display_w,
                                display_h,
                            },
                        });
                    }
                }

                PdfRequest::ExtractText {
                    doc_id,
                    page,
                    display_w,
                    display_h: _,
                    generation,
                } => {
                    if let Some((document, tx, _)) = documents.get(&doc_id) {
                        debug!("PDF Worker: 开始提取页面 {} 的文本", page);
                        let pdf_page = match document.load_page(page as i32) {
                            Ok(p) => p,
                            Err(e) => {
                                let _ = tx.send(PdfResponse::FatalError(format!(
                                    "Failed to get page: {e:?}"
                                )));
                                continue;
                            }
                        };

                        let bounds = match pdf_page.bounds() {
                            Ok(b) => b,
                            Err(e) => {
                                let _ = tx.send(PdfResponse::FatalError(format!(
                                    "Failed to get page bounds: {e:?}"
                                )));
                                continue;
                            }
                        };

                        let page_width = bounds.x1 - bounds.x0;
                        let scale = display_w / page_width;

                        let text_page = match pdf_page.to_text_page(mupdf::TextPageFlags::empty()) {
                            Ok(t) => t,
                            Err(e) => {
                                error!("PDF Worker: 文本提取失败 - 页面 {}: {e:?}", page);
                                continue;
                            }
                        };

                        let mut all_chars: Vec<TextChar> = Vec::new();
                        let mut i = 0;

                        for block in text_page.blocks() {
                            if block.r#type() == mupdf::text_page::TextBlockType::Text {
                                for line in block.lines() {
                                    for ch in line.chars() {
                                        let quad = ch.quad();
                                        let unicode_char = ch.char().unwrap_or(' ');
                                        let origin = ch.origin();
                                        let font_size = ch.size();

                                        let ascender = 0.75;
                                        let descender = -0.2;

                                        let pdf_top = origin.y - font_size * ascender;
                                        let pdf_bottom = origin.y - font_size * descender;

                                        let left = [quad.ul.x, quad.ur.x, quad.ll.x, quad.lr.x]
                                            .into_iter()
                                            .fold(f32::MAX, f32::min)
                                            * scale;
                                        let right = [quad.ul.x, quad.ur.x, quad.ll.x, quad.lr.x]
                                            .into_iter()
                                            .fold(f32::MIN, f32::max)
                                            * scale;

                                        let x = left;
                                        let top = pdf_top * scale;
                                        let width = right - left;
                                        let height = (pdf_bottom - pdf_top) * scale;
                                        let baseline = ch.origin().y * scale;

                                        all_chars.push(TextChar {
                                            id: i,
                                            char: unicode_char,
                                            x,
                                            y: top,
                                            width,
                                            height,
                                            font_size: font_size * scale,
                                            baseline,
                                        });
                                        i += 1;
                                    }
                                }
                            }
                        }

                        debug!(
                            "PDF Worker: 文本提取成功 - 页面 {}, 共 {} 个字符",
                            page,
                            all_chars.len()
                        );
                        let _ = tx.send(PdfResponse::TextExtracted {
                            page,
                            generation,
                            data: TextPageData {
                                chars: all_chars,
                                display_w,
                            },
                        });
                    }
                }
            }
        }
        info!("PDF Global Worker: 线程已退出");
    });
}

// ─── 带标注 PDF 导出 ──────────────────────────────────────────

/// 将 `annotations` 以原生 PDF 标注写入 `dest_path`（源文件只读不改写）。
pub(crate) fn export_annotated_pdf(
    src_path: &std::path::Path,
    dest_path: &std::path::Path,
    annotations: &[Annotation],
) -> Result<(), String> {
    // 1. 复制源文件到目标路径（与 ExtractPages 保存方式保持 100% 一致）
    std::fs::copy(src_path, dest_path).map_err(|e| format!("复制源文件到目标路径失败: {e:?}"))?;

    let dest_str = match dest_path.to_str() {
        Some(s) => s,
        None => {
            let _ = std::fs::remove_file(dest_path);
            return Err("目标路径不是有效 UTF-8".to_string());
        }
    };

    // 2. 打开副本文件以进行修改并增量保存
    let document = match mupdf::Document::open(dest_str) {
        Ok(d) => d,
        Err(e) => {
            let _ = std::fs::remove_file(dest_path);
            return Err(format!("打开副本 PDF 失败: {e:?}"));
        }
    };
    let pdf_doc = match mupdf::pdf::PdfDocument::try_from(document) {
        Ok(p) => p,
        Err(e) => {
            let _ = std::fs::remove_file(dest_path);
            return Err(format!("PDF 转换失败: {e:?}"));
        }
    };

    // 3. 按页收集需要导出的标注
    let mut by_page: HashMap<u16, Vec<&Annotation>> = HashMap::new();
    for ann in annotations {
        if ann.is_deleted {
            continue;
        }
        match &ann.kind {
            AnnotationKind::Highlight | AnnotationKind::Underline => {
                if let Some(range) = &ann.range {
                    let end = range.end_page_or();
                    for p in range.start_page..=end {
                        by_page.entry(p).or_default().push(ann);
                    }
                }
            }
            AnnotationKind::Rectangle { .. } => {
                by_page.entry(ann.page).or_default().push(ann);
            }
        }
    }

    let page_count = match pdf_doc.page_count() {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_file(dest_path);
            return Err(format!("读取页数失败: {e:?}"));
        }
    };

    for (page_no, anns) in by_page {
        if page_no as i32 >= page_count {
            continue;
        }
        let mut pdf_page = match pdf_doc.load_pdf_page(page_no as i32) {
            Ok(p) => p,
            Err(e) => {
                error!("加载第 {} 页失败: {e:?}", page_no);
                continue;
            }
        };
        let bounds = match pdf_page.bounds() {
            Ok(b) => b,
            Err(_) => continue,
        };
        let page_w = bounds.x1 - bounds.x0;
        let page_h = bounds.y1 - bounds.y0;

        let char_quads = extract_page_char_quads(&pdf_page).unwrap_or_default();

        for ann in anns {
            match &ann.kind {
                AnnotationKind::Highlight | AnnotationKind::Underline => {
                    let range = match &ann.range {
                        Some(r) => r,
                        None => continue,
                    };
                    let end_page = range.end_page_or();
                    let start = if page_no == range.start_page {
                        range.start_char
                    } else {
                        0
                    };
                    let end = if page_no == end_page {
                        range.end_char
                    } else {
                        char_quads.len().saturating_sub(1)
                    };
                    if start > end || start >= char_quads.len() {
                        continue;
                    }
                    let end = end.min(char_quads.len().saturating_sub(1));
                    let line_quads = merge_quads_to_lines(&char_quads[start..=end]);
                    if line_quads.is_empty() {
                        continue;
                    }
                    let annot = if matches!(ann.kind, AnnotationKind::Highlight) {
                        pdf_page.add_highlight_annotation(mupdf::pdf::AnnotationQuadPoints::new(
                            line_quads,
                        ))
                    } else {
                        pdf_page.add_underline_annotation(mupdf::pdf::AnnotationQuadPoints::new(
                            line_quads,
                        ))
                    };
                    if let Ok(mut annot) = annot {
                        let _ = apply_annotation_common(&mut annot, ann);
                        let _ = annot.update();
                    }
                }
                AnnotationKind::Rectangle { x, y, w, h } => {
                    // bounds() 与 add_rect_annotation() 均在设备空间（左上原点、y 向下），
                    // 与 UI 归一化坐标系一致，无需 Y 轴翻转。
                    let x0 = bounds.x0 + x * page_w;
                    let y0 = bounds.y0 + y * page_h;
                    let x1 = bounds.x0 + (x + w) * page_w;
                    let y1 = bounds.y0 + (y + h) * page_h;

                    let rect = mupdf::Rect::new(x0, y0, x1, y1);
                    if let Ok(mut annot) = pdf_page.add_rect_annotation(rect) {
                        let _ = apply_annotation_common(&mut annot, ann);
                        let _ = annot.update();
                    }
                }
            }
        }
    }

    // 4. 原路 save 保存回副本路径
    if let Err(e) = pdf_doc.save(dest_str) {
        let _ = std::fs::remove_file(dest_path);
        return Err(format!("保存 PDF 标注失败: {e:?}"));
    }

    info!("PDF Global Worker: 带标注 PDF 导出完成 -> {:?}", dest_path);
    Ok(())
}

/// 提取页面内所有字符的 Quad（PDF 空间、未缩放），顺序与 ExtractText 的 char id 一致。
fn extract_page_char_quads(pdf_page: &mupdf::Page) -> Result<Vec<mupdf::Quad>, String> {
    let text_page = pdf_page
        .to_text_page(mupdf::TextPageFlags::empty())
        .map_err(|e| format!("文本提取失败: {e:?}"))?;
    let mut quads = Vec::new();
    for block in text_page.blocks() {
        if block.r#type() == mupdf::text_page::TextBlockType::Text {
            for line in block.lines() {
                for ch in line.chars() {
                    quads.push(ch.quad());
                }
            }
        }
    }
    Ok(quads)
}

/// 将一段连续字符的 quads 按行合并：同一行的字符合并为一个横跨整行的 quad。
fn merge_quads_to_lines(quads: &[mupdf::Quad]) -> Vec<mupdf::Quad> {
    if quads.is_empty() {
        return Vec::new();
    }

    let mut lines: Vec<Vec<&mupdf::Quad>> = Vec::new();
    for q in quads {
        // 垂直中心与高度，用于行聚类
        let y0 = q.ll.y.min(q.lr.y);
        let y1 = q.ul.y.max(q.ur.y);
        let mid = (y0 + y1) / 2.0;
        let h = (y1 - y0).abs().max(0.001);

        let mut placed = false;
        if let Some(line) = lines.last_mut() {
            let l_y0 = line
                .iter()
                .map(|lq| lq.ll.y.min(lq.lr.y))
                .fold(f32::MAX, f32::min);
            let l_y1 = line
                .iter()
                .map(|lq| lq.ul.y.max(lq.ur.y))
                .fold(f32::MIN, f32::max);
            let l_mid = (l_y0 + l_y1) / 2.0;
            let l_h = (l_y1 - l_y0).abs().max(0.001);
            // 中线差距小于两行高度的较大者，则视为同一行
            if (mid - l_mid).abs() < l_h.max(h) * 0.75 {
                line.push(q);
                placed = true;
            }
        }
        if !placed {
            lines.push(vec![q]);
        }
    }

    lines
        .into_iter()
        .map(|line| {
            let mut min_x = f32::MAX;
            let mut max_x = f32::MIN;
            let mut min_y = f32::MAX;
            let mut max_y = f32::MIN;
            for q in line {
                min_x = min_x.min(q.ul.x).min(q.ur.x).min(q.ll.x).min(q.lr.x);
                max_x = max_x.max(q.ul.x).max(q.ur.x).max(q.ll.x).max(q.lr.x);
                min_y = min_y.min(q.ul.y).min(q.ur.y).min(q.ll.y).min(q.lr.y);
                max_y = max_y.max(q.ul.y).max(q.ur.y).max(q.ll.y).max(q.lr.y);
            }
            // 顶点坐标：ul(左上), ur(右上), ll(左下), lr(右下)
            mupdf::Quad::new(
                mupdf::Point::new(min_x, min_y), // ul
                mupdf::Point::new(max_x, min_y), // ur
                mupdf::Point::new(min_x, max_y), // ll
                mupdf::Point::new(max_x, max_y), // lr
            )
        })
        .collect()
}

/// 对已创建的标注设置颜色、作者与笔记内容。
fn apply_annotation_common(
    annot: &mut mupdf::pdf::PdfAnnotation,
    ann: &Annotation,
) -> Result<(), String> {
    if let Some((r, g, b)) = parse_hex_color(ann.color) {
        // 高亮标注在 PDF 导出时与白色背景进行 Alpha 预混合 (alpha = 0.376)，
        // 确保写出的 RGB 真实色彩与 UI 呈现的柔和淡色 100% 精确一致，不受外部 PDF 阅读器透明度兼容性影响。
        let (red, green, blue) = match ann.kind {
            AnnotationKind::Highlight => {
                let alpha = 0.376;
                (
                    r * alpha + 1.0 * (1.0 - alpha),
                    g * alpha + 1.0 * (1.0 - alpha),
                    b * alpha + 1.0 * (1.0 - alpha),
                )
            }
            _ => (r, g, b),
        };
        annot
            .set_color(mupdf::color::AnnotationColor::Rgb { red, green, blue })
            .map_err(|e| format!("设置标注颜色失败: {e:?}"))?;
    }
    let _ = annot.set_author("Lumen");
    if let Some(note) = &ann.note
        && !note.trim().is_empty()
    {
        annot
            .set_contents(note)
            .map_err(|e| format!("设置标注笔记失败: {e:?}"))?;
    }
    Ok(())
}

/// 将 `AnnotationColor::to_hex()` 的 `#rrggbb` 解析为 0..1 的 RGB 浮点。
fn parse_hex_color(color: AnnotationColor) -> Option<(f32, f32, f32)> {
    let hex = color.to_hex().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0))
}
