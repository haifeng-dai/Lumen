use components::{IconName, add_drag_behavior};
use gpui::prelude::*;
use gpui::{AnyElement, FontWeight, Window, div, px, rems, size};
use gpui_component::{
    ActiveTheme, Icon,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use i18n::{I18nKey, t};
use services::{app::MainApp, sync::FileLibraryPreflight};
use std::sync::Arc;

impl super::super::MainWindow {
    pub(crate) fn open_file_library_action(
        &mut self,
        preflight: FileLibraryPreflight,
        cx: &mut Context<Self>,
    ) {
        let app = self.app.clone();
        self.open_modal_window(size(px(520.), px(260.)), cx, move |_window, _cx| {
            FileLibraryIdentityDialog::new(app, preflight)
        });
    }
}

struct FileLibraryIdentityDialog {
    app: Arc<MainApp>,
    preflight: FileLibraryPreflight,
    error: Option<String>,
    completed: bool,
}

impl FileLibraryIdentityDialog {
    fn new(app: Arc<MainApp>, preflight: FileLibraryPreflight) -> Self {
        Self {
            app,
            preflight,
            error: None,
            completed: false,
        }
    }
}

impl Render for FileLibraryIdentityDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let lang = self.app.current_language();
        let preflight = self.preflight.clone();

        let title = match &preflight {
            FileLibraryPreflight::InitializationRequired => {
                t(I18nKey::FileLibraryInitRequiredTitle, lang).to_string()
            }
            FileLibraryPreflight::UnidentifiedRemote => {
                t(I18nKey::FileLibraryUnidentified, lang).to_string()
            }
            FileLibraryPreflight::IdentityMismatch { .. } => {
                t(I18nKey::FileLibraryIdentityMismatch, lang).to_string()
            }
            FileLibraryPreflight::Error(e) => e.clone(),
            _ => String::new(),
        };

        let explanation = self.error.clone().unwrap_or_else(|| match &preflight {
            FileLibraryPreflight::InitializationRequired => {
                t(I18nKey::FileLibraryInitRequiredDesc, lang).to_string()
            }
            FileLibraryPreflight::UnidentifiedRemote => {
                t(I18nKey::FileLibraryUnidentified, lang).to_string()
            }
            FileLibraryPreflight::IdentityMismatch {
                local_db_lib_id,
                remote_db_lib_id,
            } => format!(
                "{}\n(Local: {local_db_lib_id}, Remote: {remote_db_lib_id})",
                t(I18nKey::FileLibraryIdentityMismatch, lang)
            ),
            FileLibraryPreflight::Error(e) => e.clone(),
            _ => String::new(),
        });

        let can_confirm = matches!(preflight, FileLibraryPreflight::InitializationRequired);
        let app = self.app.clone();
        let this = cx.entity().downgrade();

        let confirm: AnyElement = if can_confirm && !self.completed {
            Button::new("file-library-init-confirm")
                .label(t(I18nKey::Confirm, lang))
                .primary()
                .on_click(cx.listener(move |_, _, _, cx| {
                    let app = app.clone();
                    let this = this.clone();
                    cx.spawn(async move |_, cx| {
                        match app.confirm_file_library_initialization().await {
                            Ok(_) => {
                                if let Some(this) = this.upgrade() {
                                    this.update(cx, |dialog, cx| {
                                        dialog.completed = true;
                                        cx.notify();
                                    });
                                }
                            }
                            Err(e) => {
                                if let Some(this) = this.upgrade() {
                                    this.update(cx, |dialog, cx| {
                                        dialog.error = Some(e.to_string());
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
                    .id("file-library-identity-drag-overlay")
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
                    Button::new("file-library-identity-close")
                        .ghost()
                        .child(Icon::new(IconName::Close).size(rems(0.75)))
                        .occlude()
                        .on_click(|_, window, _| window.remove_window()),
                ),
            )
            .p_6()
            .gap_4()
            .child(div().font_weight(FontWeight::BOLD).child(title))
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
                        Button::new("file-library-cancel")
                            .label(t(I18nKey::Cancel, lang))
                            .ghost()
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(confirm),
            )
    }
}
