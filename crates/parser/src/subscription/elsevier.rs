use crate::text;
use anyhow::{Result, anyhow};
use log::{debug, error, info};
use models::FeedItem;
use models::constructors::*;
use quick_xml::{events::Event, reader::Reader};

/// Elsevier RSS 解析器
///
/// 负责解析 `ScienceDirect` 提供的 RSS 订阅源（如 Automatica 等期刊）
/// 格式示例：
/// <item>
///   <title>...</title>
///   <description><![CDATA[<p>Publication date: April 2026</p><p><b>Source:</b> Automatica, Volume 186</p><p>Author(s): Name1, Name2</p>]]></description>
///   <link>https://www.sciencedirect.com/science/article/pii/S0005109826000300</link>
///   <guid>https://www.sciencedirect.com/science/article/pii/S0005109826000300</guid>
/// </item>
pub struct ElsevierSubscriptionParser;

impl ElsevierSubscriptionParser {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for ElsevierSubscriptionParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ElsevierSubscriptionParser {
    /// 解析 Elsevier RSS 订阅源
    pub fn parse(
        xml: &str,
        feed_id: &str,
    ) -> Result<(Vec<FeedItem>, Option<String>, Option<String>)> {
        info!("开始解析 Elsevier RSS 订阅源: {feed_id}");
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);

        let mut items = Vec::new();
        let mut buf = Vec::new();

        let mut in_item = false;
        let mut current_tag = String::new();

        let mut title = String::new();
        let mut url = String::new();
        let mut description = String::new();
        let mut date_str = String::new();
        let mut source_info = String::new();
        let mut authors_str = String::new();
        let mut channel_update_time = None;
        let mut channel_title = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = e.name().as_ref().to_string();
                    if name == "item" {
                        in_item = true;
                        title.clear();
                        url.clear();
                        description.clear();
                        date_str.clear();
                        source_info.clear();
                        authors_str.clear();
                    }
                    current_tag = name;
                }
                Ok(Event::Text(ref e)) => {
                    let text = e.as_ref().to_string();
                    apply_field_text(
                        &text,
                        current_tag.as_str(),
                        in_item,
                        &mut title,
                        &mut url,
                        &mut description,
                        &mut channel_update_time,
                        &mut channel_title,
                    );
                }
                Ok(Event::CData(ref e)) => {
                    let text = e.as_ref().to_string();
                    apply_field_text(
                        &text,
                        current_tag.as_str(),
                        in_item,
                        &mut title,
                        &mut url,
                        &mut description,
                        &mut channel_update_time,
                        &mut channel_title,
                    );
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name().as_ref().to_string();
                    if name == "item" {
                        in_item = false;

                        // 从 description 中提取信息
                        let pub_date = extract_pub_date(&description);
                        let source = extract_source_info(&description);
                        let authors = extract_authors(&description);

                        let journal = source.as_ref().map(|(j, _)| j.as_str());
                        let volume = source.as_ref().map(|(_, v)| v.as_str());

                        // 从 URL 中提取 PII 作为 ID
                        let item_id = extract_pii_from_url(&url);

                        let mut item = create_feed_item(
                            item_id,
                            text::clean_title(&title),
                            feed_id.to_string(),
                        );
                        item.url = text::clean_optional_text(Some(&url));
                        item.abstract_text = text::clean_optional_text(Some(&description));
                        // 这里的 text::clean_optional_text 需要 Option<&str>
                        // 而 journal 和 volume 是 Option<&str>
                        item.journal = text::clean_optional_text(journal);
                        item.volume = text::clean_optional_text(volume);
                        item.published_at =
                            pub_date.map(|s| crate::time::normalize_time_string(&s));

                        // 添加作者
                        if let Some(authors_list) = authors {
                            for author_name in authors_list {
                                let parts: Vec<&str> = author_name.split_whitespace().collect();
                                if !parts.is_empty() {
                                    let last = text::clean_author_name(parts.last().unwrap_or(&""));
                                    let first = text::clean_author_name(
                                        &parts[..parts.len() - 1].join(" "),
                                    );
                                    item.authors.push(create_author(last, first));
                                }
                            }
                        }

                        debug!("解析到 Elsevier 条目: {} (PII: {})", item.title, item.id);
                        items.push(item);
                    }
                    current_tag.clear();
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    error!("Elsevier XML 解析错误: {e}");
                    return Err(anyhow!("XML 解析错误: {e}"));
                }
                _ => {}
            }
            buf.clear();
        }

        info!(
            "Elsevier 解析完成, 共获取 {} 条文献, 频道标题: {:?}, 更新时间: {:?}",
            items.len(),
            channel_title,
            channel_update_time
        );
        let channel_title = if channel_title.trim().is_empty() {
            None
        } else {
            Some(text::clean_title(&channel_title))
        };
        Ok((items, channel_title, channel_update_time))
    }
}

