use crate::ui::notification::{NotificationType, show_notification};
use components::{IconName, add_drag_behavior};
use database;
use gpui::prelude::*;
use gpui::{AnyElement, FontWeight, SharedString, Window, div, px, rems, size};
use gpui_component::{
    ActiveTheme, Icon,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use i18n::{I18nKey, t};
use services::{
    app::MainApp,
    database_sync::{
        IdentityDecision, IdentityPlan, UiDatabaseSyncConflict, UiDatabaseSyncSummary,
    },
    sync::DatabaseSyncStatus,
};
use std::sync::Arc;

impl super::super::MainWindow {
    pub(crate) fn open_database_sync_action(
        &mut self,
        status: DatabaseSyncStatus,
        cx: &mut Context<Self>,
    ) {
        match status {
            DatabaseSyncStatus::NeedsRemoteInitialization => {
                self.open_database_sync_identity(IdentityDecision::NeedsRemoteInitialization, cx)
            }
            DatabaseSyncStatus::NeedsRemoteAdoption => {
                let app = self.app.clone();
                let this = cx.entity().downgrade();
                cx.spawn(async move |_, cx| {
                    if let Ok(decision @ IdentityDecision::NeedsRemoteAdoption { .. }) =
                        app.database_sync_preflight().await
                        && let Some(this) = this.upgrade()
                    {
                        let _ = this.update(cx, |this, cx| {
                            this.open_database_sync_identity(decision, cx)
                        });
                    }
                    anyhow::Ok(())
                })
                .detach();
            }
            DatabaseSyncStatus::IdentityMismatch => self.open_database_sync_identity(
                IdentityDecision::Mismatch {
                    local_id: String::new(),
                    remote_id: String::new(),
                },
                cx,
            ),
            DatabaseSyncStatus::Conflict => self.open_database_sync_conflicts(cx),
            DatabaseSyncStatus::PartialFailure
            | DatabaseSyncStatus::PendingLocalChanges
            | DatabaseSyncStatus::RemoteVersionRegression => self.open_database_sync_summary(cx),
            DatabaseSyncStatus::Idle | DatabaseSyncStatus::Error(_) => {
                let app = self.app.clone();
                crate::RUNTIME.spawn(async move { app.sync_service.force_sync().await });
            }
            DatabaseSyncStatus::Syncing => {}
        }
    }

    fn open_database_sync_identity(&mut self, decision: IdentityDecision, cx: &mut Context<Self>) {
        let app = self.app.clone();
        self.open_modal_window(size(px(520.), px(260.)), cx, move |_window, _cx| {
            DatabaseSyncIdentityDialog::new(app, decision)
        });
    }

    fn open_database_sync_conflicts(&mut self, cx: &mut Context<Self>) {
        let Ok(conflicts) = self.app.list_database_sync_conflicts() else {
            return;
        };
        let app = self.app.clone();
        self.open_modal_window(size(px(700.), px(560.)), cx, move |_window, _cx| {
            DatabaseSyncConflictDialog::new(app, conflicts)
        });
    }

    fn open_database_sync_summary(&mut self, cx: &mut Context<Self>) {
        let summary = self.app.database_sync_summary().ok().flatten();
        let app = self.app.clone();
        self.open_modal_window(size(px(520.), px(300.)), cx, move |_window, _cx| {
            DatabaseSyncSummaryDialog::new(app, summary)
        });
    }

    /// UI-002 / OBS-001: 打开文件冲突列表
    pub(crate) fn open_file_sync_conflicts(&mut self, cx: &mut Context<Self>) {
        let conflicts = self
            .app
            .list_attachment_file_conflicts()
            .unwrap_or_default();
        let app = self.app.clone();
        self.open_modal_window(size(px(700.), px(480.)), cx, move |_window, _cx| {
            FileSyncConflictDialog::new(app, conflicts)
        });
    }
}

struct FileSyncConflictDialog {
    app: Arc<MainApp>,
    conflicts: Vec<database::sqlite::AttachmentFileConflict>,
}

impl FileSyncConflictDialog {
    fn new(app: Arc<MainApp>, conflicts: Vec<database::sqlite::AttachmentFileConflict>) -> Self {
        Self { app, conflicts }
    }
}

impl Render for FileSyncConflictDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let lang = self.app.current_language();
        let app = self.app.clone();
        let this = cx.entity().downgrade();
        v_flex()
            .size_full()
            .relative()
            .child(add_drag_behavior(
                div()
                    .id("file-sync-conflict-drag")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(40.0)),
                window,
                cx,
            ))
            .child(
                h_flex().h(rems(2.5)).justify_end().px_4().child(
                    Button::new("file-conflict-close")
                        .ghost()
                        .child(Icon::new(IconName::Close).size(rems(0.75)))
                        .occlude()
                        .on_click(|_, window, _| window.remove_window()),
                ),
            )
            .p_5()
            .gap_3()
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child(t(I18nKey::FileSyncNeedsAttention, lang)),
            )
            .children(self.conflicts.iter().map(|c| {
                let short_id = if c.attachment_id.len() > 8 {
                    format!("{}…", &c.attachment_id[..8])
                } else {
                    c.attachment_id.clone()
                };
                let short_hash = if c.local_sha256.len() > 12 {
                    format!("{}…", &c.local_sha256[..12])
                } else {
                    c.local_sha256.clone()
                };
                let reason = match c.reason.as_str() {
                    "file_conflict" => t(I18nKey::SyncConflicts, lang).to_string(),
                    "unknown_divergence" => t(I18nKey::SyncUnknownDivergence, lang).to_string(),
                    other => other.to_string(),
                };
                let att_id = c.attachment_id.clone();
                let att_id2 = c.attachment_id.clone();
                let lib_id = c.file_library_id.clone();
                let reason_id = c.reason.clone();
                let app = app.clone();
                let this = this.clone();
                let this2 = this.clone();
                div()
                    .p_3()
                    .border_1()
                    .border_color(theme.border)
                    .rounded_md()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .child(SharedString::from(format!("{} · {reason}", short_id))),
                    )
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        SharedString::from(format!(
                            "remote={} local_hash={short_hash}",
                            c.remote_version
                        )),
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .mt_2()
                            .child(
                                Button::new(SharedString::from(format!("dismiss-{att_id2}")))
                                    .label(t(I18nKey::LocalData, lang))
                                    .ghost()
                                    .on_click(move |_, window, cx| {
                                        if let Err(e) = app.dismiss_attachment_file_conflict(
                                            &att_id, &lib_id, &reason_id,
                                        ) {
                                            show_notification(
                                                NotificationType::Error,
                                                format!("移除冲突失败: {e}"),
                                                cx,
                                            );
                                            return;
                                        }
                                        if let Some(this) = this.upgrade() {
                                            let _ = this.update(cx, |dialog, cx| {
                                                dialog.conflicts = dialog
                                                    .app
                                                    .list_attachment_file_conflicts()
                                                    .unwrap_or_default();
                                                cx.notify();
                                            });
                                        }
                                        if this.upgrade().map(|d| d.read(cx).conflicts.is_empty())
                                            == Some(true)
                                        {
                                            window.remove_window();
                                        }
                                    }),
                            )
                            .child(
                                Button::new(SharedString::from(format!("recheck-{att_id2}")))
                                    .label(t(I18nKey::CheckLocalFiles, lang))
                                    .ghost()
                                    .on_click(move |_, _, cx| {
                                        if let Some(this) = this2.upgrade() {
                                            let _ = this.update(cx, |dialog, cx| {
                                                dialog.conflicts = dialog
                                                    .app
                                                    .list_attachment_file_conflicts()
                                                    .unwrap_or_default();
                                                cx.notify();
                                            });
                                        }
                                    }),
                            ),
                    )
            }))
            .child(
                h_flex().justify_end().child(
                    Button::new("file-conflicts-close")
                        .label(t(I18nKey::Cancel, lang))
                        .ghost()
                        .on_click(|_, window, _| window.remove_window()),
                ),
            )
    }
}

