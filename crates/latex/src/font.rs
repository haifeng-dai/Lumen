use gpui::{AppContext, FontId, SharedString};
use std::sync::OnceLock;

/// 内置数学字体名称定义
pub const DEFAULT_MATH_FONT_FAMILY: &str = "Latin Modern Math";

/// 全局数学字体注册与状态
pub struct MathFontContext {
    pub font_family: SharedString,
    pub font_id: Option<FontId>,
}

static MATH_FONT_CTX: OnceLock<MathFontContext> = OnceLock::new();

impl MathFontContext {
    /// 获取全局数学字体上下文单例
    pub fn global() -> &'static Self {
        MATH_FONT_CTX.get_or_init(|| Self {
            font_family: SharedString::from(DEFAULT_MATH_FONT_FAMILY),
            font_id: None,
        })
    }

    /// 在应用启动时初始化并注册数学字体
    pub fn init(_cx: &mut impl AppContext) {
        // 后续在此处加载嵌入的 OpenType 字体数据并注册到 cx.text_system()
        let _ = Self::global();
    }
}
