use anyhow::Result;
use models::constructors::*;
use models::{
    Attachment, Author, Feed, FeedItem, FeedType, Folder, FolderType, Literature, LiteratureType,
    Publication, PublicationType, Tag,
};
use mysql_common::value::Value;

/// 把时间字符串解析为 Unix 秒（支持 RFC2822 / `%Y-%m-%d %H:%M:%S` / RFC3339 / `%d %b %Y %H:%M:%S` / `%d %b %Y`）。
fn str_to_ts(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    use chrono::{DateTime, NaiveDateTime};
    if let Ok(dt) = DateTime::parse_from_rfc2822(s) {
        return Some(dt.timestamp());
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(dt.and_utc().timestamp());
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp());
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%d %b %Y %H:%M:%S") {
        return Some(dt.and_utc().timestamp());
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%d %b %Y") {
        return d.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc().timestamp());
    }
    None
}

/// 容错读取时间戳列：远程列可能是 BIGINT（Int/UInt，已是 Unix 秒）也可能是历史 TEXT
/// （datetime 串或数字串）。先以原始 `Value` 读取（恒不 panic），再归一化为 `i64`。
fn value_to_ts(v: Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(i),
        Value::UInt(i) => Some(i as i64),
        Value::Bytes(b) => {
            let s = String::from_utf8(b).ok()?;
            // 已是整数串（unix 秒）→ 直接解析；否则按时间串解析
            if let Ok(ts) = s.trim().parse::<i64>() {
                return Some(ts);
            }
            str_to_ts(&s)
        }
        _ => None,
    }
}

pub struct AuthorRow {
    pub id: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub middle_name: Option<String>,
    pub is_deleted: Option<bool>,
    pub version: Option<i32>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}
impl AuthorRow {
    pub fn from_mysql_row(row: mysql_async::Row) -> Result<Self> {
        Ok(Self {
            id: row.get::<Option<String>, _>("id").flatten(),
            first_name: row.get::<Option<String>, _>("first_name").flatten(),
            last_name: row.get::<Option<String>, _>("last_name").flatten(),
            middle_name: row.get::<Option<String>, _>("middle_name").flatten(),
            is_deleted: row.get::<Option<bool>, _>("is_deleted").flatten(),
            version: row.get::<Option<i32>, _>("version").flatten(),
            created_at: row.get::<Value, _>("created_at").and_then(value_to_ts),
            updated_at: row.get::<Option<i64>, _>("updated_at").flatten(),
        })
    }
    #[must_use]
    pub fn into_model(self) -> Author {
        Author {
            id: self.id.unwrap_or_default(),
            first_name: self.first_name.unwrap_or_default(),
            last_name: self.last_name.unwrap_or_default(),
            middle_name: self.middle_name,
            is_dirty: false,
            is_deleted: self.is_deleted.unwrap_or(false),
            version: self.version.unwrap_or(1),
            created_at: self.created_at.unwrap_or(0),
            updated_at: self.updated_at.unwrap_or(0),
        }
    }
}

pub struct FolderRow {
    pub id: Option<String>,
    pub name: Option<String>,
    pub f_type: Option<String>,
    pub parent_id: Option<String>,
    pub is_deleted: Option<bool>,
    pub version: Option<i32>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}
impl FolderRow {
    pub fn from_mysql_row(row: mysql_async::Row) -> Result<Self> {
        Ok(Self {
            id: row.get::<Option<String>, _>("id").flatten(),
            name: row.get::<Option<String>, _>("name").flatten(),
            f_type: row.get::<Option<String>, _>("folder_type").flatten(),
            parent_id: row.get::<Option<String>, _>("parent_id").flatten(),
            is_deleted: row.get::<Option<bool>, _>("is_deleted").flatten(),
            version: row.get::<Option<i32>, _>("version").flatten(),
            created_at: row.get::<Value, _>("created_at").and_then(value_to_ts),
            updated_at: row.get::<Option<i64>, _>("updated_at").flatten(),
        })
    }
    #[must_use]
    pub fn into_model(self) -> Folder {
        let ft_str = self.f_type.unwrap_or_else(|| "custom".to_string());
        let ft = serde_json::from_str(&format!("\"{ft_str}\"")).unwrap_or(FolderType::Custom);
        let mut f = create_folder(
            self.id.unwrap_or_default(),
            self.name.unwrap_or_default(),
            ft,
        );
        f.parent_id = self.parent_id;
        f.is_dirty = false;
        f.is_deleted = self.is_deleted.unwrap_or(false);
        f.version = self.version.unwrap_or(1);
        f.created_at = self.created_at.unwrap_or(0);
        f.updated_at = self.updated_at.unwrap_or(0);
        f
    }
}

