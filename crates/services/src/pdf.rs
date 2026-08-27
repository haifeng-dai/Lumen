pub use annotation::*;
use anyhow::Result;
use i18n::Language;
use log::{debug, info};
pub use math_preprocess::preprocess_math;
use models::chat::{ChatMessage, ChatSession};
pub use models::{Annotation, AnnotationColor, AnnotationKind, TextRange};
pub use pdf_worker::*;
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc::{Receiver, sync_channel},
    },
};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiBackendItem {
    pub name: String,
    pub kind: String,
    pub model: String,
}

pub mod annotation;
mod math_preprocess;
mod pdf_worker;

/// 单个字符的文本信息
#[derive(Debug, Clone)]
pub struct TextChar {
    pub id: usize,
    pub char: char,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
    pub baseline: f32,
}

/// 页面的所有文本数据
#[derive(Debug, Clone)]
pub struct TextPageData {
    pub chars: Vec<TextChar>,
    /// 生成此数据时的 display_width_px，用于缓存键验证
    pub display_w: f32,
}

impl TextPageData {
    pub fn merge_char_blocks(&self, start: usize, end: usize) -> Vec<(f32, f32, f32, f32)> {
        if self.chars.is_empty() || start > end || end >= self.chars.len() {
            return Vec::new();
        }

        // 1. 全页物理行聚类，获得每行真正的平均 Y 和 Height（包括未选中的字符）
        let mut char_line_ids = vec![0; self.chars.len()];
        let mut current_line_ys = Vec::new();
        let mut current_line_heights = Vec::new();
        let mut current_line_char_indices = Vec::new();
        let mut line_count = 0;

        for (i, ch) in self.chars.iter().enumerate() {
            if !current_line_char_indices.is_empty() {
                let avg_y = current_line_ys.iter().sum::<f32>() / current_line_ys.len() as f32;
                let avg_h =
                    current_line_heights.iter().sum::<f32>() / current_line_heights.len() as f32;

                let overlap_y1 = avg_y.max(ch.y);
                let overlap_y2 = (avg_y + avg_h).min(ch.y + ch.height);
                let overlap = overlap_y2 - overlap_y1;
                let min_h = avg_h.min(ch.height);

                let overlaps_vertically = overlap > 0.0 && overlap >= 0.3 * min_h;

                if overlaps_vertically {
                    char_line_ids[i] = line_count;
                    current_line_ys.push(ch.y);
                    current_line_heights.push(ch.height);
                    current_line_char_indices.push(i);
                } else {
                    line_count += 1;
                    char_line_ids[i] = line_count;
                    current_line_ys.clear();
                    current_line_heights.clear();
                    current_line_char_indices.clear();
                    current_line_ys.push(ch.y);
                    current_line_heights.push(ch.height);
                    current_line_char_indices.push(i);
                }
            } else {
                char_line_ids[i] = line_count;
                current_line_ys.push(ch.y);
                current_line_heights.push(ch.height);
                current_line_char_indices.push(i);
            }
        }

        // 2. 根据选中的 [start, end] 范围内的字符，根据行 ID 合并高亮块
        let mut blocks = Vec::new();
        let mut current_line_chars: Vec<&TextChar> = Vec::new();
        let mut current_line_id: Option<usize> = None;

        let push_line = |line: &Vec<&TextChar>, blocks: &mut Vec<(f32, f32, f32, f32)>| {
            if line.is_empty() {
                return;
            }

            let mut bx = f32::MAX;
            let mut by = f32::MAX;
            let mut b_max_x = f32::MIN;
            let mut b_max_y = f32::MIN;
            let mut has_visible = false;

            for ch in line {
                if ch.char.is_whitespace() || ch.char == '\u{00A0}' {
                    continue;
                }
                has_visible = true;
                bx = bx.min(ch.x);
                by = by.min(ch.y);
                b_max_x = b_max_x.max(ch.x + ch.width);
                b_max_y = b_max_y.max(ch.y + ch.height);
            }

            if has_visible {
                blocks.push((bx, by, b_max_x, b_max_y));
            }
        };

        for (i, ch) in self.chars.iter().enumerate().take(end + 1).skip(start) {
            let line_id = char_line_ids[i];
            if let Some(curr_id) = current_line_id {
                if line_id == curr_id {
                    current_line_chars.push(ch);
                } else {
                    push_line(&current_line_chars, &mut blocks);
                    current_line_chars.clear();
                    current_line_chars.push(ch);
                    current_line_id = Some(line_id);
                }
            } else {
                current_line_chars.push(ch);
                current_line_id = Some(line_id);
            }
        }

        push_line(&current_line_chars, &mut blocks);

        blocks
    }
}

