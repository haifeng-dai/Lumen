use crate::app_state::theme::surface;
use components::IconName;
use gpui::prelude::*;
use gpui::{
    ElementId, FontWeight, SharedString, Window, div, px, relative, rems, transparent_black,
};
use gpui_component::{
    ActiveTheme, Icon, Sizable,
    button::{Button, ButtonVariants},
    scroll::{Scrollable, ScrollableElement},
    v_flex,
};
use i18n::LiteratureTypeExt;
use i18n::{I18nKey, t};
use log::{error, info, warn};
use models::constructors::*;
use models::{Literature, PublicationType};
use parser::normalize::*;
use services::app::MainApp;
use std::sync::Arc;

/// 字段选中状态
#[derive(Clone, Default)]
pub struct FieldSelection {
    pub literature_type: bool,
    pub title: bool,
    pub authors: bool,
    pub year: bool,
    pub month: bool,
    pub day: bool,
    pub journal: bool,
    pub conference: bool,
    pub volume: bool,
    pub issue: bool,
    pub pages: bool,
    pub publisher: bool,
    pub abstract_text: bool,
    pub doi: bool,
    pub arxiv_id: bool,
    pub url: bool,
}

impl FieldSelection {
    /// 检查是否有任何字段不同
    #[must_use]
    pub fn has_any_diff(&self) -> bool {
        self.literature_type
            || self.title
            || self.authors
            || self.year
            || self.month
            || self.day
            || self.journal
            || self.conference
            || self.volume
            || self.issue
            || self.pages
            || self.publisher
            || self.abstract_text
            || self.doi
            || self.arxiv_id
            || self.url
    }

    /// 对比两条文献，生成差异统计
    #[must_use]
    pub fn compare(original: &Literature, new_lit: &Literature) -> Self {
        let original_authors = original
            .authors
            .iter()
            .map(author_full_name)
            .collect::<Vec<_>>()
            .join(", ");
        let new_authors = new_lit
            .authors
            .iter()
            .map(author_full_name)
            .collect::<Vec<_>>()
            .join(", ");

        Self {
            literature_type: original.literature_type != new_lit.literature_type,
            title: original.title != new_lit.title,
            authors: !new_lit.authors.is_empty()
                && (original.authors.is_empty() || original_authors != new_authors),
            year: new_lit.year.is_some() && original.year != new_lit.year,
            month: new_lit.month.is_some() && original.month != new_lit.month,
            day: new_lit.day.is_some() && original.day != new_lit.day,
            // 统一使用 publication 字段进行比较，journal/conference 现在作为 UI 选择标记
            journal: new_lit.publication.is_some()
                && original
                    .publication
                    .as_ref()
                    .map(|p| p.name.clone())
                    .unwrap_or_default()
                    != new_lit
                        .publication
                        .as_ref()
                        .map(|p| p.name.clone())
                        .unwrap_or_default(),
            conference: false, // 不再单独区分 conference
            volume: new_lit.volume.is_some() && original.volume != new_lit.volume,
            issue: new_lit.issue.is_some() && original.issue != new_lit.issue,
            pages: new_lit.pages.is_some() && original.pages != new_lit.pages,
            publisher: {
                let orig_pub = original
                    .publication
                    .as_ref()
                    .and_then(|p| p.publisher.clone());
                let new_pub = new_lit
                    .publication
                    .as_ref()
                    .and_then(|p| p.publisher.clone());
                new_pub.is_some() && orig_pub != new_pub
            },
            abstract_text: new_lit.abstract_text.is_some()
                && (original.abstract_text.is_none()
                    || original.abstract_text.as_ref().unwrap().len()
                        < new_lit.abstract_text.as_ref().unwrap().len()),
            doi: new_lit.doi.is_some() && original.doi != new_lit.doi,
            arxiv_id: new_lit.arxiv_id.is_some() && {
                let norm_orig = original
                    .arxiv_id
                    .as_ref()
                    .map(|s| s.to_lowercase().replace("arxiv:", ""));
                let norm_new = new_lit
                    .arxiv_id
                    .as_ref()
                    .map(|s| s.to_lowercase().replace("arxiv:", ""));
                norm_orig != norm_new
            },
            url: new_lit.url.is_some() && original.url != new_lit.url,
        }
    }
}

