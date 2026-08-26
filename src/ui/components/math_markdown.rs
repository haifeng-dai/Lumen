use gpui::prelude::*;
use gpui::{App, Hsla, Pixels, SharedString, div, px};
use gpui_component::ActiveTheme;
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::v_flex;
use latex::MathElement;

enum MarkdownSegment {
    /// 普通 Markdown 文本内容（交由 TextView 渲染）
    Text(String),
    /// 原生 LaTeX 公式（100% 交由 MathElement 矢量引擎渲染）
    Math { latex: String, is_display: bool },
}

#[derive(Debug, Clone, PartialEq)]
enum MathNode {
    Sequence(Vec<MathNode>),
    Text(String),
    Command {
        name: String,
        args: Vec<MathNode>,
    },
    Subscript {
        base: Box<MathNode>,
        script: Box<MathNode>,
    },
    Superscript {
        base: Box<MathNode>,
        script: Box<MathNode>,
    },
}

struct MathParser {
    chars: Vec<char>,
    position: usize,
}

impl MathParser {
    fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            position: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.position += 1;
        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.position += 1;
        }
    }

    fn parse(&mut self) -> MathNode {
        self.parse_sequence(None)
    }

    fn parse_sequence(&mut self, stop: Option<char>) -> MathNode {
        let mut nodes = Vec::new();
        while let Some(ch) = self.peek() {
            if Some(ch) == stop {
                break;
            }

            let mut node = self.parse_atom();
            loop {
                match self.peek() {
                    Some('_') => {
                        self.next();
                        node = MathNode::Subscript {
                            base: Box::new(node),
                            script: Box::new(self.parse_script_argument()),
                        };
                    }
                    Some('^') => {
                        self.next();
                        node = MathNode::Superscript {
                            base: Box::new(node),
                            script: Box::new(self.parse_script_argument()),
                        };
                    }
                    _ => break,
                }
            }
            nodes.push(node);
        }
        MathNode::Sequence(nodes)
    }

    fn parse_atom(&mut self) -> MathNode {
        match self.peek() {
            Some('\\') => self.parse_command(),
            Some('{') => self.parse_group(),
            Some(_) => MathNode::Text(self.next().unwrap().to_string()),
            None => MathNode::Text(String::new()),
        }
    }

    fn parse_group(&mut self) -> MathNode {
        self.next();
        let node = self.parse_sequence(Some('}'));
        if self.peek() == Some('}') {
            self.next();
        }
        node
    }

    fn parse_script_argument(&mut self) -> MathNode {
        self.skip_whitespace();
        if self.peek() == Some('{') {
            self.parse_group()
        } else {
            self.parse_atom()
        }
    }

    fn parse_command(&mut self) -> MathNode {
        self.next();
        let Some(first) = self.next() else {
            return MathNode::Text("\\".to_string());
        };

        if matches!(first, '{' | '}' | '|') {
            return MathNode::Text(first.to_string());
        }
        if matches!(first, ';' | ',' | ':') {
            return MathNode::Text(" ".to_string());
        }
        if first == '!' {
            return MathNode::Text(String::new());
        }

        let mut name = String::from(first);
        while self.peek().is_some_and(char::is_alphabetic) {
            name.push(self.next().unwrap());
        }

        let args = match name.as_str() {
            "frac" | "dfrac" | "tfrac" => {
                vec![self.parse_script_argument(), self.parse_script_argument()]
            }
            "sqrt" => {
                self.skip_whitespace();
                if self.peek() == Some('[') {
                    self.next();
                    while let Some(ch) = self.next() {
                        if ch == ']' {
                            break;
                        }
                    }
                }
                vec![self.parse_script_argument()]
            }
            "text" | "mathrm" | "mathbf" | "bm" | "boldsymbol" | "mathbb" | "mathcal" => {
                vec![self.parse_script_argument()]
            }
            "left" | "right" => {
                self.skip_whitespace();
                let delimiter = if self.peek() == Some('\\') {
                    self.next();
                    self.next().unwrap_or_default()
                } else {
                    self.next().unwrap_or_default()
                };
                if delimiter == '.' {
                    Vec::new()
                } else {
                    vec![MathNode::Text(delimiter.to_string())]
                }
            }
            _ => Vec::new(),
        };

        MathNode::Command { name, args }
    }
}

