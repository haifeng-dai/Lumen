use components::{IconName, add_drag_behavior, make_window_controls};
use gpui::prelude::*;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, ScrollHandle, SharedString, Window, div, px, rems,
};
#[cfg(not(windows))]
use gpui_component::InteractiveElementExt;
use gpui_component::{ActiveTheme, Icon, h_flex};
use log::info;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::app_state::config::ConfigStore;
use crate::pdf::PdfReaderView;
use crate::ui::views::main_window::AppPdfDelegate;
use models::Literature;
use services::app::MainApp;

pub struct PdfWindowController {
    app: Arc<MainApp>,
    main_window_handle: Option<gpui::AnyWindowHandle>,
    active_tab_id: Option<String>,
    open_pdf_tabs: HashMap<String, Option<Entity<PdfReaderView>>>,
    open_pdf_tab_order: Vec<String>,
    pdf_tab_titles: HashMap<String, String>,
    pdf_tab_paths: HashMap<String, (Arc<Literature>, Option<PathBuf>)>,
    tab_scroll_handle: ScrollHandle,
}

impl PdfWindowController {
    pub fn new(
        app: Arc<MainApp>,
        main_window_handle: Option<gpui::AnyWindowHandle>,
        cx: &mut Context<Self>,
    ) -> Self {
        // 监听全局主题和配置更新，使独立窗口能实时同步重绘
        cx.observe_global::<gpui_component::Theme>(|_, cx| {
            cx.notify();
        })
        .detach();

        cx.observe_global::<ConfigStore>(|_, cx| {
            cx.notify();
        })
        .detach();

        Self {
            app,
            main_window_handle,
            active_tab_id: None,
            open_pdf_tabs: HashMap::new(),
            open_pdf_tab_order: Vec::new(),
            pdf_tab_titles: HashMap::new(),
            pdf_tab_paths: HashMap::new(),
            tab_scroll_handle: ScrollHandle::new(),
        }
    }

    pub fn open_pdf(&mut self, lit: Arc<Literature>, path: PathBuf, cx: &mut Context<Self>) {
        let doc_id = lit
            .attachments
            .iter()
            .find(|a| a.file_path == path.to_string_lossy())
            .map(|a| format!("{}::{}", lit.id, a.id))
            .unwrap_or_else(|| lit.id.clone());

        if self.open_pdf_tabs.contains_key(&doc_id) {
            info!("PdfWindowController: PDF 已打开，切换标签: {doc_id}");
            self.activate_pdf_tab(doc_id, cx);
            return;
        }

        self.pdf_tab_titles
            .insert(doc_id.clone(), lit.title.clone());
        self.pdf_tab_paths
            .insert(doc_id.clone(), (lit.clone(), Some(path)));

        self.open_pdf_tabs.insert(doc_id.clone(), None);
        self.open_pdf_tab_order.push(doc_id.clone());

        self.activate_pdf_tab(doc_id, cx);
    }

    pub fn activate_pdf_tab(&mut self, doc_id: String, cx: &mut Context<Self>) {
        self.active_tab_id = Some(doc_id.clone());

        if let Some(tab_index) = self.open_pdf_tab_order.iter().position(|id| id == &doc_id) {
            self.tab_scroll_handle.scroll_to_item(tab_index);
        }

        if self.open_pdf_tabs.get(&doc_id).is_none_or(|v| v.is_none()) {
            self.reload_pdf_tab(doc_id, cx);
        }
        cx.notify();
    }