/// 从 description 中提取发布日期
fn extract_pub_date(description: &str) -> Option<String> {
    let start = description.find("Publication date: ")?;
    let rest = &description[start + "Publication date: ".len()..];
    let end = rest.find("</p>")?;
    Some(rest[..end].trim().to_string())
}

/// 从 description 中提取期刊和卷号
fn extract_source_info(description: &str) -> Option<(String, String)> {
    let start = description.find("<b>Source:</b> ")?;
    let rest = &description[start + "<b>Source:</b> ".len()..];
    let end = rest.find("</p>")?;
    let source_text = rest[..end].trim();

    // 解析格式: "Automatica, Volume 186"
    let parts: Vec<&str> = source_text.split(", ").collect();
    if parts.len() >= 2 {
        let journal = parts[0].trim().to_string();
        let volume = parts[1].trim_start_matches("Volume ").to_string();
        Some((journal, volume))
    } else {
        Some((source_text.to_string(), String::new()))
    }
}

/// 从 description 中提取作者列表
fn extract_authors(description: &str) -> Option<Vec<String>> {
    let start = description.find("Author(s): ")?;
    let rest = &description[start + "Author(s): ".len()..];
    let end = rest.find("</p>")?;
    let authors_str = rest[..end].trim();

    // 按逗号分割作者
    Some(
        authors_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

/// 从 URL 中提取 PII ID
fn extract_pii_from_url(url: &str) -> String {
    // URL 格式: https://www.sciencedirect.com/science/article/pii/S0005109826000300
    if let Some(start) = url.find("/pii/") {
        let rest = &url[start + 5..]; // "/pii/" 的长度是 5
        // 提取 PII ID (直到 ? 或路径结束)
        if let Some(end) = rest.find('?') {
            rest[..end].trim().to_string()
        } else {
            rest.trim().to_string()
        }
    } else {
        // 如果没有 PII，使用 URL 作为 ID
        url.trim().to_string()
    }
}

/// 把 Text / CData 事件的文本按当前标签写入对应字段（两者逻辑完全一致）。
/// ScienceDirect 的 `title` / `description` 以 CDATA 包裹，故两个事件都要处理。
#[allow(clippy::too_many_arguments)]
fn apply_field_text(
    text: &str,
    current_tag: &str,
    in_item: bool,
    title: &mut String,
    url: &mut String,
    description: &mut String,
    channel_update_time: &mut Option<String>,
    channel_title: &mut String,
) {
    if in_item {
        match current_tag {
            "title" => *title = text.to_string(),
            "link" => *url = text.to_string(),
            "description" => *description = text.to_string(),
            _ => {}
        }
    } else if current_tag == "lastBuildDate" {
        *channel_update_time = Some(text.to_string());
    } else if current_tag == "title" && channel_title.is_empty() {
        // 频道级标题（期刊名），仅在非 item 内捕获
        *channel_title = text.to_string();
    }
}
