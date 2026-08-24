use crate::app_state::theme::surface;
use crate::ui::notification::show_notification;
use crate::ui::{
    components::{CollapsibleText, DetailRow},
    views::main_window::ContextMenuType,
};
use gpui::prelude::*;
use gpui::{DragMoveEvent, ExternalPaths, FontWeight, MouseButton, Window, div, rems};
use gpui_component::{
    Theme, ThemeMode, h_flex, notification::NotificationType, rating::Rating, v_flex,
};
use i18n::{I18nKey, Language, t};
use log::{error, info};
use models::Literature;
use std::path::{Path, PathBuf};

use super::SingleDetailBuffer;

impl super::LiteratureDetailView {
    pub(super) fn render_single_detail(
        &self,
        buffer: &SingleDetailBuffer,
        theme: &Theme,
        lang: Language,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let literature = &buffer.literature;
        let lit_id = literature.id.clone();

        div()
            .id("literature-detail-container")
            .relative()
            .size_full()
            .bg(if theme.mode == ThemeMode::Light {
                theme.background
            } else {
                theme.muted
            })
            .border_l_1()
            .border_color(theme.border)
            .on_drag_move::<ExternalPaths>(cx.listener(
                |view, event: &DragMoveEvent<ExternalPaths>, _window, cx| {
                    let is_inside = event.bounds.contains(&event.event.position);
                    if view.is_dragging != is_inside {
                        view.is_dragging = is_inside;
                        cx.notify();
                    }
                },
            ))
            .child(
                div()
                    .id("literature-detail-main")
                    .size_full()
                    .bg(theme.background)
                    .overflow_y_scroll()
                    .px_3()
                    .py_3()
                    .child(
                        v_flex()
                            .child(self.render_title_section(
                                &literature.title,
                                buffer.reading_status,
                                &lit_id,
                                theme,
                                lang,
                                cx,
                            ))
                            .child(
                                Rating::new("lit-rating")
                                    .value(buffer.rating as usize)
                                    .max(5)
                                    .color(theme.primary)
                                    .on_click({
                                        let app = self.app.clone();
                                        let lit_id = lit_id.clone();
                                        move |&value, _window, _cx| {
                                            info!(
                                                "详情: 评分设置 id={}, rating={}/5",
                                                lit_id, value
                                            );
                                            if let Ok(mut lit) = app.db.get_literature(&lit_id)
                                                && let Some(ref mut l) = lit
                                            {
                                                l.rating = value as i32;
                                                let _ = app.update_literature(l.clone());
                                            }
                                        }
                                    }),
                            )
                            .when(!buffer.authors_text.is_empty(), |this| {
                                this.child(self.render_field_row(
                                    t(I18nKey::Authors, lang),
                                    &buffer.authors_text,
                                    "authors",
                                    theme,
                                    cx,
                                ))
                            })
                            .when(!buffer.pub_name.is_empty(), |this| {
                                this.child(
                                    DetailRow::new(
                                        t(I18nKey::Publication, lang),
                                        buffer.pub_name.clone(),
                                        self.copied_field.as_ref()
                                            == Some(&"publication".to_string()),
                                        cx.listener({
                                            let val = buffer.pub_name.clone();
                                            move |this, _, window, cx| {
                                                this.copy_text(
                                                    val.clone(),
                                                    "publication".to_string(),
                                                    window.window_handle(),
                                                    cx,
                                                );
                                            }
                                        }),
                                    )
                                    .child(
                                        h_flex()
                                            .mt_1()
                                            .gap_2()
                                            .children(
                                                buffer
                                                    .jcr_badge
                                                    .as_ref()
                                                    .map(|b| self.render_badge(b)),
                                            )
                                            .children(
                                                buffer
                                                    .cas_badge
                                                    .as_ref()
                                                    .map(|b| self.render_badge(b)),
                                            )
                                            .children(
                                                buffer
                                                    .ccf_badge
                                                    .as_ref()
                                                    .map(|b| self.render_badge(b)),
                                            ),
                                    )
                                    .render(theme),
                                )
                            })
                            .when(
                                !buffer.pub_abbreviation.is_empty()
                                    && buffer.pub_abbreviation != buffer.pub_name,
                                |this| {
                                    this.child(self.render_field_row(
                                        t(I18nKey::PublicationAbbreviation, lang),
                                        &buffer.pub_abbreviation,
                                        "publication-abbreviation",
                                        theme,
                                        cx,
                                    ))
                                },
                            )
                            .child(
                                h_flex()
                                    .gap_4()
                                    .when_some(literature.year, |this, year| {
                                        this.child(self.render_field_row(
                                            t(I18nKey::Year, lang),
                                            &year.to_string(),
                                            "year",
                                            theme,
                                            cx,
                                        ))
                                    })
                                    .when_some(literature.month, |this, month| {
                                        this.child(self.render_field_row(
                                            t(I18nKey::Month, lang),
                                            &format!("{:02}", month),
                                            "month",
                                            theme,
                                            cx,
                                        ))
                                    })
                                    .when_some(literature.day, |this, day| {
                                        this.child(self.render_field_row(
                                            t(I18nKey::Day, lang),
                                            &format!("{:02}", day),
                                            "day",
                                            theme,
                                            cx,
                                        ))
                                    }),
                            )
                            .child(
                                h_flex()
                                    .gap_4()
                                    .when_some(
                                        literature.volume.as_ref().filter(|v| !v.trim().is_empty()),
                                        |this, vol| {
                                            this.child(self.render_field_row(
                                                t(I18nKey::Volume, lang),
                                                vol,
                                                "vol",
                                                theme,
                                                cx,
                                            ))
                                        },
                                    )
                                    .when_some(
                                        literature.issue.as_ref().filter(|i| !i.trim().is_empty()),
                                        |this, iss| {
                                            this.child(self.render_field_row(
                                                t(I18nKey::Issue, lang),
                                                iss,
                                                "issue",
                                                theme,
                                                cx,
                                            ))
                                        },
                                    )
                                    .when_some(
                                        literature.pages.as_ref().filter(|p| !p.trim().is_empty()),
                                        |this, pag| {
                                            this.child(self.render_field_row(
                                                t(I18nKey::Pages, lang),
                                                pag,
                                                "pages",
                                                theme,
                                                cx,
                                            ))
                                        },
                                    ),
                            )
                            .when_some(
                                literature
                                    .publication
                                    .as_ref()
                                    .and_then(|p| p.publisher.as_ref())
                                    .filter(|p| !p.trim().is_empty()),
                                |this, pub_name| {
                                    this.child(self.render_field_row(
                                        t(I18nKey::Publisher, lang),
                                        pub_name,
                                        "publisher",
                                        theme,
                                        cx,
                                    ))
                                },
                            )
                            .when_some(literature.doi.clone(), |this, doi| {
                                if doi.trim().is_empty() {
                                    this
                                } else {
                                    let url = if doi.starts_with("http") {
                                        doi.clone()
                                    } else {
                                        format!("https://doi.org/{doi}")
                                    };
                                    this.child(self.render_link_row(
                                        t(I18nKey::Doi, lang),
                                        &doi,
                                        &url,
                                        "doi",
                                        theme,
                                        cx,
                                    ))
                                }
                            })
                            .when_some(literature.arxiv_id.clone(), |this, id| {
                                if id.trim().is_empty() {
                                    this
                                } else {
                                    let url = format!("https://arxiv.org/abs/{id}");
                                    this.child(self.render_link_row(
                                        t(I18nKey::ArXiv, lang),
                                        &id,
                                        &url,
                                        "arxiv",
                                        theme,
                                        cx,
                                    ))
                                }
                            })
                            .when_some(literature.url.clone(), |this, url| {
                                if url.trim().is_empty() {
                                    this
                                } else {
                                    this.child(self.render_link_row(
                                        t(I18nKey::Url, lang),
                                        &url,
                                        &url,
                                        "url",
                                        theme,
                                        cx,
                                    ))
                                }
                            })
                            .when(!buffer.abstract_display.is_empty(), |this| {
                                let abstract_text =
                                    literature.abstract_text.clone().unwrap_or_default();
                                this.child(
                                    CollapsibleText::new(
                                        t(I18nKey::Abstract, lang),
                                        buffer.abstract_display.clone(),
                                        self.abstract_expanded,
                                        self.copied_field.as_ref() == Some(&"abstract".to_string()),
                                        (t(I18nKey::Expand, lang), t(I18nKey::Collapse, lang)),
                                        cx.listener(|this, _, _window, cx| {
                                            this.toggle_abstract(cx);
                                        }),
                                        cx.listener({
                                            let val = abstract_text.clone();
                                            move |this, _, window, cx| {
                                                this.copy_text(
                                                    val.clone(),
                                                    "abstract".to_string(),
                                                    window.window_handle(),
                                                    cx,
                                                );
                                            }
                                        }),
                                    )
                                    .on_double_click(cx.listener({
                                        let val = abstract_text.clone();
                                        move |this, _, window, cx| {
                                            this.copy_text(
                                                val.clone(),
                                                "abstract".to_string(),
                                                window.window_handle(),
                                                cx,
                                            );
                                        }
                                    }))
                                    .render(theme),
                                )
                            })
                            .child(self.render_tags_section(buffer, theme, cx))
                            .child(self.render_folders_section(buffer, theme, cx))
                            .child(self.render_citations_section(buffer, theme, cx))
                            .child(self.render_notes_section(buffer, window, theme, cx))
                            .child(self.render_files(literature, theme)),
                    ),
            )
            .when(self.is_dragging, |this| {
                let lit_id = lit_id.clone();
                this.child(self.render_drop_zone(&lit_id, lang, theme, cx))
            })
            .into_any_element()
    }