/// PDF 大纲（书签）节点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineItem {
    pub title: String,
    pub page_index: u16,            // 点击后跳转的目标页码
    pub children: Vec<OutlineItem>, // 嵌套的子大纲列表
}

/// PDF 链接信息
#[derive(Debug, Clone)]
pub struct LinkInfo {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub url: String,
}

/// 页面的所有链接数据
#[derive(Debug, Clone)]
pub struct LinkPageData {
    pub links: Vec<LinkInfo>,
    /// 生成此数据时的 display 尺寸，用于缓存键验证
    pub display_w: f32,
    pub display_h: f32,
}

/// 量化缩放级别 - 减少缓存变体
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZoomLevel {
    VerySmall = 0, // 0.25x - 0.5x
    Small = 1,     // 0.5x - 0.75x
    Normal = 2,    // 0.75x - 1.25x
    Large = 3,     // 1.25x - 1.75x
    VeryLarge = 4, // 1.75x - 2.5x
    Huge = 5,      // 2.5x+
}

impl From<f32> for ZoomLevel {
    fn from(zoom: f32) -> Self {
        match zoom {
            z if z <= 0.5 => ZoomLevel::VerySmall,
            z if z <= 0.75 => ZoomLevel::Small,
            z if z <= 1.25 => ZoomLevel::Normal,
            z if z <= 1.75 => ZoomLevel::Large,
            z if z <= 2.5 => ZoomLevel::VeryLarge,
            _ => ZoomLevel::Huge,
        }
    }
}

/// PDF 的初始状态
#[derive(Debug, Clone)]
pub struct PdfInitialState {
    pub page_index: u16,
    pub zoom_level: f32,
    pub offset_y: f32,
    pub fit_to_width: bool,
    pub auto_translate: bool,
    pub is_left_sidebar_open: bool,
    pub is_right_sidebar_open: bool,
    pub left_sidebar_width: f32,
    pub right_sidebar_width: f32,
    pub translation_font_size: f32,
    pub translation_original_expanded: bool,
}

impl Default for PdfInitialState {
    fn default() -> Self {
        Self {
            page_index: 0,
            zoom_level: 1.0,
            offset_y: 0.0,
            fit_to_width: true,
            auto_translate: true,
            is_left_sidebar_open: true,
            is_right_sidebar_open: true,
            left_sidebar_width: 0.0,
            right_sidebar_width: 0.0,
            translation_font_size: 14.0,
            translation_original_expanded: true,
        }
    }
}

/// 从 PDF 文件中提取全部文本（同步，用于 AI Chat 附件）
pub fn extract_text_from_pdf(path: &str) -> Result<String> {
    use log::debug;
    let doc = mupdf::Document::open(path)?;
    let page_count = doc.page_count()?;
    debug!("extract_text_from_pdf: path={path}, pages={page_count}");
    let mut all_text = String::new();
    for i in 0..page_count {
        let page = doc.load_page(i)?;
        let text_page = page.to_text_page(mupdf::TextPageFlags::empty())?;
        for block in text_page.blocks() {
            if block.r#type() == mupdf::text_page::TextBlockType::Text {
                for line in block.lines() {
                    for ch in line.chars() {
                        let c = ch.char().unwrap_or(' ');
                        all_text.push(c);
                    }
                    all_text.push('\n');
                }
                all_text.push('\n');
            }
        }
    }
    debug!("extract_text_from_pdf: done, chars={}", all_text.len());
    Ok(all_text)
}