    fn reload_pdf_tab(&mut self, doc_id: String, cx: &mut Context<Self>) {
        if let Some((lit, preferred_path)) = self.pdf_tab_paths.get(&doc_id).cloned() {
            let path = preferred_path.clone().or_else(|| {
                lit.attachments
                    .iter()
                    .find(|a| a.is_main)
                    .map(|a| PathBuf::from(&a.file_path))
            });
            let Some(path) = path else {
                return;
            };

            let app = self.app.clone();
            let doc_id_for_open = doc_id.clone();
            let lit_id = lit.id.clone();

            let (pdf_service, response_rx) =
                services::pdf::PdfService::new(path.clone()).expect("Failed to create PdfService");
            let delegate = Arc::new(AppPdfDelegate {
                app: app.clone(),
                literature_id: lit_id,
                pdf: services::library::PdfPersistence::new(),
            });

            let view = cx.new(|cx| {
                let mut view = PdfReaderView::new(
                    pdf_service,
                    Some(delegate),
                    doc_id_for_open,
                    path.clone(),
                    cx,
                );
                view.set_document_title(lit.title.clone());
                view.init_workers(response_rx, cx);
                view
            });

            // 观察全局配置更新（如语言等）
            cx.observe_global::<ConfigStore>({
                let view_weak = view.downgrade();
                move |_this: &mut Self, cx: &mut gpui::Context<Self>| {
                    if let Some(view) = view_weak.upgrade() {
                        view.update(cx, |this, cx| {
                            let lang = cx.global::<ConfigStore>().current_language();
                            this.set_language(lang, cx);
                        });
                    }
                }
            })
            .detach();

            self.open_pdf_tabs.insert(doc_id, Some(view));
        }
    }

    pub fn close_pdf_tab(&mut self, doc_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Some(view)) = self.open_pdf_tabs.get(doc_id) {
            let images_to_drop = view.update(cx, |this, _cx| this.drain_images_to_drop());
            let mut count = 0;
            for img in images_to_drop {
                if let Err(e) = window.drop_image(img) {
                    log::error!("drop_image failed: {e}");
                }
                count += 1;
            }
            info!(
                "PdfWindowController: 已显式从 Window Sprite Atlas 释放 {} 个 PDF 纹理",
                count
            );
        }

        self.open_pdf_tabs.remove(doc_id);
        self.pdf_tab_titles.remove(doc_id);
        self.pdf_tab_paths.remove(doc_id);
        self.open_pdf_tab_order.retain(|id| id != doc_id);

        if Some(doc_id.to_string()) == self.active_tab_id {
            self.active_tab_id = self.open_pdf_tab_order.last().cloned();
            if let Some(ref active_id) = self.active_tab_id {
                self.activate_pdf_tab(active_id.clone(), cx);
            }
        }

