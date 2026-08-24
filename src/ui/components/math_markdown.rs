use gpui::prelude::*;
use gpui::{App, Hsla, Pixels, SharedString, div, px};
use gpui_component::ActiveTheme;
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::v_flex;
use latex::MathElement;

enum MarkdownSegment {
    Text(String),
    BlockMath(String),
}

/// 将 ASCII 字母转换为 Unicode 数学斜体字符 (Mathematical Italic, U+1D434 / U+1D44E)
fn to_math_italic_char(c: char) -> char {
    match c {
        'a'..='z' => {
            if c == 'h' {
                'ℎ' // U+210E PLANCK CONSTANT (Unicode 数学斜体 h)
            } else {
                char::from_u32(0x1D44E + (c as u32 - 'a' as u32)).unwrap_or(c)
            }
        }
        'A'..='Z' => char::from_u32(0x1D434 + (c as u32 - 'A' as u32)).unwrap_or(c),
        _ => c,
    }
}

/// 将行内公式转化为高质量 Unicode 数学字符流（支持 \beta, \alpha 等符号宏与拉丁斜体变量）
fn format_inline_math(math: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = math.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' {
            i += 1;
            let mut cmd = String::new();
            while i < chars.len() && chars[i].is_alphabetic() {
                cmd.push(chars[i]);
                i += 1;
            }
            if let Some(sym) = latex::lookup_symbol(&cmd) {
                out.push_str(sym);
            } else {
                out.push('\\');
                out.push_str(&cmd);
            }
            continue;
        }

        // 单个拉丁变量转换为标准数学斜体字符
        if ch.is_ascii_alphabetic() {
            out.push(to_math_italic_char(ch));
            i += 1;
            continue;
        }

        out.push(ch);
        i += 1;
    }
    out
}

