//! 搜索相关 UI 类型。

use models::SearchField;

/// 搜索匹配结果
#[derive(Debug, Clone)]
pub struct SearchMatch {
    /// 匹配度分数 (可选，用于未来排序)
    pub score: f32,
    /// 匹配的字段类别 (Title, Author, Journal)
    pub field: SearchField,
}
