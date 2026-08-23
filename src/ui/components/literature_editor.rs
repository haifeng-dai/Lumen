use components::IconName;
use components::{add_drag_behavior, labeled_input, muted_textarea_raw, selector};
use gpui::prelude::*;
use gpui::{AppContext, Entity, FontWeight, SharedString, Window, div, rems};
#[cfg(not(windows))]
use gpui_component::InteractiveElementExt;
use gpui_component::{
    ActiveTheme, Icon,
    button::{Button, ButtonVariants},
    h_flex,
    input::{InputState, Textarea, TextareaState},
    label::Label,
    scroll::ScrollableElement,
    v_flex,
};
use i18n::LiteratureTypeExt;
use i18n::{I18nKey, t};
use log::{debug, info};
use models::constructors::*;
use models::{Literature, LiteratureType, PublicationType};
use parser::normalize::*;
use services::app::MainApp;
use std::sync::Arc;

pub type LiteratureEditorCallback =
    Box<dyn Fn(Option<Literature>, &mut Window, &mut Context<LiteratureEditor>) + Send + Sync>;

/// 文献编辑器组件 (用于手动添加、编辑及导入核对)
pub struct LiteratureEditor {
    app: Arc<MainApp>,
    literature: Literature,
    selected_type: LiteratureType,
    title_input: Entity<InputState>,
    authors_input: Entity<InputState>,
    journal_input: Entity<InputState>,
    abbreviation_input: Entity<InputState>,
    year_input: Entity<InputState>,
    month_input: Entity<InputState>,
    day_input: Entity<InputState>,
    volume_input: Entity<InputState>,
    issue_input: Entity<InputState>,
    pages_input: Entity<InputState>,
    doi_input: Entity<InputState>,
    arxiv_id_input: Entity<InputState>,
    url_input: Entity<InputState>,
    publisher_input: Entity<InputState>,
    abstract_input: Entity<TextareaState>,
    notes_input: Entity<TextareaState>,
    // 回调函数：当完成时调用 (Some(literature) 表示确认修改，None 表示取消)
    on_complete: LiteratureEditorCallback,
}

impl LiteratureEditor {
    pub fn new(
        app: Arc<MainApp>,
        literature: Literature,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_complete: impl Fn(Option<Literature>, &mut Window, &mut Context<Self>)
        + Send
        + Sync
        + 'static,
    ) -> Self {
        debug!(
            "EDITOR_NEW: 构造 LiteratureEditor (title='{}')",
            literature.title
        );
        let lang = app.current_language();

        // 创建所有输入框并填充初始数据
        let title_input =
            Self::create_input(&literature.title, t(I18nKey::Title, lang), window, cx);

        let authors_str = literature
            .authors
            .iter()
            .map(author_full_name)
            .collect::<Vec<_>>()
            .join(", ");
        let authors_input = Self::create_input(
            &authors_str,
            t(I18nKey::AuthorPlaceholder, lang),
            window,
            cx,
        );

        let journal_input = Self::create_input(
            &literature
                .publication
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            t(I18nKey::JournalPlaceholder, lang),
            window,
            cx,
        );
        let abbreviation_input = Self::create_input(
            literature
                .publication
                .as_ref()
                .and_then(|p| p.abbreviation.as_deref())
                .unwrap_or_default(),
            t(I18nKey::PublicationAbbreviation, lang),
            window,
            cx,
        );
        let year_input = Self::create_input(
            &literature.year.map(|y| y.to_string()).unwrap_or_default(),
            t(I18nKey::Year, lang),
            window,
            cx,
        );
        let month_input = Self::create_input(
            &literature.month.map(|m| m.to_string()).unwrap_or_default(),
            t(I18nKey::Month, lang),
            window,
            cx,
        );
        let day_input = Self::create_input(
            &literature.day.map(|d| d.to_string()).unwrap_or_default(),
            t(I18nKey::Day, lang),
            window,
            cx,
        );
        let volume_input = Self::create_input(
            literature.volume.as_deref().unwrap_or(""),
            t(I18nKey::Volume, lang),
            window,
            cx,
        );
        let issue_input = Self::create_input(
            literature.issue.as_deref().unwrap_or(""),
            t(I18nKey::Issue, lang),
            window,
            cx,
        );
        let pages_input = Self::create_input(
            literature.pages.as_deref().unwrap_or(""),
            t(I18nKey::Pages, lang),
            window,
            cx,
        );
        let doi_input =
            Self::create_input(literature.doi.as_deref().unwrap_or(""), "DOI", window, cx);
        let arxiv_id_input = Self::create_input(
            literature.arxiv_id.as_deref().unwrap_or(""),
            "ArXiv ID",
            window,
            cx,
        );
        let url_input =
            Self::create_input(literature.url.as_deref().unwrap_or(""), "URL", window, cx);
        let publisher_input = Self::create_input(
            literature
                .publication
                .as_ref()
                .and_then(|p| p.publisher.as_deref())
                .unwrap_or(""),
            t(I18nKey::Publisher, lang),
            window,
            cx,
        );
        let abstract_input = Self::create_textarea(
            literature.abstract_text.as_deref().unwrap_or(""),
            t(I18nKey::Abstract, lang),
            window,
            cx,
        );
        let notes_initial = app
            .db
            .list_notes(&literature.id)
            .ok()
            .and_then(|notes| notes.into_iter().next().map(|n| n.content))
            .unwrap_or_default();
        let notes_input =
            Self::create_textarea(&notes_initial, t(I18nKey::Notes, lang), window, cx);

        debug!("EDITOR_NEW: 提交 Self (title='{}')", literature.title);
        Self {
            app,
            literature: literature.clone(),
            selected_type: literature.literature_type.clone(),
            title_input,
            authors_input,
            journal_input,
            abbreviation_input,
            year_input,
            month_input,
            day_input,
            volume_input,
            issue_input,
            pages_input,
            doi_input,
            arxiv_id_input,
            url_input,
            publisher_input,
            abstract_input,
            notes_input,
            on_complete: Box::new(on_complete),
        }
    }

