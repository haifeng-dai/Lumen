use crate::app_state::theme::surface;
use crate::ui::dialogs::compare_dialog::FieldSelection;
use components::IconName;
use gpui::prelude::*;
use gpui::{ElementId, MouseButton, SharedString, Window, div, rems, transparent_black};
use gpui_component::{
    ActiveTheme, Icon, TitleBar,
    button::{Button, ButtonVariants},
    h_flex,
    scroll::ScrollableElement,
    v_flex,
};
use i18n::{I18nKey, t};
use models::Literature;
use services::app::MainApp;
use std::sync::Arc;

pub struct MergeDialogResult {
    pub master_id: String,
    pub source_id: String,
    pub selection: FieldSelection,
    pub keep_a_main_pdf: bool,
    pub keep_b_main_pdf: bool,
    pub keep_attachment_ids: std::collections::HashSet<String>,
}

pub type MergeDialogCallback =
    Box<dyn FnOnce(Option<MergeDialogResult>, &mut Window, &mut gpui::App)>;

pub struct MergeDialog {
    app: Arc<MainApp>,
    item_a: Literature,
    item_b: Literature,
    is_a_master: bool,
    select_a: FieldSelection,
    select_b: FieldSelection,
    diff_fields: FieldSelection,
    keep_a_main_pdf: bool,
    keep_b_main_pdf: bool,
    keep_attachment_ids: std::collections::HashSet<String>,
    on_complete: Option<MergeDialogCallback>,
}

#[derive(Clone)]
enum FieldId {
    LiteratureType,
    Title,
    Authors,
    Year,
    Month,
    Day,
    Journal,
    Volume,
    Issue,
    Pages,
    Publisher,
    Abstract,
    Doi,
    ArxivId,
    Url,
}

fn fmt_author_name(a: &models::Author) -> String {
    format!(
        "{}{}{}",
        a.first_name,
        if let Some(ref m) = a.middle_name {
            format!(" {}", m)
        } else {
            String::new()
        },
        if a.last_name.is_empty() {
            String::new()
        } else {
            format!(" {}", a.last_name)
        },
    )
    .trim()
    .to_string()
}

fn fmt_authors(authors: &[models::Author]) -> String {
    authors
        .iter()
        .map(fmt_author_name)
        .collect::<Vec<_>>()
        .join("; ")
}

fn get_main_and_others(lit: &Literature) -> (Option<models::Attachment>, Vec<models::Attachment>) {
    let mut main_att = None;
    let mut others = Vec::new();
    if let Some(pos) = lit.attachments.iter().position(|a| a.is_main) {
        main_att = Some(lit.attachments[pos].clone());
        for (i, att) in lit.attachments.iter().enumerate() {
            if i != pos {
                others.push(att.clone());
            }
        }
    } else if let Some(first) = lit.attachments.first() {
        main_att = Some(first.clone());
        others = lit.attachments[1..].to_vec();
    }
    (main_att, others)
}

impl MergeDialog {
    pub fn new(
        app: Arc<MainApp>,
        item_a: Literature,
        item_b: Literature,
        diff_fields: FieldSelection,
        on_complete: MergeDialogCallback,
    ) -> Self {
        let select_a = diff_fields.clone();
        let mut select_b = diff_fields.clone();
        select_b.literature_type = false;
        select_b.title = false;
        select_b.authors = false;
        select_b.year = false;
        select_b.month = false;
        select_b.day = false;
        select_b.journal = false;
        select_b.volume = false;
        select_b.issue = false;
        select_b.pages = false;
        select_b.publisher = false;
        select_b.abstract_text = false;
        select_b.doi = false;
        select_b.arxiv_id = false;
        select_b.url = false;

        let has_a_main =
            item_a.attachments.iter().any(|a| a.is_main) || !item_a.attachments.is_empty();
        let has_b_main =
            item_b.attachments.iter().any(|a| a.is_main) || !item_b.attachments.is_empty();

        let mut keep_attachment_ids = std::collections::HashSet::new();
        let (_, a_others) = get_main_and_others(&item_a);
        let (_, b_others) = get_main_and_others(&item_b);
        for att in a_others {
            keep_attachment_ids.insert(att.id.clone());
        }
        for att in b_others {
            keep_attachment_ids.insert(att.id.clone());
        }

        Self {
            app,
            item_a,
            item_b,
            is_a_master: true,
            select_a,
            select_b,
            diff_fields,
            keep_a_main_pdf: has_a_main,
            keep_b_main_pdf: !has_a_main && has_b_main,
            keep_attachment_ids,
            on_complete: Some(on_complete),
        }
    }

