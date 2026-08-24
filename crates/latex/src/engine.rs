use crate::layout_tree::{LayoutResult, MathNode};
use gpui::{Bounds, Pixels, Point, px, size};

/// LaTeX 排版引擎核心调度器
pub struct MathEngine;

impl MathEngine {
    /// 将常用 LaTeX 宏命令替换为对应的 Unicode 数学符号
    fn map_latex_symbol(command: &str) -> Option<&'static str> {
        crate::symbols::lookup_symbol(command)
    }

    /// 对输入的 LaTeX 源码执行排版计算，输出绘制图元列表
    pub fn layout(
        latex: &str,
        text_size: Pixels,
        is_display: bool,
    ) -> Result<LayoutResult, String> {
        let trimmed = latex.trim();
        if trimmed.is_empty() {
            return Ok(LayoutResult::default());
        }

        let mut nodes = Vec::new();
        let scale = f32::from(text_size) / 14.0;
        let mut cursor_x = px(0.0);
        let mut max_y = px(14.0 * scale);
        let mut min_y = px(0.0);

        let chars: Vec<char> = trimmed.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let ch = chars[i];

            // 1. 处理反斜杠命令 \command 或转义符号 \, \; \: \! \{ \} \|
            if ch == '\\' {
                i += 1;
                if i >= chars.len() {
                    break;
                }

                // 处理非字母单字符命令，如 \, \; \: \! \{ \} \| \\
                if !chars[i].is_alphabetic() {
                    let esc_char = chars[i];
                    i += 1;
                    match esc_char {
                        ',' => cursor_x += px(3.0 * scale),
                        ';' => cursor_x += px(5.0 * scale),
                        ':' => cursor_x += px(4.0 * scale),
                        '!' => cursor_x -= px(2.0 * scale),
                        '{' => {
                            nodes.push(MathNode::Text {
                                text: "{".to_string(),
                                font_size: text_size,
                                position: Point {
                                    x: cursor_x,
                                    y: px(0.0),
                                },
                            });
                            cursor_x += px(7.0 * scale);
                        }
                        '}' => {
                            nodes.push(MathNode::Text {
                                text: "}".to_string(),
                                font_size: text_size,
                                position: Point {
                                    x: cursor_x,
                                    y: px(0.0),
                                },
                            });
                            cursor_x += px(7.0 * scale);
                        }
                        '|' => {
                            nodes.push(MathNode::Text {
                                text: "‖".to_string(),
                                font_size: text_size,
                                position: Point {
                                    x: cursor_x,
                                    y: px(0.0),
                                },
                            });
                            cursor_x += px(8.0 * scale);
                        }
                        '\\' => {
                            cursor_x += px(10.0 * scale);
                        }
                        _ => {
                            nodes.push(MathNode::Text {
                                text: esc_char.to_string(),
                                font_size: text_size,
                                position: Point {
                                    x: cursor_x,
                                    y: px(0.0),
                                },
                            });
                            cursor_x += px(8.0 * scale);
                        }
                    }
                    continue;
                }

                let start = i;
                while i < chars.len() && chars[i].is_alphabetic() {
                    i += 1;
                }
                let cmd: String = chars[start..i].iter().collect();

                // \left 与 \right 自适应定界符动态拉伸解析
                if cmd == "left" {
                    while i < chars.len() && chars[i].is_whitespace() {
                        i += 1;
                    }
                    let left_delim = if i < chars.len() {
                        let c = chars[i];
                        if c == '\\' {
                            i += 1;
                            let d_start = i;
                            while i < chars.len() && chars[i].is_alphabetic() {
                                i += 1;
                            }
                            let d_cmd: String = chars[d_start..i].iter().collect();
                            if d_cmd.is_empty() && i < chars.len() {
                                let special_c = chars[i];
                                i += 1;
                                match special_c {
                                    '{' => "{".to_string(),
                                    '}' => "}".to_string(),
                                    '|' => "‖".to_string(),
                                    _ => special_c.to_string(),
                                }
                            } else {
                                crate::symbols::lookup_symbol(&d_cmd)
                                    .unwrap_or(&d_cmd)
                                    .to_string()
                            }
                        } else {
                            i += 1;
                            if c == '.' {
                                String::new()
                            } else {
                                c.to_string()
                            }
                        }
                    } else {
                        String::new()
                    };

                    // 匹配对应的 \right 定界符
                    let body_start = i;
                    let mut depth = 1;
                    let mut body_end = chars.len();
                    let mut right_delim = String::new();

                    while i < chars.len() {
                        if chars[i] == '\\' {
                            let sub: String = chars[i..].iter().collect();
                            if sub.starts_with("\\left")
                                && sub.chars().nth(5).map_or(true, |c| !c.is_alphabetic())
                            {
                                depth += 1;
                                i += 5;
                                continue;
                            } else if sub.starts_with("\\right")
                                && sub.chars().nth(6).map_or(true, |c| !c.is_alphabetic())
                            {
                                depth -= 1;
                                if depth == 0 {
                                    body_end = i;
                                    i += 6; // 跳过 \right
                                    while i < chars.len() && chars[i].is_whitespace() {
                                        i += 1;
                                    }
                                    if i < chars.len() {
                                        let rc = chars[i];
                                        if rc == '\\' {
                                            i += 1;
                                            let rd_start = i;
                                            while i < chars.len() && chars[i].is_alphabetic() {
                                                i += 1;
                                            }
                                            let rd_cmd: String =
                                                chars[rd_start..i].iter().collect();
                                            if rd_cmd.is_empty() && i < chars.len() {
                                                let special_rc = chars[i];
                                                i += 1;
                                                right_delim = match special_rc {
                                                    '{' => "{".to_string(),
                                                    '}' => "}".to_string(),
                                                    '|' => "‖".to_string(),
                                                    _ => special_rc.to_string(),
                                                };
                                            } else {
                                                right_delim =
                                                    crate::symbols::lookup_symbol(&rd_cmd)
                                                        .unwrap_or(&rd_cmd)
                                                        .to_string();
                                            }
                                        } else {
                                            i += 1;
                                            right_delim = if rc == '.' {
                                                String::new()
                                            } else {
                                                rc.to_string()
                                            };
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                        i += 1;
                    }

                    let body_content: String = chars[body_start..body_end].iter().collect();
                    let body_layout = Self::layout(&body_content, text_size, false)?;

                    let content_h = body_layout.height.max(px(14.0 * scale));
                    let delim_font_size = (content_h * 1.05).max(text_size);
                    let baseline_shift = -content_h / 2.0 + px(4.5 * scale);

                    // 绘制左定界符
                    if !left_delim.is_empty() {
                        nodes.push(MathNode::Text {
                            text: left_delim,
                            font_size: delim_font_size,
                            position: Point {
                                x: cursor_x,
                                y: baseline_shift,
                            },
                        });
                        cursor_x += px(7.0 * scale) + (delim_font_size - text_size) * 0.15;
                    }

                    let content_start_x = cursor_x;
                    for node in body_layout.nodes {
                        match node {
                            MathNode::Text {
                                text,
                                font_size,
                                position,
                            } => {
                                nodes.push(MathNode::Text {
                                    text,
                                    font_size,
                                    position: Point {
                                        x: content_start_x + position.x,
                                        y: position.y,
                                    },
                                });
                            }
                            MathNode::Glyph {
                                glyph_id,
                                font_size,
                                position,
                            } => {
                                nodes.push(MathNode::Glyph {
                                    glyph_id,
                                    font_size,
                                    position: Point {
                                        x: content_start_x + position.x,
                                        y: position.y,
                                    },
                                });
                            }
                            MathNode::Rule { bounds } => {
                                nodes.push(MathNode::Rule {
                                    bounds: Bounds {
                                        origin: Point {
                                            x: content_start_x + bounds.origin.x,
                                            y: bounds.origin.y,
                                        },
                                        size: bounds.size,
                                    },
                                });
                            }
                        }
                    }

                    cursor_x = content_start_x + body_layout.width + px(1.5 * scale);

                    // 绘制右定界符
                    if !right_delim.is_empty() {
                        nodes.push(MathNode::Text {
                            text: right_delim,
                            font_size: delim_font_size,
                            position: Point {
                                x: cursor_x,
                                y: baseline_shift,
                            },
                        });
                        cursor_x += px(7.0 * scale) + (delim_font_size - text_size) * 0.15;
                    }

                    max_y = max_y.max(body_layout.height.max(baseline_shift + delim_font_size));
                    min_y = min_y.min(baseline_shift.min(-body_layout.depth));
                    continue;
                }

                // 矩阵、多行方程与分支环境解析：\begin{env} ... \end{env}
                if cmd == "begin" {
                    while i < chars.len() && chars[i].is_whitespace() {
                        i += 1;
                    }
                    let mut env_name = String::new();
                    if i < chars.len() && chars[i] == '{' {
                        i += 1;
                        let e_start = i;
                        while i < chars.len() && chars[i] != '}' {
                            i += 1;
                        }
                        env_name = chars[e_start..i].iter().collect();
                        if i < chars.len() {
                            i += 1;
                        } // 跳过 '}'
                    }

                    let end_tag = format!("\\end{{{}}}", env_name);
                    let body_start = i;
                    let mut env_depth = 1;
                    let mut body_end = chars.len();

                    while i < chars.len() {
                        if chars[i] == '\\' {
                            let sub: String = chars[i..].iter().collect();
                            if sub.starts_with(&format!("\\begin{{{}}}", env_name)) {
                                env_depth += 1;
                            } else if sub.starts_with(&end_tag) {
                                env_depth -= 1;
                                if env_depth == 0 {
                                    body_end = i;
                                    i += end_tag.chars().count();
                                    break;
                                }
                            }
                        }
                        i += 1;
                    }

                    let env_body: String = chars[body_start..body_end].iter().collect();
                    let sub_scale = scale;

                    // 解析行与列：以 \\ 或 \cr 分割行，以 & 分割单元格
                    let mut raw_rows: Vec<Vec<String>> = Vec::new();
                    let mut current_row: Vec<String> = Vec::new();
                    let mut current_cell = String::new();
                    let b_chars: Vec<char> = env_body.chars().collect();
                    let mut bi = 0;
                    let mut brace_depth = 0;

                    while bi < b_chars.len() {
                        let bc = b_chars[bi];
                        if bc == '{' {
                            brace_depth += 1;
                            current_cell.push(bc);
                            bi += 1;
                        } else if bc == '}' {
                            if brace_depth > 0 {
                                brace_depth -= 1;
                            }
                            current_cell.push(bc);
                            bi += 1;
                        } else if brace_depth == 0 && bc == '&' {
                            current_row.push(std::mem::take(&mut current_cell));
                            bi += 1;
                        } else if brace_depth == 0
                            && bc == '\\'
                            && bi + 1 < b_chars.len()
                            && b_chars[bi + 1] == '\\'
                        {
                            current_row.push(std::mem::take(&mut current_cell));
                            raw_rows.push(std::mem::take(&mut current_row));
                            bi += 2;
                        } else {
                            current_cell.push(bc);
                            bi += 1;
                        }
                    }
                    if !current_cell.trim().is_empty() || !current_row.is_empty() {
                        current_row.push(current_cell);
                        raw_rows.push(current_row);
                    }

                    if !raw_rows.is_empty() {
                        let row_count = raw_rows.len();
                        let mut col_count = 0;
                        for r in &raw_rows {
                            col_count = col_count.max(r.len());
                        }

                        // 排版每个单元格并记录尺寸
                        let mut cell_layouts: Vec<Vec<LayoutResult>> = Vec::new();
                        let mut col_widths = vec![px(0.0); col_count];
                        let mut row_heights = vec![px(0.0); row_count];

                        let is_cases = env_name == "cases";
                        let is_aligned =
                            env_name == "aligned" || env_name == "align" || env_name == "align*";

                        let cell_font_size =
                            px(f32::from(text_size) * if is_cases { 0.95 } else { 0.9 });
                        let col_gap = px(if is_cases {
                            14.0
                        } else if is_aligned {
                            8.0
                        } else {
                            12.0
                        } * sub_scale);
                        let row_gap = px(6.0 * sub_scale);

                        for (r_idx, row) in raw_rows.iter().enumerate() {
                            let mut row_cells = Vec::new();
                            for (c_idx, cell_str) in row.iter().enumerate() {
                                let l = Self::layout(cell_str.trim(), cell_font_size, false)?;
                                col_widths[c_idx] = col_widths[c_idx].max(l.width);
                                row_heights[r_idx] =
                                    row_heights[r_idx].max(l.height.max(px(14.0 * sub_scale)));
                                row_cells.push(l);
                            }
                            cell_layouts.push(row_cells);
                        }

                        let total_matrix_w: Pixels =
                            col_widths.iter().copied().fold(px(0.0), |a, b| a + b)
                                + col_gap * col_count.saturating_sub(1);
                        let total_matrix_h: Pixels =
                            row_heights.iter().copied().fold(px(0.0), |a, b| a + b)
                                + row_gap * row_count.saturating_sub(1);

                        let baseline_shift = -total_matrix_h / 2.0 + px(5.0 * sub_scale);

                        // 确定左右定界符 (如 pmatrix -> (, ), bmatrix -> [, ], cases -> { )
                        let (left_delim, right_delim) = match env_name.as_str() {
                            "pmatrix" => (Some("("), Some(")")),
                            "bmatrix" => (Some("["), Some("]")),
                            "Bmatrix" => (Some("{"), Some("}")),
                            "vmatrix" => (Some("|"), Some("|")),
                            "Vmatrix" => (Some("‖"), Some("‖")),
                            "cases" => (Some("{"), None),
                            _ => (None, None),
                        };

                        let delim_font_size = px(f32::from(total_matrix_h) * 0.9).max(text_size);

                        // 左定界符
                        if let Some(ld) = left_delim {
                            nodes.push(MathNode::Text {
                                text: ld.to_string(),
                                font_size: delim_font_size,
                                position: Point {
                                    x: cursor_x,
                                    y: baseline_shift - px(2.0 * sub_scale),
                                },
                            });
                            cursor_x += px(10.0 * sub_scale);
                        }

                        let matrix_start_x = cursor_x;
                        let mut current_y = baseline_shift + total_matrix_h - row_heights[0];

                        // 放置各单元格节点
                        for (r_idx, row_cells) in cell_layouts.into_iter().enumerate() {
                            let mut current_x = matrix_start_x;
                            for (c_idx, cell_layout) in row_cells.into_iter().enumerate() {
                                let cell_offset_x = if is_aligned {
                                    // aligned 环境：第 1 列右对齐，第 2 列左对齐
                                    if c_idx % 2 == 0 {
                                        current_x + (col_widths[c_idx] - cell_layout.width)
                                    } else {
                                        current_x
                                    }
                                } else if is_cases {
                                    // cases 环境：左对齐
                                    current_x
                                } else {
                                    // 默认矩阵环境：居中对齐
                                    current_x + (col_widths[c_idx] - cell_layout.width) / 2.0
                                };

                                for node in cell_layout.nodes {
                                    match node {
                                        MathNode::Text {
                                            text,
                                            font_size,
                                            position,
                                        } => {
                                            nodes.push(MathNode::Text {
                                                text,
                                                font_size,
                                                position: Point {
                                                    x: cell_offset_x + position.x,
                                                    y: current_y + position.y,
                                                },
                                            });
                                        }
                                        MathNode::Glyph {
                                            glyph_id,
                                            font_size,
                                            position,
                                        } => {
                                            nodes.push(MathNode::Glyph {
                                                glyph_id,
                                                font_size,
                                                position: Point {
                                                    x: cell_offset_x + position.x,
                                                    y: current_y + position.y,
                                                },
                                            });
                                        }
                                        MathNode::Rule { bounds } => {
                                            nodes.push(MathNode::Rule {
                                                bounds: Bounds {
                                                    origin: Point {
                                                        x: cell_offset_x + bounds.origin.x,
                                                        y: current_y + bounds.origin.y,
                                                    },
                                                    size: bounds.size,
                                                },
                                            });
                                        }
                                    }
                                }
                                current_x += col_widths[c_idx] + col_gap;
                            }
                            if r_idx + 1 < row_count {
                                current_y -= row_heights[r_idx + 1] + row_gap;
                            }
                        }

                        cursor_x = matrix_start_x + total_matrix_w + px(2.0 * sub_scale);

                        // 右定界符
                        if let Some(rd) = right_delim {
                            nodes.push(MathNode::Text {
                                text: rd.to_string(),
                                font_size: delim_font_size,
                                position: Point {
                                    x: cursor_x,
                                    y: baseline_shift - px(2.0 * sub_scale),
                                },
                            });
                            cursor_x += px(10.0 * sub_scale);
                        }

                        max_y = max_y.max(baseline_shift + total_matrix_h);
                        min_y = min_y.min(baseline_shift);
                        continue;
                    }
                }

                // 分式解析：\frac{numerator}{denominator}
                if cmd == "frac" {
                    let mut num = String::new();
                    let mut den = String::new();

                    // 解析分子
                    while i < chars.len() && chars[i].is_whitespace() {
                        i += 1;
                    }
                    if i < chars.len() && chars[i] == '{' {
                        i += 1;
                        let n_start = i;
                        let mut depth = 1;
                        while i < chars.len() && depth > 0 {
                            if chars[i] == '{' {
                                depth += 1;
                            } else if chars[i] == '}' {
                                depth -= 1;
                            }
                            i += 1;
                        }
                        num = chars[n_start..i - 1].iter().collect();
                    }

                    // 解析分母
                    while i < chars.len() && chars[i].is_whitespace() {
                        i += 1;
                    }
                    if i < chars.len() && chars[i] == '{' {
                        i += 1;
                        let d_start = i;
                        let mut depth = 1;
                        while i < chars.len() && depth > 0 {
                            if chars[i] == '{' {
                                depth += 1;
                            } else if chars[i] == '}' {
                                depth -= 1;
                            }
                            i += 1;
                        }
                        den = chars[d_start..i - 1].iter().collect();
                    }

                    // 递归排版分子与分母
                    let sub_size = px(f32::from(text_size) * 0.85);
                    let num_layout = Self::layout(&num, sub_size, false)?;
                    let den_layout = Self::layout(&den, sub_size, false)?;

                    let frac_width = num_layout.width.max(den_layout.width).max(px(16.0 * scale))
                        + px(6.0 * scale);
                    let line_thickness = px(1.2 * scale);

                    // 绘制分数线
                    let line_y = px(4.0 * scale);
                    nodes.push(MathNode::Rule {
                        bounds: Bounds {
                            origin: Point {
                                x: cursor_x,
                                y: line_y,
                            },
                            size: size(frac_width, line_thickness),
                        },
                    });

                    // 放置分子（居中）
                    let num_offset_x = cursor_x + (frac_width - num_layout.width) / 2.0;
                    let num_offset_y = line_y + px(3.0 * scale) + num_layout.depth;
                    for node in num_layout.nodes {
                        match node {
                            MathNode::Text {
                                text,
                                font_size,
                                position,
                            } => {
                                nodes.push(MathNode::Text {
                                    text,
                                    font_size,
                                    position: Point {
                                        x: num_offset_x + position.x,
                                        y: num_offset_y + position.y,
                                    },
                                });
                            }
                            MathNode::Glyph {
                                glyph_id,
                                font_size,
                                position,
                            } => {
                                nodes.push(MathNode::Glyph {
                                    glyph_id,
                                    font_size,
                                    position: Point {
                                        x: num_offset_x + position.x,
                                        y: num_offset_y + position.y,
                                    },
                                });
                            }
                            MathNode::Rule { bounds } => {
                                nodes.push(MathNode::Rule {
                                    bounds: Bounds {
                                        origin: Point {
                                            x: num_offset_x + bounds.origin.x,
                                            y: num_offset_y + bounds.origin.y,
                                        },
                                        size: bounds.size,
                                    },
                                });
                            }
                        }
                    }

                    // 放置分母（居中）
                    let den_offset_x = cursor_x + (frac_width - den_layout.width) / 2.0;
                    let den_offset_y =
                        line_y - den_layout.height + den_layout.depth - px(3.0 * scale);
                    for node in den_layout.nodes {
                        match node {
                            MathNode::Text {
                                text,
                                font_size,
                                position,
                            } => {
                                nodes.push(MathNode::Text {
                                    text,
                                    font_size,
                                    position: Point {
                                        x: den_offset_x + position.x,
                                        y: den_offset_y + position.y,
                                    },
                                });
                            }
                            MathNode::Glyph {
                                glyph_id,
                                font_size,
                                position,
                            } => {
                                nodes.push(MathNode::Glyph {
                                    glyph_id,
                                    font_size,
                                    position: Point {
                                        x: den_offset_x + position.x,
                                        y: den_offset_y + position.y,
                                    },
                                });
                            }
                            MathNode::Rule { bounds } => {
                                nodes.push(MathNode::Rule {
                                    bounds: Bounds {
                                        origin: Point {
                                            x: den_offset_x + bounds.origin.x,
                                            y: den_offset_y + bounds.origin.y,
                                        },
                                        size: bounds.size,
                                    },
                                });
                            }
                        }
                    }

                    cursor_x += frac_width + px(4.0 * scale);
                    max_y = max_y.max(num_offset_y + num_layout.height);
                    min_y = min_y.min(den_offset_y);
                    continue;
                }

                // 根号解析：\sqrt{body} 或 \sqrt[index]{body}
                if cmd == "sqrt" {
                    let mut index = String::new();
                    let mut body = String::new();

                    // 可选根指数 [index]
                    while i < chars.len() && chars[i].is_whitespace() {
                        i += 1;
                    }
                    if i < chars.len() && chars[i] == '[' {
                        i += 1;
                        let idx_start = i;
                        while i < chars.len() && chars[i] != ']' {
                            i += 1;
                        }
                        if i < chars.len() {
                            index = chars[idx_start..i].iter().collect();
                            i += 1; // 跳过 ']'
                        }
                    }

                    // 被开方数 {body}
                    while i < chars.len() && chars[i].is_whitespace() {
                        i += 1;
                    }
                    if i < chars.len() && chars[i] == '{' {
                        i += 1;
                        let b_start = i;
                        let mut depth = 1;
                        while i < chars.len() && depth > 0 {
                            if chars[i] == '{' {
                                depth += 1;
                            } else if chars[i] == '}' {
                                depth -= 1;
                            }
                            i += 1;
                        }
                        body = chars[b_start..i - 1].iter().collect();
                    } else if i < chars.len() {
                        body.push(chars[i]);
                        i += 1;
                    }

                    // 排版根指数与被开方数
                    let body_layout = Self::layout(&body, text_size, false)?;

                    let sqrt_hook_width = px(9.0 * scale);
                    let line_thickness = px(1.2 * scale);
                    let body_width = body_layout.width.max(px(8.0 * scale));
                    let body_height = body_layout.height.max(px(14.0 * scale));
                    let body_depth = body_layout.depth;

                    // 若有指数，先放置小指数
                    if !index.is_empty() {
                        let idx_size = px(f32::from(text_size) * 0.6);
                        if let Ok(idx_layout) = Self::layout(&index, idx_size, false) {
                            let idx_offset_x = cursor_x;
                            let idx_offset_y = px(6.0 * scale);
                            for node in idx_layout.nodes {
                                if let MathNode::Text {
                                    text,
                                    font_size,
                                    position,
                                } = node
                                {
                                    nodes.push(MathNode::Text {
                                        text,
                                        font_size,
                                        position: Point {
                                            x: idx_offset_x + position.x,
                                            y: idx_offset_y + position.y,
                                        },
                                    });
                                }
                            }
                            cursor_x += idx_layout.width.max(px(5.0 * scale));
                        }
                    }

                    // 放置根号符号 √
                    nodes.push(MathNode::Text {
                        text: "√".to_string(),
                        font_size: px(f32::from(text_size) * 1.15),
                        position: Point {
                            x: cursor_x,
                            y: px(0.0),
                        },
                    });

                    let body_start_x = cursor_x + sqrt_hook_width;
                    let top_line_y = body_height - body_depth + px(2.0 * scale);

                    // 绘制根号上方的水平横线
                    nodes.push(MathNode::Rule {
                        bounds: Bounds {
                            origin: Point {
                                x: body_start_x,
                                y: top_line_y,
                            },
                            size: size(body_width + px(2.0 * scale), line_thickness),
                        },
                    });

                    // 放置被开方数节点
                    for node in body_layout.nodes {
                        match node {
                            MathNode::Text {
                                text,
                                font_size,
                                position,
                            } => {
                                nodes.push(MathNode::Text {
                                    text,
                                    font_size,
                                    position: Point {
                                        x: body_start_x + position.x,
                                        y: position.y,
                                    },
                                });
                            }
                            MathNode::Glyph {
                                glyph_id,
                                font_size,
                                position,
                            } => {
                                nodes.push(MathNode::Glyph {
                                    glyph_id,
                                    font_size,
                                    position: Point {
                                        x: body_start_x + position.x,
                                        y: position.y,
                                    },
                                });
                            }
                            MathNode::Rule { bounds } => {
                                nodes.push(MathNode::Rule {
                                    bounds: Bounds {
                                        origin: Point {
                                            x: body_start_x + bounds.origin.x,
                                            y: bounds.origin.y,
                                        },
                                        size: bounds.size,
                                    },
                                });
                            }
                        }
                    }

                    cursor_x = body_start_x + body_width + px(3.0 * scale);
                    max_y = max_y.max(top_line_y + line_thickness);
                    min_y = min_y.min(-body_depth);
                    continue;
                }

                // 数学重音符号解析
                let accent_symbol = match cmd.as_str() {
                    "hat" | "widehat" => Some("^"),
                    "bar" | "overline" => Some("—"),
                    "tilde" | "widetilde" => Some("~"),
                    "vec" => Some("→"),
                    "dot" => Some("˙"),
                    "ddot" => Some("¨"),
                    "dddot" => Some("⋯"),
                    "check" => Some("ˇ"),
                    "breve" => Some("˘"),
                    "mathring" => Some("˚"),
                    _ => None,
                };

                if let Some(accent_mark) = accent_symbol {
                    let mut body = String::new();
                    while i < chars.len() && chars[i].is_whitespace() {
                        i += 1;
                    }
                    if i < chars.len() && chars[i] == '{' {
                        i += 1;
                        let b_start = i;
                        let mut depth = 1;
                        while i < chars.len() && depth > 0 {
                            if chars[i] == '{' {
                                depth += 1;
                            } else if chars[i] == '}' {
                                depth -= 1;
                            }
                            i += 1;
                        }
                        body = chars[b_start..i - 1].iter().collect();
                    } else if i < chars.len() {
                        body.push(chars[i]);
                        i += 1;
                    }

                    // 排版基础字符
                    let body_layout = Self::layout(&body, text_size, false)?;
                    let body_width = body_layout.width.max(px(7.0 * scale));
                    let _body_height = body_layout.height.max(px(12.0 * scale));

                    // 放置底部的基准字符
                    for node in body_layout.nodes {
                        match node {
                            MathNode::Text {
                                text,
                                font_size,
                                position,
                            } => {
                                nodes.push(MathNode::Text {
                                    text,
                                    font_size,
                                    position: Point {
                                        x: cursor_x + position.x,
                                        y: position.y,
                                    },
                                });
                            }
                            MathNode::Glyph {
                                glyph_id,
                                font_size,
                                position,
                            } => {
                                nodes.push(MathNode::Glyph {
                                    glyph_id,
                                    font_size,
                                    position: Point {
                                        x: cursor_x + position.x,
                                        y: position.y,
                                    },
                                });
                            }
                            MathNode::Rule { bounds } => {
                                nodes.push(MathNode::Rule {
                                    bounds: Bounds {
                                        origin: Point {
                                            x: cursor_x + bounds.origin.x,
                                            y: bounds.origin.y,
                                        },
                                        size: bounds.size,
                                    },
                                });
                            }
                        }
                    }

                    // 顶置重音符号（居中偏上，紧凑贴合字母顶部）
                    let accent_size = px(f32::from(text_size) * 0.75);
                    let accent_y = px(5.5 * scale);
                    let accent_x = cursor_x + (body_width - px(5.0 * scale)) / 2.0;

                    if cmd == "bar" || cmd == "overline" {
                        // 细横线线条
                        nodes.push(MathNode::Rule {
                            bounds: Bounds {
                                origin: Point {
                                    x: cursor_x,
                                    y: px(12.0 * scale),
                                },
                                size: size(body_width, px(1.0 * scale)),
                            },
                        });
                    } else {
                        nodes.push(MathNode::Text {
                            text: accent_mark.to_string(),
                            font_size: accent_size,
                            position: Point {
                                x: accent_x,
                                y: accent_y,
                            },
                        });
                    }

                    cursor_x += body_width + px(1.0 * scale);
                    max_y = max_y.max(accent_y + px(6.0 * scale));
                    min_y = min_y.min(-body_layout.depth);
                    continue;
                }

                // 空格控制宏：\quad, \qquad, \;, \:, \,, \!
                if cmd == "quad" {
                    cursor_x += px(14.0 * scale);
                    continue;
                } else if cmd == "qquad" {
                    cursor_x += px(28.0 * scale);
                    continue;
                }

                // 字体与包装命令解析：\mathbb{R}, \mathcal{L}, \text{...}, \mathbf{...}, \mathrm{...}, \bm{...}
                if cmd == "mathbb"
                    || cmd == "mathcal"
                    || cmd == "text"
                    || cmd == "mathrm"
                    || cmd == "mathbf"
                    || cmd == "boldsymbol"
                    || cmd == "bm"
                {
                    let mut body = String::new();
                    while i < chars.len() && chars[i].is_whitespace() {
                        i += 1;
                    }
                    if i < chars.len() && chars[i] == '{' {
                        i += 1;
                        let b_start = i;
                        let mut depth = 1;
                        while i < chars.len() && depth > 0 {
                            if chars[i] == '{' {
                                depth += 1;
                            } else if chars[i] == '}' {
                                depth -= 1;
                            }
                            i += 1;
                        }
                        body = chars[b_start..i - 1].iter().collect();
                    } else if i < chars.len() {
                        body.push(chars[i]);
                        i += 1;
                    }

                    if cmd == "mathbb" {
                        let mapped = format!("mathbb{}", body.trim());
                        if let Some(sym) = crate::symbols::lookup_symbol(&mapped) {
                            let sym_width = px(10.5 * scale);
                            nodes.push(MathNode::Text {
                                text: sym.to_string(),
                                font_size: text_size,
                                position: Point {
                                    x: cursor_x,
                                    y: px(0.0),
                                },
                            });
                            cursor_x += sym_width;
                            continue;
                        }
                    } else if cmd == "mathcal" {
                        let mapped = format!("mathcal{}", body.trim());
                        if let Some(sym) = crate::symbols::lookup_symbol(&mapped) {
                            let sym_width = px(10.5 * scale);
                            nodes.push(MathNode::Text {
                                text: sym.to_string(),
                                font_size: text_size,
                                position: Point {
                                    x: cursor_x,
                                    y: px(0.0),
                                },
                            });
                            cursor_x += sym_width;
                            continue;
                        }
                    }

                    // 递归排版 body 文本内容
                    if let Ok(body_layout) = Self::layout(&body, text_size, false) {
                        for node in body_layout.nodes {
                            match node {
                                MathNode::Text {
                                    text,
                                    font_size,
                                    position,
                                } => {
                                    nodes.push(MathNode::Text {
                                        text,
                                        font_size,
                                        position: Point {
                                            x: cursor_x + position.x,
                                            y: position.y,
                                        },
                                    });
                                }
                                MathNode::Glyph {
                                    glyph_id,
                                    font_size,
                                    position,
                                } => {
                                    nodes.push(MathNode::Glyph {
                                        glyph_id,
                                        font_size,
                                        position: Point {
                                            x: cursor_x + position.x,
                                            y: position.y,
                                        },
                                    });
                                }
                                MathNode::Rule { bounds } => {
                                    nodes.push(MathNode::Rule {
                                        bounds: Bounds {
                                            origin: Point {
                                                x: cursor_x + bounds.origin.x,
                                                y: bounds.origin.y,
                                            },
                                            size: bounds.size,
                                        },
                                    });
                                }
                            }
                        }
                        cursor_x += body_layout.width;
                        max_y = max_y.max(body_layout.height);
                        min_y = min_y.min(-body_layout.depth);
                        continue;
                    }
                }

                // 常规符号替换
                if let Some(sym) = Self::map_latex_symbol(&cmd) {
                    let sym_width = px(9.5 * scale);
                    nodes.push(MathNode::Text {
                        text: sym.to_string(),
                        font_size: text_size,
                        position: Point {
                            x: cursor_x,
                            y: px(0.0),
                        },
                    });
                    cursor_x += sym_width;
                } else {
                    // 未知命令直接作为文本
                    let txt_width = px(8.0 * scale * cmd.len() as f32);
                    nodes.push(MathNode::Text {
                        text: cmd,
                        font_size: text_size,
                        position: Point {
                            x: cursor_x,
                            y: px(0.0),
                        },
                    });
                    cursor_x += txt_width;
                }
                continue;
            }

            // 2. 上下标解析（支持同时存在 ^ 与 _，实现同轴垂直对齐）
            if ch == '^' || ch == '_' {
                let mut sup_text: Option<String> = None;
                let mut sub_text: Option<String> = None;

                // 循环读取可能连续出现的 ^ 与 _（如 x_i^2 或 x^2_i）
                while i < chars.len() && (chars[i] == '^' || chars[i] == '_') {
                    let is_sup = chars[i] == '^';
                    i += 1;
                    let mut script_content = String::new();
                    if i < chars.len() && chars[i] == '{' {
                        i += 1;
                        let s_start = i;
                        let mut depth = 1;
                        while i < chars.len() && depth > 0 {
                            if chars[i] == '{' {
                                depth += 1;
                            } else if chars[i] == '}' {
                                depth -= 1;
                            }
                            i += 1;
                        }
                        script_content = chars[s_start..i - 1].iter().collect();
                    } else if i < chars.len() && !chars[i].is_whitespace() {
                        script_content.push(chars[i]);
                        i += 1;
                    }

                    if is_sup {
                        sup_text = Some(script_content);
                    } else {
                        sub_text = Some(script_content);
                    }
                }

                let sub_size = px(f32::from(text_size) * 0.7);
                let mut max_script_width = px(0.0);

                // 上标排版
                if let Some(ref sup) = sup_text {
                    if let Ok(sup_layout) = Self::layout(sup, sub_size, false) {
                        let sup_y = px(7.0 * scale);
                        for node in sup_layout.nodes {
                            if let MathNode::Text {
                                text,
                                font_size,
                                position,
                            } = node
                            {
                                nodes.push(MathNode::Text {
                                    text,
                                    font_size,
                                    position: Point {
                                        x: cursor_x + position.x,
                                        y: sup_y + position.y,
                                    },
                                });
                            }
                        }
                        max_script_width = max_script_width.max(sup_layout.width);
                        max_y = max_y.max(sup_y + sup_layout.height);
                    }
                }

                // 下标排版（与上标同 X 轴起点对齐）
                if let Some(ref sub) = sub_text {
                    if let Ok(sub_layout) = Self::layout(sub, sub_size, false) {
                        let sub_y = -px(4.0 * scale);
                        for node in sub_layout.nodes {
                            if let MathNode::Text {
                                text,
                                font_size,
                                position,
                            } = node
                            {
                                nodes.push(MathNode::Text {
                                    text,
                                    font_size,
                                    position: Point {
                                        x: cursor_x + position.x,
                                        y: sub_y + position.y,
                                    },
                                });
                            }
                        }
                        max_script_width = max_script_width.max(sub_layout.width);
                        min_y = min_y.min(sub_y - sub_layout.depth);
                    }
                }

                cursor_x += max_script_width + px(1.5 * scale);
                continue;
            }

            // 4. 普通符号 / 空格
            if ch.is_whitespace() {
                cursor_x += px(4.0 * scale);
                i += 1;
                continue;
            }

            // 单个字符
            let char_width = if "=+-*/<>".contains(ch) {
                px(10.0 * scale)
            } else {
                px(8.5 * scale)
            };

            nodes.push(MathNode::Text {
                text: ch.to_string(),
                font_size: text_size,
                position: Point {
                    x: cursor_x,
                    y: px(0.0),
                },
            });
            cursor_x += char_width;
            i += 1;
        }

        let depth = -min_y.min(px(0.0)) + px(3.0 * scale);
        let total_height = max_y + depth + if is_display { px(8.0 * scale) } else { px(0.0) };

        Ok(LayoutResult {
            width: cursor_x,
            height: total_height,
            depth,
            nodes,
        })
    }
}