    /// 辅助方法：创建并初始化输入框
    fn create_input(
        initial_value: &str,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let initial_value: SharedString = initial_value.to_string().into();
        let placeholder: SharedString = placeholder.to_string().into();
        cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(initial_value)
        })
    }

    /// 辅助方法：创建并初始化多行文本域
    fn create_textarea(
        initial_value: &str,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextareaState> {
        let initial_value: SharedString = initial_value.to_string().into();
        let placeholder: SharedString = placeholder.to_string().into();
        cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(placeholder)
                .default_value(initial_value)
                .rows(5)
        })
    }

    fn handle_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        info!("编辑器: 用户点击保存，正在提取表单数据...");
        let mut lit = self.literature.clone();

        // 更新文献类型
        lit.literature_type = self.selected_type.clone();

        lit.title = self.title_input.read(cx).text().to_string();
        // 更新 publication 字段
        let journal_text = self.journal_input.read(cx).text().to_string();
        let abbreviation_text = self.abbreviation_input.read(cx).text().to_string();
        if journal_text.is_empty() {
            lit.publication = None;
        } else {
            // 保留现有类型，如果没有则默认为 Journal
            let pub_type = lit
                .publication
                .as_ref()
                .map_or(PublicationType::Journal, |p| p.publication_type.clone());
            let old_abbreviation = lit
                .publication
                .as_ref()
                .and_then(|p| p.abbreviation.clone())
                .filter(|a| !a.trim().is_empty());
            let mut new_pub = create_publication(journal_text, pub_type);
            // 用户手动填写的缩写优先；留空则保留原有值；均无则由保存时的自动填充兜底
            if !abbreviation_text.trim().is_empty() {
                new_pub.abbreviation = Some(abbreviation_text);
            } else {
                new_pub.abbreviation = old_abbreviation;
            }
            lit.publication = Some(new_pub);
        }
        lit.volume = Some(self.volume_input.read(cx).text().to_string());
        lit.issue = Some(self.issue_input.read(cx).text().to_string());
        lit.pages = Some(self.pages_input.read(cx).text().to_string());
        lit.doi = Some(self.doi_input.read(cx).text().to_string());
        lit.arxiv_id = Some(self.arxiv_id_input.read(cx).text().to_string());
        lit.url = Some(self.url_input.read(cx).text().to_string());

        let publisher_text = self.publisher_input.read(cx).text().to_string();
        if !publisher_text.is_empty() {
            if let Some(ref mut pub_data) = lit.publication {
                pub_data.publisher = Some(publisher_text);
            } else {
                let pub_type = if lit.literature_type == models::LiteratureType::Conference {
                    PublicationType::Conference
                } else {
                    PublicationType::Journal
                };
                let mut new_pub = create_publication(String::new(), pub_type);
                new_pub.publisher = Some(publisher_text);
                lit.publication = Some(new_pub);
            }
        } else if let Some(ref mut pub_data) = lit.publication {
            pub_data.publisher = None;
        }

        // 自动规范化 ArXiv 标识符
        sanitize_arxiv_identifiers(&mut lit);

        lit.abstract_text = Some(self.abstract_input.read(cx).text().to_string());

        let year_text = self.year_input.read(cx).text().to_string();
        if let Ok(year) = year_text.parse::<i32>() {
            lit.year = Some(year);
        }

        let month_text = self.month_input.read(cx).text().to_string();
        if let Ok(month) = month_text.parse::<i32>() {
            lit.month = Some(month);
        }

        let day_text = self.day_input.read(cx).text().to_string();
        if let Ok(day) = day_text.parse::<i32>() {
            lit.day = Some(day);
        }

        // 解析作者
        let authors_text = self.authors_input.read(cx).text().to_string();
        let authors = parse_author_list(&authors_text);

        if !authors.is_empty() {
            lit.authors = authors;
        }

        let notes_content = self.notes_input.read(cx).text().to_string();
        if !notes_content.is_empty() {
            let existing = self
                .app
                .literature_service
                .list_notes(&self.app.db, &lit.id);
            if let Some(first) = existing.into_iter().next() {
                let _ = self.app.literature_service.update_note(
                    &self.app.db,
                    &first.id,
                    None,
                    Some(&notes_content),
                );
            } else {
                if let Some(new_id) =
                    self.app
                        .literature_service
                        .create_note(&self.app.db, &lit.id, "笔记")
                {
                    let _ = self.app.literature_service.update_note(
                        &self.app.db,
                        &new_id,
                        None,
                        Some(&notes_content),
                    );
                }
            }
        }

        info!(
            "编辑器: 数据提取完成，标题: '{}', 作者数: {}",
            lit.title,
            lit.authors.len()
        );
        (self.on_complete)(Some(lit), window, cx);
    }

    fn handle_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        (self.on_complete)(None, window, cx);
    }
}