/// 校验行内公式的层级结构并保留原始 LaTeX。
/// 实际排版统一交给 MathElement，避免复杂文本上下标退回普通 Markdown。
fn format_inline_math(math: &str) -> String {
    let mut parser = MathParser::new(math);
    let _ = parser.parse();
    math.to_string()
}
/// 第一阶段：优先扫描切分出块级 LaTeX 公式段与 Markdown 文本段（行内公式保留在段落文本流中）
fn parse_markdown_segments(text: &str) -> Vec<MarkdownSegment> {
    let mut segments = Vec::new();
    let mut current_text = String::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // 遇到代码块 ``` 则跳过其内部可能出现的数学公式符号
        if i + 2 < len && chars[i] == '`' && chars[i + 1] == '`' && chars[i + 2] == '`' {
            current_text.push_str("```");
            i += 3;
            while i < len {
                if i + 2 < len && chars[i] == '`' && chars[i + 1] == '`' && chars[i + 2] == '`' {
                    current_text.push_str("```");
                    i += 3;
                    break;
                }
                current_text.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // 1. 遇到 \[...\] 块级公式（始终作为独立块级排版）
        if i + 1 < len && chars[i] == '\\' && chars[i + 1] == '[' {
            i += 2;
            let start = i;
            let mut closed = false;

            while i + 1 < len {
                if chars[i] == '\\' && chars[i + 1] == ']' {
                    closed = true;
                    break;
                }
                i += 1;
            }

            if closed {
                let math_content: String = chars[start..i].iter().collect();
                i += 2; // 跳过 \]

                if !current_text.trim().is_empty() {
                    segments.push(MarkdownSegment::Text(std::mem::take(&mut current_text)));
                } else {
                    current_text.clear();
                }

                let trimmed_math = math_content.trim();
                if !trimmed_math.is_empty() {
                    segments.push(MarkdownSegment::Math {
                        latex: trimmed_math.to_string(),
                        is_display: true,
                    });
                }
            } else {
                let remaining_math: String = chars[start..len].iter().collect();
                let trimmed_math = remaining_math.trim();
                if !trimmed_math.is_empty() {
                    if !current_text.trim().is_empty() {
                        segments.push(MarkdownSegment::Text(std::mem::take(&mut current_text)));
                    }
                    segments.push(MarkdownSegment::Math {
                        latex: trimmed_math.to_string(),
                        is_display: true,
                    });
                } else {
                    current_text.push_str("\\[");
                }
                break;
            }
            continue;
        }

        // 2. 遇到 $$...$$ 块级公式（始终作为独立块级排版）
        if i + 1 < len && chars[i] == '$' && chars[i + 1] == '$' {
            i += 2;
            let start = i;
            let mut closed = false;

            while i + 1 < len {
                if chars[i] == '$' && chars[i + 1] == '$' {
                    closed = true;
                    break;
                }
                i += 1;
            }

            if closed {
                let math_content: String = chars[start..i].iter().collect();
                i += 2; // 跳过闭合 $$

                if !current_text.trim().is_empty() {
                    segments.push(MarkdownSegment::Text(std::mem::take(&mut current_text)));
                } else {
                    current_text.clear();
                }

                let trimmed_math = math_content.trim();
                if !trimmed_math.is_empty() {
                    segments.push(MarkdownSegment::Math {
                        latex: trimmed_math.to_string(),
                        is_display: true,
                    });
                }
            } else {
                let remaining_math: String = chars[start..len].iter().collect();
                let trimmed_math = remaining_math.trim();
                if !trimmed_math.is_empty() {
                    if !current_text.trim().is_empty() {
                        segments.push(MarkdownSegment::Text(std::mem::take(&mut current_text)));
                    }
                    segments.push(MarkdownSegment::Math {
                        latex: trimmed_math.to_string(),
                        is_display: true,
                    });
                } else {
                    current_text.push_str("$$");
                }
                break;
            }
            continue;
        }

        // 3. 遇到 \(...\) 公式
        if i + 1 < len && chars[i] == '\\' && chars[i + 1] == '(' {
            i += 2;
            let start = i;
            let mut closed = false;

            while i + 1 < len {
                if chars[i] == '\\' && chars[i + 1] == ')' {
                    closed = true;
                    break;
                }
                i += 1;
            }

            if closed {
                let math_content: String = chars[start..i].iter().collect();
                i += 2; // 跳过 \)

                let trimmed_math = math_content.trim();
                let is_standalone_line = (current_text.trim().is_empty()
                    || current_text.ends_with('\n'))
                    && (i >= len || chars[i] == '\n' || chars[i] == '\r');

                if is_standalone_line
                    && (trimmed_math.contains('=') || trimmed_math.contains("\\frac"))
                {
                    if !current_text.trim().is_empty() {
                        segments.push(MarkdownSegment::Text(std::mem::take(&mut current_text)));
                    } else {
                        current_text.clear();
                    }
                    segments.push(MarkdownSegment::Math {
                        latex: trimmed_math.to_string(),
                        is_display: true,
                    });
                } else if !trimmed_math.is_empty() {
                    if !current_text.trim().is_empty() {
                        segments.push(MarkdownSegment::Text(std::mem::take(&mut current_text)));
                    }
                    segments.push(MarkdownSegment::Math {
                        latex: format_inline_math(trimmed_math),
                        is_display: false,
                    });
                }
            } else {
                current_text.push_str("\\(");
            }
            continue;
        }

        // 4. 遇到 $...$ 行内/单行公式
        if chars[i] == '$' {
            i += 1;
            let start = i;
            let mut closed = false;

            while i < len {
                if chars[i] == '$' && (i == 0 || chars[i - 1] != '\\') {
                    closed = true;
                    break;
                }
                i += 1;
            }

            if closed {
                let math_content: String = chars[start..i].iter().collect();
                i += 1; // 跳过闭合 $

                let trimmed_math = math_content.trim();
                // 只有当该公式独占单独一行且包含等式或分式时，才切分为独立块级渲染
                let is_standalone_line = (current_text.trim().is_empty()
                    || current_text.ends_with('\n'))
                    && (i >= len || chars[i] == '\n' || chars[i] == '\r');

                if is_standalone_line
                    && (trimmed_math.contains('=')
                        || trimmed_math.contains("\\frac")
                        || trimmed_math.contains("\\argmax"))
                {
                    if !current_text.trim().is_empty() {
                        segments.push(MarkdownSegment::Text(std::mem::take(&mut current_text)));
                    } else {
                        current_text.clear();
                    }
                    segments.push(MarkdownSegment::Math {
                        latex: trimmed_math.to_string(),
                        is_display: true,
                    });
                } else if !trimmed_math.is_empty() {
                    // 行内公式也交给 MathElement，避免复杂上下标退回普通 Markdown 文本。
                    if !current_text.trim().is_empty() {
                        segments.push(MarkdownSegment::Text(std::mem::take(&mut current_text)));
                    }
                    segments.push(MarkdownSegment::Math {
                        latex: format_inline_math(trimmed_math),
                        is_display: false,
                    });
                }
            } else {
                current_text.push('$');
            }
            continue;
        }

        current_text.push(chars[i]);
        i += 1;
    }

    if !current_text.trim().is_empty() {
        segments.push(MarkdownSegment::Text(current_text));
    }

    segments
}

use gpui_component::Icon;
use gpui_component::IconName;
use gpui_component::h_flex;

fn render_inline_flow(
    base_id: &str,
    parts: Vec<(usize, MarkdownSegment)>,
    font_size: Pixels,
    text_color: Hsla,
    heading_style: Option<&TextViewStyle>,
) -> impl IntoElement {
    let mut flow = div()
        .w_full()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x_0()
        .gap_y_1();

    for (ix, segment) in parts {
        match segment {
            MarkdownSegment::Text(text) => {
                for (part_ix, part) in split_inline_text(&text).into_iter().enumerate() {
                    let mut view = TextView::markdown(
                        SharedString::from(format!("{base_id}-text-{ix}-{part_ix}")),
                        SharedString::from(part),
                    )
                    .selectable(true)
                    .text_size(font_size)
                    .text_color(text_color);
                    if let Some(style) = heading_style {
                        view = view.style(style.clone());
                    }
                    flow = flow.child(view);
                }
            }
            MarkdownSegment::Math {
                latex,
                is_display: false,
            } => {
                flow = flow.child(
                    MathElement::new(latex)
                        .text_size(font_size)
                        .color(text_color)
                        .display(false),
                );
            }
            MarkdownSegment::Math {
                is_display: true, ..
            } => unreachable!("display math must be rendered outside the inline flow"),
        }
    }

    flow
}

fn split_inline_text(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();

    let flush = |parts: &mut Vec<String>, current: &mut String| {
        if !current.is_empty() {
            parts.push(std::mem::take(current));
        }
    };

    for ch in text.chars() {
        if ch == '\n' || ch == '\r' {
            flush(&mut parts, &mut current);
            parts.push(ch.to_string());
        } else if ch.is_whitespace() {
            current.push(ch);
        } else if is_cjk_character(ch) || ch.is_ascii_punctuation() {
            flush(&mut parts, &mut current);
            parts.push(ch.to_string());
        } else {
            current.push(ch);
        }
    }
    flush(&mut parts, &mut current);
    parts
}

fn is_cjk_character(ch: char) -> bool {
    matches!(
        ch,
        '\u{2E80}'..='\u{2FFF}'
            | '\u{3000}'..='\u{303F}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
    )
}

/// 渲染包含纯原生 LaTeX 矢量公式的 Markdown 视图（支持公式段优先切分与原生矢量排版）
pub fn render_math_markdown(
    base_id: impl Into<SharedString>,
    content: &str,
    font_size: Pixels,
    color: Option<Hsla>,
    heading_style: Option<TextViewStyle>,
    cx: &mut App,
) -> impl IntoElement {
    let base_id_str = base_id.into().to_string();
    let theme = cx.theme().clone();
    let theme_foreground = theme.foreground;
    let text_color = color.unwrap_or(theme_foreground);
    let copied_formula_id = cx
        .global::<crate::app_state::ui::UiState>()
        .copied_formula_id
        .clone();

    let segments = parse_markdown_segments(content);

    // 如果只有一个普通文本段落，直接高效直出
    if segments.len() == 1 {
        if let Some(MarkdownSegment::Text(txt)) = segments.first() {
            let mut tv = TextView::markdown(
                SharedString::from(base_id_str),
                SharedString::from(txt.clone()),
            )
            .selectable(true)
            .text_size(font_size)
            .text_color(text_color);

            if let Some(style) = heading_style {
                tv = tv.style(style);
            }
            return div().w_full().child(tv);
        }
    }

    let mut container = v_flex().w_full().gap_1();
    let mut inline_parts = Vec::new();

    for (ix, seg) in segments.into_iter().enumerate() {
        match seg {
            segment @ (MarkdownSegment::Text(_)
            | MarkdownSegment::Math {
                is_display: false, ..
            }) => inline_parts.push((ix, segment)),
            // 原生 LaTeX 矢量公式节点：100% 交由 MathElement 矢量排版，并支持一键复制 LaTeX 源码
            MarkdownSegment::Math {
                latex,
                is_display: true,
            } => {
                if !inline_parts.is_empty() {
                    container = container.child(render_inline_flow(
                        &base_id_str,
                        std::mem::take(&mut inline_parts),
                        font_size,
                        text_color,
                        heading_style.as_ref(),
                    ));
                }

                let is_display = true;
                let math_src = latex.clone();
                let seg_id = format!("{}-math-{}", base_id_str, ix);
                let is_copied = copied_formula_id.as_deref() == Some(&seg_id);
                let theme = theme.clone();
                container = container.child(
                    div()
                        .w_full()
                        .max_w_full()
                        .overflow_x_hidden()
                        .py_1()
                        .child(
                            v_flex()
                                .w_full()
                                .when(is_display, |this| this.items_center())
                                .when(!is_display, |this| this.items_start())
                                .child(
                                    MathElement::new(latex)
                                        .text_size(font_size + if is_display { px(2.0) } else { px(0.0) })
                                        .color(text_color)
                                        .display(is_display),
                                )
                                .when(is_display, |this| {
                                    this.child(
                                        h_flex()
                                            .w_full()
                                            .justify_end()
                                            .pt_0p5()
                                            .child(
                                                div()
                                                    .id(gpui::SharedString::from(format!("copy-latex-btn-{}", seg_id)))
                                                    .cursor_pointer()
                                                    .p_0p5()
                                                    .rounded_sm()
                                                    .hover(|s| s.bg(theme.muted.opacity(0.4)))
                                                    .on_click({
                                                        let math_src = math_src.clone();
                                                        let seg_id = seg_id.clone();
                                                        move |_, _, cx| {
                                                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(math_src.clone()));
                                                            crate::app_state::ui::UiState::update(cx, |s| {
                                                                s.copied_formula_id = Some(seg_id.clone());
                                                            });

                                                            cx.spawn({
                                                                let seg_id = seg_id.clone();
                                                                move |cx: &mut gpui::AsyncApp| {
                                                                    let cx = cx.clone();
                                                                    async move {
                                                                        cx.background_executor()
                                                                            .timer(std::time::Duration::from_millis(1500))
                                                                            .await;
                                                                        let _ = cx.update(|cx| {
                                                                            crate::app_state::ui::UiState::update(cx, |s| {
                                                                                if s.copied_formula_id.as_deref() == Some(&seg_id) {
                                                                                    s.copied_formula_id = None;
                                                                                }
                                                                            });
                                                                        });
                                                                    }
                                                                }
                                                            }).detach();
                                                        }
                                                    })
                                                    .child(
                                                        Icon::new(if is_copied {
                                                            IconName::Check
                                                        } else {
                                                            IconName::Copy
                                                        })
                                                        .size(px(11.0))
                                                        .text_color(if is_copied {
                                                            theme.primary
                                                        } else {
                                                            theme.muted_foreground
                                                        }),
                                                    ),
                                            ),
                                    )
                                }),
                        ),
                );
            }
        }
    }

    if !inline_parts.is_empty() {
        container = container.child(render_inline_flow(
            &base_id_str,
            inline_parts,
            font_size,
            text_color,
            heading_style.as_ref(),
        ));
    }

    div().w_full().child(container)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_text_command_inside_subscript() {
        let mut parser = MathParser::new(r"P_{\text{collect}}");
        let tree = parser.parse();

        assert_eq!(
            tree,
            MathNode::Sequence(vec![MathNode::Subscript {
                base: Box::new(MathNode::Text("P".to_string())),
                script: Box::new(MathNode::Sequence(vec![MathNode::Command {
                    name: "text".to_string(),
                    args: vec![MathNode::Sequence(
                        "collect"
                            .chars()
                            .map(|ch| MathNode::Text(ch.to_string()))
                            .collect(),
                    )],
                }])),
            },])
        );
    }

    #[test]
    fn keeps_nested_script_groups_balanced() {
        let source = r"\frac{a_{i+1}}{b^{t-1}}";
        assert_eq!(format_inline_math(source), source);
    }

    #[test]
    fn formats_existing_simple_scripts() {
        assert_eq!(format_inline_math("x_i"), "x_i");
        assert_eq!(format_inline_math("x^2"), "x^2");
    }

    #[test]
    fn keeps_inline_math_as_a_math_element_segment() {
        let segments = parse_markdown_segments("数据 $U_{\\text{batch}}$ 上：");

        assert!(matches!(segments.first(), Some(MarkdownSegment::Text(_))));
        assert!(matches!(
            segments.get(1),
            Some(MarkdownSegment::Math {
                latex,
                is_display: false,
            }) if latex == r"U_{\text{batch}}"
        ));
        assert!(matches!(segments.get(2), Some(MarkdownSegment::Text(_))));
    }
}