struct DatabaseSyncIdentityDialog {
    app: Arc<MainApp>,
    decision: IdentityDecision,
    error: Option<String>,
    completed: bool,
}

impl DatabaseSyncIdentityDialog {
    fn new(app: Arc<MainApp>, decision: IdentityDecision) -> Self {
        Self {
            app,
            decision,
            error: None,
            completed: false,
        }
    }
}

impl Render for DatabaseSyncIdentityDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let lang = self.app.current_language();
        let decision = self.decision.clone();
        let message = match &decision {
            IdentityDecision::NeedsRemoteInitialization => {
                t(I18nKey::DatabaseSyncNeedsInitialization, lang).to_string()
            }
            IdentityDecision::NeedsRemoteAdoption { library_id } => format!(
                "{} ({library_id})",
                t(I18nKey::DatabaseSyncNeedsAdoption, lang)
            ),
            IdentityDecision::Mismatch { .. } => {
                t(I18nKey::DatabaseSyncIdentityMismatch, lang).to_string()
            }
            IdentityDecision::Ready { .. } => t(I18nKey::DatabaseSyncInProgress, lang).to_string(),
        };
        let explanation = self.error.clone().unwrap_or_else(|| match decision {
            IdentityDecision::Mismatch { .. } => {
                t(I18nKey::DatabaseSyncIdentityMismatch, lang).to_string()
            }
            _ => t(I18nKey::DatabaseSyncInProgress, lang).to_string(),
        });
        let can_confirm = matches!(
            self.decision,
            IdentityDecision::NeedsRemoteInitialization
                | IdentityDecision::NeedsRemoteAdoption { .. }
        );
        let app = self.app.clone();
        let this = cx.entity().downgrade();
        let decision_for_click = self.decision.clone();
        let confirm: AnyElement = if can_confirm && !self.completed {
            Button::new("database-sync-confirm")
                .label(t(I18nKey::Confirm, lang))
                .primary()
                .on_click(cx.listener(move |_, _, _, cx| {
                    let app = app.clone();
                    let this = this.clone();
                    let decision = decision_for_click.clone();
                    cx.spawn(async move |_, cx| {
                        let result: anyhow::Result<IdentityPlan> = match decision {
                            IdentityDecision::NeedsRemoteInitialization => {
                                app.confirm_remote_initialization().await
                            }
                            IdentityDecision::NeedsRemoteAdoption { .. } => {
                                app.confirm_remote_adoption().await
                            }
                            _ => unreachable!(),
                        };
                        match result {
                            Ok(_) => {
                                if let Some(this) = this.upgrade() {
                                    let _ = this.update(cx, |dialog, cx| {
                                        dialog.completed = true;
                                        cx.notify();
                                    });
                                }
                            }
                            Err(error) => {
                                if let Some(this) = this.upgrade() {
                                    let _ = this.update(cx, |dialog, cx| {
                                        dialog.error = Some(error.to_string());
                                        cx.notify();
                                    });
                                }
                            }
                        }
                        anyhow::Ok(())
                    })
                    .detach();
                }))
                .into_any_element()
        } else {
            div().into_any_element()
        };
        v_flex()
            .size_full()
            .relative()
            .child(add_drag_behavior(
                div()
                    .id("database-sync-identity-drag-overlay")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(40.0)),
                window,
                cx,
            ))
            .child(
                h_flex().h(rems(2.5)).justify_end().px_4().child(
                    Button::new("database-sync-identity-close")
                        .ghost()
                        .child(Icon::new(IconName::Close).size(rems(0.75)))
                        .occlude()
                        .on_click(|_, window, _| window.remove_window()),
                ),
            )
            .p_6()
            .gap_4()
            .child(div().font_weight(FontWeight::BOLD).child(message))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(explanation),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("database-sync-cancel")
                            .label(t(I18nKey::Cancel, lang))
                            .ghost()
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(confirm),
            )
    }
}