pub type CompareDialogCallback =
    Box<dyn Fn(Option<Literature>, &mut Window, &mut Context<CompareDialog>) + Send + Sync>;

/// 文献对比与合并窗口视图
pub struct CompareDialog {
    app: Arc<MainApp>,
    original: Literature,
    new_data: Option<Literature>,
    error: Option<String>,
    selection: FieldSelection,
    diff_fields: FieldSelection, // 记录哪些字段是有差异的，用于控制显示隐藏
    on_complete: CompareDialogCallback,
}

impl CompareDialog {
    /// 静态构造方法：使用已经抓取好的数据打开窗口
    pub fn new_with_data(
        app: Arc<MainApp>,
        original: Arc<Literature>,
        new_lit: Literature,
        selection: FieldSelection,
        on_complete: impl Fn(Option<Literature>, &mut Window, &mut Context<Self>)
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            app,
            original: (*original).clone(),
            new_data: Some(new_lit),
            error: None,
            diff_fields: selection.clone(), // 初始差异即为初始显示范围
            selection,
            on_complete: Box::new(on_complete),
        }
    }

    fn handle_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        info!("文献对比窗口: 用户点击保存/合并按钮");
        if let Some(new_lit) = &self.new_data {
            let mut merged = self.original.clone();
            // ... (合并逻辑保持不变)
            if self.selection.literature_type {
                merged.literature_type = new_lit.literature_type.clone();
            }
            if self.selection.title {
                merged.title = new_lit.title.clone();
            }
            if self.selection.authors {
                merged.authors = new_lit.authors.clone();
            }
            if self.selection.year {
                merged.year = new_lit.year;
            }
            if self.selection.month {
                merged.month = new_lit.month;
            }
            if self.selection.day {
                merged.day = new_lit.day;
            }
            // 使用 publication 字段代替 journal/conference
            if self.selection.journal || self.selection.conference {
                merged.publication = new_lit.publication.clone();
            }
            if self.selection.volume {
                merged.volume = new_lit.volume.clone();
            }
            if self.selection.issue {
                merged.issue = new_lit.issue.clone();
            }
            if self.selection.pages {
                merged.pages = new_lit.pages.clone();
            }
            if self.selection.publisher
                && let Some(publisher_str) = new_lit
                    .publication
                    .as_ref()
                    .and_then(|p| p.publisher.clone())
            {
                if let Some(ref mut pub_data) = merged.publication {
                    // Publication exists, just update publisher
                    pub_data.publisher = Some(publisher_str);
                } else {
                    // Create new Publication with empty name and the publisher
                    let pub_type = if merged.literature_type == models::LiteratureType::Conference {
                        PublicationType::Conference
                    } else {
                        PublicationType::Journal
                    };
                    let mut new_pub = create_publication(String::new(), pub_type);
                    new_pub.publisher = Some(publisher_str);
                    merged.publication = Some(new_pub);
                }
            }
            if self.selection.abstract_text {
                merged.abstract_text = new_lit.abstract_text.clone();
            }
            if self.selection.doi {
                merged.doi = new_lit.doi.clone();
            }
            if self.selection.arxiv_id {
                merged.arxiv_id = new_lit.arxiv_id.clone();
            }
            if self.selection.url {
                merged.url = new_lit.url.clone();
            }

            // 执行保存
            info!("文献对比窗口: 正在更新原始文献记录 {}", merged.id);
            match self.app.update_literature(merged.clone()) {
                Ok(()) => info!("文献对比窗口: 原始文献记录更新成功"),
                Err(e) => error!("文献对比窗口: 原始文献记录更新失败: {e}"),
            }

            info!("文献对比窗口: 调用完成回调");
            (self.on_complete)(Some(merged), window, cx);
        } else {
            warn!("文献对比窗口: new_data 为空，无法合并");
        }
    }

    fn render_compare_row(
        &self,
        props: CompareRowProps<Self>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let surface = surface(cx);
        let theme = cx.theme().clone();
        let has_diff = props.original_val != props.new_val && !props.new_val.is_empty();
        let is_odd = !props.index.is_multiple_of(2);

        div()
            .flex()
            .flex_row()
            .w_full()
            .bg(if is_odd {
                surface.info_bg
            } else {
                theme.background
            })
            .border_b_1()
            .border_color(theme.border)
            .child(
                // 1. 字段名列
                div()
                    .w(rems(6.25))
                    .px_3()
                    .py_3()
                    .flex_shrink_0()
                    .border_r_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.muted_foreground)
                            .child(props.label.clone()),
                    ),
            )
            .child(
                // 2. 现有数据列
                div()
                    .flex_grow(1.0)
                    .w_0()
                    .px_3()
                    .py_3()
                    .border_r_1()
                    .border_color(theme.border)
                    .child(div().text_sm().line_height(relative(1.4)).child(
                        if props.original_val.is_empty() {
                            "-".to_string()
                        } else {
                            props.original_val
                        },
                    )),
            )
            .child(
                // 3. 新抓取数据列
                div()
                    .flex_grow(1.0)
                    .w_0()
                    .px_3()
                    .py_3()
                    .bg(if has_diff && props.is_selected {
                        surface.selected_faint
                    } else {
                        transparent_black()
                    })
                    .child(
                        // 此处内部对齐仍可使用 h_flex()，但需注意布局
                        div()
                            .flex()
                            .flex_row()
                            .gap_3()
                            .items_start()
                            .child(if has_diff {
                                // 手写自定义 checkbox，与 merge_dialog 样式一致
                                let label_id = SharedString::from(format!("check-{}", props.label));
                                let is_selected = props.is_selected;
                                div()
                                    .id(ElementId::from(label_id))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(rems(1.0))
                                    .h(rems(1.0))
                                    .flex_shrink_0()
                                    .mt(rems(0.125))
                                    .rounded_sm()
                                    .when(is_selected, |s| {
                                        s.bg(theme.primary).text_color(theme.background)
                                    })
                                    .when(!is_selected, |s| {
                                        s.border_1()
                                            .border_color(theme.border)
                                            .bg(transparent_black())
                                    })
                                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let new_val = !is_selected;
                                        (props.on_toggle)(this, &new_val, window, cx);
                                    }))
                                    .child(Icon::new(IconName::Check).size(rems(0.75)).text_color(
                                        if is_selected {
                                            theme.background
                                        } else {
                                            transparent_black()
                                        },
                                    ))
                                    .into_any_element()
                            } else {
                                div().w(rems(1.0)).into_any_element()
                            })
                            .child(
                                div()
                                    .flex_grow(1.0)
                                    .w_0()
                                    .text_sm()
                                    .line_height(relative(1.4))
                                    .text_color(if has_diff && props.is_selected {
                                        theme.primary
                                    } else if has_diff {
                                        theme.foreground
                                    } else {
                                        theme.muted_foreground
                                    })
                                    .child(if props.new_val.is_empty() {
                                        "-".to_string()
                                    } else {
                                        props.new_val
                                    }),
                            ),
                    ),
            )
    }
}

