use crate::ui::dialogs::{DuplicateListDialog, FieldSelection};
use crate::ui::notification::show_notification;
use gpui::prelude::*;
use gpui::{AppContext, Window, px, size};
use gpui_component::{WindowExt, notification::NotificationType};
use i18n::{I18nKey, t, tf};
use log::{error, info};
use models::Literature;
use std::sync::Arc;

impl super::super::MainWindow {
    pub fn run_duplicate_detection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let groups = self.app.find_duplicates();
        let lang = self.app.current_language();

        if groups.is_empty() {
            show_notification(
                NotificationType::Info,
                format!(
                    "{}: {}",
                    t(I18nKey::DuplicateGroups, lang),
                    t(I18nKey::NoDuplicatesFound, lang)
                ),
                cx,
            );
            return;
        }

        let app = self.app.clone();
        let this_weak = cx.entity().downgrade();
        let groups_clone = groups.clone();

        let entity = cx.new(|_| DuplicateListDialog::new(app.clone(), groups, false));
        let entity_weak = entity.downgrade();
        entity.update(cx, |dc, _| {
            dc.set_on_complete(Box::new(move |idx, w, cx| {
                w.close_dialog(cx);
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, cx| {
                        if let Some(idx) = idx {
                            let group = groups_clone[idx].clone();
                            this.start_merge_flow(group, cx);
                        }
                        cx.notify();
                    });
                }
            }));
        });
        self.duplicate_dialog = Some(entity.clone());

        window.open_dialog(cx, move |dialog, _, _cx| {
            let entity_weak_content = entity_weak.clone();
            dialog
                .w(px(600.))
                .title(t(I18nKey::DuplicateGroups, app.current_language()))
                .content(move |content, _, _cx| {
                    if let Some(e) = entity_weak_content.upgrade() {
                        content.child(e)
                    } else {
                        content
                    }
                })
        });
    }

    fn start_merge_flow(&mut self, mut group: Vec<Literature>, cx: &mut Context<Self>) {
        if group.len() < 2 {
            return;
        }

        // 智能推荐最佳主文件（附件多、元数据完备的优先）
        let best = group
            .iter()
            .enumerate()
            .max_by_key(|(_, lit)| {
                let pub_name = lit
                    .publication
                    .as_ref()
                    .map(|p| p.name.as_str())
                    .unwrap_or("");
                let meta_score = if lit.title.is_empty() { 0 } else { 1 }
                    + if lit.doi.is_some() { 1 } else { 0 }
                    + if !pub_name.is_empty() { 1 } else { 0 };
                lit.attachments.len() * 10 + meta_score
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
        if best != 0 {
            group.swap(0, best);
        }

        let original = group.remove(0);
        self.merge_next_in_group(Arc::new(original), group, cx);
    }

    fn merge_next_in_group(
        &mut self,
        original: Arc<Literature>,
        mut remaining: Vec<Literature>,
        cx: &mut Context<Self>,
    ) {
        if remaining.is_empty() {
            return;
        }

        let next_lit = remaining.remove(0);
        let next_lit_id = next_lit.id.clone();

        let selection = FieldSelection::compare(&original, &next_lit);

        if !selection.has_any_diff() {
            info!("查重合并: 发现完全一致的副本 {next_lit_id}, 正在自动合并并继续...");
            if let Err(e) = self
                .app
                .merge_literature_relations(&next_lit_id, &original.id)
            {
                error!("合并流程: 自动合并关联关系失败: {e}");
            }

            if let Err(e) = self.app.delete_literature_by_id(&next_lit_id) {
                error!("合并流程: 自动合并副本失败: {e}");
            }

            let original_clone = original.clone();
            let remaining_clone = remaining.clone();

            let lang = self.app.current_language();
            show_notification(
                NotificationType::Success,
                format!(
                    "{}: {}",
                    t(I18nKey::LiteratureMergedTitle, lang),
                    tf(I18nKey::LiteratureMergedMsg, lang, &[&next_lit.title])
                ),
                cx,
            );

            self.continue_merge_flow(original_clone, remaining_clone, cx);
            return;
        }

        let app = self.app.clone();
        let this_weak = cx.entity().downgrade();

        let diff = FieldSelection::compare(&original, &next_lit);
        let size = size(px(1100.0), px(750.0));

        self.open_modal_window(size, cx, move |_window, _cx| {
            let this_weak_cb = this_weak.clone();
            let app_cb = app.clone();
            let remaining_cb = remaining.clone();
            let original_cb = original.clone();
            let next_cb = next_lit.clone();
            let diff_cb = diff.clone();

            crate::ui::dialogs::MergeDialog::new(
                app.clone(),
                (*original_cb).clone(),
                next_cb.clone(),
                diff_cb,
                Box::new(move |result, window, cx| {
                    if let Some(this) = this_weak_cb.upgrade() {
                        if let Some(res) = result {
                            let master_id = &res.master_id;
                            let source_id = &res.source_id;
                            let sel = &res.selection;

                            let (master_lit, source_lit) = if master_id == &original_cb.id {
                                (original_cb.as_ref(), &next_cb)
                            } else {
                                (&next_cb, original_cb.as_ref())
                            };

                            // 应用字段选择
                            let mut merged = master_lit.clone();
                            if sel.literature_type {
                                merged.literature_type = source_lit.literature_type.clone();
                            }
                            if sel.title {
                                merged.title = source_lit.title.clone();
                            }
                            if sel.authors {
                                merged.authors = source_lit.authors.clone();
                            }
                            if sel.year {
                                merged.year = source_lit.year;
                            }
                            if sel.month {
                                merged.month = source_lit.month;
                            }
                            if sel.day {
                                merged.day = source_lit.day;
                            }
                            if sel.journal {
                                merged.publication = source_lit.publication.clone();
                            }
                            if sel.volume {
                                merged.volume = source_lit.volume.clone();
                            }
                            if sel.issue {
                                merged.issue = source_lit.issue.clone();
                            }
                            if sel.pages {
                                merged.pages = source_lit.pages.clone();
                            }
                            if sel.publisher
                                && let Some(ref pub_src) = source_lit.publication
                            {
                                if let Some(ref p) = merged.publication {
                                    let mut p2 = p.clone();
                                    p2.publisher = pub_src.publisher.clone();
                                    merged.publication = Some(p2);
                                } else {
                                    merged.publication = Some(pub_src.clone());
                                }
                            }
                            if sel.abstract_text {
                                merged.abstract_text = source_lit.abstract_text.clone();
                            }
                            if sel.doi {
                                merged.doi = source_lit.doi.clone();
                            }
                            if sel.arxiv_id {
                                merged.arxiv_id = source_lit.arxiv_id.clone();
                            }
                            if sel.url {
                                merged.url = source_lit.url.clone();
                            }

                            info!("合并流程: 确认合并。主文件={}, 源={}", master_id, source_id);

                            let (a_main, _a_others) = {
                                let mut main_att = None;
                                let mut others = Vec::new();
                                if let Some(pos) =
                                    original_cb.attachments.iter().position(|a| a.is_main)
                                {
                                    main_att = Some(original_cb.attachments[pos].clone());
                                    for (i, att) in original_cb.attachments.iter().enumerate() {
                                        if i != pos {
                                            others.push(att.clone());
                                        }
                                    }
                                } else if let Some(first) = original_cb.attachments.first() {
                                    main_att = Some(first.clone());
                                    others = original_cb.attachments[1..].to_vec();
                                }
                                (main_att, others)
                            };

                            let (b_main, _b_others) = {
                                let mut main_att = None;
                                let mut others = Vec::new();
                                if let Some(pos) =
                                    next_cb.attachments.iter().position(|a| a.is_main)
                                {
                                    main_att = Some(next_cb.attachments[pos].clone());
                                    for (i, att) in next_cb.attachments.iter().enumerate() {
                                        if i != pos {
                                            others.push(att.clone());
                                        }
                                    }
                                } else if let Some(first) = next_cb.attachments.first() {
                                    main_att = Some(first.clone());
                                    others = next_cb.attachments[1..].to_vec();
                                }
                                (main_att, others)
                            };

                            // 统一处理所有附件：若非选中的主PDF且未在保留列表中，则删除
                            let mut all_atts = Vec::new();
                            all_atts.extend(original_cb.attachments.clone());
                            all_atts.extend(next_cb.attachments.clone());

                            for att in all_atts {
                                let is_chosen_main = (Some(att.id.clone())
                                    == a_main.as_ref().map(|x| x.id.clone())
                                    && res.keep_a_main_pdf)
                                    || (Some(att.id.clone())
                                        == b_main.as_ref().map(|x| x.id.clone())
                                        && res.keep_b_main_pdf);

                                if !is_chosen_main
                                    && !res.keep_attachment_ids.contains(&att.id)
                                    && let Err(e) = app_cb.delete_attachment_file(&att.id)
                                {
                                    error!("合并流程: 删除未保留的附件失败: {e}");
                                }
                            }

                            if let Err(e) = app_cb.update_literature(merged.clone()) {
                                error!("合并流程: 保存合并结果失败: {e}");
                            }

                            if let Err(e) = app_cb.merge_literature_relations(source_id, master_id)
                            {
                                error!("合并流程: 合并关联关系失败: {e}");
                            }

                            if let Err(e) = app_cb.delete_literature_by_id(source_id) {
                                error!("合并流程: 移动副本到回收站失败: {e}");
                            }

                            let lang = app_cb.current_language();
                            show_notification(
                                NotificationType::Success,
                                format!(
                                    "{}: {}",
                                    t(I18nKey::LiteratureMergedTitle, lang),
                                    tf(I18nKey::LiteratureMergedMsg, lang, &[&source_lit.title])
                                ),
                                cx,
                            );

                            this.update(cx, |this, cx| {
                                this.continue_merge_flow(
                                    Arc::new(merged),
                                    remaining_cb.clone(),
                                    cx,
                                );
                            });
                        } else {
                            info!("合并流程: 跳过当前副本。");
                            this.update(cx, |this, cx| {
                                this.continue_merge_flow(
                                    original_cb.clone(),
                                    remaining_cb.clone(),
                                    cx,
                                );
                            });
                        }
                    }
                    window.remove_window();
                }),
            )
        });
    }

    fn continue_merge_flow(
        &mut self,
        original: Arc<Literature>,
        remaining: Vec<Literature>,
        cx: &mut Context<Self>,
    ) {
        if remaining.is_empty() {
            return;
        }

        let this_weak = cx.entity().downgrade();
        cx.spawn(move |_, cx: &mut gpui::AsyncApp| {
            let cx = cx.clone();
            async move {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(150))
                    .await;
                cx.update(|cx| {
                    if let Some(this) = this_weak.upgrade() {
                        this.update(cx, |this, cx| {
                            this.merge_next_in_group(original, remaining, cx);
                        });
                    }
                });
            }
        })
        .detach();
    }
}
