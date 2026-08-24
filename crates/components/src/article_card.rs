use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, FontWeight, Hsla, MouseButton, MouseDownEvent, SharedString, Window,
    div, rems,
};
use gpui_component::{ActiveTheme, h_flex, v_flex};

/// 文献/订阅条目通用 3 行卡片组件
#[derive(IntoElement)]
pub struct ArticleCard {
    pub id: SharedString,
    pub title: SharedString,
    pub authors: SharedString,
    pub meta: SharedString,
    pub status_pill: Option<Hsla>,
    pub is_selected: bool,
    pub is_bold: bool,
    pub top_right: Option<AnyElement>,
    pub bottom_right: Option<AnyElement>,
    pub on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static>>,
    pub on_right_mouse_down: Option<Box<dyn Fn(&MouseDownEvent, &mut Window, &mut gpui::App) + 'static>>,
}

impl ArticleCard {
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: SharedString::default(),
            authors: SharedString::default(),
            meta: SharedString::default(),
            status_pill: None,
            is_selected: false,
            is_bold: true,
            top_right: None,
            bottom_right: None,
            on_click: None,
            on_right_mouse_down: None,
        }
    }

    pub fn title(mut self, v: impl Into<SharedString>) -> Self { self.title = v.into(); self }
    pub fn authors(mut self, v: impl Into<SharedString>) -> Self { self.authors = v.into(); self }
    pub fn meta(mut self, v: impl Into<SharedString>) -> Self { self.meta = v.into(); self }
    pub fn status_pill(mut self, v: Option<Hsla>) -> Self { self.status_pill = v; self }
    pub fn selected(mut self, v: bool) -> Self { self.is_selected = v; self }
    pub fn bold(mut self, v: bool) -> Self { self.is_bold = v; self }
    pub fn top_right(mut self, v: impl IntoElement) -> Self { self.top_right = Some(v.into_any_element()); self }
    pub fn bottom_right(mut self, v: impl IntoElement) -> Self { self.bottom_right = Some(v.into_any_element()); self }
    pub fn on_click(mut self, h: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static) -> Self { self.on_click = Some(Box::new(h)); self }
    pub fn on_right_mouse_down(mut self, h: impl Fn(&MouseDownEvent, &mut Window, &mut gpui::App) + 'static) -> Self { self.on_right_mouse_down = Some(Box::new(h)); self }
}

impl RenderOnce for ArticleCard {
    fn render(self, _window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let theme = cx.theme();
        let is_selected = self.is_selected;

        let mut card = div()
            .id(self.id)
            .w_full()
            .rounded_md()
            .overflow_hidden()
            .border_y_1()
            .border_color(theme.border)
            .when(is_selected, |s| s.bg(theme.primary).text_color(theme.primary_foreground))
            .when(!is_selected, |s| s.hover(|s| s.bg(theme.primary.opacity(0.08))));

        if let Some(on_click) = self.on_click {
            card = card.on_click(on_click);
        }
        if let Some(on_rmd) = self.on_right_mouse_down {
            card = card.on_mouse_down(MouseButton::Right, on_rmd);
        }

        let meta_row = div().overflow_hidden().text_xs().text_ellipsis().child(self.meta);

        card.child(
            v_flex()
                .w_full()
                .py(rems(0.3125))
                .px_2()
                // 第 1 行：状态胶囊柱 + 标题 + 右侧插槽
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_start()
                        .gap_2()
                        .child(
                            h_flex()
                                .flex_grow(1.0)
                                .min_w_0()
                                .gap_2()
                                .items_center()
                                .when_some(self.status_pill, |this, pill| {
                                    this.child(
                                        div()
                                            .w(rems(0.1875))
                                            .h(rems(0.875))
                                            .flex_shrink_0()
                                            .rounded_full()
                                            .bg(if is_selected { theme.primary_foreground } else { pill }),
                                    )
                                })
                                .child(
                                    div()
                                        .flex_grow(1.0)
                                        .overflow_hidden()
                                        .text_sm()
                                        .font_weight(if self.is_bold { FontWeight::BOLD } else { FontWeight::NORMAL })
                                        .text_color(if is_selected { theme.primary_foreground } else { theme.foreground })
                                        .text_ellipsis()
                                        .child(self.title),
                                ),
                        )
                        .children(self.top_right),
                )
                // 第 2 行：作者列表
                .child(
                    div()
                        .w_full()
                        .overflow_hidden()
                        .text_xs()
                        .line_height(rems(1.0))
                        .text_color(if is_selected { theme.primary_foreground } else { theme.foreground })
                        .text_ellipsis()
                        .child(self.authors),
                )
                // 第 3 行：元数据行（统一为柔和灰色 theme.muted_foreground）+ 底部右侧插槽
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .line_height(rems(1.0))
                        .child(
                            h_flex()
                                .gap_1()
                                .flex_grow(1.0)
                                .min_w_0()
                                .overflow_hidden()
                                .text_xs()
                                .text_color(if is_selected { theme.primary_foreground } else { theme.muted_foreground })
                                .child(meta_row),
                        )
                        .children(self.bottom_right),
                ),
        )
    }
}