    pub fn set_master(&mut self, is_a: bool, cx: &mut Context<Self>) {
        self.is_a_master = is_a;

        let has_a_main = self.item_a.attachments.iter().any(|a| a.is_main)
            || !self.item_a.attachments.is_empty();
        let has_b_main = self.item_b.attachments.iter().any(|a| a.is_main)
            || !self.item_b.attachments.is_empty();

        if is_a {
            self.keep_a_main_pdf = has_a_main;
            self.keep_b_main_pdf = !has_a_main && has_b_main;

            self.select_a = self.diff_fields.clone();
            self.select_b = self.diff_fields.clone();
            self.select_b.literature_type = false;
            self.select_b.title = false;
            self.select_b.authors = false;
            self.select_b.year = false;
            self.select_b.month = false;
            self.select_b.day = false;
            self.select_b.journal = false;
            self.select_b.volume = false;
            self.select_b.issue = false;
            self.select_b.pages = false;
            self.select_b.publisher = false;
            self.select_b.abstract_text = false;
            self.select_b.doi = false;
            self.select_b.arxiv_id = false;
            self.select_b.url = false;
        } else {
            self.keep_b_main_pdf = has_b_main;
            self.keep_a_main_pdf = !has_b_main && has_a_main;

            self.select_b = self.diff_fields.clone();
            self.select_a = self.diff_fields.clone();
            self.select_a.literature_type = false;
            self.select_a.title = false;
            self.select_a.authors = false;
            self.select_a.year = false;
            self.select_a.month = false;
            self.select_a.day = false;
            self.select_a.journal = false;
            self.select_a.volume = false;
            self.select_a.issue = false;
            self.select_a.pages = false;
            self.select_a.publisher = false;
            self.select_a.abstract_text = false;
            self.select_a.doi = false;
            self.select_a.arxiv_id = false;
            self.select_a.url = false;
        }
        cx.notify();
    }

    fn toggle_field_a(&mut self, field: FieldId, cx: &mut Context<Self>) {
        let v = |current: bool| !current;
        match field {
            FieldId::LiteratureType => {
                self.select_a.literature_type = v(self.select_a.literature_type);
                if self.select_a.literature_type {
                    self.select_b.literature_type = false;
                }
            }
            FieldId::Title => {
                self.select_a.title = v(self.select_a.title);
                if self.select_a.title {
                    self.select_b.title = false;
                }
            }
            FieldId::Authors => {
                self.select_a.authors = v(self.select_a.authors);
                if self.select_a.authors {
                    self.select_b.authors = false;
                }
            }
            FieldId::Year => {
                self.select_a.year = v(self.select_a.year);
                if self.select_a.year {
                    self.select_b.year = false;
                }
            }
            FieldId::Month => {
                self.select_a.month = v(self.select_a.month);
                if self.select_a.month {
                    self.select_b.month = false;
                }
            }
            FieldId::Day => {
                self.select_a.day = v(self.select_a.day);
                if self.select_a.day {
                    self.select_b.day = false;
                }
            }
            FieldId::Journal => {
                self.select_a.journal = v(self.select_a.journal);
                if self.select_a.journal {
                    self.select_b.journal = false;
                }
            }
            FieldId::Volume => {
                self.select_a.volume = v(self.select_a.volume);
                if self.select_a.volume {
                    self.select_b.volume = false;
                }
            }
            FieldId::Issue => {
                self.select_a.issue = v(self.select_a.issue);
                if self.select_a.issue {
                    self.select_b.issue = false;
                }
            }
            FieldId::Pages => {
                self.select_a.pages = v(self.select_a.pages);
                if self.select_a.pages {
                    self.select_b.pages = false;
                }
            }
            FieldId::Publisher => {
                self.select_a.publisher = v(self.select_a.publisher);
                if self.select_a.publisher {
                    self.select_b.publisher = false;
                }
            }
            FieldId::Abstract => {
                self.select_a.abstract_text = v(self.select_a.abstract_text);
                if self.select_a.abstract_text {
                    self.select_b.abstract_text = false;
                }
            }
            FieldId::Doi => {
                self.select_a.doi = v(self.select_a.doi);
                if self.select_a.doi {
                    self.select_b.doi = false;
                }
            }
            FieldId::ArxivId => {
                self.select_a.arxiv_id = v(self.select_a.arxiv_id);
                if self.select_a.arxiv_id {
                    self.select_b.arxiv_id = false;
                }
            }
            FieldId::Url => {
                self.select_a.url = v(self.select_a.url);
                if self.select_a.url {
                    self.select_b.url = false;
                }
            }
        }
        cx.notify();
    }