pub struct AttachmentRow {
    pub id: Option<String>,
    pub lit_id: Option<String>,
    pub path: Option<String>,
    pub name: Option<String>,
    pub size: Option<u64>,
    pub mime: Option<String>,
    pub etag: Option<String>,
    pub hash: Option<String>,
    pub is_main: Option<bool>,
    pub is_deleted: Option<bool>,
    pub version: Option<i32>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}
impl AttachmentRow {
    pub fn from_mysql_row(row: mysql_async::Row) -> Result<Self> {
        Ok(Self {
            id: row.get::<Option<String>, _>("id").flatten(),
            lit_id: row.get::<Option<String>, _>("literature_id").flatten(),
            path: row.get::<Option<String>, _>("file_path").flatten(),
            name: row.get::<Option<String>, _>("file_name").flatten(),
            size: row.get::<Option<u64>, _>("file_size").flatten(),
            mime: row.get::<Option<String>, _>("mime_type").flatten(),
            etag: row.get::<Option<String>, _>("etag").flatten(),
            hash: row.get::<Option<String>, _>("hash").flatten(),
            is_main: row.get::<Option<bool>, _>("is_main").flatten(),
            is_deleted: row.get::<Option<bool>, _>("is_deleted").flatten(),
            version: row.get::<Option<i32>, _>("version").flatten(),
            created_at: row.get::<Value, _>("created_at").and_then(value_to_ts),
            updated_at: row.get::<Option<i64>, _>("updated_at").flatten(),
        })
    }
    #[must_use]
    pub fn into_model(self, base_path: &std::path::Path) -> Attachment {
        let raw_path = self.path.unwrap_or_default();
        let abs_path = if raw_path.is_empty() {
            String::new()
        } else {
            let normalized_relative = raw_path.replace('\\', "/");
            base_path
                .join(normalized_relative)
                .to_string_lossy()
                .to_string()
        };

        Attachment {
            id: self.id.unwrap_or_default(),
            literature_id: self.lit_id.unwrap_or_default(),
            file_path: abs_path,
            file_name: self.name.unwrap_or_default(),
            file_size: self.size.unwrap_or(0),
            mime_type: self.mime,
            etag: self.etag,
            hash: self.hash,
            is_main: self.is_main.unwrap_or(false),
            is_dirty: false,
            is_deleted: self.is_deleted.unwrap_or(false),
            version: self.version.unwrap_or(1),
            created_at: self.created_at.unwrap_or(0),
            updated_at: self.updated_at.unwrap_or(0),
        }
    }
}

pub struct FeedRow {
    pub id: Option<String>,
    pub name: Option<String>,
    pub f_type: Option<String>,
    pub url: Option<String>,
    pub last_up: Option<String>,
    pub update_interval: Option<u32>,
    pub is_deleted: Option<bool>,
    pub version: Option<i32>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}
impl FeedRow {
    pub fn from_mysql_row(row: mysql_async::Row) -> Result<Self> {
        Ok(Self {
            id: row.get::<Option<String>, _>("id").flatten(),
            name: row.get::<Option<String>, _>("name").flatten(),
            f_type: row.get::<Option<String>, _>("feed_type").flatten(),
            url: row.get::<Option<String>, _>("url").flatten(),
            last_up: row.get::<Option<String>, _>("last_updated_at").flatten(),
            update_interval: row.get::<Option<u32>, _>("update_interval").flatten(),
            is_deleted: row.get::<Option<bool>, _>("is_deleted").flatten(),
            version: row.get::<Option<i32>, _>("version").flatten(),
            created_at: row.get::<Value, _>("created_at").and_then(value_to_ts),
            updated_at: row.get::<Option<i64>, _>("updated_at").flatten(),
        })
    }
    #[must_use]
    pub fn into_model(self) -> Feed {
        let ft_str = self.f_type.unwrap_or_else(|| "rss".to_string());
        let ft = serde_json::from_str(&format!("\"{ft_str}\"")).unwrap_or(FeedType::Rss);
        let mut f = create_feed(
            self.id.unwrap_or_default(),
            self.name.unwrap_or_default(),
            ft,
        );
        f.url = self.url;
        f.last_updated_at = self.last_up;
        f.update_interval = self.update_interval.unwrap_or(24);
        f.is_dirty = false;
        f.is_deleted = self.is_deleted.unwrap_or(false);
        f.version = self.version.unwrap_or(1);
        f.created_at = self.created_at.unwrap_or(0);
        f.updated_at = self.updated_at.unwrap_or(0);
        f
    }
}

