use crate::ui::components::LiteratureEditor;
use crate::ui::dialogs::{FetchDialog, FetchMode};
use crate::ui::notification::show_notification;
use gpui::prelude::*;
use gpui::{AppContext, AsyncApp, Window, px, size};
use gpui_component::{WindowExt, dialog::DialogButtonProps, notification::NotificationType};
use i18n::{I18nKey, t, tf};
use log::{debug, error, info};
use models::constructors::create_literature;
use models::{FetchSource, Literature, LiteratureType};
use std::sync::Arc;
use uuid::Uuid;

impl super::super::MainWindow {
    pub fn open_manual_add_modal(&mut self, cx: &mut Context<Self>) {
        info!("UI: 用户触发手动添加文献");
        let mut lit = create_literature(Uuid::new_v4().to_string(), "", LiteratureType::Article);

        let ui_folder = cx
            .global::<crate::app_state::ui::UiState>()
            .selected_folder_id
            .clone();
        if let Some(folder_id) = &ui_folder
            && folder_id != "all"
            && folder_id != "uncategorized"
            && folder_id != "trash"
        {
            lit.folder_ids.push(folder_id.clone());
        }

        self.show_literature_editor(lit, true, cx);
    }

    pub fn open_fetch_modal(
        &mut self,
        mode: FetchMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        info!("UI: 用户打开文献抓取对话框 (Dialog 版), 模式: {mode:?}");
        let app = self.app.clone();
        let this_weak = cx.entity().downgrade();
        let window_handle = window.window_handle();

        // 1. 创建 FetchDialog 实体，管理抓取状态
        let entity = cx.new(|cx| FetchDialog::new(app.clone(), mode, window_handle, window, cx));

        // 2. 订阅 Enter 键触发抓取
        entity.update(cx, |fc, cx| {
            let input = fc.input_entity().clone();
            cx.subscribe(&input, move |fc, _, event, cx| {
                if let gpui_component::input::InputEvent::PressEnter { .. } = event {
                    fc.handle_fetch(cx);
                }
            })
            .detach();
        });

        // 3. 抓取完成回调：关闭 Dialog + 处理结果
        let this_weak2 = this_weak.clone();
        entity.update(cx, |fc, _| {
            fc.set_on_complete(Box::new(move |lits, window, cx| {
                use gpui_component::WindowExt;
                debug!("FETCH_DEBUG: on_complete 触发, 即将 close_dialog, lits.len={}", lits.len());
                window.close_dialog(cx);

                if let Some(this) = this_weak2.upgrade() {
                    this.update(cx, |this, cx| {
                        let should_select = lits.len() > 1
                            && (mode == FetchMode::Dblp || mode == FetchMode::OpenAlex);
                        debug!(
                            "FETCH_DEBUG: on_complete 处理中, lits.len={}, mode={:?}, should_select={}, pending_imports.len={}, pending_selectors.len={}",
                            lits.len(), mode, should_select, this.pending_imports.len(), this.pending_selectors.len(),
                        );

                        if should_select {
                            this.pending_selectors.push((
                                lits.into_iter().map(Arc::new).collect(),
                                Box::new(|this, lit: Literature, _window, _cx| {
                                    this.pending_imports.push(lit);
                                }),
                            ));
                            this.process_next_pending_selector(cx);
                        } else {
                            this.pending_imports.extend(lits);
                            this.process_next_pending_import(cx);
                        }
                        this.fetch_dialog = None;
                    });
                }
            }));
        });

        // 4. 打开 Dialog
        let mode_text = match mode {
            FetchMode::Doi => "DOI",
            FetchMode::ArXiv => "ArXiv",
            FetchMode::BibTeX => "BibTeX",
            FetchMode::Dblp => "DBLP",
            FetchMode::OpenAlex => "OpenAlex",
        };
        let lang = app.current_language();
        let title = tf(I18nKey::FetchFromSource, lang, &[mode_text]);

        window.open_dialog(cx, move |dialog, _, _cx| {
            dialog
                .w(px(500.))
                .title(title.clone())
                .content({
                    let this_weak = this_weak.clone();
                    move |content, _, cx| {
                        let entity = this_weak
                            .upgrade()
                            .and_then(|this| this.read(cx).fetch_dialog.clone());
                        if let Some(entity) = entity {
                            content.child(entity.clone())
                        } else {
                            content
                        }
                    }
                })
                .button_props(
                    DialogButtonProps::default()
                        .show_cancel(true)
                        .ok_text(t(I18nKey::ConfirmFetch, app.current_language()))
                        .on_ok({
                            let this_weak = this_weak.clone();
                            move |_, _, cx| {
                                if let Some(this) = this_weak.upgrade() {
                                    this.update(cx, |this, cx| {
                                        if let Some(entity) = &this.fetch_dialog {
                                            entity.update(cx, |fc, cx| fc.handle_fetch(cx));
                                        }
                                    });
                                }
                                false
                            }
                        })
                        .on_cancel({
                            let this_weak = this_weak.clone();
                            move |_, _, cx| {
                                if let Some(this) = this_weak.upgrade() {
                                    this.update(cx, |this, cx| {
                                        this.fetch_dialog = None;
                                        cx.notify();
                                        cx.notify();
                                    });
                                }
                                true
                            }
                        }),
                )
        });

        self.fetch_dialog = Some(entity.clone());
        cx.notify();

        window.defer(cx, {
            let entity = entity.clone();
            move |window, cx| {
                entity.update(cx, |this, cx| {
                    this.input_entity().update(cx, |state, cx| {
                        state.focus(window, cx);
                    });
                });
            }
        });
    }

