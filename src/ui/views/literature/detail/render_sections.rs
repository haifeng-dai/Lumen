use crate::app_state::theme::surface;
use crate::ui::components::render_icon_button;
use components::IconName;
use gpui::prelude::*;
use gpui::{ClickEvent, MouseButton, SharedString, Window, div, rems};
use gpui_component::{
    Colorize, Icon, Theme, h_flex,
    input::{InputState, TextareaState},
    v_flex,
};
use i18n::{I18nKey, Language, t};
use models::Literature;
use models::theme::ResolvedSurface;

use super::SingleDetailBuffer;

impl super::LiteratureDetailView {
    fn render_folder_paths(
        &self,
        buffer: &SingleDetailBuffer,
        theme: &Theme,
        lang: Language,
    ) -> impl IntoElement {
        let folder_paths = buffer.folder_paths.clone();
        let list: Vec<Vec<String>> = if folder_paths.is_empty() {
            vec![vec![t(I18nKey::Uncategorized, lang).to_string()]]
        } else {
            folder_paths
        };

        v_flex()
            .gap_1p5()
            .children(list.into_iter().enumerate().map(|(idx, path)| {
                let path_len = path.len();
                h_flex()
                    .id(("folder-path", idx))
                    .gap_1()
                    .items_start()
                    .child(
                        div().h(rems(0.9)).flex().items_center().child(
                            Icon::new(IconName::Folder)
                                .size(rems(0.75))
                                .text_color(theme.muted_foreground),
                        ),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_center()
                            .gap_1()
                            .line_height(rems(0.9))
                            .children(path.into_iter().enumerate().map(|(p_idx, name)| {
                                h_flex()
                                    .items_center()
                                    .child(div().text_xs().text_color(theme.foreground).child(name))
                                    .when(p_idx < path_len - 1, |this| {
                                        this.child(
                                            Icon::new(IconName::ChevronRight)
                                                .size(rems(0.625))
                                                .text_color(theme.muted_foreground),
                                        )
                                    })
                            })),
                    )
            }))
    }