type ToggleHandler<S> = Box<dyn Fn(&mut S, &bool, &mut Window, &mut Context<S>) + 'static>;

struct CompareRowProps<S: 'static> {
    label: SharedString,
    original_val: String,
    new_val: String,
    is_selected: bool,
    index: usize,
    on_toggle: ToggleHandler<S>,
}

impl Render for CompareDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let lang = self.app.current_language();

        // 主条目已提升为标题行（不可取消），始终纳入合并。
        self.selection.title = true;

        let format_date = |y: Option<i32>, m: Option<i32>, d: Option<i32>| match (y, m, d) {
            (Some(year), Some(month), Some(day)) => format!("{}-{:02}-{:02}", year, month, day),
            (Some(year), Some(month), None) => format!("{}-{:02}", year, month),
            (Some(year), None, _) => year.to_string(),
            _ => String::new(),
        };

        div()
            .size_full()
            .bg(theme.background)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(components::add_drag_behavior(
                div()
                    .id("compare-drag-overlay")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(40.0)),
                window,
                cx,
            ))
            // 1. 顶部工具栏 (占用顶部空间)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .w_full()
                    .h(rems(3.25)) // 固定紧凑高度
                    .px_8()
                    .justify_end()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("close-compare")
                                    .ghost()
                                    .child(Icon::new(IconName::Close).size(rems(0.75)))
                                    .occlude()
                                    .on_click(cx.listener(
                                        |_, _, window: &mut Window, _: &mut Context<Self>| {
                                            window.remove_window();
                                        },
                                    )),
                            )
                            .when(self.new_data.is_some(), |this: gpui::Div| {
                                this.child(
                                    Button::new("save-merge")
                                        .child(Icon::new(IconName::Check).size(rems(0.75)))
                                        .primary()
                                        .occlude()
                                        .on_click(cx.listener(
                                            |this: &mut Self,
                                             _,
                                             window: &mut Window,
                                             cx: &mut Context<Self>| {
                                                this.handle_confirm(window, cx);
                                            },
                                        )),
                                )
                            }),
                    ),
            )
            // 2. 主内容区域
            .child(
                div().flex_1().px_8().pb_8().overflow_hidden().child(
                    match (&self.new_data, &self.error) {
                        (None, None) => div()
                            .h(rems(18.75))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(t(I18nKey::LoadingMetadata, lang)),
                            )
                            .into_any_element(),
                        (None, Some(err)) => v_flex()
                            .h(rems(18.75))
                            .items_center()
                            .justify_center()
                            .gap_4()
                            .child(div().text_sm().text_color(theme.danger).child(format!(
                                "{}: {}",
                                t(I18nKey::FetchFailed, lang),
                                err
                            )))
                            .child(
                                Button::new("retry-fetch")
                                    .child(t(I18nKey::Close, lang))
                                    .large()
                                    .on_click(cx.listener(|_, _, window, _| {
                                        window.remove_window();
                                    })),
                            )
                            .into_any_element(),
                        (Some(new_lit), _) => {
                            let original = &self.original;
                            v_flex()
                                .size_full()
                                .rounded_lg()
                                .border_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                // 卡片标题：主条目（用于对比的文献标题，单行展示）
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .w_full()
                                        .items_center()
                                        .gap_2()
                                        .px_3()
                                        .py_2()
                                        .border_b_1()
                                        .border_color(theme.border)
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(theme.muted_foreground)
                                                .child(SharedString::from("主条目")),
                                        )
                                        .child(
                                            div()
                                                .flex_grow(1.0)
                                                .w_0()
                                                .text_base()
                                                .font_weight(FontWeight::BOLD)
                                                .line_height(relative(1.4))
                                                .child(if original.title.is_empty() {
                                                    "-".to_string()
                                                } else {
                                                    original.title.clone()
                                                }),
                                        ),
                                )
                                // 表头行：属性 | 库内数据 | 新获取数据
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .w_full()
                                        .bg(theme.muted)
                                        .border_b_1()
                                        .border_color(theme.border)
                                        .child(
                                            div()
                                                .w(rems(6.25))
                                                .px_3()
                                                .py_2()
                                                .border_r_1()
                                                .border_color(theme.border)
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_color(theme.muted_foreground)
                                                        .child(t(I18nKey::Field, lang)),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex_grow(1.0)
                                                .w_0()
                                                .px_3()
                                                .py_2()
                                                .border_r_1()
                                                .border_color(theme.border)
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_color(theme.muted_foreground)
                                                        .child(t(I18nKey::LocalData, lang)),
                                                ),
                                        )
                                        .child(
                                            div().flex_grow(1.0).w_0().px_3().py_2().child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(theme.muted_foreground)
                                                    .child(t(I18nKey::RemoteData, lang)),
                                            ),
                                        ),
                                )
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .overflow_y_scrollbar()
                                        .when(
                                            self.diff_fields.literature_type,
                                            |this: Scrollable<gpui::Div>| {
                                                this.child(
                                                    self.render_compare_row(
                                                        CompareRowProps {
                                                            label: t(I18nKey::Type, lang).into(),
                                                            original_val: t(
                                                                original.literature_type.i18n_key(),
                                                                lang,
                                                            )
                                                            .to_string(),
                                                            new_val: t(
                                                                new_lit.literature_type.i18n_key(),
                                                                lang,
                                                            )
                                                            .to_string(),
                                                            is_selected: self
                                                                .selection
                                                                .literature_type,
                                                            index: 0,
                                                            on_toggle: Box::new(
                                                                |this, checked, _, _| {
                                                                    this.selection
                                                                        .literature_type = *checked;
                                                                },
                                                            ),
                                                        },
                                                        cx,
                                                    ),
                                                )
                                            },
                                        )
                                        .when(
                                            self.diff_fields.authors,
                                            |this: Scrollable<gpui::Div>| {
                                                this.child(
                                                    self.render_compare_row(
                                                        CompareRowProps {
                                                            label: t(I18nKey::Authors, lang).into(),
                                                            original_val: original
                                                                .authors
                                                                .iter()
                                                                .map(author_full_name)
                                                                .collect::<Vec<_>>()
                                                                .join(", "),
                                                            new_val: new_lit
                                                                .authors
                                                                .iter()
                                                                .map(author_full_name)
                                                                .collect::<Vec<_>>()
                                                                .join(", "),
                                                            is_selected: self.selection.authors,
                                                            index: 2,
                                                            on_toggle: Box::new(
                                                                |this, checked, _, _| {
                                                                    this.selection.authors =
                                                                        *checked;
                                                                },
                                                            ),
                                                        },
                                                        cx,
                                                    ),
                                                )
                                            },
                                        )
                                        .when(
                                            self.diff_fields.year
                                                || self.diff_fields.month
                                                || self.diff_fields.day,
                                            |this: Scrollable<gpui::Div>| {
                                                this.child(self.render_compare_row(
                                                    CompareRowProps {
                                                        label: t(I18nKey::Year, lang).into(),
                                                        original_val: format_date(
                                                            original.year,
                                                            original.month,
                                                            original.day,
                                                        ),
                                                        new_val: format_date(
                                                            new_lit.year,
                                                            new_lit.month,
                                                            new_lit.day,
                                                        ),
                                                        is_selected: self.selection.year
                                                            || self.selection.month
                                                            || self.selection.day,
                                                        index: 3,
                                                        on_toggle: Box::new(
                                                            |this, checked, _, _| {
                                                                this.selection.year = *checked;
                                                                this.selection.month = *checked;
                                                                this.selection.day = *checked;
                                                            },
                                                        ),
                                                    },
                                                    cx,
                                                ))
                                            },
                                        )
                                        .when(
                                            self.diff_fields.journal,
                                            |this: Scrollable<gpui::Div>| {
                                                this.child(
                                                    self.render_compare_row(
                                                        CompareRowProps {
                                                            label: t(I18nKey::Journal, lang).into(),
                                                            original_val: original
                                                                .publication
                                                                .as_ref()
                                                                .map(|p| p.name.clone())
                                                                .unwrap_or_default(),
                                                            new_val: new_lit
                                                                .publication
                                                                .as_ref()
                                                                .map(|p| p.name.clone())
                                                                .unwrap_or_default(),
                                                            is_selected: self.selection.journal,
                                                            index: 4,
                                                            on_toggle: Box::new(
                                                                |this, checked, _, _| {
                                                                    this.selection.journal =
                                                                        *checked;
                                                                },
                                                            ),
                                                        },
                                                        cx,
                                                    ),
                                                )
                                            },
                                        )
                                        // 由于 conference 已合并到 publication，diff_fields.conference 始终为 false，这部分不再显示
                                        .when(
                                            self.diff_fields.conference,
                                            |this: Scrollable<gpui::Div>| {
                                                this.child(
                                                    self.render_compare_row(
                                                        CompareRowProps {
                                                            label: "Conference".into(),
                                                            original_val: original
                                                                .publication
                                                                .as_ref()
                                                                .map(|p| p.name.clone())
                                                                .unwrap_or_default(),
                                                            new_val: new_lit
                                                                .publication
                                                                .as_ref()
                                                                .map(|p| p.name.clone())
                                                                .unwrap_or_default(),
                                                            is_selected: self.selection.conference,
                                                            index: 5,
                                                            on_toggle: Box::new(
                                                                |this, checked, _, _| {
                                                                    this.selection.conference =
                                                                        *checked;
                                                                },
                                                            ),
                                                        },
                                                        cx,
                                                    ),
                                                )
                                            },
                                        )
                                        .when(
                                            self.diff_fields.volume,
                                            |this: Scrollable<gpui::Div>| {
                                                this.child(
                                                    self.render_compare_row(
                                                        CompareRowProps {
                                                            label: t(I18nKey::Volume, lang).into(),
                                                            original_val: original
                                                                .volume
                                                                .clone()
                                                                .unwrap_or_default(),
                                                            new_val: new_lit
                                                                .volume
                                                                .clone()
                                                                .unwrap_or_default(),
                                                            is_selected: self.selection.volume,
                                                            index: 6,
                                                            on_toggle: Box::new(
                                                                |this, checked, _, _| {
                                                                    this.selection.volume =
                                                                        *checked;
                                                                },
                                                            ),
                                                        },
                                                        cx,
                                                    ),
                                                )
                                            },
                                        )
                                        .when(
                                            self.diff_fields.issue,
                                            |this: Scrollable<gpui::Div>| {
                                                this.child(
                                                    self.render_compare_row(
                                                        CompareRowProps {
                                                            label: t(I18nKey::Issue, lang).into(),
                                                            original_val: original
                                                                .issue
                                                                .clone()
                                                                .unwrap_or_default(),
                                                            new_val: new_lit
                                                                .issue
                                                                .clone()
                                                                .unwrap_or_default(),
                                                            is_selected: self.selection.issue,
                                                            index: 7,
                                                            on_toggle: Box::new(
                                                                |this, checked, _, _| {
                                                                    this.selection.issue = *checked;
                                                                },
                                                            ),
                                                        },
                                                        cx,
                                                    ),
                                                )
                                            },
                                        )
                                        .when(
                                            self.diff_fields.pages,
                                            |this: Scrollable<gpui::Div>| {
                                                this.child(
                                                    self.render_compare_row(
                                                        CompareRowProps {
                                                            label: t(I18nKey::Pages, lang).into(),
                                                            original_val: original
                                                                .pages
                                                                .clone()
                                                                .unwrap_or_default(),
                                                            new_val: new_lit
                                                                .pages
                                                                .clone()
                                                                .unwrap_or_default(),
                                                            is_selected: self.selection.pages,
                                                            index: 8,
                                                            on_toggle: Box::new(
                                                                |this, checked, _, _| {
                                                                    this.selection.pages = *checked;
                                                                },
                                                            ),
                                                        },
                                                        cx,
                                                    ),
                                                )
                                            },
                                        )
                                        .when(
                                            self.diff_fields.publisher,
                                            |this: Scrollable<gpui::Div>| {
                                                this.child(
                                                    self.render_compare_row(
                                                        CompareRowProps {
                                                            label: t(I18nKey::Publisher, lang)
                                                                .into(),
                                                            original_val: original
                                                                .publication
                                                                .as_ref()
                                                                .and_then(|p| p.publisher.clone())
                                                                .unwrap_or_default(),
                                                            new_val: new_lit
                                                                .publication
                                                                .as_ref()
                                                                .and_then(|p| p.publisher.clone())
                                                                .unwrap_or_default(),
                                                            is_selected: self.selection.publisher,
                                                            index: 9,
                                                            on_toggle: Box::new(
                                                                |this, checked, _, _| {
                                                                    this.selection.publisher =
                                                                        *checked;
                                                                },
                                                            ),
                                                        },
                                                        cx,
                                                    ),
                                                )
                                            },
                                        )
                                        .when(
                                            self.diff_fields.doi,
                                            |this: Scrollable<gpui::Div>| {
                                                this.child(
                                                    self.render_compare_row(
                                                        CompareRowProps {
                                                            label: "DOI".into(),
                                                            original_val: original
                                                                .doi
                                                                .clone()
                                                                .unwrap_or_default(),
                                                            new_val: new_lit
                                                                .doi
                                                                .clone()
                                                                .unwrap_or_default(),
                                                            is_selected: self.selection.doi,
                                                            index: 10,
                                                            on_toggle: Box::new(
                                                                |this, checked, _, _| {
                                                                    this.selection.doi = *checked;
                                                                },
                                                            ),
                                                        },
                                                        cx,
                                                    ),
                                                )
                                            },
                                        )
                                        // Always try to show ArXiv if it exists in either side to make it clear
                                        .when(
                                            self.diff_fields.arxiv_id
                                                || original
                                                    .arxiv_id
                                                    .as_ref()
                                                    .is_some_and(|s| !s.is_empty())
                                                || new_lit
                                                    .arxiv_id
                                                    .as_ref()
                                                    .is_some_and(|s| !s.is_empty()),
                                            |this: Scrollable<gpui::Div>| {
                                                this.child(
                                                    self.render_compare_row(
                                                        CompareRowProps {
                                                            label: "ArXiv".into(),
                                                            original_val: original
                                                                .arxiv_id
                                                                .clone()
                                                                .unwrap_or_default(),
                                                            new_val: new_lit
                                                                .arxiv_id
                                                                .clone()
                                                                .unwrap_or_default(),
                                                            is_selected: self.selection.arxiv_id,
                                                            index: 12,
                                                            on_toggle: Box::new(
                                                                |this, checked, _, _| {
                                                                    this.selection.arxiv_id =
                                                                        *checked;
                                                                },
                                                            ),
                                                        },
                                                        cx,
                                                    ),
                                                )
                                            },
                                        )
                                        .when(
                                            self.diff_fields.url,
                                            |this: Scrollable<gpui::Div>| {
                                                this.child(
                                                    self.render_compare_row(
                                                        CompareRowProps {
                                                            label: "URL".into(),
                                                            original_val: original
                                                                .url
                                                                .clone()
                                                                .unwrap_or_default(),
                                                            new_val: new_lit
                                                                .url
                                                                .clone()
                                                                .unwrap_or_default(),
                                                            is_selected: self.selection.url,
                                                            index: 13,
                                                            on_toggle: Box::new(
                                                                |this, checked, _, _| {
                                                                    this.selection.url = *checked;
                                                                },
                                                            ),
                                                        },
                                                        cx,
                                                    ),
                                                )
                                            },
                                        )
                                        .when(
                                            self.diff_fields.abstract_text,
                                            |this: Scrollable<gpui::Div>| {
                                                this.child(
                                                    self.render_compare_row(
                                                        CompareRowProps {
                                                            label: t(I18nKey::Abstract, lang)
                                                                .into(),
                                                            original_val: original
                                                                .abstract_text
                                                                .clone()
                                                                .unwrap_or_default(),
                                                            new_val: new_lit
                                                                .abstract_text
                                                                .clone()
                                                                .unwrap_or_default(),
                                                            is_selected: self
                                                                .selection
                                                                .abstract_text,
                                                            index: 14,
                                                            on_toggle: Box::new(
                                                                |this, checked, _, _| {
                                                                    this.selection.abstract_text =
                                                                        *checked;
                                                                },
                                                            ),
                                                        },
                                                        cx,
                                                    ),
                                                )
                                            },
                                        ),
                                )
                                .into_any_element()
                        }
                    },
                ),
            )
    }
}