    pub(crate) fn start_fetch_and_compare(
        &mut self,
        lit: Arc<Literature>,
        source: FetchSource,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = self.app.clone();
        let this_weak = cx.entity().downgrade();

        cx.spawn(move |_, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                // 将可能调用网络请求的异步代码交由全局 Tokio Runtime (RUNTIME) 执行，防止 DNS 解析器找不到 Reactor 导致崩溃
                let fetch_res = crate::RUNTIME
                    .spawn(async move { app.fetch_metadata_from_source(source).await })
                    .await;

                match fetch_res {
                    Ok(Ok(fetched)) => {
                        if let Some(this) = this_weak.upgrade() {
                            this.update(&mut cx, |this, cx| {
                                this.show_literature_compare(lit, fetched, cx);
                            });
                        }
                    }
                    Ok(Err(e)) => {
                        error!("元数据获取失败: {e}");
                    }
                    Err(e) => {
                        error!("Tokio 任务运行失败: {e}");
                    }
                }
            }
        })
        .detach();
    }

    pub(crate) fn start_fetch_openalex(
        &mut self,
        lit: Arc<Literature>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = self.app.clone();
        let this_weak = cx.entity().downgrade();

        cx.spawn(move |_, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let lit_for_fetch = lit.clone();
                // 将可能调用网络请求的异步代码交由全局 Tokio Runtime (RUNTIME) 执行，防止 DNS 解析器找不到 Reactor 导致崩溃
                let fetch_res = crate::RUNTIME
                    .spawn(async move {
                        app.fetcher_service
                            .resolve_openalex_auto(&lit_for_fetch)
                            .await
                    })
                    .await;

                match fetch_res {
                    Ok(Ok(Some(fetched))) => {
                        if let Some(this) = this_weak.upgrade() {
                            this.update(&mut cx, |this, cx| {
                                this.show_literature_compare(lit, fetched, cx);
                            });
                        }
                    }
                    Ok(Ok(None)) => {
                        if let Some(this) = this_weak.upgrade() {
                            this.update(&mut cx, |this, cx| {
                                let lang = this.app.current_language();
                                show_notification(
                                    NotificationType::Error,
                                    t(I18nKey::FetchFailed, lang),
                                    cx,
                                );
                            });
                        }
                    }
                    Ok(Err(e)) => {
                        error!("元数据获取失败: {e}");
                    }
                    Err(e) => {
                        error!("Tokio 任务运行失败: {e}");
                    }
                }
            }
        })
        .detach();
    }

    pub(crate) fn process_next_pending_selector(&mut self, cx: &mut Context<Self>) {
        if self.pending_selectors.is_empty() {
            return;
        }
        info!(
            "FETCH_DEBUG: process_next_pending_selector, 剩余 {} 个",
            self.pending_selectors.len(),
        );
        let (candidates, on_select) = self.pending_selectors.remove(0);
        self.open_metadata_selector(candidates, cx, on_select);
    }

    pub(crate) fn process_next_pending_import(&mut self, cx: &mut Context<Self>) {
        if self.pending_imports.is_empty() {
            return;
        }

        let lit = self.pending_imports.remove(0);
        info!(
            "UI: 处理批量导入队列，剩余 {} 条，正在打开编辑器: {} (active_popup_count={})",
            self.pending_imports.len(),
            lit.title,
            self.active_popup_count,
        );
        self.show_literature_editor(lit, true, cx);
    }

    pub(crate) fn show_literature_editor(
        &mut self,
        lit: Literature,
        is_new: bool,
        cx: &mut Context<Self>,
    ) {
        debug!("EDITOR: show_literature_editor 进入 (is_new={})", is_new);
        let app = self.app.clone();
        let this_weak = cx.entity().downgrade();
        let size = size(px(600.0), px(700.0));

        self.open_modal_window(size, cx, move |window, cx| {
            debug!("EDITOR: open_modal_window 回调执行，创建 LiteratureEditor");
            LiteratureEditor::new(app, lit, window, cx, move |result, window, cx| {
                debug!("EDITOR: 编辑器回调触发 (result={})", result.is_some());
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, cx| {
                        if let Some(lit) = result {
                            if is_new {
                                debug!("EDITOR: 调用 confirm_add_literature");
                                this.confirm_add_literature(lit, cx);
                            } else {
                                debug!("EDITOR: 调用 confirm_edit_literature");
                                this.confirm_edit_literature(lit, cx);
                            }
                        } else {
                            debug!("EDITOR: 用户取消");
                        }
                        cx.notify();

                        debug!("EDITOR: 回调处理完毕");
                    });
                }
                debug!("EDITOR: 关闭编辑器窗口");
                window.remove_window();
            })
        });
    }

    pub fn open_edit_modal(&mut self, target_id: Option<String>, cx: &mut Context<Self>) {
        let lit = if let Some(id) = target_id {
            {
                let data = self.data_store.read(cx);
                data.literatures.iter().find(|l| l.id == id).cloned()
            }
        } else {
            let first_id = cx
                .global::<crate::app_state::ui::UiState>()
                .selected_literature_ids
                .iter()
                .next()
                .cloned();
            if let Some(id) = first_id {
                {
                    let data = self.data_store.read(cx);
                    data.literatures.iter().find(|l| l.id == id).cloned()
                }
            } else {
                None
            }
        };

        if let Some(lit) = lit {
            self.show_literature_editor((*lit).clone(), false, cx);
        }
    }

    fn confirm_add_literature(&mut self, lit: Literature, cx: &mut Context<Self>) {
        let title = lit.title.clone();
        let lit_id = lit.id.clone();
        info!("业务: 用户确认添加新文献: {title}");

        match self.app.add_literature(lit) {
            Ok(()) => {
                info!("成功添加文献: {title}");
                // 选中新添加的文献
                crate::app_state::ui::UiState::update(cx, |state| {
                    state.selected_literature_ids.clear();
                    state.selected_literature_ids.insert(lit_id.clone());
                });
                // 如果当前选中的是某个自定义文件夹，自动将新文献加入此文件夹
                let state = cx.global::<crate::app_state::ui::UiState>();
                if let Some(ref folder_id) = state.selected_folder_id {
                    let virtual_folders =
                        ["all", "trash", "unread", "reading", "read", "favorites"];
                    if !virtual_folders.contains(&folder_id.as_str()) {
                        info!(
                            "业务: 自动将新文献[{}]加入当前选中文件夹[{}]",
                            lit_id, folder_id
                        );
                        if let Err(e) = self.app.add_literature_to_folder(&lit_id, folder_id) {
                            error!("自动关联文件夹失败: {e}");
                        }
                    }
                }
            }
            Err(e) => {
                error!("添加文献失败: {e}");
                show_notification(NotificationType::Error, format!("添加文献失败: {e}"), cx);
            }
        }
        cx.notify();
    }

    fn confirm_edit_literature(&mut self, lit: Literature, cx: &mut Context<Self>) {
        info!("业务: 用户确认更新文献: {} (ID: {})", lit.title, lit.id);
        match self.app.update_literature(lit.clone()) {
            Ok(()) => {
                info!("成功更新文献: {}", lit.title);
            }
            Err(e) => {
                error!("更新文献失败: {e}");
            }
        }
        cx.notify();
    }
}
