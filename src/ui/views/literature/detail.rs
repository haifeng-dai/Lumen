use crate::app_state::data::DataStore;
use crate::ui::{components::muted_input, views::main_window::MainWindow};
use components::IconName;
use gpui::prelude::*;
use gpui::{
    AnyWindowHandle, AsyncApp, Entity, FontWeight, SharedString, Task, WeakEntity, Window, div,
    rems,
};
use gpui_component::{
    ActiveTheme, Icon, ThemeMode,
    button::{Button, ButtonVariants},
    h_flex,
    input::InputState,
    label::Label,
    v_flex,
};
use i18n::{I18nKey, t, tf};
use log::{debug, info};
use models::{Literature, ReadingStatus};
use services::app::MainApp;
use std::sync::Arc;

mod ai_summary;
mod build;
mod render;
mod render_common;
mod render_sections;
mod sync;

/// 文献详情视图的预实体化状态 (Buffer)
#[derive(Clone)]
struct DetailState {
    /// 当前选中的 ID 列表（用于变更检测）
    selected_ids: Vec<String>,
    /// 当前缓冲文献的版本号（用于检测内容变更）
    content_version: i32,
    /// 渲染模式
    mode: DetailMode,
}

#[derive(Clone)]
struct TagData {
    name: String,
    color: String,
}

#[derive(Clone)]
enum DetailMode {
    /// 无选中
    None,
    /// 选中多个
    Multiple(usize),
    /// 选中单个并预处理好数据
    Single(Box<SingleDetailBuffer>),
}

/// 单个文献的渲染缓冲数据
#[derive(Clone)]
struct SingleDetailBuffer {
    literature: Arc<Literature>,
    ccf_badge: Option<BadgeData>,
    jcr_badge: Option<BadgeData>,
    cas_badge: Option<BadgeData>,
    authors_text: String,
    pub_name: String,
    pub_abbreviation: String,
    abstract_display: String,
    rating: i32,
    tags: Vec<TagData>,
    references: Vec<Arc<Literature>>,
    cited_by: Vec<Arc<Literature>>,
    reading_status: ReadingStatus,
    folder_paths: Vec<Vec<String>>,
}

#[derive(Clone)]
struct BadgeData {
    text: String,
    bg: gpui::Hsla,
    fg: gpui::Hsla,
}

/// 右侧文献详情视图
pub struct LiteratureDetailView {
    /// 应用控制器
    app: Arc<MainApp>,
    /// 数据存储实体
    pub data_store: Entity<DataStore>,
    /// 是否正在拖入文件
    is_dragging: bool,
    /// 摘要是否展开
    abstract_expanded: bool,
    /// 多笔记卡片
    notes_cache: Vec<models::LiteratureNote>,
    editing_note_index: Option<usize>,
    edit_note_title: Option<Entity<InputState>>,
    edit_note_content: Option<Entity<InputState>>,
    /// AI 总结任务句柄
    summary_task: Option<Task<()>>,
    /// 是否正在生成 AI 总结
    is_generating_summary: bool,
    /// 上一次 AI 总结的笔记 ID（用于替换）
    last_ai_summary_note_id: Option<String>,
    /// 父视图句柄 (`MainWindow`)
    parent_view: Option<WeakEntity<MainWindow>>,
    /// 预实体化缓冲状态
    state: DetailState,

    /// Copy feedback state
    copied_field: Option<String>,
    /// 展开的单个笔记 ID 集合
    expanded_notes: std::collections::HashSet<String>,
}

impl LiteratureDetailView {
    pub fn new(app: Arc<MainApp>, data_store: Entity<DataStore>) -> Self {
        debug!("文献详情: 初始化");
        Self {
            app,
            data_store,
            is_dragging: false,
            abstract_expanded: false,
            notes_cache: Vec::new(),
            editing_note_index: None,
            edit_note_title: None,
            edit_note_content: None,
            summary_task: None,
            is_generating_summary: false,
            last_ai_summary_note_id: None,
            parent_view: None,
            state: DetailState {
                selected_ids: Vec::new(),
                content_version: -1,
                mode: DetailMode::None,
            },
            copied_field: None,
            expanded_notes: std::collections::HashSet::new(),
        }
    }