struct DatabaseSyncConflictDialog {
    app: Arc<MainApp>,
    conflicts: Vec<UiDatabaseSyncConflict>,
}

impl DatabaseSyncConflictDialog {
    fn new(app: Arc<MainApp>, conflicts: Vec<UiDatabaseSyncConflict>) -> Self {
        Self { app, conflicts }
    }
}

impl Render for DatabaseSyncConflictDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let lang = self.app.current_language();
        let app = self.app.clone();
        let this = cx.entity().downgrade();
        v_flex()
            .size_full()
            .relative()
            .child(add_drag_behavior(
                div()
                    .id("database-sync-conflict-drag-overlay")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(40.0)),
                window,
                cx,
            ))
            .child(
                h_flex().h(rems(2.5)).justify_end().px_4().child(
                    Button::new("database-sync-conflict-close")
                        .ghost()
                        .child(Icon::new(IconName::Close).size(rems(0.75)))
                        .occlude()
                        .on_click(|_, window, _| window.remove_window()),
                ),
            )
            .p_5()
            .gap_3()
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child(t(I18nKey::DatabaseSyncConflict, lang)),
            )
            .children(self.conflicts.iter().map(|conflict| {
                let entity_type = conflict.entity_type.clone();
                let entity_id = conflict.entity_id.clone();
                let local_entity_type = entity_type.clone();
                let local_entity_id = entity_id.clone();
                let remote_app = app.clone();
                let local_app = app.clone();
                let remote_this = this.clone();
                let local_this = this.clone();
                let key = format!("{}-{}", conflict.entity_type, conflict.entity_id);
                // UI-001: 知情展示 — 可识别名称、时间、两侧关键字段
                let local_val: serde_json::Value =
                    serde_json::from_str(&conflict.local_record).unwrap_or_default();
                let remote_val: serde_json::Value =
                    serde_json::from_str(&conflict.remote_record).unwrap_or_default();
                let display_name = local_val
                    .get("title")
                    .or_else(|| local_val.get("name"))
                    .or_else(|| remote_val.get("title"))
                    .or_else(|| remote_val.get("name"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        if entity_id.len() > 12 {
                            format!("{}…", &entity_id[..12])
                        } else {
                            entity_id.clone()
                        }
                    });
                let mut field_diff = String::new();
                if let (Some(lo), Some(ro)) = (local_val.as_object(), remote_val.as_object()) {
                    for k in ["title", "name", "year", "doi", "content", "color"] {
                        if let (Some(lv), Some(rv)) = (lo.get(k), ro.get(k)) {
                            if lv != rv {
                                field_diff.push_str(&format!("{k}: 本地 {lv} / 远端 {rv}\n"));
                            }
                        }
                    }
                }
                if field_diff.is_empty() {
                    field_diff = format!("本地 {local_val}\n远端 {remote_val}");
                }
                let detected = chrono::DateTime::from_timestamp(conflict.detected_at, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| conflict.detected_at.to_string());
                div()
                    .p_3()
                    .border_1()
                    .border_color(theme.border)
                    .rounded_md()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .child(SharedString::from(format!(
                                "{} · {}",
                                t(
                                    match entity_type.as_str() {
                                        "tags" => I18nKey::Tags,
                                        "literatures" => I18nKey::AllLiterature,
                                        "folders" => I18nKey::Folders,
                                        _ => I18nKey::DatabaseSyncConflict,
                                    },
                                    lang
                                ),
                                display_name
                            ))),
                    )
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        SharedString::from(format!(
                            "{} {} · v{}",
                            t(I18nKey::DatabaseSyncConflict, lang),
                            detected,
                            conflict.remote_version
                        )),
                    ))
                    .child(div().mt_2().text_sm().child(SharedString::from(field_diff)))
                    .child(
                        h_flex()
                            .gap_2()
                            .mt_2()
                            .child(
                                Button::new(SharedString::from(format!("remote-{key}")))
                                    .label(t(I18nKey::UseRemoteDatabase, lang))
                                    .primary()
                                    .on_click(move |_, window, cx| {
                                        if let Err(error) = remote_app
                                            .choose_remote_database_conflict(
                                                &local_entity_type,
                                                &local_entity_id,
                                            )
                                        {
                                            log::error!("choose remote conflict failed: {error:#}");
                                            show_notification(
                                                NotificationType::Error,
                                                format!("使用远端失败: {error}"),
                                                cx,
                                            );
                                            // UI-001: 失败不得关闭窗口
                                            return;
                                        }
                                        if let Some(this) = remote_this.upgrade() {
                                            let _ = this.update(cx, |dialog, cx| {
                                                dialog.conflicts = dialog
                                                    .app
                                                    .list_database_sync_conflicts()
                                                    .unwrap_or_default();
                                                cx.notify();
                                            });
                                        }
                                        window.remove_window();
                                    }),
                            )
                            .child(
                                Button::new(SharedString::from(format!("local-{key}")))
                                    .label(t(I18nKey::LocalData, lang))
                                    .ghost()
                                    .on_click(move |_, window, cx| {
                                        if let Err(error) = local_app
                                            .keep_local_database_conflict(&entity_type, &entity_id)
                                        {
                                            log::error!("keep local conflict failed: {error:#}");
                                            show_notification(
                                                NotificationType::Error,
                                                format!("保留本地失败: {error}"),
                                                cx,
                                            );
                                            return;
                                        }
                                        if let Some(this) = local_this.upgrade() {
                                            let _ = this.update(cx, |dialog, cx| {
                                                dialog.conflicts = dialog
                                                    .app
                                                    .list_database_sync_conflicts()
                                                    .unwrap_or_default();
                                                cx.notify();
                                            });
                                        }
                                        window.remove_window();
                                    }),
                            ),
                    )
            }))
            .child(
                Button::new("database-sync-conflicts-close")
                    .label(t(I18nKey::Cancel, lang))
                    .ghost()
                    .on_click(|_, window, _| window.remove_window()),
            )
    }
}