/// PDF 阅读器委托，用于处理跨模块交互
pub trait PdfReaderDelegate: Send + Sync + 'static {
    /// 获取初始状态（从持久化存储中读取）
    fn get_initial_state(&self, _id: String) -> PdfInitialState {
        PdfInitialState::default()
    }

    /// 当阅读状态（页码或缩放）改变时触发
    #[allow(clippy::too_many_arguments)]
    fn save_state(
        &self,
        _id: String,
        _page: u16,
        _zoom: f32,
        _offset_y: f32,
        _fit_to_width: bool,
        _is_left_sidebar_open: bool,
        _is_right_sidebar_open: bool,
        _left_sidebar_width: f32,
        _right_sidebar_width: f32,
        _auto_translate: bool,
    ) {
    }

    /// 翻译文本
    fn translate(
        &self,
        _text: String,
        _force: bool,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
        Box::pin(async {
            Ok(i18n::t(
                i18n::I18nKey::TranslationNotImplemented,
                i18n::Language::ZhCn,
            )
            .to_string())
        })
    }

    /// 获取可用的翻译引擎列表
    fn get_translation_engines(&self) -> Vec<String> {
        vec!["google_free".to_string(), "bing_free".to_string()]
    }

    /// 获取当前翻译引擎 ID
    fn current_translation_engine_id(&self) -> String {
        "google_free".to_string()
    }

    /// 切换翻译引擎
    fn set_translation_engine(&self, _name: String) {}

    /// 加载文档的所有注释
    fn load_annotations(&self, _document_id: &str) -> Vec<Annotation> {
        vec![]
    }

    /// 保存单条注释
    fn save_annotation(&self, _annotation: &Annotation) {}

    /// 删除单条注释
    fn delete_annotation(&self, _id: &str) {}

    /// 点击 PDF 链接时的回调
    fn on_link_click(&self, _url: String) {}

    /// 获取当前语言
    fn current_language(&self) -> Language {
        Language::ZhCn
    }

    /// 设置翻译字体大小
    fn set_translation_font_size(&self, _size: f32) {}

    /// 获取当前文献的附件列表
    fn current_literature_attachments(&self) -> Vec<models::Attachment> {
        Vec::new()
    }

    /// 获取当前翻译字体大小
    fn translation_font_size(&self) -> f32 {
        14.0
    }

    /// 设置原文框展开/收起（全局持久化）
    fn set_translation_original_expanded(&self, _expanded: bool) {}

    /// 获取全局的页面底色模式 ("white", "sepia", "eyeprotect")
    fn get_page_color_mode(&self) -> String {
        "white".to_string()
    }

    /// 设置全局的页面底色模式
    fn set_page_color_mode(&self, _mode: String) {}

    /// 获取所有笔记
    fn list_notes(&self, _literature_id: &str) -> Vec<models::LiteratureNote> {
        Vec::new()
    }

    /// 创建笔记，返回新 ID
    fn create_note(&self, _literature_id: &str, _title: &str) -> Option<String> {
        None
    }

    /// 更新笔记标题和内容
    fn update_note(&self, _note_id: &str, _title: Option<&str>, _content: Option<&str>) -> bool {
        false
    }

    /// 删除笔记
    fn delete_note(&self, _note_id: &str) -> bool {
        false
    }

    // ── AI 对话 ─────────────────────────────────────────

    /// 获取某文献的所有对话
    fn list_chat_sessions(&self, _literature_id: &str) -> Vec<ChatSession> {
        Vec::new()
    }

    /// 创建新对话，返回 ID
    fn create_chat_session(
        &self,
        _literature_id: &str,
        _title: &str,
        _system_prompt: &str,
    ) -> Option<String> {
        None
    }

    /// 删除对话
    fn delete_chat_session(&self, _session_id: &str) -> bool {
        false
    }

    /// 更新对话标题或系统提示词
    fn update_chat_session(
        &self,
        _session_id: &str,
        _title: Option<&str>,
        _system_prompt: Option<&str>,
    ) -> bool {
        false
    }

    /// 获取所有可用的 AI 后端列表
    fn list_ai_backends(&self) -> Vec<AiBackendItem> {
        Vec::new()
    }

    /// 获取当前聊天活跃后端名
    fn get_active_chat_backend(&self) -> Option<String> {
        None
    }

    /// 切换聊天活跃后端
    fn set_active_chat_backend(&self, _name: String) {}

    /// 获取某对话的所有消息
    fn list_chat_messages(&self, _session_id: &str) -> Vec<ChatMessage> {
        Vec::new()
    }

    /// 添加消息到对话，返回消息 ID（attachments 为 file_path 列表）
    fn add_chat_message(
        &self,
        _session_id: &str,
        _role: &str,
        _content: &str,
        _attachments: &[String],
        _reasoning: Option<&str>,
    ) -> Option<String> {
        None
    }

    /// 添加消息到对话并指定父消息，返回消息 ID
    fn add_chat_message_with_parent(
        &self,
        _session_id: &str,
        _role: &str,
        _content: &str,
        _attachments: &[String],
        _reasoning: Option<&str>,
        _parent_id: Option<&str>,
    ) -> Option<String> {
        None
    }

    /// 获取某个消息的所有兄弟节点 ID
    fn get_message_siblings(&self, _message_id: &str) -> Vec<String> {
        Vec::new()
    }

    /// 切换会话活跃的叶子节点
    fn switch_active_message(
        &self,
        _session_id: &str,
        _leaf_message_id: &str,
    ) -> Result<(), String> {
        Err("Not implemented".to_string())
    }

    /// 沿着某个分支一直向下，找到最新的叶子节点
    fn find_deepest_leaf(&self, _start_message_id: &str) -> Result<String, String> {
        Err("Not implemented".to_string())
    }

    /// 级联回退：删除指定消息之后的所有消息
    fn truncate_chat_messages_after(
        &self,
        _session_id: &str,
        _target_message_id: &str,
    ) -> Result<(), String> {
        Err("Not implemented".to_string())
    }

    /// AI 流式对话，返回文本令牌流
    fn chat_stream(
        &self,
        _session_id: String,
        _messages: Vec<models::chat::ChatMessage>,
        _system_prompt: String,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = std::result::Result<
                        tokio::sync::mpsc::UnboundedReceiver<models::chat::ChatResponseChunk>,
                        String,
                    >,
                > + Send,
        >,
    > {
        Box::pin(async {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let _ = tx.send(models::chat::ChatResponseChunk::Content(
                "AI 对话功能未实现".to_string(),
            ));
            Ok(rx)
        })
    }
}