    fn toggle_field_b(&mut self, field: FieldId, cx: &mut Context<Self>) {
        let v = |current: bool| !current;
        match field {
            FieldId::LiteratureType => {
                self.select_b.literature_type = v(self.select_b.literature_type);
                if self.select_b.literature_type {
                    self.select_a.literature_type = false;
                }
            }
            FieldId::Title => {
                self.select_b.title = v(self.select_b.title);
                if self.select_b.title {
                    self.select_a.title = false;
                }
            }
            FieldId::Authors => {
                self.select_b.authors = v(self.select_b.authors);
                if self.select_b.authors {
                    self.select_a.authors = false;
                }
            }
            FieldId::Year => {
                self.select_b.year = v(self.select_b.year);
                if self.select_b.year {
                    self.select_a.year = false;
                }
            }
            FieldId::Month => {
                self.select_b.month = v(self.select_b.month);
                if self.select_b.month {
                    self.select_a.month = false;
                }
            }
            FieldId::Day => {
                self.select_b.day = v(self.select_b.day);
                if self.select_b.day {
                    self.select_a.day = false;
                }
            }
            FieldId::Journal => {
                self.select_b.journal = v(self.select_b.journal);
                if self.select_b.journal {
                    self.select_a.journal = false;
                }
            }
            FieldId::Volume => {
                self.select_b.volume = v(self.select_b.volume);
                if self.select_b.volume {
                    self.select_a.volume = false;
                }
            }
            FieldId::Issue => {
                self.select_b.issue = v(self.select_b.issue);
                if self.select_b.issue {
                    self.select_a.issue = false;
                }
            }
            FieldId::Pages => {
                self.select_b.pages = v(self.select_b.pages);
                if self.select_b.pages {
                    self.select_a.pages = false;
                }
            }
            FieldId::Publisher => {
                self.select_b.publisher = v(self.select_b.publisher);
                if self.select_b.publisher {
                    self.select_a.publisher = false;
                }
            }
            FieldId::Abstract => {
                self.select_b.abstract_text = v(self.select_b.abstract_text);
                if self.select_b.abstract_text {
                    self.select_a.abstract_text = false;
                }
            }
            FieldId::Doi => {
                self.select_b.doi = v(self.select_b.doi);
                if self.select_b.doi {
                    self.select_a.doi = false;
                }
            }
            FieldId::ArxivId => {
                self.select_b.arxiv_id = v(self.select_b.arxiv_id);
                if self.select_b.arxiv_id {
                    self.select_a.arxiv_id = false;
                }
            }
            FieldId::Url => {
                self.select_b.url = v(self.select_b.url);
                if self.select_b.url {
                    self.select_a.url = false;
                }
            }
        }
        cx.notify();
    }

    fn handle_confirm(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(callback) = self.on_complete.take() {
            let (master_id, source_id, selection) = if self.is_a_master {
                (
                    self.item_a.id.clone(),
                    self.item_b.id.clone(),
                    self.select_b.clone(),
                )
            } else {
                (
                    self.item_b.id.clone(),
                    self.item_a.id.clone(),
                    self.select_a.clone(),
                )
            };

            let result = MergeDialogResult {
                master_id,
                source_id,
                selection,
                keep_a_main_pdf: self.keep_a_main_pdf,
                keep_b_main_pdf: self.keep_b_main_pdf,
                keep_attachment_ids: self.keep_attachment_ids.clone(),
            };
            callback(Some(result), _window, cx);
        }
    }

