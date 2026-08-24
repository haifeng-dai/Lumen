use crate::engine::MathEngine;
use crate::layout_tree::MathNode;
use gpui::prelude::*;
use gpui::*;

/// 纯 GPUI 原生 LaTeX 渲染元素
#[derive(Clone)]
pub struct MathElement {
    latex: SharedString,
    text_size: Pixels,
    color: Option<Hsla>,
    is_display: bool,
}

impl MathElement {
    /// 创建一个新的 LaTeX 公式元素
    pub fn new(latex: impl Into<SharedString>) -> Self {
        Self {
            latex: latex.into(),
            text_size: px(14.0),
            color: None,
            is_display: false,
        }
    }

    /// 设置公式字号
    pub fn text_size(mut self, size: Pixels) -> Self {
        self.text_size = size;
        self
    }

    /// 设置公式颜色（默认取当前主题前景文字色）
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// 设置是否为独立块级公式（Display Mode）
    pub fn display(mut self, is_display: bool) -> Self {
        self.is_display = is_display;
        self
    }
}

impl IntoElement for MathElement {
    type Element = Div;

    fn into_element(self) -> Self::Element {
        let layout_res =
            MathEngine::layout(&self.latex, self.text_size, self.is_display).unwrap_or_default();

        let container_w = layout_res.width;
        let container_h = layout_res.height;
        let depth = layout_res.depth;

        let mut root = div()
            .relative()
            .w(container_w)
            .h(container_h)
            .overflow_hidden();

        for node in layout_res.nodes {
            match node {
                MathNode::Text {
                    text,
                    font_size,
                    position,
                } => {
                    let left = position.x;
                    let top = container_h - depth - position.y - font_size;

                    // 智能正斜体与字体判定：变量字母（包括拉丁 a-z、希腊字母 α, β, θ 等）使用 KaTeX_Math 斜体，数字、函数名、运算符与括号使用 KaTeX_Main 正体
                    let is_variable = text.chars().next().map_or(false, |c| c.is_alphabetic())
                        && text.len() <= 4; // 排除长函数名如 sin, cos, softmax

                    let font_fam = if is_variable {
                        crate::font::KATEX_MATH_FONT
                    } else {
                        crate::font::KATEX_MAIN_FONT
                    };

                    let mut text_div = div()
                        .absolute()
                        .left(left)
                        .top(top)
                        .text_size(font_size)
                        .font_family(gpui::SharedString::from(font_fam))
                        .when(is_variable, |this| this.italic())
                        .child(text);

                    if let Some(c) = self.color {
                        text_div = text_div.text_color(c);
                    }

                    root = root.child(text_div);
                }
                MathNode::Glyph {
                    glyph_id: _,
                    font_size,
                    position,
                } => {
                    let left = position.x;
                    let top = container_h - depth - position.y - font_size;
                    let glyph_div = div().absolute().left(left).top(top);
                    root = root.child(glyph_div);
                }
                MathNode::Rule { bounds } => {
                    let left = bounds.origin.x;
                    let top = container_h - depth - bounds.origin.y;
                    let w = bounds.size.width;
                    let h = bounds.size.height;

                    let mut line_div = div().absolute().left(left).top(top).w(w).h(h);

                    if let Some(c) = self.color {
                        line_div = line_div.bg(c);
                    } else {
                        line_div = line_div.bg(gpui::white());
                    }

                    root = root.child(line_div);
                }
            }
        }

        root
    }
}