static NEXT_SERVICE_DOC_ID: AtomicU32 = AtomicU32::new(1);

pub struct PdfService {
    task_queue: Arc<PdfTaskQueue>,
    doc_id: u32,
}

impl PdfService {
    pub fn new(path: PathBuf) -> Result<(Arc<Self>, Receiver<PdfResponse>)> {
        info!("PdfService: 正在请求加载文档: {:?}", path);

        let task_queue = get_global_pdf_queue();
        let (response_tx, response_rx) = sync_channel(100);

        // 由前台提前生成唯一的 doc_id 并传递给后台，确保生命周期从最开始就绑定
        let doc_id = NEXT_SERVICE_DOC_ID.fetch_add(1, Ordering::SeqCst);

        // 发送 OpenDocument 请求
        task_queue.push(PdfRequest::OpenDocument {
            doc_id,
            path,
            tx: response_tx,
        });

        let service = Arc::new(Self { task_queue, doc_id });

        Ok((service, response_rx))
    }

    pub fn get_doc_id(&self) -> u32 {
        self.doc_id
    }

    pub fn send_render(&self, page: u16, scale: f32, generation: u64) {
        let doc_id = self.get_doc_id();
        debug!(
            "PdfService: 发送渲染请求 - 页面: {}, 缩放: {}, 代数: {}",
            page, scale, generation
        );
        self.task_queue.push(PdfRequest::RenderPage {
            doc_id,
            page,
            scale,
            generation,
        });
    }

