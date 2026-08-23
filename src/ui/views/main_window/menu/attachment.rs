use super::super::MainWindow;
use crate::RUNTIME;
use crate::ui::notification::show_notification;
use anyhow::{Error, anyhow};
use components::IconName;
use gpui::prelude::*;
use gpui::{AsyncApp, PathPromptOptions, WeakEntity};
use gpui_component::notification::NotificationType;
use gpui_component::{
    ActiveTheme, Icon,
    menu::{PopupMenu, PopupMenuItem},
};
use i18n::{I18nKey, Language, t};
use log::error;

use super::danger_menu_item;

pub(super) fn build_attachment_menu(
    menu: PopupMenu,
    att_id: String,
    attachment_lit_data: Option<(String, bool, String)>,
    this_weak: WeakEntity<MainWindow>,
    lang: Language,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let mut menu = menu;
    // 获取附件对应文献ID以及是否是主文件（已在 build_context_menu 外提前读取）
    let cached_lit_data = attachment_lit_data.clone();

    // 1. 更换文件
    let this_weak_clone = this_weak.clone();
    let att_id_change = att_id.clone();
    let cached_lit_data_clone = cached_lit_data.clone();
    menu = menu.item(
        PopupMenuItem::new(t(I18nKey::ReplaceFile, lang))
            .icon(Icon::new(IconName::Edit))
            .on_click(move |_, _window, cx| {
                if let Some(this) = this_weak_clone.upgrade() {
                    let app = this.read(cx).app.clone();
                    let att_id = att_id_change.clone();
                    let cached_lit_data = cached_lit_data_clone.clone();

                    // 弹窗选择文件
                    let receiver = cx.prompt_for_paths(PathPromptOptions {
                        files: true,
                        directories: false,
                        multiple: false,
                        prompt: Some(t(I18nKey::SelectNewFile, lang).into()),
                    });

                    // 使用 Runtime 开启后台文件导入
                    RUNTIME.spawn(async move {
                        if let Ok(Ok(Some(paths))) = receiver.await
                            && let Some(path) = paths.first().cloned()
                        {
                            let result = (|| {
                                let (lit_id, is_main, _) = cached_lit_data
                                    .ok_or_else(|| anyhow!("Literature not found"))?;

                                if is_main {
                                    app.import_file_to_literature(&lit_id, &path, true)?;
                                } else {
                                    app.delete_attachment_file(&att_id)?;
                                    app.import_file_to_literature(&lit_id, &path, false)?;
                                }
                                Ok::<(), Error>(())
                            })();

                            if let Err(e) = result {
                                error!("更换文件失败: {e}");
                            }
                        }
                    });

                    this.update(cx, |this, cx| {
                        this.close_menus(cx);
                    });
                }
            }),
    );

    // 1.5 在 Finder/Explorer 中显示
    if let Some((_, _, ref file_path)) = cached_lit_data {
        let path_clone = file_path.clone();
        let this_weak_ret = this_weak.clone();
        menu = menu.item(
            PopupMenuItem::new(t(I18nKey::OpenPath, lang))
                .icon(Icon::new(IconName::FolderOpen))
                .on_click(move |_, _window, cx| {
                    cx.reveal_path(std::path::Path::new(&path_clone));
                    if let Some(this) = this_weak_ret.upgrade() {
                        this.update(cx, |this, cx| {
                            this.close_menus(cx);
                        });
                    }
                }),
        );

        // 1.8 导出带批注 PDF
        if let Some((ref lit_id, _, _)) = cached_lit_data {
            let lit_id = lit_id.clone();
            let src_path = std::path::PathBuf::from(file_path);
            let att_id_export = att_id.clone();
            let this_weak_export = this_weak.clone();
            menu = menu.item(
                PopupMenuItem::new(t(I18nKey::Export, lang))
                    .icon(Icon::new(IconName::Download))
                    .on_click(move |_, _window, cx| {
                        let lang = lang;
                        if let Some(this) = this_weak_export.upgrade() {
                            let app = this.read(cx).app.clone();
                            let att_id = att_id_export.clone();
                            let lit_id = lit_id.clone();
                            let src_path = src_path.clone();

                            // 联合查询所有可能的 document_id 键并去重
                            let keys_to_try = [
                                format!("{}::{}", lit_id, att_id),
                                att_id.clone(),
                                lit_id.clone(),
                            ];
                            let mut valid_annotations = Vec::new();
                            let mut seen_ids = std::collections::HashSet::new();

                            for key in &keys_to_try {
                                if let Ok(anns) = app.db.load_annotations(key) {
                                    for a in anns {
                                        if !a.is_deleted && seen_ids.insert(a.id.clone()) {
                                            valid_annotations.push(a);
                                        }
                                    }
                                }
                            }

                            if valid_annotations.is_empty() {
                                show_notification(
                                    NotificationType::Warning,
                                    t(I18nKey::ExportAnnotatedPdfNoAnnotations, lang),
                                    cx,
                                );
                                this.update(cx, |this, cx| {
                                    this.close_menus(cx);
                                });
                                return;
                            }

                            let stem = src_path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("document")
                                .to_string();
                            let suggested_name = format!("{}_annotated.pdf", stem);
                            let dir = src_path
                                .parent()
                                .unwrap_or_else(|| std::path::Path::new("."));

                            let receiver =
                                cx.prompt_for_new_path(dir, Some(suggested_name.as_str()));

                            cx.spawn(move |cx: &mut AsyncApp| {
                                let cx = cx.clone();
                                async move {
                                    if let Ok(Ok(Some(dest_path))) = receiver.await {
                                        let res = services::pdf::export_annotated_pdf(
                                            &src_path,
                                            &dest_path,
                                            &valid_annotations,
                                        );
                                        let (notif_type, text) = match res {
                                            Ok(()) => (
                                                NotificationType::Success,
                                                t(I18nKey::ExportAnnotatedPdfSuccess, lang)
                                                    .to_string(),
                                            ),
                                            Err(e) => (
                                                NotificationType::Error,
                                                format!(
                                                    "{}: {}",
                                                    t(I18nKey::ExportAnnotatedPdfFailed, lang),
                                                    e
                                                ),
                                            ),
                                        };
                                        cx.update(|app| {
                                            show_notification(notif_type, text, app);
                                        });
                                    }
                                }
                            })
                            .detach();

                            this.update(cx, |this, cx| {
                                this.close_menus(cx);
                            });
                        }
                    }),
            );
        }
    }

    // 2. 删除附件
    let this_weak_clone = this_weak.clone();
    let att_id_delete = att_id.clone();
    menu = menu.item(
        danger_menu_item(
            cx.theme().danger,
            t(I18nKey::DeleteFile, lang),
            IconName::Trash,
        )
        .on_click(move |_, _window, cx| {
            if let Some(this) = this_weak_clone.upgrade() {
                let att_id = att_id_delete.clone();
                let app = this.read(cx).app.clone();
                let _ = app.delete_attachment_file(&att_id);
                this.update(cx, |this, cx| {
                    this.close_menus(cx);
                });
            }
        }),
    );
    menu
}
