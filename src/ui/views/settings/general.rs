use crate::app_state::config::ConfigStore;
use crate::app_state::theme::{ThemeLoaderState, surface};
use components::IconName;
use components::{muted_input, selector};
use gpui::prelude::*;
use gpui::{AppContext, AsyncApp, Entity, MouseButton, SharedString, div, rems, transparent_black};
use gpui_component::{
    ActiveTheme, Icon,
    button::Button,
    h_flex,
    input::{InputEvent, InputState},
    setting::{SettingGroup, SettingItem, SettingPage},
    v_flex,
};
use i18n::{I18nKey, Language, t};
use services::{app::MainApp, utils::filename};
use std::sync::Arc;

use super::{
    SettingsWindow, config_bool, config_str, lang, path_picker_element, set_config_bool,
    set_config_str, switch_setting_item,
};

impl SettingsWindow {
    pub(super) fn general_page(&self, app: Arc<MainApp>, cx: &mut Context<Self>) -> SettingPage {
        let surface = surface(cx);
        let l = lang(cx);

        // ── Dropdown option builders ───────────────────────────────

        let lang_options: Vec<(SharedString, SharedString)> = [
            (Language::ZhCn, "简体中文"),
            (Language::ZhTw, "繁體中文"),
            (Language::En, "English"),
            (Language::Ja, "日本語"),
            (Language::Ko, "한국어"),
            (Language::Ru, "Русский"),
            (Language::Fr, "Français"),
            (Language::De, "Deutsch"),
            (Language::Es, "Español"),
        ]
        .iter()
        .map(|(l, label)| (l.as_str().to_string().into(), (*label).into()))
        .collect();

        let scale_options: Vec<(SharedString, SharedString)> = (0..=12)
            .map(|i| {
                let v = 0.8 + i as f32 * 0.1;
                (
                    format!("{v:.1}").into(),
                    format!("{}%", (v * 100.0) as u32).into(),
                )
            })
            .collect();

        let log_options: Vec<(SharedString, SharedString)> = [
            ("debug", "Debug"),
            ("info", "Info"),
            ("warn", "Warn"),
            ("error", "Error"),
        ]
        .iter()
        .map(|(v, l)| ((*v).into(), (*l).into()))
        .collect();

        let notif_options: Vec<(SharedString, SharedString)> =
            [("all", "All"), ("warn", "Warn"), ("error", "Error")]
                .iter()
                .map(|(v, l)| ((*v).into(), (*l).into()))
                .collect();

        let mut theme_style_options: Vec<(SharedString, SharedString)> =
            vec![("default".into(), "Default".into())];
        for name in ThemeLoaderState::read(cx).available_themes() {
            theme_style_options.push((name.clone().into(), name.into()));
        }

        // ── Library Settings group ─────────────────────────────────

        let app_for_lib = app.clone();
        let library_group = {
            SettingGroup::new()
                .title(t(I18nKey::LibrarySettings, l))
                .item(SettingItem::render({
                    move |_, window, cx| {
                        let l = lang(cx);
                        path_picker_element(
                            "base-dir",
                            |cx| {
                                cx.global::<ConfigStore>()
                                    .inner
                                    .base_dir
                                    .to_string_lossy()
                                    .to_string()
                                    .into()
                            },
                            |v, cx| {
                                cx.update_global::<ConfigStore, _>(|store, _| {
                                    store.inner.base_dir = v.to_string().into();
                                });
                            },
                            "Browse...".into(),
                            t(I18nKey::DatabaseDir, l).into(),
                            t(I18nKey::DatabaseDir, l),
                            t(I18nKey::DatabaseDirDesc, l),
                            window,
                            cx,
                        )
                        .into_any_element()
                    }
                }))
                .item(SettingItem::render({
                    move |_, window, cx| {
                        let l = lang(cx);
                        path_picker_element(
                            "attachment-path",
                            |cx| {
                                cx.global::<ConfigStore>()
                                    .inner
                                    .attachment_path
                                    .to_string_lossy()
                                    .to_string()
                                    .into()
                            },
                            |v, cx| {
                                cx.update_global::<ConfigStore, _>(|store, _| {
                                    store.inner.attachment_path = v.to_string().into();
                                });
                            },
                            "Browse...".into(),
                            t(I18nKey::AttachmentDir, l).into(),
                            t(I18nKey::AttachmentDir, l),
                            t(I18nKey::AttachmentDirDesc, l),
                            window,
                            cx,
                        )
                        .into_any_element()
                    }
                }))
                .item(SettingItem::render({
                    let app = app_for_lib.clone();
                    move |_, p_window, cx| {
                        let l = lang(cx);

                        // Build input state first, before borrowing theme
                        struct FnState {
                            input: Entity<InputState>,
                            _sub: gpui::Subscription,
                        }
                        let template = cx.global::<ConfigStore>().inner.filename_template.clone();
                        let state = p_window.use_keyed_state::<FnState>(
                            "filename-template-input",
                            cx,
                            |window, cx| {
                                let input = cx
                                    .new(|cx| InputState::new(window, cx).default_value(template));
                                let _sub = cx.subscribe(&input, {
                                    move |_, emitter, event: &InputEvent, cx| {
                                        if let InputEvent::Change = event {
                                            let v = emitter.read(cx).value();
                                            cx.update_global::<ConfigStore, _>(|store, _| {
                                                store.inner.filename_template = v.to_string();
                                            });
                                        }
                                    }
                                });
                                FnState { input, _sub }
                            },
                        );

                        let theme = cx.theme();
                        let preview_options = filename::FilenameOptions::new(
                            "He",
                            "Kaiming",
                            "2022",
                            "Masked Autoencoders Are Scalable Vision Learners",
                            "CVPR",
                            "pdf",
                            true,
                        );
                        let active_template =
                            cx.global::<ConfigStore>().inner.filename_template.clone();
                        let preview_name = filename::generate_filename_from_template(
                            &active_template,
                            &preview_options,
                        );

                        v_flex()
                            .gap_2()
                            .w_full()
                            .child(
                                v_flex()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child(t(I18nKey::FilenameTemplate, l)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .whitespace_normal()
                                            .child(t(I18nKey::FilenameTemplateDesc, l)),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(muted_input(&state.read(cx).input, theme).flex_grow(1.0))
                                    .child(
                                        Button::new("batch-rename")
                                            .child(t(I18nKey::BatchRename, l))
                                            .w(rems(4.5))
                                            .on_click({
                                                let app = app.clone();
                                                move |_, _, cx| {
                                                    let app = app.clone();
                                                    cx.spawn(move |_: &mut AsyncApp| {
                                                        let app = app.clone();
                                                        async move {
                                                            if let Err(e) = app.batch_rename_files()
                                                            {
                                                                log::error!("批量重命名失败: {e}");
                                                            }
                                                        }
                                                    })
                                                    .detach();
                                                }
                                            }),
                                    )
                                    .child(
                                        Button::new("cleanup-orphaned")
                                            .child(t(I18nKey::CleanupOrphanedFiles, l))
                                            .w(rems(4.5))
                                            .on_click({
                                                let app = app.clone();
                                                move |_, _, cx| {
                                                    let app = app.clone();
                                                    cx.spawn(move |_: &mut AsyncApp| {
                                                        let app = app.clone();
                                                        async move {
                                                            if let Err(e) =
                                                                app.cleanup_orphaned_files()
                                                            {
                                                                log::error!(
                                                                    "清理孤立文件失败: {e}"
                                                                );
                                                            }
                                                        }
                                                    })
                                                    .detach();
                                                }
                                            }),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .child(format!("{}: ", t(I18nKey::Preview, l))),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.primary)
                                            .child(preview_name),
                                    ),
                            )
                            .into_any_element()
                    }
                }))
        };

        // ── PDF Viewer Settings group ──────────────────────────────

        let pdf_group = {
            SettingGroup::new()
                .title(t(I18nKey::PdfViewerSettings, l))
                .item(switch_setting_item(
                    "pdf-use-custom-switch",
                    |l| t(I18nKey::UseCustomPdfViewer, l).into(),
                    config_bool(|c| c.pdf_viewer.use_custom),
                    set_config_bool(|c| &mut c.pdf_viewer.use_custom),
                ))
                .item(SettingItem::render(move |_, window, cx| {
                    let enabled = cx.global::<ConfigStore>().pdf_viewer.use_custom;
                    if !enabled {
                        return div().into_any_element();
                    }

                    let cfg_get = config_str(|c| &c.pdf_viewer.macos_app);
                    let cfg_set = set_config_str(|c| &mut c.pdf_viewer.macos_app);

                    v_flex()
                        .gap_4()
                        .when(cfg!(target_os = "macos"), |this| {
                            this.child(path_picker_element(
                                "pdf-macos",
                                move |cx| cfg_get(cx),
                                move |v, cx| cfg_set(v, cx),
                                t(I18nKey::SelectMacosPdfReader, lang(cx)).into(),
                                t(I18nKey::SelectMacosPdfReader, lang(cx)).into(),
                                t(I18nKey::PdfViewerPathMacos, lang(cx)),
                                "",
                                window,
                                cx,
                            ))
                        })
                        .when(cfg!(target_os = "windows"), |this| {
                            this.child(path_picker_element(
                                "pdf-windows",
                                config_str(|c| &c.pdf_viewer.windows_app),
                                set_config_str(|c| &mut c.pdf_viewer.windows_app),
                                t(I18nKey::SelectWindowsPdfReader, lang(cx)).into(),
                                t(I18nKey::SelectWindowsPdfReader, lang(cx)).into(),
                                t(I18nKey::PdfViewerPathWindows, lang(cx)),
                                "",
                                window,
                                cx,
                            ))
                        })
                        .into_any_element()
                }))
        };

        // ── Citation group ─────────────────────────────────────────

        let citation_group = {
            SettingGroup::new()
                .title(t(I18nKey::CitationSettings, l))
                .item(switch_setting_item(
                    "citation-abbrev-switch",
                    |l| t(I18nKey::AbbreviateJournalInCitation, l).into(),
                    config_bool(|c| c.citation.abbreviate_journal),
                    set_config_bool(|c| &mut c.citation.abbreviate_journal),
                ))
        };

        // ── Proxy group ────────────────────────────────────────────

        let proxy_group = {
            SettingGroup::new()
                .title(t(I18nKey::NetworkProxySettings, l))
                .item(switch_setting_item(
                    "proxy-enable-switch",
                    |l| t(I18nKey::EnableProxyServer, l).into(),
                    config_bool(|c| c.proxy.enabled),
                    set_config_bool(|c| &mut c.proxy.enabled),
                ))
                .item(SettingItem::render(move |_, window, cx| {
                    struct ProxyState {
                        input: Entity<InputState>,
                        _sub: gpui::Subscription,
                    }
                    let enabled = cx.global::<ConfigStore>().proxy.enabled;
                    if !enabled {
                        return div().into_any_element();
                    }
                    let val = cx.global::<ConfigStore>().proxy.url.clone();
                    let state = window.use_keyed_state::<ProxyState>(
                        "proxy-url-input",
                        cx,
                        |window, cx| {
                            let input = cx.new(|cx| InputState::new(window, cx).default_value(val));
                            let _sub = cx.subscribe(&input, {
                                move |_, emitter, event: &InputEvent, cx| {
                                    if let InputEvent::Change = event {
                                        let v = emitter.read(cx).value();
                                        cx.update_global::<ConfigStore, _>(|store, _| {
                                            store.inner.proxy.url = v.into();
                                        });
                                    }
                                }
                            });
                            ProxyState { input, _sub }
                        },
                    );
                    let theme = cx.theme();
                    h_flex()
                        .gap_2()
                        .child(muted_input(&state.read(cx).input, theme).flex_grow(1.0))
                        .into_any_element()
                }))
        };

        // ── Assemble General page ──────────────────────────────────

        SettingPage::new(t(I18nKey::General, l))
            .icon(Icon::new(IconName::Settings))
            .group(
                SettingGroup::new()
                    .title(t(I18nKey::GeneralOptions, l))
                    .item(SettingItem::render({
                        let app = app.clone();
                        let lang_options = lang_options.clone();
                        move |_, _, cx| {
                            let l = lang(cx);
                            let current = config_str(|c| &c.ui.language)(cx);
                            let app_clone = app.clone();
                            h_flex()
                                .justify_between()
                                .items_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .child(t(I18nKey::Language, l)),
                                )
                                .child(selector(
                                    "lang-select",
                                    lang_options.clone(),
                                    current,
                                    true,
                                    move |v, _, cx| {
                                        cx.update_global::<ConfigStore, _>(|store, _| {
                                            store.inner.ui.language = v.to_string();
                                        });
                                        let _ = app_clone.update_config(
                                            cx.global::<ConfigStore>().inner.clone(),
                                        );
                                    },
                                ))
                        }
                    }))
                    .item(SettingItem::render({
                        let app = app.clone();
                        let theme_style_options = theme_style_options.clone();
                        move |_, _, cx| {
                            let l = lang(cx);
                            let current = config_str(|c| &c.ui.theme_style)(cx);
                            let app_clone = app.clone();
                            h_flex()
                                .justify_between()
                                .items_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .child(t(I18nKey::ThemeStyle, l)),
                                )
                                .child(selector(
                                    "theme-style-select",
                                    theme_style_options.clone(),
                                    current,
                                    false,
                                    move |v, _, cx| {
                                        cx.update_global::<ConfigStore, _>(|store, _| {
                                            store.inner.ui.theme_style = v.to_string();
                                        });
                                        let _ = app_clone.update_config(
                                            cx.global::<ConfigStore>().inner.clone(),
                                        );
                                    },
                                ))
                        }
                    }))
                    .item(SettingItem::render({
                        let app = app.clone();
                        let scale_options = scale_options.clone();
                        move |_, _, cx| {
                            let l = lang(cx);
                            let current =
                                format!("{:.1}", cx.global::<ConfigStore>().ui.ui_scale).into();
                            let app_clone = app.clone();
                            h_flex()
                                .justify_between()
                                .items_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .child(t(I18nKey::UiScale, l)),
                                )
                                .child(selector(
                                    "ui-scale-select",
                                    scale_options.clone(),
                                    current,
                                    false,
                                    move |v, _, cx| {
                                        if let Ok(scale) = v.parse::<f32>() {
                                            cx.update_global::<ConfigStore, _>(|store, _| {
                                                store.inner.ui.ui_scale = scale;
                                            });
                                            let _ = app_clone.update_config(
                                                cx.global::<ConfigStore>().inner.clone(),
                                            );
                                        }
                                    },
                                ))
                        }
                    }))
                    .item(SettingItem::render({
                        let app = app.clone();
                        let log_options = log_options.clone();
                        move |_, _, cx| {
                            let l = lang(cx);
                            let current = config_str(|c| &c.log_level)(cx);
                            let app_clone = app.clone();
                            h_flex()
                                .justify_between()
                                .items_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .child(t(I18nKey::LogLevel, l)),
                                )
                                .child(selector(
                                    "log-level-select",
                                    log_options.clone(),
                                    current,
                                    false,
                                    move |v, _, cx| {
                                        cx.update_global::<ConfigStore, _>(|store, _| {
                                            store.inner.log_level = v.to_string();
                                        });
                                        let _ = app_clone.update_config(
                                            cx.global::<ConfigStore>().inner.clone(),
                                        );
                                    },
                                ))
                        }
                    }))
                    .item(SettingItem::render({
                        let app = app.clone();
                        let notif_options = notif_options.clone();
                        move |_, _, cx| {
                            let l = lang(cx);
                            let current = config_str(|c| &c.notification_level)(cx);
                            let app_clone = app.clone();
                            h_flex()
                                .justify_between()
                                .items_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .child(t(I18nKey::NotificationLevel, l)),
                                )
                                .child(selector(
                                    "notif-level-select",
                                    notif_options.clone(),
                                    current,
                                    false,
                                    move |v, _, cx| {
                                        cx.update_global::<ConfigStore, _>(|store, _| {
                                            store.inner.notification_level = v.to_string();
                                        });
                                        let _ = app_clone.update_config(
                                            cx.global::<ConfigStore>().inner.clone(),
                                        );
                                    },
                                ))
                        }
                    }))
                    .item(SettingItem::render({
                        let app = app.clone();
                        move |_, _, cx| {
                            let l = lang(cx);
                            let theme = cx.theme();
                            let current_mode = config_str(|c| &c.ui.theme_mode)(cx);

                            let mk = |_id: &'static str, val: &'static str, label: SharedString| {
                                let app = app.clone();
                                let is_active = current_mode == val;
                                div()
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .bg(if is_active {
                                        surface.chip_bg
                                    } else {
                                        transparent_black()
                                    })
                                    .text_color(theme.foreground)
                                    .text_sm()
                                    .on_mouse_down(MouseButton::Left, {
                                        move |_, _, cx| {
                                            cx.update_global::<ConfigStore, _>(|store, _| {
                                                store.inner.ui.theme_mode = val.to_string();
                                            });
                                            let _ = app.update_config(
                                                cx.global::<ConfigStore>().inner.clone(),
                                            );
                                        }
                                    })
                                    .child(label)
                            };

                            h_flex()
                                .w_full()
                                .justify_between()
                                .items_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .child(t(I18nKey::Appearance, l)),
                                )
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .p_1()
                                        .bg(theme.muted)
                                        .rounded_md()
                                        .child(mk(
                                            "theme-light",
                                            "light",
                                            t(I18nKey::Light, l).into(),
                                        ))
                                        .child(mk("theme-dark", "dark", t(I18nKey::Dark, l).into()))
                                        .child(mk(
                                            "theme-system",
                                            "system",
                                            t(I18nKey::System, l).into(),
                                        )),
                                )
                        }
                    })),
            )
            .group(library_group)
            .group(pdf_group)
            .group(citation_group)
            .group(proxy_group)
    }
}