    pub(super) fn render_tags_section(
        &self,
        buffer: &SingleDetailBuffer,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app = self.app.clone();
        let lit_id = buffer.literature.id.clone();
        let current_tags: Vec<String> = buffer.tags.iter().map(|t| t.name.clone()).collect();
        let lit_id_selector = buffer.literature.id.clone();
        let app_selector = self.app.clone();
        let lang = self.app.current_language();

        let mut tags = buffer.tags.clone();
        tags.sort_by_key(|a| a.name.to_lowercase());

        v_flex()
            .group("row_group")
            .gap_2()
            .mt_2()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(t(I18nKey::Tags, lang)),
                    )
                    .child(render_icon_button(
                        "add-tag-btn",
                        IconName::Plus,
                        theme.muted_foreground,
                        theme,
                        {
                            cx.listener(move |this, event: &ClickEvent, window, cx| {
                                if let Some(parent) = &this.parent_view {
                                    let app_sel = app_selector.clone();
                                    let lit_id_sel = lit_id_selector.clone();
                                    let tags = current_tags.clone();
                                    let _ = parent.update(cx, move |parent, cx| {
                                        parent.open_tag_selector(
                                            tags,
                                            move |tag_name, _window, _cx| {
                                                let notify = {
                                                    let a = app_sel.clone();
                                                    move || a.notify_data_changed()
                                                };
                                                let _ = app_sel.tag_service.add_tag_to_literature(
                                                    &app_sel.db,
                                                    notify,
                                                    &lit_id_sel,
                                                    &tag_name,
                                                );
                                            },
                                            event.position(),
                                            window,
                                            cx,
                                        );
                                    });
                                }
                                cx.notify();
                            })
                        },
                    )),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_1()
                    .items_center()
                    .children(tags.iter().map(|tag| {
                        let tag_name = tag.name.clone();
                        let lit_id = lit_id.clone();
                        let app = app.clone();
                        let color = tag.color.clone();
                        let tag_color =
                            gpui::Hsla::parse_hex(&color).unwrap_or(theme.muted_foreground);
                        let tag_group = SharedString::from(format!("tag-item-{tag_name}"));

                        h_flex()
                            .group(tag_group.clone())
                            .items_center()
                            .gap_1p5()
                            .rounded_full()
                            .px_2()
                            .py_0p5()
                            .bg(tag_color.opacity(0.15))
                            .child(div().size(rems(0.5)).rounded_full().bg(tag_color))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(tag_color)
                                    .child(tag_name.clone()),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("remove-tag-{tag_name}")))
                                    .cursor_pointer()
                                    .opacity(0.0)
                                    .group_hover(tag_group.clone(), |s| s.opacity(1.0))
                                    .child(
                                        Icon::new(IconName::Close)
                                            .size(rems(0.5))
                                            .text_color(tag_color),
                                    )
                                    .on_mouse_up(MouseButton::Left, move |_, _, _| {
                                        let notify = {
                                            let a = app.clone();
                                            move || a.notify_data_changed()
                                        };
                                        let _ = app.tag_service.remove_tag_from_literature(
                                            &app.db, notify, &lit_id, &tag_name,
                                        );
                                    }),
                            )
                    })),
            )
    }

    pub(super) fn render_folders_section(
        &self,
        buffer: &SingleDetailBuffer,
        theme: &Theme,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let lang = self.app.current_language();

        v_flex()
            .group("row_group")
            .gap_2()
            .mt_2()
            .child(
                h_flex().justify_between().items_center().child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(t(I18nKey::Folders, lang)),
                ),
            )
            .child(self.render_folder_paths(buffer, theme, lang))
    }

    fn render_citation_row_static(
        &self,
        target_lit: &Literature,
        current_lit_id: &str,
        is_reference: bool,
        theme: &Theme,
        surface: &ResolvedSurface,
    ) -> impl IntoElement {
        let app = self.app.clone();
        let target_id = target_lit.id.clone();
        let source_id = if is_reference {
            current_lit_id.to_string()
        } else {
            target_lit.id.clone()
        };
        let target_id_for_removal = if is_reference {
            target_lit.id.clone()
        } else {
            current_lit_id.to_string()
        };
        let app_for_remove = app.clone();

        let this_view = self.parent_view.clone();

        div()
            .group("citation-row")
            .flex()
            .justify_between()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_sm()
            .hover(|s| s.bg(surface.hover_bg))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .cursor_pointer()
                    .on_mouse_up(MouseButton::Left, {
                        let target_id = target_id.clone();
                        let this_view = this_view.clone();
                        move |_, _, cx| {
                            if let Some(parent) =
                                this_view.as_ref().and_then(gpui::WeakEntity::upgrade)
                            {
                                parent.update(cx, |mw, cx| {
                                    mw.select_literature(target_id.clone(), cx);
                                });
                            }
                        }
                    })
                    .child(
                        Icon::new(IconName::FileSolid)
                            .size(rems(0.625))
                            .text_color(theme.muted_foreground)
                            .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(target_lit.title.clone()),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .cursor_pointer()
                    .opacity(1.0)
                    .child(
                        Icon::new(IconName::Close)
                            .size(rems(0.625))
                            .text_color(theme.danger),
                    )
                    .on_mouse_up(MouseButton::Left, {
                        let app_for_remove = app_for_remove.clone();
                        let source_id = source_id.clone();
                        let target_id_for_removal = target_id_for_removal.clone();
                        move |_, _, _cx| {
                            let _ = app_for_remove
                                .db
                                .remove_citation(&source_id, &target_id_for_removal);
                            app_for_remove.notify_data_changed();
                        }
                    }),
            )
    }

    pub(super) fn render_citations_section(
        &self,
        buffer: &SingleDetailBuffer,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let surface = surface(cx);
        let app = self.app.clone();
        let lit_id = buffer.literature.id.clone();
        let references = buffer.references.clone();
        let cited_by = buffer.cited_by.clone();
        let parent_view = self.parent_view.clone();
        let theme_clone = theme.clone();
        let lang = self.app.current_language();

        v_flex()
            .group("row_group")
            .gap_2()
            .mt_2()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(t(I18nKey::RelatedLiterature, lang)),
                    )
                    .child(render_icon_button(
                        "add-citation-btn",
                        IconName::Plus,
                        theme.muted_foreground,
                        theme,
                        cx.listener(move |_this, _, _window, cx| {
                            if let Some(parent) = &parent_view {
                                let app = app.clone();
                                let lit_id = lit_id.clone();
                                let _ = parent.update(cx, move |parent, cx| {
                                    parent.open_citation_selector(
                                        lit_id.clone(),
                                        move |target_id, _window, _cx| {
                                            let _ = app.db.add_citation(&lit_id, &target_id);
                                            app.notify_data_changed();
                                        },
                                        cx,
                                    );
                                });
                            }
                        }),
                    )),
            )
            .child(
                v_flex()
                    .gap_2()
                    .when(!references.is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme_clone.muted_foreground)
                                .child(format!(
                                    "{} · {}",
                                    t(I18nKey::References, lang),
                                    references.len()
                                )),
                        )
                        .children(references.iter().map(|lit| {
                            self.render_citation_row_static(
                                lit,
                                &buffer.literature.id,
                                true,
                                &theme_clone,
                                &surface,
                            )
                        }))
                    })
                    .when(!cited_by.is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme_clone.muted_foreground)
                                .mt_2()
                                .child(format!(
                                    "{} · {}",
                                    t(I18nKey::CitedBy, lang),
                                    cited_by.len()
                                )),
                        )
                        .children(cited_by.iter().map(|lit| {
                            self.render_citation_row_static(
                                lit,
                                &buffer.literature.id,
                                false,
                                &theme_clone,
                                &surface,
                            )
                        }))
                    }),
            )
    }

    pub(super) fn render_notes_section(
        &self,
        buffer: &SingleDetailBuffer,
        window: &mut Window,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let lang = self.app.current_language();
        let lit_id = buffer.literature.id.clone();

        let note_cards: Vec<gpui::AnyElement> = {
            let cache = self.notes_cache.clone();
            cache
                .iter()
                .enumerate()
                .map(|(i, note)| {
                    let note_id = note.id.clone();
                    let note_title = note.title.clone();
                    let note_content = note.content.clone();

                    let this_weak = cx.entity().downgrade();
                    let et = note_title.clone();
                    let ec = note_content.clone();
                    let note_id_edit = note_id.clone();
                    let on_edit =
                        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                            let _ = this_weak.update(cx, |this, cx| {
                                if let Some(current_idx) =
                                    this.notes_cache.iter().position(|n| n.id == note_id_edit)
                                {
                                    this.editing_note_index = Some(current_idx);
                                    let entity = cx
                                        .new(|cx| InputState::new(window, cx).placeholder("标题"));
                                    entity.update(cx, |s, cx| {
                                        s.set_value(&et, window, cx);
                                    });
                                    this.edit_note_title = Some(entity);
                                    let entity2 = cx.new(|cx| TextareaState::new(window, cx));
                                    entity2.update(cx, |s, cx| {
                                        s.set_value(&ec, window, cx);
                                    });
                                    this.edit_note_content = Some(entity2);
                                    cx.notify();
                                }
                            });
                        };

                    let this_weak = cx.entity().downgrade();
                    let note_id_del = note_id.clone();
                    let on_delete =
                        move |_: &gpui::ClickEvent, _window: &mut Window, cx: &mut gpui::App| {
                            let _ = this_weak.update(cx, |this, cx| {
                                let _ = this
                                    .app
                                    .literature_service
                                    .delete_note(&this.app.db, &note_id_del);
                                this.notes_cache.retain(|n| n.id != note_id_del);
                                this.app.notify_data_changed();
                                cx.notify();
                            });
                        };

                    let this_weak = cx.entity().downgrade();
                    let note_id_exp = note_id.clone();
                    let on_toggle_expand =
                        move |_: &gpui::ClickEvent, _window: &mut Window, cx: &mut gpui::App| {
                            let _ = this_weak.update(cx, |this, cx| {
                                if this.expanded_notes.contains(&note_id_exp) {
                                    this.expanded_notes.remove(&note_id_exp);
                                } else {
                                    this.expanded_notes.insert(note_id_exp.clone());
                                }
                                cx.notify();
                            });
                        };

                    let is_note_expanded = self.expanded_notes.contains(&note_id);

                    crate::pdf::components::right_sidebar::render_shared_note_card(
                        i,
                        note,
                        is_note_expanded,
                        theme.clone(),
                        window,
                        cx,
                        on_edit,
                        on_delete,
                        on_toggle_expand,
                    )
                    .into_any_element()
                })
                .collect()
        };

        v_flex()
            .group("row_group")
            .gap_2()
            .mt_2()
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(t(I18nKey::Notes, lang)),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(render_icon_button(
                                "ai-summary-btn",
                                IconName::Star,
                                if self.is_generating_summary {
                                    theme.primary
                                } else {
                                    theme.muted_foreground
                                },
                                theme,
                                cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    if this.is_generating_summary {
                                        return;
                                    }
                                    this.generate_ai_summary(window, cx);
                                }),
                            ))
                            .child(render_icon_button(
                                "add-note-btn",
                                IconName::Plus,
                                theme.muted_foreground,
                                theme,
                                cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    let title = "".to_string();
                                    let now = chrono::Utc::now().timestamp();
                                    this.notes_cache.push(models::LiteratureNote {
                                        id: "temp_new_note".to_string(),
                                        literature_id: lit_id.clone(),
                                        title,
                                        content: String::new(),
                                        sort_order: this.notes_cache.len() as i32,
                                        created_at: now,
                                        updated_at: now,
                                        is_deleted: false,
                                        is_dirty: false,
                                        version: 1,
                                    });
                                    this.editing_note_index = Some(this.notes_cache.len() - 1);
                                    this.edit_note_title = None;
                                    this.edit_note_content = None;
                                    cx.notify();
                                }),
                            )),
                    ),
            )
            .when(!note_cards.is_empty(), |this| this.children(note_cards))
    }
}
