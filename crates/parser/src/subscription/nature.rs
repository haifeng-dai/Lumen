use crate::text;
use anyhow::{Result, anyhow};
use log::{debug, error, info};
use models::FeedItem;
use models::constructors::*;
use quick_xml::{events::Event, reader::Reader};
use uuid::Uuid;

/// Nature 订阅解析器
///
/// 专门针对 `nature.com` 的 RSS 1.0 (RDF) 订阅源（如 `nature.rss`、`natmachintell.rss`）。
/// 真实字段：
/// - `title` / `dc:title`（CDATA 包裹）
/// - `link`
/// - `content:encoded`（CDATA 包裹，含 HTML 摘要）
/// - `dc:creator`（作者，可多个）
/// - `dc:identifier` / `prism:doi`（DOI）
/// - `dc:date`（日期）
/// - `prism:publicationName`（期刊名）
/// - `prism:volume` / `prism:number`（卷 / 期，部分文章有）
///
/// RSS 1.0 把 `<item>` 放在 `<channel>` 之外（RDF 序列化），但本解析器基于事件流扫描，
/// 不依赖层级，故不受影响。仅处理 Nature 实际出现的字段，不存在的字段直接忽略。
pub struct NatureSubscriptionParser;

impl NatureSubscriptionParser {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for NatureSubscriptionParser {
    fn default() -> Self {
        Self::new()
    }
}

impl NatureSubscriptionParser {
    pub fn parse(
        xml: &str,
        feed_id: &str,
    ) -> Result<(Vec<FeedItem>, Option<String>, Option<String>)> {
        info!("开始使用 Nature 解析器解析: {feed_id}");
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);

        let mut items = Vec::new();
        let mut buf = Vec::new();

        let mut in_item = false;
        let mut current_tag = String::new();