struct DatabaseSyncSummaryDialog {
    app: Arc<MainApp>,
    summary: Option<UiDatabaseSyncSummary>,
}

impl DatabaseSyncSummaryDialog {
    fn new(app: Arc<MainApp>, summary: Option<UiDatabaseSyncSummary>) -> Self {
        Self { app, summary }
    }
}

impl Render for DatabaseSyncSummaryDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.app.current_language();
        let summary = self.summary.clone().unwrap_or_default();
        let app = self.app.clone();
        v_flex()
            .size_full()
            .relative()
            .child(add_drag_behavior(
                div()
                    .id("database-sync-summary-drag-overlay")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(40.0)),
                window,
                cx,
            ))
            .child(
                h_flex().h(rems(2.5)).justify_end().px_4().child(
                    Button::new("database-sync-summary-close-icon")
                        .ghost()
                        .child(Icon::new(IconName::Close).size(rems(0.75)))
                        .occlude()
                        .on_click(|_, window, _| window.remove_window()),
                ),
            )
            .p_6()
            .gap_3()
            .child(div().font_weight(FontWeight::BOLD).child(
                if summary.version_regressions > 0 && summary.failures == 0 {
                    t(I18nKey::DatabaseSyncRemoteVersionRegression, lang)
                } else if summary.superseded > 0 && summary.failures == 0 {
                    t(I18nKey::DatabaseSyncPendingLocalChanges, lang)
                } else {
                    t(I18nKey::DatabaseSyncPartialFailure, lang)
                },
            ))
            .child(
                v_flex()
                    .gap_1()
                    .child(format!(
                        "{}: {}",
                        t(I18nKey::DatabaseSyncUploaded, lang),
                        summary.uploaded
                    ))
                    .child(format!(
                        "{}: {}",
                        t(I18nKey::DatabaseSyncDownloaded, lang),
                        summary.downloaded
                    ))
                    .child(format!(
                        "{}: {}",
                        t(I18nKey::DatabaseSyncConflictsCount, lang),
                        summary.conflicts
                    ))
                    .child(format!(
                        "{}: {}",
                        t(I18nKey::DatabaseSyncFailuresCount, lang),
                        summary.failures
                    ))
                    .child(format!(
                        "{}: {}",
                        t(I18nKey::DatabaseSyncPendingCount, lang),
                        summary.superseded
                    ))
                    .child(format!(
                        "{}: {}",
                        t(I18nKey::DatabaseSyncVersionRegressionsCount, lang),
                        summary.version_regressions
                    )),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("database-sync-summary-close")
                            .label(t(I18nKey::Cancel, lang))
                            .ghost()
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        Button::new("database-sync-summary-retry")
                            .label(t(I18nKey::Retry, lang))
                            .primary()
                            .on_click(move |_, window, _| {
                                let app = app.clone();
                                crate::RUNTIME
                                    .spawn(async move { app.sync_service.force_sync().await });
                                window.remove_window();
                            }),
                    ),
            )
    }
}