    fn handle_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(callback) = self.on_complete.take() {
            callback(None, window, cx);
        }
    }

    fn render_file_badge(
        &self,
        att: &models::Attachment,
        _lit_id: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        let display_ext = std::path::Path::new(&att.file_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("FILE")
            .to_uppercase();

        let att_id_str = att.id.clone();
        let app_clone = self.app.clone();

        div()
            .text_xs()
            .bg(if att.is_main {
                theme.primary.opacity(0.15)
            } else {
                theme.muted
            })
            .text_color(if att.is_main {
                theme.primary
            } else {
                theme.muted_foreground
            })
            .px_1p5()
            .py_0p5()
            .rounded_sm()
            .cursor_pointer()
            .when(att.is_main, |s| s.font_weight(gpui::FontWeight::BOLD))
            .child(display_ext)
            .on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                cx.stop_propagation();
                let _ = app_clone.open_attachment(&att_id_str);
            })
    }

    fn render_custom_checkbox(
        &self,
        id: ElementId,
        checked: bool,
        on_click: impl Fn(&bool, &mut Window, &mut gpui::App) + 'static,
        cx: &mut gpui::App,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        let click_handler = std::sync::Arc::new(on_click);
        let click_handler_clone = click_handler.clone();
        let next_checked = !checked;

        div()
            .id(id)
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .w(rems(1.0))
            .h(rems(1.0))
            .rounded_sm()
            .when(checked, |s| {
                s.bg(theme.primary).text_color(theme.background)
            })
            .when(!checked, |s| {
                s.border_1()
                    .border_color(theme.border)
                    .bg(transparent_black())
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_click(move |_, window, cx| {
                (click_handler_clone)(&next_checked, window, cx);
            })
            .child(
                Icon::new(IconName::Check)
                    .size(rems(0.75))
                    .text_color(if checked {
                        theme.background
                    } else {
                        transparent_black()
                    }),
            )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_field_row(
        &self,
        label: &'static str,
        i18n_key: I18nKey,
        field_id: FieldId,
        val_a: SharedString,
        val_b: SharedString,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let surface = surface(cx);
        let theme = cx.theme().clone();
        let lang = self.app.current_language();
        let label_str = t(i18n_key, lang);

        let (is_a_selected, is_b_selected, _has_diff) = match field_id {
            FieldId::LiteratureType => (
                self.select_a.literature_type,
                self.select_b.literature_type,
                self.diff_fields.literature_type,
            ),
            FieldId::Title => (
                self.select_a.title,
                self.select_b.title,
                self.diff_fields.title,
            ),
            FieldId::Authors => (
                self.select_a.authors,
                self.select_b.authors,
                self.diff_fields.authors,
            ),
            FieldId::Year => (
                self.select_a.year,
                self.select_b.year,
                self.diff_fields.year,
            ),
            FieldId::Month => (
                self.select_a.month,
                self.select_b.month,
                self.diff_fields.month,
            ),
            FieldId::Day => (self.select_a.day, self.select_b.day, self.diff_fields.day),
            FieldId::Journal => (
                self.select_a.journal,
                self.select_b.journal,
                self.diff_fields.journal,
            ),
            FieldId::Volume => (
                self.select_a.volume,
                self.select_b.volume,
                self.diff_fields.volume,
            ),
            FieldId::Issue => (
                self.select_a.issue,
                self.select_b.issue,
                self.diff_fields.issue,
            ),
            FieldId::Pages => (
                self.select_a.pages,
                self.select_b.pages,
                self.diff_fields.pages,
            ),
            FieldId::Publisher => (
                self.select_a.publisher,
                self.select_b.publisher,
                self.diff_fields.publisher,
            ),
            FieldId::Abstract => (
                self.select_a.abstract_text,
                self.select_b.abstract_text,
                self.diff_fields.abstract_text,
            ),
            FieldId::Doi => (self.select_a.doi, self.select_b.doi, self.diff_fields.doi),
            FieldId::ArxivId => (
                self.select_a.arxiv_id,
                self.select_b.arxiv_id,
                self.diff_fields.arxiv_id,
            ),
            FieldId::Url => (self.select_a.url, self.select_b.url, self.diff_fields.url),
        };

        let id_a = format!("check-a-{}", label);
        let id_b = format!("check-b-{}", label);
        let field_id_clone = field_id.clone();
        let field_id_clone2 = field_id.clone();

        let is_odd = !index.is_multiple_of(2);
        h_flex()
            .w_full()
            .px_2()
            .py_1p5()
            .gap_2()
            .items_center()
            .bg(if is_odd {
                surface.info_bg
            } else {
                theme.background
            })
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .w(rems(5.0))
                    .flex_shrink_0()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(label_str),
            )
            .child(
                h_flex()
                    .flex_grow(1.0)
                    .w_0()
                    .gap_2()
                    .items_center()
                    .py_0p5()
                    .child(self.render_custom_checkbox(
                        ElementId::from(SharedString::from(id_a)),
                        is_a_selected,
                        cx.listener(move |this, _, _window, cx| {
                            this.toggle_field_a(field_id_clone.clone(), cx);
                        }),
                        cx,
                    ))
                    .child(
                        div()
                            .flex_grow(1.0)
                            .w_0()
                            .text_sm()
                            .text_color(if is_a_selected {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .child(if val_a.is_empty() {
                                SharedString::from("-")
                            } else {
                                val_a.clone()
                            }),
                    ),
            )
            .child(
                h_flex()
                    .flex_grow(1.0)
                    .w_0()
                    .gap_2()
                    .items_center()
                    .py_0p5()
                    .child(self.render_custom_checkbox(
                        ElementId::from(SharedString::from(id_b)),
                        is_b_selected,
                        cx.listener(move |this, _, _window, cx| {
                            this.toggle_field_b(field_id_clone2.clone(), cx);
                        }),
                        cx,
                    ))
                    .child(
                        div()
                            .flex_grow(1.0)
                            .w_0()
                            .text_sm()
                            .text_color(if is_b_selected {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .child(if val_b.is_empty() {
                                SharedString::from("-")
                            } else {
                                val_b.clone()
                            }),
                    ),
            )
    }
}

impl Render for MergeDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let surface = surface(cx);
        let theme = cx.theme().clone();
        let lang = self.app.current_language();
        let is_a_master = self.is_a_master;

        div()
            .size_full()
            .bg(theme.background)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                TitleBar::new()
                    .bg(theme.background)
                    .border_color(transparent_black()),
            )
            .child(div().flex_1().px_4().pb_4().overflow_hidden().child({
                // 表格外框容器
                let table_content = {
                    let fmt_val = |s: &Option<String>| s.clone().unwrap_or_default();
                    let fn_pub_name = |lit: &Literature| {
                        lit.publication
                            .as_ref()
                            .map(|p| p.publisher.clone().unwrap_or_default())
                            .unwrap_or_default()
                    };

                    let a_title = self.item_a.title.clone();
                    let b_title = self.item_b.title.clone();

                    let (a_main_pdf, a_others) = get_main_and_others(&self.item_a);
                    let (b_main_pdf, b_others) = get_main_and_others(&self.item_b);

                    let this_weak = cx.entity().downgrade();
                    let mut rows = Vec::new();
                    let mut row_idx: usize = 0;

                    // ── 主条目对比行 ──
                    {
                        let is_odd = !row_idx.is_multiple_of(2);
                        row_idx += 1;
                        rows.push(
                            h_flex()
                                .w_full()
                                .px_2()
                                .py_2()
                                .gap_2()
                                .items_center()
                                .bg(if is_odd {
                                    surface.info_bg
                                } else {
                                    theme.background
                                })
                                .border_b_1()
                                .border_color(theme.border)
                                .child(
                                    div()
                                        .w(rems(5.0))
                                        .flex_shrink_0()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(theme.primary)
                                        .child("主条目"),
                                )
                                .child(
                                    h_flex()
                                        .flex_grow(1.0)
                                        .w_0()
                                        .gap_2()
                                        .items_center()
                                        .py_0p5()
                                        .child(self.render_custom_checkbox(
                                            ElementId::from("master-a"),
                                            is_a_master,
                                            cx.listener(|this, _, _, cx| {
                                                this.set_master(true, cx);
                                            }),
                                            cx,
                                        ))
                                        .child(
                                            div()
                                                .flex_grow(1.0)
                                                .w_0()
                                                .text_sm()
                                                .text_color(if is_a_master {
                                                    theme.foreground
                                                } else {
                                                    theme.muted_foreground
                                                })
                                                .child(a_title),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .flex_grow(1.0)
                                        .w_0()
                                        .gap_2()
                                        .items_center()
                                        .py_0p5()
                                        .child(self.render_custom_checkbox(
                                            ElementId::from("master-b"),
                                            !is_a_master,
                                            cx.listener(|this, _, _, cx| {
                                                this.set_master(false, cx);
                                            }),
                                            cx,
                                        ))
                                        .child(
                                            div()
                                                .flex_grow(1.0)
                                                .w_0()
                                                .text_sm()
                                                .text_color(if !is_a_master {
                                                    theme.foreground
                                                } else {
                                                    theme.muted_foreground
                                                })
                                                .child(b_title),
                                        ),
                                )
                                .into_any_element(),
                        );
                    }

                    // ── 主文件对比行 ──
                    if a_main_pdf.is_some() || b_main_pdf.is_some() {
                        let is_odd = !row_idx.is_multiple_of(2);
                        row_idx += 1;
                        rows.push(
                            h_flex()
                                .w_full()
                                .px_2()
                                .py_2()
                                .gap_2()
                                .items_center()
                                .bg(if is_odd {
                                    surface.info_bg
                                } else {
                                    theme.background
                                })
                                .border_b_1()
                                .border_color(theme.border)
                                .child(
                                    div()
                                        .w(rems(5.0))
                                        .flex_shrink_0()
                                        .text_sm()
                                        .text_color(theme.muted_foreground)
                                        .child(t(I18nKey::MainFile, lang)),
                                )
                                .child(
                                    h_flex()
                                        .flex_grow(1.0)
                                        .w_0()
                                        .gap_2()
                                        .items_center()
                                        .child(self.render_custom_checkbox(
                                            ElementId::from("main-pdf-a"),
                                            self.keep_a_main_pdf,
                                            cx.listener(|this, _, _, cx| {
                                                this.keep_a_main_pdf = true;
                                                this.keep_b_main_pdf = false;
                                                cx.notify();
                                            }),
                                            cx,
                                        ))
                                        .when_some(a_main_pdf.clone(), |this, att| {
                                            this.child(self.render_file_badge(
                                                &att,
                                                &self.item_a.id,
                                                cx,
                                            ))
                                        }),
                                )
                                .child(
                                    h_flex()
                                        .flex_grow(1.0)
                                        .w_0()
                                        .gap_2()
                                        .items_center()
                                        .child(self.render_custom_checkbox(
                                            ElementId::from("main-pdf-b"),
                                            self.keep_b_main_pdf,
                                            cx.listener(|this, _, _, cx| {
                                                this.keep_b_main_pdf = true;
                                                this.keep_a_main_pdf = false;
                                                cx.notify();
                                            }),
                                            cx,
                                        ))
                                        .when_some(b_main_pdf.clone(), |this, att| {
                                            this.child(self.render_file_badge(
                                                &att,
                                                &self.item_b.id,
                                                cx,
                                            ))
                                        }),
                                )
                                .into_any_element(),
                        );
                    }

                    // ── 其它附件对比行 ──
                    {
                        let max_others = std::cmp::max(a_others.len(), b_others.len());
                        let this_weak_att = this_weak.clone();
                        for i in 0..max_others {
                            let label_str = if i == 0 { "附件" } else { "" };
                            let a_att = a_others.get(i);
                            let b_att = b_others.get(i);
                            let this_weak_loop = this_weak_att.clone();
                            let is_odd = !row_idx.is_multiple_of(2);
                            row_idx += 1;

                            let mut row = h_flex()
                                .w_full()
                                .px_2()
                                .py_1p5()
                                .gap_2()
                                .items_center()
                                .bg(if is_odd {
                                    surface.info_bg
                                } else {
                                    theme.background
                                })
                                .border_b_1()
                                .border_color(theme.border)
                                .child(
                                    div()
                                        .w(rems(5.0))
                                        .flex_shrink_0()
                                        .text_sm()
                                        .text_color(theme.muted_foreground)
                                        .child(label_str),
                                );

                            if let Some(att) = a_att {
                                let is_checked = self.keep_attachment_ids.contains(&att.id);
                                let att_id = att.id.clone();
                                let this_weak_click = this_weak_loop.clone();
                                row = row.child(
                                    h_flex()
                                        .flex_grow(1.0)
                                        .w_0()
                                        .gap_2()
                                        .items_center()
                                        .child(self.render_custom_checkbox(
                                            ElementId::from(SharedString::from(format!(
                                                "att-a-{}",
                                                att.id
                                            ))),
                                            is_checked,
                                            {
                                                let this_weak_inner = this_weak_click.clone();
                                                let att_id_inner = att_id.clone();
                                                move |_, _, cx| {
                                                    if let Some(this) = this_weak_inner.upgrade() {
                                                        this.update(cx, |this, cx| {
                                                            if this
                                                                .keep_attachment_ids
                                                                .contains(&att_id_inner)
                                                            {
                                                                this.keep_attachment_ids
                                                                    .remove(&att_id_inner);
                                                            } else {
                                                                this.keep_attachment_ids
                                                                    .insert(att_id_inner.clone());
                                                            }
                                                            cx.notify();
                                                        });
                                                    }
                                                }
                                            },
                                            cx,
                                        ))
                                        .child(self.render_file_badge(att, &self.item_a.id, cx)),
                                );
                            } else {
                                row = row.child(div().flex_grow(1.0).w_0());
                            }

                            if let Some(att) = b_att {
                                let is_checked = self.keep_attachment_ids.contains(&att.id);
                                let att_id = att.id.clone();
                                let this_weak_click = this_weak_loop.clone();
                                row = row.child(
                                    h_flex()
                                        .flex_grow(1.0)
                                        .w_0()
                                        .gap_2()
                                        .items_center()
                                        .child(self.render_custom_checkbox(
                                            ElementId::from(SharedString::from(format!(
                                                "att-b-{}",
                                                att.id
                                            ))),
                                            is_checked,
                                            {
                                                let this_weak_inner = this_weak_click.clone();
                                                let att_id_inner = att_id.clone();
                                                move |_, _, cx| {
                                                    if let Some(this) = this_weak_inner.upgrade() {
                                                        this.update(cx, |this, cx| {
                                                            if this
                                                                .keep_attachment_ids
                                                                .contains(&att_id_inner)
                                                            {
                                                                this.keep_attachment_ids
                                                                    .remove(&att_id_inner);
                                                            } else {
                                                                this.keep_attachment_ids
                                                                    .insert(att_id_inner.clone());
                                                            }
                                                            cx.notify();
                                                        });
                                                    }
                                                }
                                            },
                                            cx,
                                        ))
                                        .child(self.render_file_badge(att, &self.item_b.id, cx)),
                                );
                            } else {
                                row = row.child(div().flex_grow(1.0).w_0());
                            }

                            rows.push(row.into_any_element());
                        }
                    }

                    // ── 字段对比行 ──
                    if self.diff_fields.literature_type {
                        rows.push(
                            self.render_field_row(
                                "type",
                                I18nKey::Type,
                                FieldId::LiteratureType,
                                self.item_a.literature_type.to_string().into(),
                                self.item_b.literature_type.to_string().into(),
                                {
                                    let idx = row_idx;
                                    row_idx += 1;
                                    idx
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    if self.diff_fields.title {
                        rows.push(
                            self.render_field_row(
                                "title",
                                I18nKey::Title,
                                FieldId::Title,
                                self.item_a.title.clone().into(),
                                self.item_b.title.clone().into(),
                                {
                                    let idx = row_idx;
                                    row_idx += 1;
                                    idx
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    if self.diff_fields.authors {
                        rows.push(
                            self.render_field_row(
                                "authors",
                                I18nKey::Authors,
                                FieldId::Authors,
                                fmt_authors(&self.item_a.authors).into(),
                                fmt_authors(&self.item_b.authors).into(),
                                {
                                    let idx = row_idx;
                                    row_idx += 1;
                                    idx
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    if self.diff_fields.year {
                        rows.push(
                            self.render_field_row(
                                "year",
                                I18nKey::Year,
                                FieldId::Year,
                                self.item_a
                                    .year
                                    .map(|v| v.to_string())
                                    .unwrap_or_default()
                                    .into(),
                                self.item_b
                                    .year
                                    .map(|v| v.to_string())
                                    .unwrap_or_default()
                                    .into(),
                                {
                                    let idx = row_idx;
                                    row_idx += 1;
                                    idx
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    if self.diff_fields.month {
                        rows.push(
                            self.render_field_row(
                                "month",
                                I18nKey::Month,
                                FieldId::Month,
                                self.item_a
                                    .month
                                    .map(|v| format!("{:02}", v))
                                    .unwrap_or_default()
                                    .into(),
                                self.item_b
                                    .month
                                    .map(|v| format!("{:02}", v))
                                    .unwrap_or_default()
                                    .into(),
                                {
                                    let idx = row_idx;
                                    row_idx += 1;
                                    idx
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    if self.diff_fields.day {
                        rows.push(
                            self.render_field_row(
                                "day",
                                I18nKey::Day,
                                FieldId::Day,
                                self.item_a
                                    .day
                                    .map(|v| format!("{:02}", v))
                                    .unwrap_or_default()
                                    .into(),
                                self.item_b
                                    .day
                                    .map(|v| format!("{:02}", v))
                                    .unwrap_or_default()
                                    .into(),
                                {
                                    let idx = row_idx;
                                    row_idx += 1;
                                    idx
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    if self.diff_fields.journal {
                        rows.push(
                            self.render_field_row(
                                "journal",
                                I18nKey::Journal,
                                FieldId::Journal,
                                self.item_a
                                    .publication
                                    .as_ref()
                                    .map(|p| p.name.clone())
                                    .unwrap_or_default()
                                    .into(),
                                self.item_b
                                    .publication
                                    .as_ref()
                                    .map(|p| p.name.clone())
                                    .unwrap_or_default()
                                    .into(),
                                {
                                    let idx = row_idx;
                                    row_idx += 1;
                                    idx
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    if self.diff_fields.volume {
                        rows.push(
                            self.render_field_row(
                                "volume",
                                I18nKey::Volume,
                                FieldId::Volume,
                                fmt_val(&self.item_a.volume).into(),
                                fmt_val(&self.item_b.volume).into(),
                                {
                                    let idx = row_idx;
                                    row_idx += 1;
                                    idx
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    if self.diff_fields.issue {
                        rows.push(
                            self.render_field_row(
                                "issue",
                                I18nKey::Issue,
                                FieldId::Issue,
                                fmt_val(&self.item_a.issue).into(),
                                fmt_val(&self.item_b.issue).into(),
                                {
                                    let idx = row_idx;
                                    row_idx += 1;
                                    idx
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    if self.diff_fields.pages {
                        rows.push(
                            self.render_field_row(
                                "pages",
                                I18nKey::Pages,
                                FieldId::Pages,
                                fmt_val(&self.item_a.pages).into(),
                                fmt_val(&self.item_b.pages).into(),
                                {
                                    let idx = row_idx;
                                    row_idx += 1;
                                    idx
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    if self.diff_fields.publisher {
                        rows.push(
                            self.render_field_row(
                                "publisher",
                                I18nKey::Publisher,
                                FieldId::Publisher,
                                fn_pub_name(&self.item_a).into(),
                                fn_pub_name(&self.item_b).into(),
                                {
                                    let idx = row_idx;
                                    row_idx += 1;
                                    idx
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    if self.diff_fields.abstract_text {
                        rows.push(
                            self.render_field_row(
                                "abstract",
                                I18nKey::Abstract,
                                FieldId::Abstract,
                                fmt_val(&self.item_a.abstract_text).into(),
                                fmt_val(&self.item_b.abstract_text).into(),
                                {
                                    let idx = row_idx;
                                    row_idx += 1;
                                    idx
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    if self.diff_fields.doi {
                        rows.push(
                            self.render_field_row(
                                "doi",
                                I18nKey::Doi,
                                FieldId::Doi,
                                fmt_val(&self.item_a.doi).into(),
                                fmt_val(&self.item_b.doi).into(),
                                {
                                    let idx = row_idx;
                                    row_idx += 1;
                                    idx
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    if self.diff_fields.arxiv_id {
                        rows.push(
                            self.render_field_row(
                                "arxiv",
                                I18nKey::ArXiv,
                                FieldId::ArxivId,
                                fmt_val(&self.item_a.arxiv_id).into(),
                                fmt_val(&self.item_b.arxiv_id).into(),
                                {
                                    let idx = row_idx;
                                    row_idx += 1;
                                    idx
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    if self.diff_fields.url {
                        rows.push(
                            self.render_field_row(
                                "url",
                                I18nKey::Url,
                                FieldId::Url,
                                fmt_val(&self.item_a.url).into(),
                                fmt_val(&self.item_b.url).into(),
                                row_idx,
                                cx,
                            )
                            .into_any_element(),
                        );
                    }

                    v_flex().gap_0().children(rows)
                };
                // 表格外框 + 表头 + 滚动内容
                v_flex()
                    .size_full()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .overflow_hidden()
                    // 表头行
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
                                    .w(rems(5.0))
                                    .flex_shrink_0()
                                    .px_3()
                                    .py_2()
                                    .border_r_1()
                                    .border_color(theme.border)
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
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
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child("A"),
                                    ),
                            )
                            .child(
                                div().flex_grow(1.0).w_0().px_3().py_2().child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .child("B"),
                                ),
                            ),
                    )
                    // 滚动内容
                    .child(div().flex_1().overflow_y_scrollbar().child(table_content))
            }))
            .child(
                h_flex()
                    .w_full()
                    .h(rems(3.5))
                    .justify_end()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .bg(theme.background)
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        Button::new("cancel-merge")
                            .child(Icon::new(IconName::Close).size(rems(0.75)))
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.handle_cancel(window, cx);
                            })),
                    )
                    .child(
                        Button::new("confirm-merge")
                            .child(Icon::new(IconName::Check).size(rems(0.75)))
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.handle_confirm(window, cx);
                            })),
                    ),
            )
    }
}