impl Render for LiteratureEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        debug!(
            "EDITOR_RENDER: 渲染 LiteratureEditor (title='{}')",
            self.literature.title
        );
        let lang = self.app.current_language();

        div()
            .size_full()
            .bg(cx.theme().background)
            .flex()
            .flex_col()
            .relative()
            .overflow_hidden()
            // 拖拽层：绝对定位覆盖在顶部，不占布局空间
            .child({
                let drag = div()
                    .id("editor-drag-area")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(rems(2.2));

                #[cfg(not(windows))]
                let drag = drag.on_double_click(|_, window, _| window.remove_window());

                add_drag_behavior(drag, _window, cx)
            })
            // 标题和按钮行
            .child(
                h_flex()
                    .w_full()
                    .px_6()
                    .pt_4()
                    .justify_between()
                    .items_center()
                    .mb_4()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .child(t(I18nKey::LiteratureEditor, lang)),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("cancel-edit")
                                    .child(Icon::new(IconName::Close).size(rems(0.75)))
                                    .ghost()
                                    .occlude()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.handle_cancel(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("save-edit")
                                    .child(Icon::new(IconName::Check).size(rems(0.75)))
                                    .primary()
                                    .occlude()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.handle_save(window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_grow(1.0)
                    .px_6()
                    .pb_6()
                    .min_h(rems(0.0)) // 关键：允许 flex 子项缩小到 0，从而触发内容溢出滚动
                    .overflow_y_scrollbar() // 启用纵向滚动
                    .pr_4() // 增加右侧间距，防止滚动条遮挡内容
                    .child(
                        v_flex()
                            .gap_4()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t(I18nKey::Type, lang)),
                                    )
                                    .child({
                                        let lang = self.app.current_language();
                                        let type_options: Vec<(SharedString, SharedString)> =
                                            <LiteratureType as LiteratureTypeExt>::all()
                                                .into_iter()
                                                .map(|lt| {
                                                    let value = lt.as_str().into();
                                                    let label = t(lt.i18n_key(), lang).into();
                                                    (value, label)
                                                })
                                                .collect();
                                        let current: SharedString =
                                            self.selected_type.as_str().into();
                                        let weak = cx.entity().downgrade();
                                        selector(
                                            "literature-type-selector",
                                            type_options,
                                            current,
                                            false,
                                            move |v, _, cx| {
                                                if let Some(this) = weak.upgrade() {
                                                    this.update(cx, |this, _| {
                                                        if let Some(lt) =
                                                            LiteratureType::from_str(&v)
                                                        {
                                                            this.selected_type = lt;
                                                        }
                                                    });
                                                }
                                            },
                                        )
                                    }),
                            )
                            // ... (其余部分代码保持逻辑一致)
                            .child(labeled_input(
                                t(I18nKey::Title, lang),
                                &self.title_input,
                                cx,
                            ))
                            .child(labeled_input(
                                t(I18nKey::Authors, lang),
                                &self.authors_input,
                                cx,
                            ))
                            .child(labeled_input(
                                t(I18nKey::Journal, lang),
                                &self.journal_input,
                                cx,
                            ))
                            .child(labeled_input(
                                t(I18nKey::PublicationAbbreviation, lang),
                                &self.abbreviation_input,
                                cx,
                            ))
                            .child(
                                v_flex()
                                    .gap_3()
                                    // 第一行：日期组（年/月/日）
                                    .child(
                                        h_flex()
                                            .gap_4()
                                            .child(div().flex_1().min_w_0().child(labeled_input(
                                                t(I18nKey::Year, lang),
                                                &self.year_input,
                                                cx,
                                            )))
                                            .child(div().flex_1().min_w_0().child(labeled_input(
                                                t(I18nKey::Month, lang),
                                                &self.month_input,
                                                cx,
                                            )))
                                            .child(div().flex_1().min_w_0().child(labeled_input(
                                                t(I18nKey::Day, lang),
                                                &self.day_input,
                                                cx,
                                            ))),
                                    )
                                    // 第二行：出版定位组（卷/期/页码）
                                    .child(
                                        h_flex()
                                            .gap_4()
                                            .child(div().flex_1().min_w_0().child(labeled_input(
                                                t(I18nKey::Volume, lang),
                                                &self.volume_input,
                                                cx,
                                            )))
                                            .child(div().flex_1().min_w_0().child(labeled_input(
                                                t(I18nKey::Issue, lang),
                                                &self.issue_input,
                                                cx,
                                            )))
                                            .child(div().flex_1().min_w_0().child(labeled_input(
                                                t(I18nKey::Pages, lang),
                                                &self.pages_input,
                                                cx,
                                            ))),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_4()
                                    .child(div().flex_grow(1.0).child(labeled_input(
                                        "DOI",
                                        &self.doi_input,
                                        cx,
                                    )))
                                    .child(div().flex_grow(1.0).child(labeled_input(
                                        "ArXiv ID",
                                        &self.arxiv_id_input,
                                        cx,
                                    ))),
                            )
                            .child(h_flex().gap_4().child(
                                div().flex_grow(1.0).child(labeled_input(
                                    "URL",
                                    &self.url_input,
                                    cx,
                                )),
                            ))
                            .child(labeled_input(
                                t(I18nKey::Publisher, lang),
                                &self.publisher_input,
                                cx,
                            ))
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(Label::new(t(I18nKey::Abstract, lang)).text_sm())
                                    .child(muted_textarea_raw(
                                        Textarea::new(&self.abstract_input).h(rems(7.5)),
                                        cx.theme(),
                                    )),
                            ),
                    ),
            )
    }
}
