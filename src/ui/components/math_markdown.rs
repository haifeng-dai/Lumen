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

/// 将 ASCII 字母转换为 Unicode 数学斜体字符 (Mathematical Italic, U+1D434 / U+1D44E)
fn to_math_italic_char(c: char) -> char {
    match c {
        'a'..='z' => {
            if c == 'h' {
                'ℎ' // U+210E PLANCK CONSTANT
            } else {
                char::from_u32(0x1D44E + (c as u32 - 'a' as u32)).unwrap_or(c)
            }
        }
        'A'..='Z' => char::from_u32(0x1D434 + (c as u32 - 'A' as u32)).unwrap_or(c),
        _ => c,
    }
}

/// 将 ASCII 字符转换为 Unicode 数学下标字符
fn to_subscript_char(c: char) -> Option<char> {
    match c {
        '0'..='9' => char::from_u32(0x2080 + (c as u32 - '0' as u32)),
        'a' => Some('ₐ'),
        'e' => Some('ₑ'),
        'h' => Some('ₕ'),
        'i' => Some('ᵢ'),
        'j' => Some('ⱼ'),
        'k' => Some('ₖ'),
        'l' => Some('ₗ'),
        'm' => Some('ₘ'),
        'n' => Some('ₙ'),
        'o' => Some('ₒ'),
        'p' => Some('ₚ'),
        'r' => Some('ᵣ'),
        's' => Some('ₛ'),
        't' => Some('ₜ'),
        'u' => Some('ᵤ'),
        'v' => Some('ᵥ'),
        'x' => Some('ₓ'),
        '+' => Some('₊'),
        '-' => Some('₋'),
        '=' => Some('₌'),
        '(' => Some('₍'),
        ')' => Some('₎'),
        _ => None,
    }
}

/// 将 ASCII 字符转换为 Unicode 数学上标字符
fn to_superscript_char(c: char) -> Option<char> {
    match c {
        '0' => Some('⁰'),
        '1' => Some('¹'),
        '2' => Some('²'),
        '3' => Some('³'),
        '4'..='9' => char::from_u32(0x2070 + (c as u32 - '0' as u32)),
        'a' => Some('ᵃ'),
        'b' => Some('ᵇ'),
        'c' => Some('ᶜ'),
        'd' => Some('ᵈ'),
        'e' => Some('ᵉ'),
        'f' => Some('ᶠ'),
        'g' => Some('ᵍ'),
        'h' => Some('ʰ'),
        'i' => Some('ⁱ'),
        'j' => Some('ʲ'),
        'k' => Some('ᵏ'),
        'l' => Some('ˡ'),
        'm' => Some('ᵐ'),
        'n' => Some('ⁿ'),
        'o' => Some('ᵒ'),
        'p' => Some('ᵖ'),
        'r' => Some('ʳ'),
        's' => Some('ˢ'),
        't' => Some('ᵗ'),
        'u' => Some('ᵘ'),
        'v' => Some('ᵛ'),
        'w' => Some('ʷ'),
        'x' => Some('ˣ'),
        'y' => Some('ʸ'),
        'z' => Some('ᶻ'),
        '+' => Some('⁺'),
        '-' => Some('⁻'),
        '=' => Some('⁼'),
        '(' => Some('⁽'),
        ')' => Some('⁾'),
        _ => None,
    }
}

/// 将常用希腊字母命令转换为 Unicode 数学斜体希腊字母 (Mathematical Italic Greek, U+1D6FC ~ U+1D714)
fn to_math_italic_greek(cmd: &str) -> Option<&'static str> {
    match cmd {
        "alpha" => Some("𝛼"),
        "beta" => Some("𝛽"),
        "gamma" => Some("𝛾"),
        "delta" => Some("𝛿"),
        "epsilon" | "varepsilon" => Some("𝜀"),
        "zeta" => Some("𝜁"),
        "eta" => Some("𝜂"),
        "theta" | "vartheta" => Some("𝜃"),
        "iota" => Some("𝜄"),
        "kappa" => Some("𝜅"),
        "lambda" => Some("𝜆"),
        "mu" => Some("𝜇"),
        "nu" => Some("𝜈"),
        "xi" => Some("𝜉"),
        "pi" | "varpi" => Some("𝜋"),
        "rho" | "varrho" => Some("𝜌"),
        "sigma" | "varsigma" => Some("𝜎"),
        "tau" => Some("𝜏"),
        "upsilon" => Some("𝜐"),
        "phi" | "varphi" => Some("𝜑"),
        "chi" => Some("𝜒"),
        "psi" => Some("𝜓"),
        "omega" => Some("𝜔"),
        _ => None,
    }
}