    fn render_drop_zone(
        &self,
        lit_id: &str,
        lang: Language,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let surface = surface(cx);
        let app = self.app.clone();
        let lit_id_main = lit_id.to_string();
        let lit_id_att = lit_id.to_string();
        let app_main = app.clone();
        let app_att = app.clone();

        div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .h(rems(5.0))
            .bg(surface.drop_overlay)
            .border_t_1()
            .border_dashed()
            .border_color(theme.border)
            .flex()
            .gap_2()
            .p_2()
            .child(
                div()
                    .id("drop-main-file")
                    .flex_1()
                    .h_full()
                    .border_2()
                    .border_dashed()
                    .border_color(theme.border)
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(t(I18nKey::SetAsMainFile, lang)),
                    )
                    .on_drop(cx.listener({
                        let app = app_main.clone();
                        let lit_id = lit_id_main.clone();
                        let _parent = self.parent_view.clone();
                        move |this, paths: &ExternalPaths, _window, cx| {
                            this.is_dragging = false;
                            if let Some(path) = paths.paths().first()
                                && let Err(e) = app.import_file_to_literature(&lit_id, path, true)
                            {
                                error!("Failed to import main file: {e}");
                                show_notification(
                                    NotificationType::Error,
                                    format!("{}: {}", t(I18nKey::ImportFailed, lang), e),
                                    cx,
                                );
                            }
                            cx.notify();
                        }
                    })),
            )
            .child(
                div()
                    .id("drop-attachment")
                    .flex_1()
                    .h_full()
                    .border_2()
                    .border_dashed()
                    .border_color(theme.border)
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(t(I18nKey::SetAsAttachment, lang)),
                    )
                    .on_drop(cx.listener({
                        let app = app_att.clone();
                        let lit_id = lit_id_att.clone();
                        let _parent = self.parent_view.clone();
                        move |this, paths: &ExternalPaths, _window, cx| {
                            this.is_dragging = false;
                            if let Some(path) = paths.paths().first()
                                && let Err(e) = app.import_file_to_literature(&lit_id, path, false)
                            {
                                error!("Failed to import attachment: {e}");
                                show_notification(
                                    NotificationType::Error,
                                    format!("{}: {}", t(I18nKey::ImportFailed, lang), e),
                                    cx,
                                );
                            }
                            cx.notify();
                        }
                    })),
            )
    }

    fn render_files(&self, literature: &Literature, theme: &Theme) -> impl IntoElement {
        let parent_view = self.parent_view.clone();

        // 1. Calculate stable numbering mapping based on the complete list
        let file_labels = models::Attachment::compute_labels(&literature.attachments);

        let mut main_elements = Vec::new();
        let mut attachment_elements = Vec::new();

        for file in &literature.attachments {
            let path_exists = Path::new(&file.file_path).exists();
            if !path_exists {
                continue;
            }
            let display_ext = file_labels.get(&file.id).cloned().unwrap_or_else(|| {
                Path::new(&file.file_name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("FILE")
                    .to_uppercase()
            });

            let att_id = file.id.clone();
            let att_id_right = file.id.clone();
            let app = self.app.clone();
            let data_store = self.data_store.clone();
            let parent = parent_view.clone();
            let file_path = file.file_path.clone();
            let file_path_pdf = file.file_path.clone();
            let parent_left = parent.clone();
            let parent_right = parent.clone();

            let badge = div()
                .text_xs()
                .bg(if file.is_main {
                    theme.primary.opacity(0.15)
                } else {
                    theme.muted
                })
                .text_color(if file.is_main {
                    theme.primary
                } else {
                    theme.muted_foreground
                })
                .px_1p5()
                .py_0p5()
                .rounded_sm()
                .when(file.is_main, |s| s.font_weight(FontWeight::BOLD))
                .cursor_pointer()
                .on_mouse_up(MouseButton::Left, {
                    let app = app.clone();
                    let file_path = file_path.clone();
                    let data_store = data_store.clone();
                    let parent_left = parent_left.clone();
                    let att_id = att_id.clone();
                    let file_path_pdf = file_path_pdf.clone();
                    move |_, _window, cx| {
                        cx.stop_propagation();
                        if !app.should_use_external_viewer(&file_path) {
                            if let Some(lit) = data_store
                                .read(cx)
                                .literatures
                                .iter()
                                .find(|l| l.attachments.iter().any(|a| a.id == att_id))
                                .cloned()
                                && let Some(parent) =
                                    parent_left.as_ref().and_then(gpui::WeakEntity::upgrade)
                            {
                                parent.update(cx, |mw, cx| {
                                    mw.open_pdf_viewer_with_path(
                                        lit,
                                        Some(PathBuf::from(&file_path_pdf)),
                                        cx,
                                    );
                                });
                            }
                        } else {
                            let _ = app.open_attachment(&att_id);
                        }
                    }
                })
                .on_mouse_down(MouseButton::Right, {
                    let att_id = att_id_right.clone();
                    move |event: &gpui::MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        if let Some(mw) = parent_right.as_ref().and_then(gpui::WeakEntity::upgrade)
                        {
                            mw.update(cx, |mw, cx| {
                                mw.show_context_menu(
                                    event.position,
                                    ContextMenuType::Attachment(att_id.clone()),
                                    window,
                                    cx,
                                );
                            });
                        }
                    }
                })
                .child(display_ext);

            if file.is_main {
                main_elements.push(badge.into_any_element());
            } else {
                attachment_elements.push(badge.into_any_element());
            }
        }

        let mut all_elements = Vec::new();
        all_elements.extend(main_elements);
        all_elements.extend(attachment_elements);

        if all_elements.is_empty() {
            return div().into_any_element();
        }

        div()
            .mt_3()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(all_elements)
            .into_any_element()
    }
}
