//! 核心搜索器（匹配原语）
//!
//! 原定义在 `src/ui/views/toolbar/search.rs`。因其含搜索逻辑（依赖
//! `parser::normalize` 与 `models::Literature`）且被服务层
//! (`src/services/data.rs`) 调用，迁移至本 crate 以断开 `services → ui`
//! 循环依赖；现归并到 `query` 域，作为 `search_literatures` 等上层
//! 编排调用的底层匹配原语。`AdvancedSearchQuery` / `SearchField`
//! 已下沉 `models`（纯数据）。

use models::AdvancedSearchQuery;
use models::Literature;
use parser::normalize::author_full_name;
use std::sync::Arc;

/// 核心搜索器
pub struct SearchEngine;

impl SearchEngine {
    /// 执行基础搜索
    ///
    /// 参数:
    /// - query: 关键词
    /// - items: 待搜索的数据迭代器
    pub fn search<'a>(
        query: &str,
        items: impl IntoIterator<Item = &'a Arc<Literature>>,
    ) -> Vec<&'a Arc<Literature>> {
        let query_trim = query.trim();
        if query_trim.is_empty() {
            return items.into_iter().collect();
        }

        let query_lower = query_trim.to_lowercase();

        items
            .into_iter()
            .filter(|lit| Self::is_match(&query_lower, lit))
            .collect()
    }

    /// 执行高级过滤
    pub fn advanced_search<'a>(
        query: &AdvancedSearchQuery,
        items: impl IntoIterator<Item = &'a Arc<Literature>>,
    ) -> Vec<&'a Arc<Literature>> {
        items
            .into_iter()
            .filter(|lit| {
                // 作者过滤
                if let Some(ref author_q) = query.author {
                    let author_q = author_q.to_lowercase();
                    if !lit
                        .authors
                        .iter()
                        .any(|a| author_full_name(a).to_lowercase().contains(&author_q))
                    {
                        return false;
                    }
                }

                // 年份范围过滤
                if let Some(start) = query.year_start
                    && lit.year.is_none_or(|y| y < start)
                {
                    return false;
                }
                if let Some(end) = query.year_end
                    && lit.year.is_none_or(|y| y > end)
                {
                    return false;
                }

                // 出版物过滤
                if let Some(ref pub_q) = query.publication {
                    let pub_q = pub_q.to_lowercase();
                    let lit_pub = lit
                        .publication
                        .as_ref()
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    if lit_pub.is_empty() || !lit_pub.to_lowercase().contains(&pub_q) {
                        return false;
                    }
                }

                true
            })
            .collect()
    }

    /// 内部匹配逻辑：判断文献是否满足搜索条件
    fn is_match(query: &str, lit: &Literature) -> bool {
        Self::match_title(query, lit)
            || Self::match_author(query, lit)
            || Self::match_journal(query, lit)
    }

    fn match_title(query: &str, lit: &Literature) -> bool {
        lit.title.to_lowercase().contains(query)
    }

    fn match_author(query: &str, lit: &Literature) -> bool {
        lit.authors.iter().any(|a| {
            a.last_name.to_lowercase().contains(query)
                || a.first_name.to_lowercase().contains(query)
                || author_full_name(a).to_lowercase().contains(query)
        })
    }

    fn match_journal(query: &str, lit: &Literature) -> bool {
        let pub_str = lit
            .publication
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default();
        !pub_str.is_empty() && pub_str.to_lowercase().contains(query)
    }
}