    pub fn reload_notes(&mut self, cx: &mut Context<Self>) {
        if let Some(lit_id) = self.state.selected_ids.first()
            && let notes = self.app.literature_service.list_notes(&self.app.db, lit_id)
        {
            let has_generating = self.is_generating_summary;
            let mut merged_notes = notes;
            if has_generating
                && let Some(gen_node) = self
                    .notes_cache
                    .iter()
                    .find(|n| n.id == "ai_generating_note")
                    .cloned()
            {
                merged_notes.push(gen_node);
            }
            self.notes_cache = merged_notes;
        }
        cx.notify();
    }

    pub fn set_parent_view(&mut self, parent: WeakEntity<MainWindow>) {
        self.parent_view = Some(parent);
    }

    pub(super) fn toggle_abstract(&mut self, cx: &mut Context<Self>) {
        debug!("详情: 切换摘要展开={}", !self.abstract_expanded);
        self.abstract_expanded = !self.abstract_expanded;
        if let DetailMode::Single(ref mut buffer) = self.state.mode
            && let Some(ref text) = buffer.literature.abstract_text
        {
            buffer.abstract_display = if !self.abstract_expanded && text.chars().count() > 30 {
                let mut truncated = text.chars().take(30).collect::<String>();
                truncated.push_str("...");
                truncated
            } else {
                text.clone()
            };
        }
        cx.notify();
    }

    pub(super) fn copy_text(
        &mut self,
        text: String,
        field_id: String,
        window: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        info!("详情: 复制字段 '{}'", field_id);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.copied_field = Some(field_id);
        cx.notify();

        cx.spawn(move |view: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(1500))
                    .await;
                let _ = cx.update_window(window, |_, _, cx| {
                    let _ = view.update(cx, |this, cx| {
                        this.copied_field = None;
                        cx.notify();
                    });
                });
            }
        })
        .detach();
    }

    // =========================================================================
    // Rendering helpers
    // =========================================================================
}

