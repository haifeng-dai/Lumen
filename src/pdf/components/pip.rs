use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Context, Entity, ImageSource, InteractiveElement, MouseButton,
    MouseDownEvent, MouseUpEvent, ParentElement, PathPromptOptions, Pixels, Point, Size, Styled,
    Window, div, img, px,
};
use gpui_component::{
    Icon, IconName,
    menu::{PopupMenu, PopupMenuItem},
};
use i18n::I18nKey;
use log::debug;
use std::sync::Arc;

/// 画中画图钉
pub struct PiPPin {
    pub id: String,
    /// 源页面
    pub page: u16,
    /// PDF 坐标中的区域 (x0, y0, x1, y1)
    pub bbox: (f32, f32, f32, f32),
    /// 当前显示位置（屏幕坐标）
    pub position: Point<Pixels>,
    /// 当前显示尺寸
    pub size: Size<Pixels>,
    /// 当前渲染的图像（None 表示正在加载）
    pub image_source: Option<ImageSource>,
    /// 原始 RGBA 像素缓存（供剪贴板复制用）
    pub raw_image: Option<Arc<image::RgbaImage>>,
    /// 当前 image_source 对应的实际渲染 scale
    pub rendered_scale: f32,
    /// 当前正在进行的渲染请求发出时的 scale（用于响应到达后正确更新 rendered_scale）
    pub pending_scale: f32,
    /// 是否已有渲染请求在进行中
    pub render_pending: bool,
}

#[derive(Clone)]
pub struct PiPDragState {
    pub pin_id: String,
    pub offset: Point<Pixels>,
}

#[derive(Clone)]
pub struct PiPResizeState {
    pub pin_id: String,
    pub start_mouse: Point<Pixels>,
    pub start_bounds: Bounds<f32>,
    pub aspect_ratio: f32,
}

/// 缩放手柄尺寸
const HANDLE_SIZE: f32 = 12.0;