    pub fn send_thumbnail_render(&self, page: u16, max_size: f32, generation: u64) {
        let doc_id = self.get_doc_id();
        debug!(
            "PdfService: 发送缩略图请求 - 页面: {}, 最大尺寸: {}, 代数: {}",
            page, max_size, generation
        );
        self.task_queue.push(PdfRequest::RenderThumbnail {
            doc_id,
            page,
            max_size,
            generation,
        });
    }

    pub fn send_links(&self, page: u16, display_w: f32, display_h: f32, generation: u64) {
        let doc_id = self.get_doc_id();
        debug!(
            "PdfService: 发送链接请求 - 页面: {}, 代数: {}",
            page, generation
        );
        self.task_queue.push(PdfRequest::ExtractLinks {
            doc_id,
            page,
            display_w,
            display_h,
            generation,
        });
    }

    pub fn send_text(&self, page: u16, display_w: f32, display_h: f32, generation: u64) {
        let doc_id = self.get_doc_id();
        debug!(
            "PdfService: 发送文本请求 - 页面: {}, 代数: {}",
            page, generation
        );
        self.task_queue.push(PdfRequest::ExtractText {
            doc_id,
            page,
            display_w,
            display_h,
            generation,
        });
    }

    pub fn send_render_pin(
        &self,
        page: u16,
        pin_id: String,
        bbox: (f32, f32, f32, f32),
        zoom: f32,
    ) {
        let doc_id = self.get_doc_id();
        debug!(
            "PdfService: 发送 Pin 渲染请求 - 页面: {}, pin: {}",
            page, pin_id
        );
        self.task_queue.push(PdfRequest::RenderPin {
            doc_id,
            page,
            pin_id,
            bbox,
            zoom,
        });
    }

    pub fn send_delete_page(&self, page: u16) {
        let doc_id = self.get_doc_id();
        info!(
            "PdfService: 发送删除页面请求 - doc_id: {}, 页面: {}",
            doc_id, page
        );
        self.task_queue
            .push(PdfRequest::DeletePage { doc_id, page });
    }
    /// 将选中的页导出为新 PDF 并保存到 dest_path（复制源文件后删除未选中页）。
    pub fn send_extract_pages(&self, pages: Vec<u16>, dest_path: PathBuf) {
        let doc_id = self.get_doc_id();
        info!(
            "PdfService: 发送导出页面请求 - doc_id: {}, 共 {} 页 -> {:?}",
            doc_id,
            pages.len(),
            dest_path
        );
        self.task_queue.push(PdfRequest::ExtractPages {
            doc_id,
            pages,
            dest_path,
        });
    }
}

impl Drop for PdfService {
    fn drop(&mut self) {
        let doc_id = self.get_doc_id();
        info!("PdfService: 发送 CloseDocument 请求, doc_id={}", doc_id);
        self.task_queue.push(PdfRequest::CloseDocument { doc_id });
    }
}

/// 将 `src_path` 的 PDF 导出为带原生标注的新文件 `dest_path`（源文件只读）。
pub fn export_annotated_pdf(
    src_path: &std::path::Path,
    dest_path: &std::path::Path,
    annotations: &[Annotation],
) -> anyhow::Result<()> {
    pdf_worker::export_annotated_pdf(src_path, dest_path, annotations)
        .map_err(|e| anyhow::anyhow!(e))
}
