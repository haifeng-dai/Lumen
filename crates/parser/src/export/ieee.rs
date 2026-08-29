use super::{Exporter, publication_display_name};
use anyhow::Result;
use models::Literature;
use std::fmt::Write;

const IEEE_MAX_LISTED_AUTHORS: usize = 6;

/// IEEE Transactions 引用格式导出器
/// 格式示例: [1] J. Doe and J. Smith, "Title of Paper," Journal Name, vol. 1, no. 2, pp. 3-4, 2024.
pub struct IeeeExporter;

impl IeeeExporter {
    fn format_ieee(&self, index: usize, lit: &Literature, abbreviate_journal: bool) -> String {
        let mut s = String::new();
        write!(s, "[{}] ", index + 1).unwrap();

        let authors = lit
            .authors
            .iter()
            .map(|author| {
                let first_initial = author
                    .first_name
                    .chars()
                    .next()
                    .map(|character| format!("{character}. "))
                    .unwrap_or_default();
                format!("{first_initial}{}", author.last_name)
            })
            .collect::<Vec<_>>();

        let authors = match authors.as_slice() {
            [] => String::new(),
            [name] => name.clone(),
            [first, second] => format!("{first} and {second}"),
            [first, ..] if authors.len() > IEEE_MAX_LISTED_AUTHORS => {
                format!("{first} et al.")
            }
            _ => {
                let last_index = authors.len() - 1;
                format!(
                    "{}, and {}",
                    authors[..last_index].join(", "),
                    authors[last_index]
                )
            }
        };

        if !authors.is_empty() {
            write!(s, "{authors}, ").unwrap();
        }

        // Title
        write!(s, "\"{},\" ", lit.title).unwrap();

        // Venue (使用 publication 字段)
        let venue = lit.publication.as_ref().map_or_else(String::new, |p| {
            publication_display_name(p, abbreviate_journal)
        });
        if !venue.is_empty() {
            write!(s, "{venue}, ").unwrap();
        }

        // Volume, Issue, Pages
        if let Some(ref vol) = lit.volume {
            write!(s, "vol. {vol}, ").unwrap();
        }
        if let Some(ref issue) = lit.issue {
            write!(s, "no. {issue}, ").unwrap();
        }
        if let Some(ref pages) = lit.pages {
            // 单页用 p.，区间用 pp.（兼容 -、–、— 三种连字符）
            let prefix = if pages.contains(['-', '–', '—']) {
                "pp."
            } else {
                "p."
            };
            write!(s, "{prefix} {pages}, ").unwrap();
        }

        // Year
        if let Some(year) = lit.year {
            write!(s, "{year}.").unwrap();
        } else {
            s.push('.');
        }

        s
    }
}

impl Exporter for IeeeExporter {
    fn format_name(&self) -> &'static str {
        "IEEE Transactions"
    }
    fn export_to_string(&self, items: &[Literature], abbreviate_journal: bool) -> Result<String> {
        let lines: Vec<String> = items
            .iter()
            .enumerate()
            .map(|(i, lit)| self.format_ieee(i, lit, abbreviate_journal))
            .collect();
        Ok(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::IeeeExporter;
    use models::{LiteratureType, create_author, create_literature};

    fn literature_with_authors(count: usize) -> models::Literature {
        let mut literature = create_literature("test", "Example", LiteratureType::Article);
        literature.authors = (1..=count)
            .map(|index| create_author(format!("Author{index}"), "Given"))
            .collect();
        literature
    }

    #[test]
    fn formats_author_lists_up_to_six_names() {
        assert!(IeeeExporter.format_ieee(0, &literature_with_authors(6), false).starts_with(
            "[1] G. Author1, G. Author2, G. Author3, G. Author4, G. Author5, and G. Author6, "
        ));
    }

    #[test]
    fn abbreviates_author_lists_with_more_than_six_names() {
        assert!(
            IeeeExporter
                .format_ieee(0, &literature_with_authors(7), false)
                .starts_with("[1] G. Author1 et al., ")
        );
    }
}