impl super::super::PdfReaderView {
    /// 渲染所有 PiP 图钉。
    /// 每个图钉：主体（图片+关闭按钮）+ 右下角缩放手柄。
    /// 若 image_source 为 None 则渲染空占位。
    pub(crate) fn render_pip_pins(
        &self,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.pins.is_empty() {
            return None;
        }

        let mut elements: Vec<AnyElement> = Vec::with_capacity(self.pins.len());
        for pin in &self.pins {
            let pin_id_drag = pin.id.clone();
            let pin_id_close = pin.id.clone();
            let pin_id_resize = pin.id.clone();
            let pin_id_ctx = pin.id.clone();

            let pw = pin.size.width;
            let ph = pin.size.height;
            let pin_img_src = pin.image_source.clone();

            elements.push(
                div()
                    .absolute()
                    .left(pin.position.x)
                    .top(pin.position.y)
                    // Pin 主体
                    .child(
                        div()
                            .w(pw)
                            .h(ph)
                            .rounded_lg()
                            .shadow_xl()
                            .border_1()
                            .border_color(gpui::transparent_black().opacity(0.15))
                            .bg(self.page_color_mode.bg_color())
                            .overflow_hidden()
                            .occlude()
                            .cursor_grab()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                                    let pin_id = pin_id_drag.clone();
                                    if let Some(p) = this.pins.iter().find(|p| p.id == pin_id) {
                                        this.dragging_pin = Some(PiPDragState {
                                            pin_id: pin_id.clone(),
                                            offset: Point {
                                                x: event.position.x - p.position.x,
                                                y: event.position.y - p.position.y,
                                            },
                                        });
                                        cx.notify();
                                    }
                                }),
                            )
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    let has_raw = this
                                        .pins
                                        .iter()
                                        .find(|p| p.id == pin_id_ctx)
                                        .map(|p| p.raw_image.is_some())
                                        .unwrap_or(false);
                                    if !has_raw {
                                        cx.notify();
                                        return;
                                    }
                                    let menu = this.build_pin_context_menu(&pin_id_ctx, window, cx);
                                    let local_pos = gpui::point(
                                        event.position.x,
                                        px((f32::from(event.position.y) - this.tab_bar_offset_px)
                                            .max(0.0)),
                                    );
                                    this.pin_context_menu = Some((local_pos, menu));
                                    cx.notify();
                                }),
                            )
                            .when_some(pin_img_src, |this, src| {
                                this.child(img(src).w_full().h_full())
                            })
                            // 关闭按钮（右上角）
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .right_0()
                                    .px(px(6.0))
                                    .py(px(2.0))
                                    .cursor_pointer()
                                    .occlude()
                                    .child(Icon::new(IconName::Close).size(gpui::rems(0.75)))
                                    .text_color(gpui::rgb(0x000000))
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |this, _event: &MouseUpEvent, window, cx| {
                                                this.pins.retain_mut(|p| {
                                                    if p.id == pin_id_close {
                                                        if let Some(gpui::ImageSource::Render(
                                                            render_img,
                                                        )) = p.image_source.take()
                                                            && let Err(e) =
                                                                window.drop_image(render_img)
                                                        {
                                                            log::error!("drop_image failed: {e}");
                                                        }
                                                        false
                                                    } else {
                                                        true
                                                    }
                                                });
                                                this.dragging_pin = None;
                                                this.resizing_pin = None;
                                                cx.notify();
                                            },
                                        ),
                                    ),
                            ),
                    )
                    // 右下角缩放手柄
                    .child(
                        div()
                            .absolute()
                            .bottom_0()
                            .right_0()
                            .w(px(HANDLE_SIZE))
                            .h(px(HANDLE_SIZE))
                            .bg(gpui::transparent_black().opacity(0.3))
                            .rounded_tl_md()
                            .cursor_nwse_resize()
                            .occlude()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                                    let pid = pin_id_resize.clone();
                                    if let Some(p) = this.pins.iter().find(|p| p.id == pid) {
                                        let w = f32::from(p.size.width);
                                        let h = f32::from(p.size.height);
                                        this.resizing_pin = Some(PiPResizeState {
                                            pin_id: pid,
                                            start_mouse: event.position,
                                            start_bounds: Bounds {
                                                origin: Point {
                                                    x: p.position.x.into(),
                                                    y: p.position.y.into(),
                                                },
                                                size: Size {
                                                    width: w,
                                                    height: h,
                                                },
                                            },
                                            aspect_ratio: w / h,
                                        });
                                        cx.notify();
                                    }
                                }),
                            ),
                    )
                    .into_any_element(),
            );
        }

        Some(
            div()
                .absolute()
                .inset_0()
                .children(elements)
                .into_any_element(),
        )
    }

    /// 为所有 Pin 更新尺寸并发送渲染请求（缩放/底色变化后调用）
    #[allow(dead_code)]
    pub(crate) fn rerender_all_pins(&mut self) {
        if self.pins.is_empty() {
            return;
        }
        let rem_size = self.last_rem_size;
        debug!(
            "pip: 重新渲染全部 {} 个 Pin (zoom={})",
            self.pins.len(),
            self.zoom_level
        );

        for pin in &mut self.pins {
            let (pdf_w, pdf_h) = self
                .page_sizes
                .get(pin.page as usize)
                .copied()
                .unwrap_or((612.0, 792.0));
            let display_w = crate::pdf::PAGE_BASE_WIDTH_REMS * self.zoom_level * rem_size;
            let display_h = display_w * (pdf_h / pdf_w);

            let (bx0, by0, bx1, by1) = pin.bbox;
            let bbox_w = bx1 - bx0;
            let bbox_h = by1 - by0;
            let pdf_pw = pdf_w.max(1.0);
            let new_w = bbox_w / pdf_pw * display_w;
            let new_h = bbox_h / pdf_h * display_h;

            pin.size = Size {
                width: px(new_w),
                height: px(new_h),
            };

            let page = pin.page;
            let pin_id = pin.id.clone();
            let bbox = pin.bbox;
            let current_w = new_w;
            // 分辨率基于当前 CSS 尺寸，独立于 PDF zoom
            let scale = current_w * self.window_scale_factor * 1.2 / bbox_w.max(1.0);
            pin.rendered_scale = scale;
            pin.pending_scale = scale;
            pin.render_pending = true;
            self.pdf_service.send_render_pin(page, pin_id, bbox, scale);
        }
    }

    /// 构建 Pin 右键菜单（返回 PopupMenu 实体）。
    pub(crate) fn build_pin_context_menu(
        &mut self,
        pin_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<PopupMenu> {
        let weak_self = cx.weak_entity();
        let pin_id_copy = pin_id.to_string();
        let pin_id_save = pin_id.to_string();
        let lang = self.language;

        let app: &mut App = cx;
        PopupMenu::build(window, app, move |mut menu, _window, _cx| {
            let weak_copy = weak_self.clone();
            let id_copy = pin_id_copy.clone();
            menu = menu.item(
                PopupMenuItem::new(i18n::t(I18nKey::CopyAsImage, lang)).on_click(
                    move |_, _window, cx| {
                        if let Some(this) = weak_copy.upgrade() {
                            this.update(cx, |this, cx| {
                                this.pin_context_menu = None;
                                this.copy_pin_image(&id_copy);
                                cx.notify();
                            });
                        }
                    },
                ),
            );

            let weak_save = weak_self.clone();
            let id_save = pin_id_save.clone();
            menu = menu.item(
                PopupMenuItem::new(i18n::t(I18nKey::SaveAsImage, lang)).on_click(
                    move |_, _window, cx| {
                        if let Some(this) = weak_save.upgrade() {
                            this.update(cx, |this, cx| {
                                this.pin_context_menu = None;
                                this.save_pin_image(&id_save, cx);
                                cx.notify();
                            });
                        }
                    },
                ),
            );

            menu
        })
    }

    /// 将指定 Pin 的原始渲染图复制到系统剪贴板。
    fn copy_pin_image(&mut self, pin_id: &str) {
        let raw = self
            .pins
            .iter()
            .find(|p| p.id == pin_id)
            .and_then(|p| p.raw_image.clone());
        if let Some(img) = raw {
            crate::pdf::helpers::copy_rgba_to_clipboard(&img);
        }
    }

    /// 将指定 Pin 的原始渲染图另存为 PNG 文件。
    fn save_pin_image(&mut self, pin_id: &str, cx: &mut Context<Self>) {
        let raw = self
            .pins
            .iter()
            .find(|p| p.id == pin_id)
            .and_then(|p| p.raw_image.clone());
        let Some(img) = raw else { return };
        let name = self.document_title.clone();

        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择保存位置".into()),
        });

        cx.background_executor()
            .spawn(async move {
                if let Ok(Ok(Some(paths))) = receiver.await
                    && let Some(dir) = paths.first()
                {
                    let path = dir.join(format!("{}.png", name));
                    let _ = img.save(&path);
                }
            })
            .detach();
    }
}
