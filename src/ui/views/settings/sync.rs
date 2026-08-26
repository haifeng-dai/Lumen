use crate::app_state::config::ConfigStore;
use crate::ui::notification::{NotificationType, show_notification};
use components::{IconName, password_input};
use gpui::prelude::*;
use gpui::{AppContext, AsyncApp, Entity, SharedString, div, rems};
use gpui_component::{
    ActiveTheme, Icon, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{InputEvent, InputState},
    setting::{SettingField, SettingGroup, SettingItem, SettingPage},
    v_flex,
};
use i18n::{I18nKey, t};
use log::{error, info};
use services::app::MainApp;
use services::runtime::RUNTIME;
use std::sync::Arc;

use super::{
    SettingsWindow, config_bool, config_str, lang, set_config_bool, set_config_str,
    switch_setting_item,
};

impl SettingsWindow {
    pub(super) fn sync_page(&self, app: Arc<MainApp>, cx: &mut Context<Self>) -> SettingPage {
        let weak = cx.entity().downgrade();
        let l = lang(cx);

        // ── WebDAV group ───────────────────────────────────────────
        let webdav_group = {
            let app = app.clone();
            SettingGroup::new()
                .title(t(I18nKey::WebDavSettings, l))
                .item(switch_setting_item(
                    "webdav-enable-switch",
                    |l| t(I18nKey::EnableWebDav, l).into(),
                    config_bool(|c| c.webdav.enabled),
                    set_config_bool(|c| &mut c.webdav.enabled),
                ))
                .item(SettingItem::new(
                    t(I18nKey::EndpointUrl, l),
                    SettingField::<SharedString>::input(
                        config_str(|c| &c.webdav.endpoint),
                        set_config_str(|c| &mut c.webdav.endpoint),
                    ),
                ))
                .item(SettingItem::new(
                    t(I18nKey::Username, l),
                    SettingField::<SharedString>::input(
                        config_str(|c| &c.webdav.username),
                        set_config_str(|c| &mut c.webdav.username),
                    ),
                ))
                .item(SettingItem::render({
                    let app = app.clone();
                    move |_, window, cx| {
                        struct WdPassState {
                            input: Entity<InputState>,
                            _sub: gpui::Subscription,
                        }
                        let val: SharedString = app
                            .local_state
                            .read()
                            .unwrap()
                            .webdav_password
                            .clone()
                            .into();
                        let state = window.use_keyed_state::<WdPassState>(
                            "webdav-password",
                            cx,
                            |window, cx| {
                                let input = cx.new(|cx| {
                                    InputState::new(window, cx)
                                        .default_value(val)
                                        .masked(true)
                                });
                                let app = app.clone();
                                let _sub = cx.subscribe(&input, {
                                    move |_, emitter, event: &InputEvent, cx| {
                                        if let InputEvent::Change = event {
                                            let v = emitter.read(cx).value();
                                            let mut s = app.local_state.write().unwrap();
                                            s.webdav_password = v.to_string();
                                        }
                                    }
                                });
                                WdPassState { input, _sub }
                            },
                        );
                        let theme = cx.theme();
                        let l = lang(cx);

                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                v_flex()
                                    .child(div().text_sm().child(t(I18nKey::Password, l))),
                            )
                            .child(
                                h_flex()
                                    .w_64()
                                    .child(
                                        password_input(&state.read(cx).input, theme)
                                            .flex_grow(1.0)
                                            .into_any_element()
                                    ),
                            )
                    }
                }))
                .item(SettingItem::new(
                    t(I18nKey::RemotePath, l),
                    SettingField::<SharedString>::input(
                        config_str(|c| &c.webdav.remote_path),
                        set_config_str(|c| &mut c.webdav.remote_path),
                    ),
                ))

                .item(SettingItem::render({
                    let app = app.clone();
                    let weak = weak.clone();
                    move |_, _window, cx| {
                        let l = lang(cx);
                        let theme = cx.theme();

                        let webdav_tested = weak.upgrade().map(|this| this.read(cx).webdav_tested).unwrap_or(false);
                        let webdav_test_result = weak.upgrade().and_then(|this| this.read(cx).webdav_test_result.clone());

                        h_flex()
                            .gap_4()
                            .justify_end()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .when_some(webdav_test_result, |this, res| match res {
                                        Ok(()) => this
                                            .child(Icon::new(IconName::Check).size(rems(0.875)).text_color(theme.success))
                                            .child(div().text_xs().text_color(theme.success).child(t(I18nKey::ConnectionSuccess, l))),
                                        Err(_) => this
                                            .child(Icon::new(IconName::TriangleAlert).size(rems(0.875)).text_color(theme.danger))
                                            .child(div().text_xs().text_color(theme.danger).child(t(I18nKey::ConnectionFailed, l))),
                                    })
                            )
                            .when(webdav_tested, |s| {
                                s.child(
                                    Button::new("sync-webdav-attachments")
                                        .label(t(I18nKey::SyncAttachments, l))
                                        .small()
                                        .primary()
                                        .on_click({
                                            let app = app.clone();
                                            let weak = weak.clone();
                                            move |_, _, cx| {
                                                let app = app.clone();
                                                cx.spawn(move |_: &mut AsyncApp| async move {
                                                    app.perform_attachments_sync();
                                                })
                                                .detach();
                                                if let Some(this) = weak.upgrade() {
                                                    this.update(cx, |_, cx| cx.notify());
                                                }
                                            }
                                        }),
                                )
                            })
                            .child(
                                Button::new("test-webdav")
                                    .label(t(I18nKey::TestConnection, l))
                                    .small()
                                    .on_click({
                                        let app = app.clone();
                                        let weak = weak.clone();
                                        move |_, window, cx| {
                                            let cfg = cx.global::<ConfigStore>().inner.clone();
                                            let app = app.clone();
                                            let weak = weak.clone();
                                            let handle = window.window_handle();
                                            let l = lang(cx);
                                            cx.spawn(move |cx: &mut AsyncApp| {
                                                let mut ax = cx.clone();
                                                async move {
                                                    let webdav_password = app
                                                        .local_state
                                                        .read()
                                                        .unwrap()
                                                        .webdav_password
                                                        .clone();
                                                    let res = app
                                                        .test_webdav_config(
                                                            cfg.webdav.endpoint,
                                                            cfg.webdav.username,
                                                            webdav_password,
                                                            cfg.webdav.remote_path,
                                                        )
                                                        .await;
                                                    let is_ok = res.is_ok();
                                                    let _ = ax.update_window(handle, |_, _, cx| {
                                                        if let Some(this) = weak.upgrade() {
                                                            this.update(cx, |this, cx| {
                                                                this.webdav_tested = is_ok;
                                                                if let Err(ref e) = res {
                                                                    crate::ui::notification::show_notification(crate::ui::notification::NotificationType::Error, format!("{}: {}", t(I18nKey::ConnectionFailed, l), e), cx);
                                                                }
                                                                this.webdav_test_result = Some(res);
                                                                cx.notify();
                                                            });
                                                        }
                                                    });
                                                }
                                            })
                                            .detach();
                                        }
                                    }),
                            )
                            .into_any_element()
                    }
                }))
        };

        // ── Google Drive group ─────────────────────────────────────
        let gdrive_group = {
            SettingGroup::new()
                .title("Google Drive")
                .item(switch_setting_item(
                    "gdrive-enable-switch",
                    |l| t(I18nKey::EnableGoogleDrive, l).into(),
                    config_bool(|c| c.google_drive.enabled),
                    set_config_bool(|c| &mut c.google_drive.enabled),
                ))
                .item(SettingItem::new(
                    t(I18nKey::ClientId, l),
                    SettingField::<SharedString>::input(
                        config_str(|c| &c.google_drive.client_id),
                        set_config_str(|c| &mut c.google_drive.client_id),
                    ),
                ))
                .item({
                    let app = app.clone();
                    SettingItem::render(move |_, window, cx| {
                        struct GdSecretState {
                            input: Entity<InputState>,
                            _sub: gpui::Subscription,
                        }
                        let cfg = cx.global::<ConfigStore>().inner.clone();
                        let val: SharedString = cfg.google_drive.client_secret.into();
                        let state = window.use_keyed_state::<GdSecretState>(
                            "gdrive-client-secret",
                            cx,
                            |window, cx| {
                                let input = cx.new(|cx| {
                                    InputState::new(window, cx)
                                        .default_value(val.clone())
                                        .masked(true)
                                });
                                let _sub = cx.subscribe(&input, {
                                    let app = app.clone();
                                    move |_, emitter, event: &InputEvent, cx| {
                                        if let InputEvent::Change = event {
                                            let v = emitter.read(cx).value();
                                            cx.update_global::<ConfigStore, _>(|store, _| {
                                                store.inner.google_drive.client_secret =
                                                    v.to_string();
                                            });
                                            let _ = app.update_config(
                                                cx.global::<ConfigStore>().inner.clone(),
                                            );
                                        }
                                    }
                                });
                                GdSecretState { input, _sub }
                            },
                        );
                        // Sync external changes
                        state.update(cx, |state, cx| {
                            if state.input.read(cx).value() != val {
                                state.input.update(cx, |input, cx| {
                                    input.set_value(val.clone(), window, cx);
                                });
                            }
                        });
                        let theme = cx.theme();
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                v_flex()
                                    .child(div().text_sm().child(t(I18nKey::ClientSecret, l))),
                            )
                            .child(
                                h_flex()
                                    .w_64()
                                    .child(
                                        password_input(
                                            &state.read(cx).input,
                                            theme,
                                        )
                                        .flex_grow(1.0)
                                        .into_any_element(),
                                    ),
                            )
                            .into_any_element()
                    })
                })
                .item(SettingItem::render({
                    let app = app.clone();
                    let _weak = weak.clone();
                    move |_, _, cx| {
                        let cfg = cx.global::<ConfigStore>().inner.clone();
                        let theme = cx.theme();
                        let l = lang(cx);
                        let is_authorized = cfg.google_drive.authorized;
                        let can_auth = cfg.google_drive.enabled
                            && !cfg.google_drive.client_id.is_empty()
                            && !cfg.google_drive.client_secret.is_empty();
                        h_flex()
                            .gap_2()
                            .when(can_auth, |this| {
                                this.child(
                                    h_flex()
                                        .gap_2()
                                        .justify_end()
                                        .child(
                                            h_flex()
                                                .gap_1()
                                                .when(is_authorized, |this| {
                                                    this
                                                        .child(Icon::new(IconName::Check).size(rems(0.875)).text_color(theme.success))
                                                        .child(div().text_xs().text_color(theme.success).child(t(I18nKey::ConnectionSuccess, l)))
                                                })
                                                .when(!is_authorized, |this| {
                                                    this
                                                        .child(Icon::new(IconName::TriangleAlert).size(rems(0.875)).text_color(theme.danger))
                                                        .child(div().text_xs().text_color(theme.danger).child(t(I18nKey::ConnectionFailed, l)))
                                                }),
                                        )
                                        .child(
                                            Button::new("authorize-google-drive")
                                            .label(t(I18nKey::Authorize, l))
                                            .small()
                                            .on_click({
                                            let app = app.clone();
                                            move |_, window, cx| {
                                                let cfg = cx
                                                    .global::<ConfigStore>()
                                                    .inner
                                                    .clone();
                                                let app = app.clone();
                                                let handle = window.window_handle();
                                                cx.spawn(move |cx: &mut AsyncApp| {
                                                    let mut ax = cx.clone();
                                                    async move {
                                                        let result = file::google_drive::complete_oauth_flow(
                                                            &cfg.google_drive.client_id,
                                                            &cfg.google_drive.client_secret,
                                                        )
                                                        .await;
                                                        match result {
                                                            Ok(refresh_token) => {
                                                                 let mut state =
                                                                    app.local_state.write().unwrap();
                                                                 state.google_drive_refresh_token =
                                                                     refresh_token;
                                                                 let _ = ax.update_window(handle, |_, _, cx| {
                                                                     cx.set_global(ConfigStore {
                                                                         inner: app.config.lock().unwrap().clone(),
                                                                     });
                                                                 });
                                                            }
                                                            Err(e) => {
                                                                error!("Google Drive OAuth 失败: {e}");
                                                                let _ = ax.update_window(handle, |_, _, cx| {
                                                                    crate::ui::notification::show_notification(
                                                                        crate::ui::notification::NotificationType::Error,
                                                                        format!("{}: {e}", t(I18nKey::ConnectionFailed, l)),
                                                                        cx,
                                                                    );
                                                                });
                                                            }
                                                        }
                                                    }
                                                })
                                                .detach();
                                            }
                                        }),
                                        )
                                )
                            })
                            .into_any_element()
                    }
                }))
        };

        // ── Database Sync group ────────────────────────────────────
        let db_group = {
            let app = app.clone();
            SettingGroup::new()
                .title(t(I18nKey::DatabaseSettings, l))
                .item(switch_setting_item(
                    "db-remote-switch",
                    |l| t(I18nKey::UseRemoteDatabase, l).into(),
                    config_bool(|c| c.database.use_remote),
                    set_config_bool(|c| &mut c.database.use_remote),
                ))
                .item(SettingItem::new(
                    t(I18nKey::Host, l),
                    SettingField::<SharedString>::input(
                        config_str(|c| &c.database.host),
                        set_config_str(|c| &mut c.database.host),
                    ),
                ))
                .item(SettingItem::new(
                    t(I18nKey::Port, l),
                    SettingField::<SharedString>::input(
                        move |cx| cx.global::<ConfigStore>().database.port.to_string().into(),
                        move |v, cx| {
                            if let Ok(port) = v.parse::<u16>() {
                                cx.update_global::<ConfigStore, _>(|store, _| {
                                    store.inner.database.port = port;
                                });
                            }
                        },
                    ),
                ))
                .item(SettingItem::new(
                    t(I18nKey::DatabaseName, l),
                    SettingField::<SharedString>::input(
                        config_str(|c| &c.database.database),
                        set_config_str(|c| &mut c.database.database),
                    ),
                ))
                .item(SettingItem::new(
                    t(I18nKey::Username, l),
                    SettingField::<SharedString>::input(
                        config_str(|c| &c.database.username),
                        set_config_str(|c| &mut c.database.username),
                    ),
                ))
                .item(SettingItem::render({
                    let app = app.clone();
                    move |_, window, cx| {
                        struct DbPassState {
                            input: Entity<InputState>,
                            _sub: gpui::Subscription,
                        }
                        let val: SharedString = config_str(|c| &c.database.password)(cx);
                        let state = window.use_keyed_state::<DbPassState>(
                            "db-password",
                            cx,
                            |window, cx| {
                                let input = cx.new(|cx| {
                                    InputState::new(window, cx)
                                        .default_value(val)
                                        .masked(true)
                                });
                                let app = app.clone();
                                let _sub = cx.subscribe(&input, {
                                    move |_, emitter, event: &InputEvent, cx| {
                                        if let InputEvent::Change = event {
                                            let v = emitter.read(cx).value();
                                            cx.update_global::<ConfigStore, _>(|store, _| {
                                                store.inner.database.password = v.to_string();
                                            });
                                            let _ = app.update_config(cx.global::<ConfigStore>().inner.clone());
                                        }
                                    }
                                });
                                DbPassState { input, _sub }
                            },
                        );
                        let theme = cx.theme();
                        let l = lang(cx);

                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                v_flex()
                                    .child(div().text_sm().child(t(I18nKey::Password, l))),
                            )
                            .child(
                                h_flex()
                                    .w_64()
                                    .child(
                                        password_input(&state.read(cx).input, theme)
                                            .flex_grow(1.0)
                                            .into_any_element()
                                    ),
                            )
                    }
                }))
                .item(switch_setting_item(
                    "db-ssl-switch",
                    |l| t(I18nKey::EnableSSL, l).into(),
                    config_bool(|c| c.database.use_ssl),
                    set_config_bool(|c| &mut c.database.use_ssl),
                ))
                .item(SettingItem::render({
                    let app = app.clone();
                    let weak = weak.clone();
                    move |_, _, cx| {
                        let l = lang(cx);
                        let theme = cx.theme();
                        let db_tested = weak.upgrade().map(|this| this.read(cx).db_tested).unwrap_or(false);
                        let db_test_result = weak.upgrade().and_then(|this| this.read(cx).db_test_result.clone());

                        h_flex()
                            .gap_4()
                            .justify_end()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .when_some(db_test_result, |this, res| match res {
                                        Ok(()) => this
                                            .child(Icon::new(IconName::Check).size(rems(0.875)).text_color(theme.success))
                                            .child(div().text_xs().text_color(theme.success).child(t(I18nKey::ConnectionSuccess, l))),
                                        Err(_) => this
                                            .child(Icon::new(IconName::TriangleAlert).size(rems(0.875)).text_color(theme.danger))
                                            .child(div().text_xs().text_color(theme.danger).child(t(I18nKey::ConnectionFailed, l))),
                                    })
                            )
                            .when(db_tested, |s| {
                                s.child(
                                    Button::new("sync-db-metadata")
                                        .label(t(I18nKey::SyncMetadata, l))
                                        .icon(IconName::Globe)
                                        .small()
                                        .primary()
                                        .on_click({
                                            let app = app.clone();
                                            let weak = weak.clone();
                                            move |_, _, cx| {
                                                let app = app.clone();
                                                cx.spawn(move |_: &mut AsyncApp| async move {
                                                    app.perform_sync();
                                                })
                                                .detach();
                                                if let Some(this) = weak.upgrade() {
                                                    this.update(cx, |_, cx| cx.notify());
                                                }
                                            }
                                        }),
                                )
                            })
                            .child(
                                Button::new("test-db")
                                    .label(t(I18nKey::TestConnection, l))
                                    .small()
                                    .on_click({
                                        let app = app.clone();
                                        let weak = weak.clone();
                                        move |_, window, cx| {
                                            let cfg = cx.global::<ConfigStore>().inner.database.clone();
                                            let app = app.clone();
                                            let weak = weak.clone();
                                            let handle = window.window_handle();
                                            let l = lang(cx);
                                            cx.spawn(move |cx: &mut AsyncApp| {
                                                let mut ax = cx.clone();
                                                async move {
                                                    let res = app.test_mysql_config(cfg).await;
                                                    let is_ok = res.is_ok();
                                                    let _ = ax.update_window(handle, |_, _, cx| {
                                                        if let Some(this) = weak.upgrade() {
                                                            this.update(cx, |this, cx| {
                                                                this.db_tested = is_ok;
                                                                if let Err(ref e) = res {
                                                                    crate::ui::notification::show_notification(crate::ui::notification::NotificationType::Error, format!("{}: {}", t(I18nKey::ConnectionFailed, l), e), cx);
                                                                }
                                                                this.db_test_result = Some(res);
                                                                cx.notify();
                                                            });
                                                        }
                                                    });
                                                }
                                            })
                                            .detach();
                                        }
                                    }),
                            )
                            .into_any_element()
                    }
                }))
        };

        // ── Data Management group ──────────────────────────────────
        let data_mgmt_group = {
            SettingGroup::new()
                .title(t(I18nKey::DataManagement, l))
                .item(SettingItem::render({
                    let app = app.clone();
                    move |_, _, cx| {
                        let l = lang(cx);
                        h_flex()
                            .w_full()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                Button::new("clear-local-db")
                                    .label(t(I18nKey::ClearLocalDb, l))
                                    .small()
                                    .text_color(cx.theme().danger)
                                    .on_click({
                                        let app = app.clone();
                                        move |_, _, _cx| {
                                            if let Err(e) = app.clear_local_database() {
                                                error!("清空本地数据库失败: {e}");
                                            }
                                        }
                                    }),
                            )
                            .child(
                                Button::new("clear-local-files")
                                    .label(t(I18nKey::ClearLocalFiles, l))
                                    .small()
                                    .text_color(cx.theme().danger)
                                    .on_click({
                                        let app = app.clone();
                                        move |_, _, _cx| {
                                            if let Err(e) = app.file_manager.trash_all() {
                                                error!("清空本地文件失败: {e}");
                                            }
                                        }
                                    }),
                            )
                            .child(
                                Button::new("check-local-files")
                                    .label(t(I18nKey::CheckLocalFiles, l))
                                    .small()
                                    .on_click({
                                        let app = app.clone();
                                        move |_, _, cx| match app.check_local_files() {
                                            Ok((checked, missing)) if missing.is_empty() => {
                                                show_notification(
                                                    NotificationType::Success,
                                                    format!("本地文件校对完成：共检查 {checked} 个，全部存在"),
                                                    cx,
                                                );
                                            }
                                            Ok((checked, missing)) => {
                                                show_notification(
                                                    NotificationType::Warning,
                                                    format!(
                                                        "本地文件校对完成：共检查 {checked} 个，缺失 {} 个",
                                                        missing.len()
                                                    ),
                                                    cx,
                                                );
                                            }
                                            Err(e) => {
                                                error!("校对本地文件失败: {e}");
                                                show_notification(
                                                    NotificationType::Error,
                                                    format!("校对本地文件失败: {e}"),
                                                    cx,
                                                );
                                            }
                                        }
                                    }),
                            )
                            .child(
                                Button::new("clear-cloud-db")
                                    .label(t(I18nKey::ClearCloudDb, l))
                                    .small()
                                    .text_color(cx.theme().danger)
                                    .on_click({
                                        let app = app.clone();
                                        move |_, _, cx| {
                                            let app = app.clone();
                                            cx.spawn(move |_: &mut AsyncApp| async move {
                                                if let Err(e) =
                                                    app.sync_service.clear_remote_database().await
                                                {
                                                    error!("清空云端数据库失败: {e}");
                                                }
                                            })
                                            .detach();
                                        }
                                    }),
                            )
                            .child(
                                Button::new("clear-cloud-files")
                                    .label(t(I18nKey::ClearCloudFiles, l))
                                    .small()
                                    .text_color(cx.theme().danger)
                                    .on_click({
                                        let app = app.clone();
                                        move |_, _, cx| {
                                            let app = app.clone();
                                            cx.spawn(move |_: &mut AsyncApp| async move {
                                                if let Err(e) =
                                                    app.sync_service.clear_remote_files().await
                                                {
                                                    error!("清空云端文件失败: {e}");
                                                }
                                            })
                                            .detach();
                                        }
                                    }),
                            )
                            .child(
                                Button::new("purge-synced-deletions")
                                    .label(t(I18nKey::PurgeSyncedDeletions, l))
                                    .small()
                                    .text_color(cx.theme().danger)
                                    .on_click({
                                        let app = app.clone();
                                        move |_, _, _cx| match app.purge_synced_deletions() {
                                            Ok(n) => info!("清理已删除数据完成，共 {n} 条"),
                                            Err(e) => error!("清理已删除数据失败: {e}"),
                                        }
                                    }),
                            )
                            .child(
                                Button::new("purge-deleted-data")
                                    .label(t(I18nKey::PurgeDeletedData, l))
                                    .small()
                                    .text_color(cx.theme().danger)
                                    .on_click({
                                        let app = app.clone();
                                        move |_, _, _cx| {
                                            let app = app.clone();
                                            RUNTIME.spawn(async move {
                                                match app.purge_deleted_data().await {
                                                    Ok(n) => {
                                                        info!("彻底清理已删除数据完成，共 {n} 条")
                                                    }
                                                    Err(e) => error!("彻底清理已删除数据失败: {e}"),
                                                }
                                            });
                                        }
                                    }),
                            )
                            .into_any_element()
                    }
                }))
        };

        SettingPage::new(t(I18nKey::Sync, l))
            .icon(Icon::new(IconName::Cloud))
            .group(webdav_group)
            .group(gdrive_group)
            .group(db_group)
            .group(data_mgmt_group)
    }
}