impl Render for LiteratureDetailView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_state(cx);
        let theme = cx.theme().clone();
        let lang = self.app.current_language();

        if let Some(index) = self.editing_note_index {
            // 确保输入框状态在新建/编辑时都被正确初始化
            if self.edit_note_title.is_none() || self.edit_note_content.is_none() {
                let note = &self.notes_cache[index];
                let title = note.title.clone();
                let content = note.content.clone();

                let entity = cx.new(|cx| InputState::new(window, cx).placeholder("输入标题..."));
                entity.update(cx, |s, cx| {
                    s.set_value(&title, window, cx);
                });
                self.edit_note_title = Some(entity);

                let entity2 = cx.new(|cx| {
                    InputState::new(window, cx)
                        .multi_line(true)
                        .placeholder("输入内容 (支持 Markdown)...")
                });
                entity2.update(cx, |s, cx| {
                    s.set_value(&content, window, cx);
                });
                self.edit_note_content = Some(entity2);
            }

            let note = &self.notes_cache[index];
            let note_id = note.id.clone();
            let muted = theme.muted_foreground;

            return div()
                .size_full()
                .bg(if theme.mode == ThemeMode::Light {
                    theme.background
                } else {
                    theme.muted
                })
                .child(
                    v_flex()
                        .size_full()
                        .p_3()
                        .gap_3()
                        .child(
                            // ── 顶部栏：包含标题和操作按钮 ──
                            h_flex()
                                .w_full()
                                .justify_between()
                                .items_center()
                                .child(
                                    Label::new("编辑笔记")
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(muted),
                                )
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "d-note-cancel-{index}"
                                            )))
                                            .ghost()
                                            .icon(IconName::Close)
                                            .compact()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if this
                                                    .notes_cache
                                                    .get(index)
                                                    .map(|n| n.id.as_str())
                                                    == Some("temp_new_note")
                                                {
                                                    this.notes_cache.remove(index);
                                                }
                                                this.editing_note_index = None;
                                                this.edit_note_title = None;
                                                this.edit_note_content = None;
                                                cx.notify();
                                            })),
                                        )
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "d-note-save-{index}"
                                            )))
                                            .ghost()
                                            .icon(IconName::Check)
                                            .compact()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                let new_title = this
                                                    .edit_note_title
                                                    .as_ref()
                                                    .map(|e| e.read(cx).text().to_string());
                                                let new_content = this
                                                    .edit_note_content
                                                    .as_ref()
                                                    .map(|e| e.read(cx).text().to_string());

                                                let mut final_note_id = note_id.clone();
                                                let is_temp = note_id == "temp_new_note";

                                                if is_temp {
                                                    let default_title =
                                                        new_title.clone().unwrap_or_else(|| {
                                                            "未命名笔记".to_string()
                                                        });
                                                    let temp_lit_id = this.notes_cache[index]
                                                        .literature_id
                                                        .clone();
                                                    if let Some(real_id) =
                                                        this.app.literature_service.create_note(
                                                            &this.app.db,
                                                            &temp_lit_id,
                                                            &default_title,
                                                        )
                                                    {
                                                        final_note_id = real_id;
                                                    }
                                                }

                                                let _ = this.app.literature_service.update_note(
                                                    &this.app.db,
                                                    &final_note_id,
                                                    new_title.as_deref(),
                                                    new_content.as_deref(),
                                                );
                                                if let Some(n) = this.notes_cache.get_mut(index) {
                                                    n.id = final_note_id;
                                                    if let Some(ref t) = new_title {
                                                        n.title = t.clone();
                                                    }
                                                    if let Some(ref c) = new_content {
                                                        n.content = c.clone();
                                                    }
                                                }
                                                this.editing_note_index = None;
                                                this.edit_note_title = None;
                                                this.edit_note_content = None;
                                                this.app.notify_data_changed();
                                                cx.notify();
                                            })),
                                        ),
                                ),
                        )
                        .when_some(self.edit_note_title.as_ref(), |this, e| {
                            this.child(muted_input(e, &theme).w_full())
                        })
                        .child(
                            // ── 内容输入框，通过 div 容器包裹撑满整个侧边栏 ──
                            div()
                                .w_full()
                                .flex_grow(1.0)
                                .h_0()
                                .when_some(self.edit_note_content.as_ref(), |this, e| {
                                    this.child(muted_input(e, &theme).w_full().h_full())
                                }),
                        ),
                )
                .into_any_element();
        }

        match &self.state.mode {
            DetailMode::None => div()
                .id("literature-detail-empty")
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.muted_foreground)
                .bg(if theme.mode == ThemeMode::Light {
                    theme.background
                } else {
                    theme.muted
                })
                .child(t(I18nKey::NoLiteratureSelected, lang))
                .into_any_element(),
            DetailMode::Multiple(count) => div()
                .id("literature-detail-multiple")
                .size_full()
                .bg(if theme.mode == ThemeMode::Light {
                    theme.background
                } else {
                    theme.muted
                })
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_4()
                .child(
                    Icon::new(IconName::BookOpen)
                        .size(rems(3.0))
                        .text_color(theme.muted_foreground),
                )
                .child(div().text_lg().text_color(theme.foreground).child(tf(
                    I18nKey::SelectedCount,
                    lang,
                    &[&count.to_string()],
                )))
                .into_any_element(),
            DetailMode::Single(buffer) => {
                self.render_single_detail(buffer, &theme, lang, window, cx)
            }
        }
    }
}