        let mut title = String::new();
        let mut link = String::new();
        let mut content_encoded = String::new();
        let mut dc_identifier = String::new();
        let mut prism_doi = String::new();
        let mut dc_date = String::new();
        let mut journal = String::new();
        let mut volume = String::new();
        let mut number = String::new();
        let mut authors_list: Vec<String> = Vec::new();
        let mut channel_title = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = e.name().as_ref().to_string();
                    if name == "item" {
                        in_item = true;
                        // 重置字段
                        title.clear();
                        link.clear();
                        content_encoded.clear();
                        dc_identifier.clear();
                        prism_doi.clear();
                        dc_date.clear();
                        journal.clear();
                        volume.clear();
                        number.clear();
                        authors_list.clear();
                    }
                    current_tag = name;
                }
                // Nature 的 title / content:encoded / dc:title 均以 CDATA 包裹，
                // 因此 Text 与 CData 两个事件都要处理（quick_xml 0.39 对 CDATA 发独立事件）。
                Ok(Event::Text(ref e)) => {
                    let text = e.as_ref().to_string();
                    apply_nature_field_text(
                        &text,
                        current_tag.as_str(),
                        in_item,
                        &mut title,
                        &mut link,
                        &mut content_encoded,
                        &mut dc_identifier,
                        &mut prism_doi,
                        &mut dc_date,
                        &mut journal,
                        &mut volume,
                        &mut number,
                        &mut authors_list,
                        &mut channel_title,
                    );
                }
                Ok(Event::CData(ref e)) => {
                    let text = e.as_ref().to_string();
                    apply_nature_field_text(
                        &text,
                        current_tag.as_str(),
                        in_item,
                        &mut title,
                        &mut link,
                        &mut content_encoded,
                        &mut dc_identifier,
                        &mut prism_doi,
                        &mut dc_date,
                        &mut journal,
                        &mut volume,
                        &mut number,
                        &mut authors_list,
                        &mut channel_title,
                    );
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name().as_ref().to_string();
                    if name == "item" {
                        in_item = false;

                        // ID：DOI > link > uuid
                        let mut doi = String::new();
                        if !prism_doi.is_empty() {
                            doi = prism_doi.trim_start_matches("doi:").to_string();
                        } else if !dc_identifier.is_empty() {
                            if dc_identifier.to_lowercase().starts_with("doi:") {
                                doi = dc_identifier[4..].trim().to_string();
                            } else if dc_identifier.contains("10.") {
                                doi = dc_identifier.trim().to_string();
                            }
                        }

                        let item_id = if !doi.is_empty() {
                            doi.clone()
                        } else if !link.is_empty() {
                            link.clone()
                        } else {
                            Uuid::new_v4().to_string()
                        };

                        let mut item = create_feed_item(
                            item_id,
                            text::clean_title(&title),
                            feed_id.to_string(),
                        );

                        item.url = text::clean_optional_text(Some(&link));
                        if !doi.is_empty() {
                            item.doi = Some(doi);
                        }

                        // 摘要：content:encoded 含 HTML，清理后使用
                        let cleaned_abstract = text::strip_html_tags(&content_encoded);
                        item.abstract_text = if cleaned_abstract.is_empty() {
                            None
                        } else {
                            Some(cleaned_abstract)
                        };

                        item.journal = text::clean_optional_text(Some(&journal));
                        item.volume = text::clean_optional_text(Some(&volume));
                        item.issue = text::clean_optional_text(Some(&number));

                        if !dc_date.is_empty() {
                            item.published_at = Some(crate::time::normalize_time_string(&dc_date));
                        }

                        // 作者：dc:creator 每个元素一个名字，兼容单串逗号分隔
                        for auth_str in &authors_list {
                            for name in auth_str.split(',') {
                                let clean_auth = name.trim();
                                if clean_auth.is_empty() {
                                    continue;
                                }
                                let parts: Vec<&str> = clean_auth.split_whitespace().collect();
                                if !parts.is_empty() {
                                    let last = text::clean_author_name(parts.last().unwrap_or(&""));
                                    let first = text::clean_author_name(
                                        &parts[..parts.len() - 1].join(" "),
                                    );
                                    item.authors.push(create_author(last, first));
                                }
                            }
                        }

                        debug!(
                            "Nature 解析器: 解析到条目: {} (DOI: {:?})",
                            item.title, item.doi
                        );
                        items.push(item);
                    }
                    current_tag.clear();
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    error!("Nature XML 解析错误: {e}");
                    return Err(anyhow!("XML 解析错误: {e}"));
                }
                _ => {}
            }
            buf.clear();
        }

        info!("Nature 解析完成, 共 {} 条", items.len());
        let channel_title = if channel_title.trim().is_empty() {
            None
        } else {
            Some(text::clean_title(&channel_title))
        };
        Ok((items, channel_title, None))
    }
}

/// 把 Text / CData 事件的文本按当前标签写入对应字段（两者逻辑完全一致）。
/// Nature 的 title / content:encoded / dc:title 均以 CDATA 包裹，故两个事件都要处理。
#[allow(clippy::too_many_arguments)]
fn apply_nature_field_text(
    text: &str,
    current_tag: &str,
    in_item: bool,
    title: &mut String,
    link: &mut String,
    content_encoded: &mut String,
    dc_identifier: &mut String,
    prism_doi: &mut String,
    dc_date: &mut String,
    journal: &mut String,
    volume: &mut String,
    number: &mut String,
    authors_list: &mut Vec<String>,
    channel_title: &mut String,
) {
    if in_item {
        match current_tag {
            "title" | "dc:title" => {
                if title.is_empty() || current_tag == "dc:title" {
                    *title = text.to_string();
                }
            }
            "link" => *link = text.to_string(),
            "content:encoded" => *content_encoded = text.to_string(),
            "dc:identifier" => *dc_identifier = text.to_string(),
            "prism:doi" => *prism_doi = text.to_string(),
            "dc:date" => *dc_date = text.to_string(),
            "prism:publicationName" => *journal = text.to_string(),
            "prism:volume" => *volume = text.to_string(),
            "prism:number" => *number = text.to_string(),
            "dc:creator" if !text.trim().is_empty() => {
                authors_list.push(text.trim().to_string());
            }
            _ => {}
        }
    } else if (current_tag == "title" || current_tag == "dc:title") && channel_title.is_empty() {
        *channel_title = text.to_string();
    }
}
