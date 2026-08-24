use gpui::{Bounds, Pixels, Point};

/// 排版引擎输出的扁平化数学图元节点
#[derive(Debug, Clone, PartialEq)]
pub enum MathNode {
    /// 真实文本/数学字符（支持 UTF-8 字符、Unicode 数学符号、上下标与变量）
    Text {
        text: String,
        font_size: Pixels,
        position: Point<Pixels>,
        font_family: Option<&'static str>,
        is_italic: bool,
    },
    /// 字体字形图元（专用数学字体字形 ID）
    Glyph {
        glyph_id: u32,
        font_size: Pixels,
        position: Point<Pixels>,
    },
    /// 规则几何矩形/线条（分数线、根号横线、矩阵边框等）
    Rule { bounds: Bounds<Pixels> },
}

/// 公式排版计算后的尺寸与图元列表
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutResult {
    /// 公式总宽度
    pub width: Pixels,
    /// 公式总高度 (基线上方高度 + 基线下深度)
    pub height: Pixels,
    /// 基线（Baseline）下方的深度 (Depth)
    pub depth: Pixels,
    /// 扁平化的所有绘制节点列表
    pub nodes: Vec<MathNode>,
}

impl Default for LayoutResult {
    fn default() -> Self {
        Self {
            width: Pixels::ZERO,
            height: Pixels::ZERO,
            depth: Pixels::ZERO,
            nodes: Vec::new(),
        }
    }
}