/// 将包含 $$...$$, \[...\], $...$, \(...\) 的 Markdown 文本解析为段落与块级公式
/// 仅将块级公式（$$...$$ 或 \[...\]）独立切分为居中排版的矢量公式节点，行内公式保留在文本流中
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

        // 遇到 \[...\] 块级公式
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
                    segments.push(MarkdownSegment::BlockMath(trimmed_math.to_string()));
                }
            } else {
                // 流式未闭合状态：若已积累前文则输出前文，并将未闭合的公式主体直接作为 BlockMath 实时渲染
                let remaining_math: String = chars[start..len].iter().collect();
                let trimmed_math = remaining_math.trim();
                if !trimmed_math.is_empty() {
                    if !current_text.trim().is_empty() {
                        segments.push(MarkdownSegment::Text(std::mem::take(&mut current_text)));
                    }
                    segments.push(MarkdownSegment::BlockMath(trimmed_math.to_string()));
                } else {
                    current_text.push_str("\\[");
                }
                break;
            }
            continue;
        }

        // 遇到 \(...\) 行内公式
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
                if !trimmed_math.is_empty() {
                    let formatted = format_inline_math(trimmed_math);
                    current_text.push_str(&formatted);
                } else {
                    current_text.push_str("\\(\\)");
                }
            } else {
                // 未闭合时保留内容
                current_text.push_str("\\(");
            }
            continue;
        }

        // 遇到 $$ 块级公式
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
                    segments.push(MarkdownSegment::BlockMath(trimmed_math.to_string()));
                }
            } else {
                // 流式未闭合状态：将已打字出来的公式部分即时作为 BlockMath 渲染，避免回退成纯文本 $$ 发生跳闪
                let remaining_math: String = chars[start..len].iter().collect();
                let trimmed_math = remaining_math.trim();
                if !trimmed_math.is_empty() {
                    if !current_text.trim().is_empty() {
                        segments.push(MarkdownSegment::Text(std::mem::take(&mut current_text)));
                    }
                    segments.push(MarkdownSegment::BlockMath(trimmed_math.to_string()));
                } else {
                    current_text.push_str("$$");
                }
                break;
            }
            continue;
        }

        // 遇到 $ 行内公式 ($...$)：转换为高保真 Unicode 数学字符嵌入当前段落
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
                if !trimmed_math.is_empty() {
                    let formatted = format_inline_math(trimmed_math);
                    current_text.push_str(&formatted);
                } else {
                    current_text.push('$');
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

/// 渲染包含纯原生 LaTeX 矢量公式的 Markdown 视图（支持 $$...$$ 块级公式与 $...$ 行内公式）
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

    // 如果只有一个段落且不是公式，直接高效直出
    if segments.len() == 1 {
        match segments.into_iter().next().unwrap() {
            MarkdownSegment::Text(txt) => {
                let mut tv =
                    TextView::markdown(SharedString::from(base_id_str), SharedString::from(txt))
                        .selectable(true)
                        .text_size(font_size)
                        .text_color(text_color);

                if let Some(style) = heading_style {
                    tv = tv.style(style);
                }
                return div().w_full().child(tv);
            }
            MarkdownSegment::BlockMath(math) => {
                let math_src = math.clone();
                let seg_id = format!("{}-math-0", base_id_str);
                let is_copied = copied_formula_id.as_deref() == Some(&seg_id);
                return div()
                    .w_full()
                    .max_w_full()
                    .overflow_x_hidden()
                    .py_2()
                    .child(
                    v_flex()
                        .w_full()
                        .items_center()
                        .child(
                            MathElement::new(math)
                                .text_size(font_size + px(2.0))
                                .color(text_color)
                                .display(true),
                        )
                        .child(
                            h_flex().w_full().justify_end().pt_0p5().child(
                                div()
                                    .id(gpui::SharedString::from(format!(
                                        "copy-latex-btn-{}",
                                        seg_id
                                    )))
                                    .cursor_pointer()
                                    .p_0p5()
                                    .rounded_sm()
                                    .hover(|s| s.bg(theme.muted.opacity(0.4)))
                                    .on_click({
                                        let math_src = math_src.clone();
                                        let seg_id = seg_id.clone();
                                        move |_, _, cx| {
                                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                                math_src.clone(),
                                            ));
                                            crate::app_state::ui::UiState::update(cx, |s| {
                                                s.copied_formula_id = Some(seg_id.clone());
                                            });

                                            cx.spawn({
                                                let seg_id = seg_id.clone();
                                                move |cx: &mut gpui::AsyncApp| {
                                                    let cx = cx.clone();
                                                    async move {
                                                        cx.background_executor()
                                                            .timer(
                                                                std::time::Duration::from_millis(
                                                                    1500,
                                                                ),
                                                            )
                                                            .await;
                                                        let _ = cx.update(|cx| {
                                                            crate::app_state::ui::UiState::update(
                                                                cx,
                                                                |s| {
                                                                    if s.copied_formula_id
                                                                        .as_deref()
                                                                        == Some(&seg_id)
                                                                    {
                                                                        s.copied_formula_id = None;
                                                                    }
                                                                },
                                                            );
                                                        });
                                                    }
                                                }
                                            })
                                            .detach();
                                        }
                                    })
                                    .child(
                                        Icon::new(if is_copied {
                                            IconName::Check
                                        } else {
                                            IconName::Copy
                                        })
                                        .size(px(11.0))
                                        .text_color(
                                            if is_copied {
                                                theme.primary
                                            } else {
                                                theme.muted_foreground
                                            },
                                        ),
                                    ),
                            ),
                        ),
                );
            }
        }
    }

    // 存在公式与正文混排
    let mut container = v_flex().w_full().gap_1();

    for (ix, seg) in segments.into_iter().enumerate() {
        match seg {
            MarkdownSegment::Text(txt) => {
                let seg_id = format!("{}-txt-{}", base_id_str, ix);
                let mut tv =
                    TextView::markdown(SharedString::from(seg_id), SharedString::from(txt))
                        .selectable(true)
                        .text_size(font_size)
                        .text_color(text_color);

                if let Some(ref style) = heading_style {
                    tv = tv.style(style.clone());
                }

                container = container.child(tv);
            }
            // 单行/独立块级公式：$$...$$ -> 独占一行、居中对齐、放大字号，并带有右下角轻量 LaTeX 复制按钮
            MarkdownSegment::BlockMath(math) => {
                let math_src = math.clone();
                let seg_id = format!("{}-math-{}", base_id_str, ix);
                let is_copied = copied_formula_id.as_deref() == Some(&seg_id);
                let theme = theme.clone();
                container = container.child(
                    div()
                        .w_full()
                        .max_w_full()
                        .overflow_x_hidden()
                        .py_2()
                        .child(
                            v_flex()
                                .w_full()
                                .items_center()
                                .child(
                                    MathElement::new(math)
                                        .text_size(font_size + px(2.0))
                                        .color(text_color)
                                        .display(true),
                                )
                                .child(
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
                                ),
                        ),
                );
            }
        }
    }

    div().w_full().child(container)
}
