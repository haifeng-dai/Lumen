use super::super::MainWindow;
use crate::ui::notification::show_notification;
use components::IconName;
use gpui::prelude::*;
use gpui::{WeakEntity, Window};
use gpui_component::notification::NotificationType;
use gpui_component::{
    ActiveTheme, Icon,
    menu::{PopupMenu, PopupMenuItem},
};
use i18n::{I18nKey, Language, t};
use models::FetchSource;
use parser::export::ExportFormat;
use std::sync::Arc;

use super::{
    BatchSource, FolderSelectClosure, LiteraturePrefetch, build_folder_level, copy_citation,
    danger_menu_item,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn build_literature_menu(
    menu: PopupMenu,
    lit_id: String,
    literature_prefetch: Option<LiteraturePrefetch>,
    current_selected_folder: Option<String>,
    window: &mut Window,
    this_weak: WeakEntity<MainWindow>,
    lang: Language,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let mut menu = menu;
    // 使用提前预取的数据，避免闭包内二次借用 cx
    let (selected_count, selected_ids, in_trash, lit, (custom_name_map, custom_children_map)) =
        literature_prefetch.clone().unwrap_or_default();
    if in_trash {
        // 1. 还原到
        let this_weak_clone = this_weak.clone();
        let lit_id_restore = lit_id.clone();
        let sel_ids = selected_ids.clone();
        let custom_nm = custom_name_map.clone();
        let custom_cm = custom_children_map.clone();

        let restore_submenu = PopupMenu::build(window, cx, move |mut m, window, cx| {
            let this_weak_inner = this_weak_clone.clone();
            let lit_id_inner = lit_id_restore.clone();
            let sel_ids_inner = sel_ids.clone();
            m = m.item(
                PopupMenuItem::new(t(I18nKey::AllLiterature, lang)).on_click(
                    move |_, _window, cx| {
                        if let Some(this) = this_weak_inner.upgrade() {
                            this.update(cx, |this, cx| {
                                let _ = this.app.smart_restore_literatures(
                                    &lit_id_inner,
                                    None,
                                    &sel_ids_inner,
                                );
                                this.close_menus(cx);
                            });
                        }
                    },
                ),
            );

            let on_select: FolderSelectClosure = Arc::new({
                let this_weak_tree = this_weak_clone.clone();
                let lit_id_tree = lit_id_restore.clone();
                let sel_ids_tree = sel_ids.clone();
                move |folder_id, _window, cx| {
                    if let Some(this) = this_weak_tree.upgrade() {
                        this.update(cx, |this, cx| {
                            let _ = this.app.smart_restore_literatures(
                                &lit_id_tree,
                                Some(folder_id),
                                &sel_ids_tree,
                            );
                            this.close_menus(cx);
                        });
                    }
                }
            });
            m = build_folder_level(m, None, &custom_nm, &custom_cm, &on_select, window, cx);
            m
        });

        menu = menu.item(
            PopupMenuItem::submenu(t(I18nKey::RestoreTo, lang), restore_submenu)
                .icon(Icon::new(IconName::Undo)),
        );

        menu = menu.separator();

        // 2. 永久删除
        let this_weak_clone = this_weak.clone();
        let lit_id_delete = lit_id.clone();
        let sel_ids = selected_ids.clone();
        menu = menu.item(
            danger_menu_item(
                cx.theme().danger,
                t(I18nKey::PermanentDelete, lang),
                IconName::Trash,
            )
            .on_click(move |_, _window, cx| {
                if let Some(this) = this_weak_clone.upgrade() {
                    this.update(cx, |this, cx| {
                        let _ = this.app.smart_delete_literature(&lit_id_delete, &sel_ids);
                        this.close_menus(cx);
                    });
                }
            }),
        );
    } else {
        // ==================== 第一菜单组：修改、查看等一级菜单 ====================
        if selected_count <= 1 {
            // 1. 编辑文献
            let this_weak_clone = this_weak.clone();
            let lit_id_edit = lit_id.clone();
            menu = menu.item(
                PopupMenuItem::new(t(I18nKey::Edit, lang))
                    .icon(Icon::new(IconName::Edit))
                    .on_click(move |_, _window, cx| {
                        if let Some(this) = this_weak_clone.upgrade() {
                            this.update(cx, |this, cx| {
                                this.open_edit_modal(Some(lit_id_edit.clone()), cx);
                                this.close_menus(cx);
                            });
                        }
                    }),
            );

            // 3. 从...获取元数据 (包含 ArXiv / DBLP / DOI / OpenAlex 原生二级子菜单)
            if let Some(lit) = &lit {
                let lit_clone = lit.clone();
                let this_weak_clone = this_weak.clone();
                let fetch_submenu = PopupMenu::build(window, cx, move |mut m, _window, _cx| {
                    // 2.1 ArXiv
                    let this_weak_inner = this_weak_clone.clone();
                    let lit_inner = lit_clone.clone();
                    m = m.item(PopupMenuItem::new("ArXiv").on_click(move |_, window, cx| {
                        if let Some(this) = this_weak_inner.upgrade() {
                            this.update(cx, |this, cx| {
                                let arxiv_id = super::super::utils::extract_arxiv_id(&lit_inner);
                                if let Some(id) = arxiv_id {
                                    this.start_fetch_and_compare(
                                        std::sync::Arc::new(lit_inner.clone()),
                                        FetchSource::ArXiv(id),
                                        window,
                                        cx,
                                    );
                                } else {
                                    show_notification(
                                        NotificationType::Error,
                                        t(I18nKey::FetchFailed, lang),
                                        cx,
                                    );
                                }
                                this.close_menus(cx);
                            });
                        }
                    }));

                    // 2.2 DBLP
                    let this_weak_inner = this_weak_clone.clone();
                    let lit_inner = lit_clone.clone();
                    m = m.item(PopupMenuItem::new("DBLP").on_click(move |_, window, cx| {
                        if let Some(this) = this_weak_inner.upgrade() {
                            this.update(cx, |this, cx| {
                                if lit_inner.title.is_empty() {
                                    show_notification(
                                        NotificationType::Error,
                                        t(I18nKey::FetchFailed, lang),
                                        cx,
                                    );
                                } else {
                                    this.start_fetch_and_compare(
                                        std::sync::Arc::new(lit_inner.clone()),
                                        FetchSource::Dblp(lit_inner.title.clone()),
                                        window,
                                        cx,
                                    );
                                }
                                this.close_menus(cx);
                            });
                        }
                    }));

                    // 2.3 DOI
                    let this_weak_inner = this_weak_clone.clone();
                    let lit_inner = lit_clone.clone();
                    m = m.item(PopupMenuItem::new("DOI").on_click(move |_, window, cx| {
                        if let Some(this) = this_weak_inner.upgrade() {
                            this.update(cx, |this, cx| {
                                let doi_opt = lit_inner.doi.clone();
                                if let Some(id) = doi_opt {
                                    this.start_fetch_and_compare(
                                        std::sync::Arc::new(lit_inner.clone()),
                                        FetchSource::Doi(id),
                                        window,
                                        cx,
                                    );
                                } else {
                                    show_notification(
                                        NotificationType::Error,
                                        t(I18nKey::FetchFailed, lang),
                                        cx,
                                    );
                                }
                                this.close_menus(cx);
                            });
                        }
                    }));

                    // 2.4 OpenAlex
                    let this_weak_inner = this_weak_clone.clone();
                    let lit_inner = lit_clone.clone();
                    m = m.item(
                        PopupMenuItem::new("OpenAlex").on_click(move |_, window, cx| {
                            if let Some(this) = this_weak_inner.upgrade() {
                                this.update(cx, |this, cx| {
                                    this.start_fetch_openalex(
                                        std::sync::Arc::new(lit_inner.clone()),
                                        window,
                                        cx,
                                    );
                                    this.close_menus(cx);
                                });
                            }
                        }),
                    );

                    m
                });

                menu = menu.item(
                    PopupMenuItem::submenu(t(I18nKey::FetchFrom, lang), fetch_submenu)
                        .icon(Icon::new(IconName::Cloud)),
                );
            }
        }

        // 3. 复制引用 (二级子菜单: BibTeX / IEEE / 爱思微尔)
        let this_weak_clone = this_weak.clone();
        let sel_ids = selected_ids.clone();
        let citation_submenu = PopupMenu::build(window, cx, move |mut m, _window, _cx| {
            // BibTeX
            let this_weak_inner = this_weak_clone.clone();
            let sel_ids_inner = sel_ids.clone();
            m = m.item(
                PopupMenuItem::new(t(I18nKey::CitationBibTeX, lang)).on_click(
                    move |_, _window, cx| {
                        if let Some(this) = this_weak_inner.upgrade() {
                            this.update(cx, |this, cx| {
                                copy_citation(
                                    &this.app,
                                    &sel_ids_inner,
                                    ExportFormat::BibTeX,
                                    lang,
                                    cx,
                                );
                                this.close_menus(cx);
                            });
                        }
                    },
                ),
            );
            // IEEE
            let this_weak_inner = this_weak_clone.clone();
            let sel_ids_inner = sel_ids.clone();
            m = m.item(PopupMenuItem::new(t(I18nKey::CitationIeee, lang)).on_click(
                move |_, _window, cx| {
                    if let Some(this) = this_weak_inner.upgrade() {
                        this.update(cx, |this, cx| {
                            copy_citation(&this.app, &sel_ids_inner, ExportFormat::IEEE, lang, cx);
                            this.close_menus(cx);
                        });
                    }
                },
            ));
            // Elsevier
            let this_weak_inner = this_weak_clone.clone();
            let sel_ids_inner = sel_ids.clone();
            m = m.item(
                PopupMenuItem::new("Elsevier").on_click(move |_, _window, cx| {
                    if let Some(this) = this_weak_inner.upgrade() {
                        this.update(cx, |this, cx| {
                            copy_citation(
                                &this.app,
                                &sel_ids_inner,
                                ExportFormat::Elsevier,
                                lang,
                                cx,
                            );
                            this.close_menus(cx);
                        });
                    }
                }),
            );
            m
        });

        menu = menu.item(
            PopupMenuItem::submenu(t(I18nKey::CopyCitation, lang), citation_submenu)
                .icon(Icon::new(IconName::Copy)),
        );

        // ==================== 第二菜单组：级联文件夹管理 ====================
        // 5. 添加到（级联文件夹菜单，多级树，不限层级）
        if !custom_name_map.is_empty() {
            let custom_nm = custom_name_map.clone();
            let custom_cm = custom_children_map.clone();
            let this_weak_clone = this_weak.clone();
            let lit_id_add = lit_id.clone();
            let sel_ids = selected_ids.clone();

            let on_select: FolderSelectClosure = Arc::new({
                let this_weak_tree = this_weak_clone.clone();
                let lit_id_tree = lit_id_add.clone();
                let sel_ids_tree = sel_ids.clone();
                move |folder_id, _window, cx| {
                    if let Some(this) = this_weak_tree.upgrade() {
                        this.update(cx, |this, cx| {
                            let _ = this.app.smart_add_literatures_to_folder(
                                &lit_id_tree,
                                folder_id,
                                &sel_ids_tree,
                            );
                            this.close_menus(cx);
                        });
                    }
                }
            });

            let add_submenu = PopupMenu::build(window, cx, move |mut m, window, cx| {
                m = build_folder_level(m, None, &custom_nm, &custom_cm, &on_select, window, cx);
                m
            });

            menu = menu.item(
                PopupMenuItem::submenu(t(I18nKey::AddTo, lang), add_submenu)
                    .icon(Icon::new(IconName::Folder)),
            );
        }

        // 6. 从文件夹中移除
        if let Some(folder_id) = &current_selected_folder
            && folder_id != "all"
            && folder_id != "uncategorized"
            && folder_id != "trash"
        {
            let this_weak_clone = this_weak.clone();
            let lit_id_remove = lit_id.clone();
            let folder_id_clone = folder_id.clone();
            let sel_ids = selected_ids.clone();
            menu = menu.separator();
            menu = menu.item(
                danger_menu_item(
                    cx.theme().danger,
                    t(I18nKey::RemoveFromFolder, lang),
                    IconName::FolderOpen,
                )
                .on_click(move |_, _window, cx| {
                    if let Some(this) = this_weak_clone.upgrade() {
                        this.update(cx, |this, cx| {
                            let _ = this.app.smart_remove_literatures_from_folder(
                                &lit_id_remove,
                                &folder_id_clone,
                                &sel_ids,
                            );
                            this.close_menus(cx);
                        });
                    }
                }),
            );
        }
        // ==================== 第三菜单组：删除与批量元数据获取 ====================
        menu = menu.separator();

        // 7. 批量获取元数据
        if selected_count > 1 {
            let this_weak_clone = this_weak.clone();
            let sel_ids = selected_ids.clone();
            let batch_submenu = PopupMenu::build(window, cx, move |mut m, _window, _cx| {
                // 7.1 ArXiv 批量
                let this_weak_inner = this_weak_clone.clone();
                let sel_ids_inner = {
                    let mut hs = Vec::new();
                    hs.extend(sel_ids.clone());
                    hs
                };
                m = m.item(PopupMenuItem::new("ArXiv").on_click(move |_, _window, cx| {
                    if let Some(this) = this_weak_inner.upgrade() {
                        this.update(cx, |this, cx| {
                            let mut items = Vec::new();
                            items.extend(sel_ids_inner.clone());
                            this.handle_batch_fetch_metadata(items, BatchSource::ArXiv, cx);
                            this.close_menus(cx);
                        });
                    }
                }));

                // 7.2 Crossref 批量
                let this_weak_inner = this_weak_clone.clone();
                let sel_ids_inner = {
                    let mut hs = Vec::new();
                    hs.extend(sel_ids.clone());
                    hs
                };
                m = m.item(
                    PopupMenuItem::new("Crossref").on_click(move |_, _window, cx| {
                        if let Some(this) = this_weak_inner.upgrade() {
                            this.update(cx, |this, cx| {
                                let mut items = Vec::new();
                                items.extend(sel_ids_inner.clone());
                                this.handle_batch_fetch_metadata(items, BatchSource::Doi, cx);
                                this.close_menus(cx);
                            });
                        }
                    }),
                );

                // 7.3 OpenAlex 批量
                let this_weak_inner = this_weak_clone.clone();
                let sel_ids_inner = {
                    let mut hs = Vec::new();
                    hs.extend(sel_ids.clone());
                    hs
                };
                m = m.item(
                    PopupMenuItem::new("OpenAlex").on_click(move |_, _window, cx| {
                        if let Some(this) = this_weak_inner.upgrade() {
                            this.update(cx, |this, cx| {
                                let mut items = Vec::new();
                                items.extend(sel_ids_inner.clone());
                                this.handle_batch_fetch_metadata(items, BatchSource::OpenAlex, cx);
                                this.close_menus(cx);
                            });
                        }
                    }),
                );
                m
            });

            menu = menu.item(
                PopupMenuItem::submenu(t(I18nKey::BatchFetchMetadata, lang), batch_submenu)
                    .icon(Icon::new(IconName::Cloud)),
            );
        }

        // 8. 删除
        let this_weak_clone = this_weak.clone();
        let lit_id_delete = lit_id.clone();
        let sel_ids = selected_ids.clone();
        let delete_label = t(I18nKey::Delete, lang);
        menu = menu.item(
            danger_menu_item(cx.theme().danger, delete_label, IconName::Trash).on_click(
                move |_, _window, cx| {
                    if let Some(this) = this_weak_clone.upgrade() {
                        this.update(cx, |this, cx| {
                            let _ = this.app.smart_delete_literature(&lit_id_delete, &sel_ids);
                            this.close_menus(cx);
                        });
                    }
                },
            ),
        );
    }
    menu
}