pub struct TagRow {
    pub id: Option<String>,
    pub name: Option<String>,
    pub color: Option<String>,
    pub is_deleted: Option<bool>,
    pub version: Option<i32>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

impl TagRow {
    pub fn from_mysql_row(row: mysql_async::Row) -> Result<Self> {
        Ok(Self {
            id: row.get::<Option<String>, _>("id").flatten(),
            name: row.get::<Option<String>, _>("name").flatten(),
            color: row.get::<Option<String>, _>("color").flatten(),
            is_deleted: row.get::<Option<bool>, _>("is_deleted").flatten(),
            version: row.get::<Option<i32>, _>("version").flatten(),
            created_at: row.get::<Value, _>("created_at").and_then(value_to_ts),
            updated_at: row.get::<Option<i64>, _>("updated_at").flatten(),
        })
    }

    #[must_use]
    pub fn into_model(self) -> Tag {
        Tag {
            id: self.id.unwrap_or_default(),
            name: self.name.unwrap_or_default(),
            color: self.color.unwrap_or_else(|| "#808080".to_string()),
            created_at: self.created_at.unwrap_or(0),
            updated_at: self.updated_at.unwrap_or(0),
            version: self.version.unwrap_or(1),
            is_deleted: self.is_deleted.unwrap_or(false),
            is_dirty: false,
        }
    }
}

pub struct FeedItemRow {
    pub id: Option<String>,
    pub title: Option<String>,
    pub fid: Option<String>,
    pub read: Option<bool>,
    pub added: Option<bool>,
    pub added_at: Option<String>,
    pub authors: Option<String>,
    pub year: Option<i32>,
    pub i_type: Option<String>,
    pub journal: Option<String>,
    pub publisher: Option<String>,
    pub abstract_text: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
    pub vol: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub pub_at: Option<String>,
    pub is_deleted: Option<bool>,
    pub version: Option<i32>,
    pub updated_at: Option<i64>,
}
impl FeedItemRow {
    pub fn from_mysql_row(row: mysql_async::Row) -> Result<Self> {
        Ok(Self {
            id: row.get::<Option<String>, _>("id").flatten(),
            title: row.get::<Option<String>, _>("title").flatten(),
            fid: row.get::<Option<String>, _>("feed_id").flatten(),
            read: row.get::<Option<bool>, _>("is_read").flatten(),
            added: row.get::<Option<bool>, _>("is_added_to_library").flatten(),
            added_at: row.get::<Option<String>, _>("added_at").flatten(),
            authors: row.get::<Option<String>, _>("authors").flatten(),
            year: row.get::<Option<i32>, _>("year").flatten(),
            i_type: row.get::<Option<String>, _>("type").flatten(),
            journal: row.get::<Option<String>, _>("journal").flatten(),
            publisher: row.get::<Option<String>, _>("publisher").flatten(),
            abstract_text: row.get::<Option<String>, _>("abstract_text").flatten(),
            doi: row.get::<Option<String>, _>("doi").flatten(),
            url: row.get::<Option<String>, _>("url").flatten(),
            vol: row.get::<Option<String>, _>("volume").flatten(),
            issue: row.get::<Option<String>, _>("issue").flatten(),
            pages: row.get::<Option<String>, _>("pages").flatten(),
            pub_at: row.get::<Option<String>, _>("published_at").flatten(),
            is_deleted: row.get::<Option<bool>, _>("is_deleted").flatten(),
            version: row.get::<Option<i32>, _>("version").flatten(),
            updated_at: row.get::<Option<i64>, _>("updated_at").flatten(),
        })
    }
    #[must_use]
    pub fn into_model(self) -> FeedItem {
        let it_str = self.i_type.unwrap_or_else(|| "article".to_string());
        let it = serde_json::from_str(&format!("\"{it_str}\"")).unwrap_or(LiteratureType::Article);
        let mut i = create_feed_item(
            self.id.unwrap_or_default(),
            self.title.unwrap_or_default(),
            self.fid.unwrap_or_default(),
        );
        i.is_read = self.read.unwrap_or(false);
        i.is_added_to_library = self.added.unwrap_or(false);
        i.added_at = self.added_at.unwrap_or_default();
        i.authors = self
            .authors
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        i.year = self.year;
        i.literature_type = it;
        i.journal = self.journal;
        i.publisher = self.publisher;
        i.abstract_text = self.abstract_text;
        i.doi = self.doi;
        i.url = self.url;
        i.volume = self.vol;
        i.issue = self.issue;
        i.pages = self.pages;
        i.published_at = self.pub_at;
        i.is_dirty = false;
        i.is_deleted = self.is_deleted.unwrap_or(false);
        i.version = self.version.unwrap_or(1);
        i.updated_at = self.updated_at.unwrap_or(0);
        i
    }
}

pub struct LiteratureRow {
    pub id: Option<String>,
    pub title: Option<String>,
    pub year: Option<i32>,
    pub month: Option<i32>,
    pub day: Option<i32>,
    pub lit_type: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub abstract_text: Option<String>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub url: Option<String>,
    pub rating: Option<i32>,
    pub reading_status: Option<String>,
    pub is_deleted: Option<bool>,
    pub version: Option<i32>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub pub_id: Option<String>,
    pub pub_name: Option<String>,
    pub pub_type: Option<String>,
    pub pub_abbr: Option<String>,
    pub pub_publisher: Option<String>,
    pub pub_ccf: Option<String>,
    pub pub_jcr: Option<String>,
    pub pub_cas: Option<String>,
    pub pub_is_deleted: Option<bool>,
    pub pub_version: Option<i32>,
    pub pub_created_at: Option<i64>,
    pub pub_updated_at: Option<i64>,
}

impl LiteratureRow {
    pub fn from_mysql_row(row: mysql_async::Row) -> Result<Self> {
        Ok(Self {
            id: row.get::<Option<String>, _>(0).flatten(),
            title: row.get::<Option<String>, _>(1).flatten(),
            year: row.get::<Option<i32>, _>(2).flatten(),
            month: row.get::<Option<i32>, _>(3).flatten(),
            day: row.get::<Option<i32>, _>(4).flatten(),
            lit_type: row.get::<Option<String>, _>(5).flatten(),
            volume: row.get::<Option<String>, _>(6).flatten(),
            issue: row.get::<Option<String>, _>(7).flatten(),
            pages: row.get::<Option<String>, _>(8).flatten(),
            abstract_text: row.get::<Option<String>, _>(9).flatten(),
            doi: row.get::<Option<String>, _>(10).flatten(),
            arxiv_id: row.get::<Option<String>, _>(11).flatten(),
            url: row.get::<Option<String>, _>(12).flatten(),
            rating: row.get::<Option<i32>, _>(13).flatten(),
            reading_status: row.get::<Option<String>, _>(14).flatten(),
            is_deleted: row.get::<Option<bool>, _>(15).flatten(),
            version: row.get::<Option<i32>, _>(16).flatten(),
            created_at: row.get::<Value, _>(17).and_then(value_to_ts),
            updated_at: row.get::<Option<i64>, _>(18).flatten(),
            pub_id: row.get::<Option<String>, _>(19).flatten(),
            pub_name: row.get::<Option<String>, _>(20).flatten(),
            pub_type: row.get::<Option<String>, _>(21).flatten(),
            pub_abbr: row.get::<Option<String>, _>(22).flatten(),
            pub_publisher: row.get::<Option<String>, _>(23).flatten(),
            pub_ccf: row.get::<Option<String>, _>(24).flatten(),
            pub_jcr: row.get::<Option<String>, _>(25).flatten(),
            pub_cas: row.get::<Option<String>, _>(26).flatten(),
            pub_is_deleted: row.get::<Option<bool>, _>(27).flatten(),
            pub_version: row.get::<Option<i32>, _>(28).flatten(),
            pub_created_at: row.get::<Option<i64>, _>(29).flatten(),
            pub_updated_at: row.get::<Option<i64>, _>(30).flatten(),
        })
    }
    #[must_use]
    pub fn into_literature(self) -> Literature {
        let lit_type_str = self.lit_type.unwrap_or_else(|| "article".to_string());
        let lit_type =
            serde_json::from_str(&format!("\"{lit_type_str}\"")).unwrap_or(LiteratureType::Article);
        let mut lit = create_literature(
            self.id.unwrap_or_default(),
            self.title.unwrap_or_default(),
            lit_type,
        );
        lit.year = self.year;
        lit.month = self.month;
        lit.day = self.day;

        if let Some(pid) = self.pub_id {
            let pt_str = self.pub_type.unwrap_or_else(|| "journal".to_string());
            let pt = match pt_str.to_lowercase().as_str() {
                "journal" => PublicationType::Journal,
                "conference" => PublicationType::Conference,
                "book" => PublicationType::Book,
                _ => PublicationType::Journal,
            };
            let mut pub_obj = create_publication(self.pub_name.unwrap_or_default(), pt);
            pub_obj.id = pid;
            pub_obj.abbreviation = self.pub_abbr;
            pub_obj.publisher = self.pub_publisher;
            pub_obj.ccf_rank = self.pub_ccf;
            pub_obj.jcr_rank = self.pub_jcr;
            pub_obj.cas_rank = self.pub_cas;
            pub_obj.version = self.pub_version.unwrap_or(1);
            lit.publication = Some(pub_obj);
        }

        lit.volume = self.volume;
        lit.issue = self.issue;
        lit.pages = self.pages;
        lit.abstract_text = self.abstract_text;
        lit.doi = self.doi;
        lit.arxiv_id = self.arxiv_id;
        lit.url = self.url;
        lit.rating = self.rating.unwrap_or(0);
        lit.reading_status = self
            .reading_status
            .as_ref()
            .and_then(|status_str| serde_json::from_str(&format!("\"{status_str}\"")).ok())
            .unwrap_or_default();
        lit.is_dirty = false;
        lit.is_deleted = self.is_deleted.unwrap_or(false);
        lit.version = self.version.unwrap_or(1);
        lit.created_at = self.created_at.unwrap_or(0);
        lit.updated_at = self.updated_at.unwrap_or(0);
        lit
    }
}

pub struct PublicationRow {
    pub id: Option<String>,
    pub name: Option<String>,
    pub pub_type: Option<String>,
    pub abbreviation: Option<String>,
    pub publisher: Option<String>,
    pub ccf_rank: Option<String>,
    pub jcr_rank: Option<String>,
    pub cas_rank: Option<String>,
    pub is_deleted: Option<bool>,
    pub version: Option<i32>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

impl PublicationRow {
    pub fn from_mysql_row(row: mysql_async::Row) -> Result<Self> {
        Ok(Self {
            id: row.get::<Option<String>, _>("id").flatten(),
            name: row.get::<Option<String>, _>("name").flatten(),
            pub_type: row.get::<Option<String>, _>("publication_type").flatten(),
            abbreviation: row.get::<Option<String>, _>("abbreviation").flatten(),
            publisher: row.get::<Option<String>, _>("publisher").flatten(),
            ccf_rank: row.get::<Option<String>, _>("ccf_rank").flatten(),
            jcr_rank: row.get::<Option<String>, _>("jcr_rank").flatten(),
            cas_rank: row.get::<Option<String>, _>("cas_rank").flatten(),
            is_deleted: row.get::<Option<bool>, _>("is_deleted").flatten(),
            version: row.get::<Option<i32>, _>("version").flatten(),
            created_at: row.get::<Option<i64>, _>("created_at").flatten(),
            updated_at: row.get::<Option<i64>, _>("updated_at").flatten(),
        })
    }

    #[must_use]
    pub fn into_model(self) -> Publication {
        let pt_str = self.pub_type.unwrap_or_else(|| "journal".to_string());
        let pt = match pt_str.to_lowercase().as_str() {
            "journal" => PublicationType::Journal,
            "conference" => PublicationType::Conference,
            "book" => PublicationType::Book,
            _ => PublicationType::Journal,
        };

        Publication {
            id: self.id.unwrap_or_default(),
            name: self.name.unwrap_or_default(),
            publication_type: pt,
            abbreviation: self.abbreviation,
            publisher: self.publisher,
            ccf_rank: self.ccf_rank,
            jcr_rank: self.jcr_rank,
            cas_rank: self.cas_rank,
            is_dirty: false,
            is_deleted: self.is_deleted.unwrap_or(false),
            version: self.version.unwrap_or(1),
            created_at: self.created_at.unwrap_or(0),
            updated_at: self.updated_at.unwrap_or(0),
        }
    }
}