/// 将行内公式转化为高保真 Unicode 数学字符流（KaTeX 专用字体字形，支持数学斜体变量、希腊符号、上下标与算子）
fn format_inline_math(math: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = math.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];

        // 1. 处理 LaTeX 宏命令 (\left, \right, \frac, \sqrt, \eta, \pi, \cos, \sin, \approx, \cdot 等)
        if ch == '\\' {
            i += 1;
            if i >= len {
                break;
            }

            // 特殊转义符号: \{, \}, \|, \;, \,, \!, \_
            if chars[i] == '{' || chars[i] == '}' || chars[i] == '|' {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            if chars[i] == ';' || chars[i] == ',' || chars[i] == ':' {
                out.push(' ');
                i += 1;
                continue;
            }
            if chars[i] == '!' {
                i += 1;
                continue;
            }

            let mut cmd = String::new();
            while i < len && chars[i].is_alphabetic() {
                cmd.push(chars[i]);
                i += 1;
            }

            // 定界符宏：\left, \right
            if cmd == "left" || cmd == "right" {
                while i < len && chars[i].is_whitespace() {
                    i += 1;
                }
                if i < len {
                    if chars[i] == '\\' {
                        i += 1;
                        if i < len && (chars[i] == '{' || chars[i] == '}' || chars[i] == '|') {
                            out.push(chars[i]);
                            i += 1;
                        }
                    } else if chars[i] != '.' {
                        out.push(chars[i]);
                        i += 1;
                    } else {
                        i += 1; // 跳过 \left. 或 \right. 的虚点
                    }
                }
                continue;
            }

            // 分式宏：\frac{num}{den}
            if cmd == "frac" || cmd == "dfrac" || cmd == "tfrac" {
                let mut num = String::new();
                let mut den = String::new();

                while i < len && chars[i].is_whitespace() {
                    i += 1;
                }
                if i < len && chars[i] == '{' {
                    i += 1;
                    let n_start = i;
                    let mut depth = 1;
                    while i < len && depth > 0 {
                        if chars[i] == '{' {
                            depth += 1;
                        } else if chars[i] == '}' {
                            depth -= 1;
                        }
                        i += 1;
                    }
                    num = chars[n_start..i - 1].iter().collect();
                } else if i < len {
                    num.push(chars[i]);
                    i += 1;
                }

                while i < len && chars[i].is_whitespace() {
                    i += 1;
                }
                if i < len && chars[i] == '{' {
                    i += 1;
                    let d_start = i;
                    let mut depth = 1;
                    while i < len && depth > 0 {
                        if chars[i] == '{' {
                            depth += 1;
                        } else if chars[i] == '}' {
                            depth -= 1;
                        }
                        i += 1;
                    }
                    den = chars[d_start..i - 1].iter().collect();
                } else if i < len {
                    den.push(chars[i]);
                    i += 1;
                }

                let formatted_num = format_inline_math(&num);
                let formatted_den = format_inline_math(&den);
                out.push_str(&formatted_num);
                out.push_str(" / ");
                out.push_str(&formatted_den);
                continue;
            }

            // 根号宏：\sqrt{body} 或 \sqrt[n]{body}
            if cmd == "sqrt" {
                while i < len && chars[i].is_whitespace() {
                    i += 1;
                }
                if i < len && chars[i] == '[' {
                    while i < len && chars[i] != ']' {
                        i += 1;
                    }
                    if i < len && chars[i] == ']' {
                        i += 1;
                    }
                }
                while i < len && chars[i].is_whitespace() {
                    i += 1;
                }
                let mut body = String::new();
                if i < len && chars[i] == '{' {
                    i += 1;
                    let b_start = i;
                    let mut depth = 1;
                    while i < len && depth > 0 {
                        if chars[i] == '{' {
                            depth += 1;
                        } else if chars[i] == '}' {
                            depth -= 1;
                        }
                        i += 1;
                    }
                    body = chars[b_start..i - 1].iter().collect();
                } else if i < len {
                    body.push(chars[i]);
                    i += 1;
                }

                out.push_str("√(");
                out.push_str(&format_inline_math(&body));
                out.push(')');
                continue;
            }

            // 文本与字体宏：\text{...}, \mathrm{...}, \mathbf{...}, \mathbb{...}
            if cmd == "text"
                || cmd == "mathrm"
                || cmd == "mathbf"
                || cmd == "bm"
                || cmd == "boldsymbol"
                || cmd == "mathbb"
                || cmd == "mathcal"
            {
                while i < len && chars[i].is_whitespace() {
                    i += 1;
                }
                let mut body = String::new();
                if i < len && chars[i] == '{' {
                    i += 1;
                    let b_start = i;
                    let mut depth = 1;
                    while i < len && depth > 0 {
                        if chars[i] == '{' {
                            depth += 1;
                        } else if chars[i] == '}' {
                            depth -= 1;
                        }
                        i += 1;
                    }
                    body = chars[b_start..i - 1].iter().collect();
                } else if i < len {
                    body.push(chars[i]);
                    i += 1;
                }
                out.push_str(&format_inline_math(&body));
                continue;
            }

            // 空格控制宏
            if cmd == "quad" {
                out.push_str("  ");
                continue;
            }
            if cmd == "qquad" {
                out.push_str("    ");
                continue;
            }

            if cmd == "mid" {
                out.push_str(" ∣ ");
                continue;
            }

            if cmd == "approx" {
                out.push_str(" ≈ ");
                continue;
            }

            if cmd == "cdot" {
                out.push_str(" · ");
                continue;
            }

            if cmd == "times" {
                out.push_str(" × ");
                continue;
            }

            if cmd == "pm" {
                out.push_str(" ± ");
                continue;
            }

            if cmd == "le" || cmd == "leq" {
                out.push_str(" ≤ ");
                continue;
            }

            if cmd == "ge" || cmd == "geq" {
                out.push_str(" ≥ ");
                continue;
            }

            // 优先匹配数学斜体希腊字母 (KaTeX_Math)
            if let Some(greek) = to_math_italic_greek(&cmd) {
                out.push_str(greek);
                continue;
            }

            // 标准数学函数算子（正体 Roman）
            if latex::is_function_operator(&cmd) {
                out.push_str(&cmd);
                continue;
            }

            // 其他特殊符号
            if let Some(sym) = latex::lookup_symbol(&cmd) {
                out.push_str(sym);
            } else {
                out.push_str(&cmd);
            }
            continue;
        }

        // 2. 处理下标 _b 或 _{i+1}
        if ch == '_' {
            i += 1;
            if i < len && chars[i] == '{' {
                i += 1;
                let mut sub_chars = Vec::new();
                while i < len && chars[i] != '}' {
                    sub_chars.push(chars[i]);
                    i += 1;
                }
                if i < len && chars[i] == '}' {
                    i += 1;
                }
                let mut all_converted = true;
                let mut converted = String::new();
                for sc in sub_chars {
                    if let Some(sub) = to_subscript_char(sc) {
                        converted.push(sub);
                    } else {
                        all_converted = false;
                        break;
                    }
                }
                if all_converted {
                    out.push_str(&converted);
                } else {
                    out.push_str("\\_");
                }
            } else if i < len {
                let sc = chars[i];
                i += 1;
                if let Some(sub) = to_subscript_char(sc) {
                    out.push(sub);
                } else {
                    out.push_str("\\_");
                    if sc.is_ascii_alphabetic() {
                        out.push(to_math_italic_char(sc));
                    } else {
                        out.push(sc);
                    }
                }
            } else {
                out.push_str("\\_");
            }
            continue;
        }

        // 3. 处理上标 ^2 或 ^{t-1}
        if ch == '^' {
            i += 1;
            if i < len && chars[i] == '{' {
                i += 1;
                let mut sup_chars = Vec::new();
                while i < len && chars[i] != '}' {
                    sup_chars.push(chars[i]);
                    i += 1;
                }
                if i < len && chars[i] == '}' {
                    i += 1;
                }
                let mut all_converted = true;
                let mut converted = String::new();
                for sc in sup_chars {
                    if let Some(sup) = to_superscript_char(sc) {
                        converted.push(sup);
                    } else {
                        all_converted = false;
                        break;
                    }
                }
                if all_converted {
                    out.push_str(&converted);
                } else {
                    out.push('^');
                }
            } else if i < len {
                let sc = chars[i];
                i += 1;
                if let Some(sup) = to_superscript_char(sc) {
                    out.push(sup);
                } else {
                    out.push('^');
                    if sc.is_ascii_alphabetic() {
                        out.push(to_math_italic_char(sc));
                    } else {
                        out.push(sc);
                    }
                }
            } else {
                out.push('^');
            }
            continue;
        }

        // 4. 连续多字母单词（如 cos, sin, exp 等普通文本函数名）保持正体，单个拉丁变量转为 Mathematical Italic 数学斜体
        if ch.is_ascii_alphabetic() {
            // 探测是否为多字母单词
            let mut word = String::new();
            let mut w_idx = i;
            while w_idx < len && chars[w_idx].is_ascii_alphabetic() {
                word.push(chars[w_idx]);
                w_idx += 1;
            }

            if word.len() > 1
                && (word == "cos"
                    || word == "sin"
                    || word == "tan"
                    || word == "exp"
                    || word == "log"
                    || word == "ln"
                    || word == "max"
                    || word == "min"
                    || word == "arg")
            {
                out.push_str(&word);
                i = w_idx;
                continue;
            }

            // 单字母变量一律转为 KaTeX 标准 Mathematical Italic 斜体
            out.push(to_math_italic_char(ch));
            i += 1;
            continue;
        }

        // 5. 转义普通字符中的下划线，防止破坏 Markdown 斜体
        if ch == '_' {
            out.push_str("\\_");
            i += 1;
            continue;
        }

        out.push(ch);
        i += 1;
    }
    out
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
                    let formatted = format_inline_math(trimmed_math);
                    current_text.push_str(&formatted);
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
                    // 行内公式：留在当前段落文本流中，保证句子连贯不换行
                    let formatted = format_inline_math(trimmed_math);
                    current_text.push_str(&formatted);
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
            // 原生 LaTeX 矢量公式节点：100% 交由 MathElement 矢量排版，并支持一键复制 LaTeX 源码
            MarkdownSegment::Math { latex, is_display } => {
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

    div().w_full().child(container)
}