        // 如果全部 Tab 已关闭，自动销毁当前独立 PDF 窗口
        if self.open_pdf_tab_order.is_empty() {
            info!("PdfWindowController: 所有 PDF 标签已关闭，销毁 PDF 窗口");
            window.remove_window();
        } else {
            cx.notify();
        }
    }

    pub fn drain_all_tab_images(&mut self, cx: &mut Context<Self>) -> Vec<Arc<gpui::RenderImage>> {
        let mut all_images = Vec::new();
        for view in self.open_pdf_tabs.values_mut().flatten() {
            view.update(cx, |v, _| {
                all_images.extend(v.drain_images_to_drop());
            });
        }
        all_images
    }

    pub fn reload_all_pdf_tabs(&mut self, cx: &mut Context<Self>) {
        for view in self.open_pdf_tabs.values().flatten() {
            view.update(cx, |v, cx| {
                v.reload_notes(cx);
                v.reload_chat_sessions(cx);
            });
        }
    }

    fn active_pdf_view(&self) -> Option<Entity<PdfReaderView>> {
        self.active_tab_id
            .as_ref()
            .and_then(|id| self.open_pdf_tabs.get(id).cloned().flatten())
    }

    fn render_title_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();

        let title_bar = div()
            .id("pdf-title-bar")
            .flex()
            .flex_row()
            .h(rems(2.2))
            .flex_shrink_0()
            .items_center()
            .bg(theme.title_bar);

        // macOS 左上角原生 traffic lights 避让区：固定宽度的保留区，
        // 加在滚动容器之外（不随标签页滚动），并作为可拖拽区域（双击可最大化/还原）。
        // 先在闭包外构建好，避免 .when 闭包捕获 cx 与 theme 的借用冲突。
        let macos_traffic_safe = {
            let safe = div()
                .id("pdf-macos-traffic-safe")
                .h_full()
                .w(rems(5.0))
                .flex_shrink_0();
            #[cfg(not(windows))]
            let safe = safe.on_double_click(|_, window, _| window.zoom_window());
            add_drag_behavior(safe, window, cx)
        };

        // 主页按钮：固定在滚动容器之外，始终可点击切换主页
        let home_button = {
            let handle = self.main_window_handle;
            div()
                .id("btn-home")
                .px(rems(0.75))
                .h_full()
                .flex()
                .items_center()
                .rounded_sm()
                .cursor_pointer()
                .hover(|this| this.bg(theme.primary.opacity(0.15)))
                .child(Icon::new(IconName::Home).size(px(16.0)))
                .on_click(move |_, _window, cx| {
                    if let Some(handle) = handle {
                        let _ = handle.update(cx, |_, window, _| {
                            window.activate_window();
                        });
                    }
                })
        };

        title_bar
            .when(cfg!(target_os = "macos"), |this| {
                this.child(macos_traffic_safe)
            })
            .child(home_button)
            // 第一段：标签页区域（自适应宽度，横向可滚动）
            .child({
                h_flex()
                    .id("pdf-tabs-area")
                    .h_full()
                    .items_center()
                    .overflow_x_scroll()
                    .track_scroll(&self.tab_scroll_handle)
                    .children(self.open_pdf_tab_order.iter().map(|doc_id| {
                        let is_active = Some(doc_id.to_string()) == self.active_tab_id;
                        let title = self
                            .pdf_tab_titles
                            .get(doc_id)
                            .map(|s| s.as_str())
                            .unwrap_or(doc_id);
                        let tab_id: SharedString = format!("tab-pdf-{doc_id}").into();
                        let doc_id_for_click = doc_id.clone();
                        let doc_id_for_close = doc_id.clone();

                        div()
                            .id(tab_id)
                            .px(rems(0.75))
                            .h_full()
                            .flex()
                            .items_center()
                            .rounded_sm()
                            .cursor_pointer()
                            .when(is_active, |this| {
                                this.bg(theme.primary).text_color(theme.primary_foreground)
                            })
                            .when(!is_active, |this| {
                                this.hover(|this| this.bg(theme.primary.opacity(0.15)))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.activate_pdf_tab(doc_id_for_click.clone(), cx);
                            }))
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(
                                        div()
                                            .max_w(rems(12.0))
                                            .truncate()
                                            .text_size(rems(0.85))
                                            .child(title.to_string()),
                                    )
                                    .child(
                                        div()
                                            .id(format!("close-{}", doc_id_for_close))
                                            .cursor_pointer()
                                            .w(rems(1.25))
                                            .h(rems(1.25))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded_sm()
                                            .hover(|this| {
                                                this.bg(theme.danger)
                                                    .text_color(theme.danger_foreground)
                                            })
                                            .on_click(cx.listener(
                                                move |this, _event, window, cx| {
                                                    // 阻止 click 冒泡到父级标签页，避免关闭时顺带激活该标签页
                                                    cx.stop_propagation();
                                                    this.close_pdf_tab(
                                                        &doc_id_for_close,
                                                        window,
                                                        cx,
                                                    );
                                                },
                                            ))
                                            .child(Icon::new(IconName::Close).size(rems(1.0))),
                                    ),
                            )
                            .into_any_element()
                    }))
            })
            // 第二段：可拖拽弹性区域
            .child({
                let spacer = div().id("pdf-drag-area").h_full().flex_grow(1.0);

                #[cfg(not(windows))]
                let spacer = spacer.on_double_click(|_, window, _| window.zoom_window());

                add_drag_behavior(spacer, window, cx)
            })
            // 第三段：窗口控件（macOS 使用原生 traffic lights，Windows/Linux 渲染自定义控件）
            .when(!cfg!(target_os = "macos"), |this| {
                this.child(make_window_controls(window, cx))
            })
    }
}

impl Render for PdfWindowController {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 根据窗口当前 rem_size 实时计算标题栏像素高度，确保坐标精度
        if let Some(view) = self.active_pdf_view() {
            let actual_offset = rems(2.2).to_pixels(window.rem_size());
            view.update(cx, |v, _| {
                v.set_tab_bar_offset_px(f32::from(actual_offset));
            });
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .child(self.render_title_bar(window, cx))
            .child(
                div()
                    .flex_grow(1.0)
                    .child(if let Some(view) = self.active_pdf_view() {
                        view.into_any_element()
                    } else {
                        div().into_any_element()
                    }),
            )
    }
}

impl Focusable for PdfWindowController {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if let Some(view) = self.active_pdf_view() {
            view.focus_handle(cx)
        } else {
            cx.focus_handle()
        }
    }
}
