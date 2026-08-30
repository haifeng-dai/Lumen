//! 文件名格式化工具
//!
//! 提供安全的文件名生成和格式化功能

use log::debug;
use models::Literature;
use std::path::Path;
use uuid::Uuid;

/// 文件名生成选项
#[derive(Debug, Clone)]
pub struct FilenameOptions {
    /// 第一作者姓
    pub last_name: String,
    /// 第一作者名
    pub first_name: String,
    /// 年份
    pub year: String,
    /// 文献标题
    pub title: String,
    /// 出版物（期刊/会议）
    pub publication: String,
    /// 文件扩展名
    pub extension: String,
    /// 是否为主文件（如论文PDF）
    pub is_main: bool,
}

impl FilenameOptions {
    /// 创建新的文件名选项
    pub fn new(
        last_name: impl Into<String>,
        first_name: impl Into<String>,
        year: impl Into<String>,
        title: impl Into<String>,
        publication: impl Into<String>,
        extension: impl Into<String>,
        is_main: bool,
    ) -> Self {
        Self {
            last_name: last_name.into(),
            first_name: first_name.into(),
            year: year.into(),
            title: title.into(),
            publication: publication.into(),
            extension: extension.into(),
            is_main,
        }
    }
}

/// 将字符串转换为安全的文件名
///
/// 保留字母数字、空格、连字符和下划线，其他字符替换为下划线
#[must_use]
pub fn sanitize_filename(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// 将作者姓名转换为安全的文件名部分
///
/// 只保留字母数字字符，其他字符替换为下划线
#[must_use]
pub fn sanitize_author_name(author: &str) -> String {
    author
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

/// 生成标准化的文件名
///
/// 格式: {作者}{年份}-{标题}.{扩展名}
/// 示例: "Smith2023-Deep_Learning_in_CV.pdf"
#[must_use]
pub fn generate_filename(options: &FilenameOptions) -> String {
    let safe_author = sanitize_author_name(&options.last_name);
    let safe_title = sanitize_filename(&options.title);
    let extension = if options.extension.is_empty() {
        String::new()
    } else {
        format!(".{}", options.extension)
    };

    let filename = format!(
        "{}{}-{}{}",
        safe_author, options.year, safe_title, extension
    );
    debug!(
        "文件命名: 生成主文件名 author={}, year={}, title='{}' => {filename}",
        safe_author,
        options.year,
        options.title.chars().take(40).collect::<String>()
    );
    filename
}

/// 获取标题的首字母缩写
fn get_first_char_title(title: &str) -> String {
    title
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .collect::<String>()
        .to_uppercase()
}

/// 根据模板生成文件名
#[must_use]
pub fn generate_filename_from_template(template: &str, options: &FilenameOptions) -> String {
    let mut result = template.to_string();

    // 预先清理各个部分
    let safe_title = sanitize_filename(&options.title);
    let safe_last_name = sanitize_author_name(&options.last_name);
    let safe_first_name = sanitize_author_name(&options.first_name);
    let safe_publication = sanitize_filename(&options.publication);
    let first_char_title = sanitize_filename(&get_first_char_title(&options.title));

    // 替换模板变量
    result = result.replace("{title}", &safe_title);
    result = result.replace("{author}", &safe_last_name);
    result = result.replace("{lastname}", &safe_last_name);
    result = result.replace("{firstname}", &safe_first_name);
    result = result.replace("{year}", &options.year);
    result = result.replace("{publication}", &safe_publication);
    result = result.replace("{firstchartitle}", &first_char_title);

    // 加上扩展名
    let final_name = if options.extension.is_empty() {
        result
    } else {
        format!("{}.{}", result, options.extension)
    };
    debug!(
        "文件命名: 模板生成 template='{template}', title='{}' => {final_name}",
        options.title.chars().take(40).collect::<String>()
    );
    final_name
}

/// 生成附件文件名
///
/// 格式: {作者}{年份}-{标题}_att_{随机后缀}.{扩展名}
/// 示例: "Smith2023-Deep_Learning_in_CV_att_a3f4.pdf"
#[must_use]
pub fn generate_attachment_filename(options: &FilenameOptions) -> String {
    let safe_author = sanitize_author_name(&options.last_name);
    let safe_title = sanitize_filename(&options.title);
    let suffix = Uuid::new_v4().to_string()[..4].to_string();

    let filename = format!(
        "{}{}-{}_att_{}.{}",
        safe_author, options.year, safe_title, suffix, options.extension
    );
    debug!(
        "文件命名: 生成附件文件名 suffix={suffix}, title='{}' => {filename}",
        options.title.chars().take(40).collect::<String>()
    );
    filename
}

/// 从路径生成文件名选项
///
/// 根据文献信息和文件路径创建文件名选项
#[must_use]
pub fn filename_options_from_path(
    last_name: &str,
    first_name: &str,
    year: Option<i32>,
    title: &str,
    publication: &str,
    file_path: &Path,
    is_main: bool,
) -> FilenameOptions {
    let year_str = year.map_or_else(|| "0000".to_string(), |y| y.to_string());

    let extension = file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_string();

    debug!(
        "文件命名: 从路径解析 author={last_name}, year={year_str}, ext={extension}, is_main={is_main}"
    );
    FilenameOptions::new(
        last_name,
        first_name,
        year_str,
        title,
        publication,
        extension,
        is_main,
    )
}

/// 从文献生成文件名选项
#[must_use]
pub fn filename_options_from_literature(
    lit: &Literature,
    extension: &str,
    is_main: bool,
) -> FilenameOptions {
    let (last_name, first_name) = lit.authors.first().map_or_else(
        || ("Unknown".to_string(), String::new()),
        |a| (a.last_name.clone(), a.first_name.clone()),
    );

    let year_str = lit
        .year
        .map_or_else(|| "0000".to_string(), |y| y.to_string());

    let publication = lit
        .publication
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_default();

    debug!(
        "文件命名: 从文献生成选项 author={last_name}, year={year_str}, pub='{publication}', title='{}'",
        lit.title.chars().take(40).collect::<String>()
    );
    FilenameOptions::new(
        last_name,
        first_name,
        year_str,
        &lit.title,
        publication,
        extension,
        is_main,
    )
}

/// 为文献生成合适的文件名
///
/// 根据是否是主文件选择不同的命名策略
/// 主文件: {AuthorYear-Title}_{4hex}.{ext}
/// 附件: {AuthorYear-Title}_att_{4hex}.{ext}
#[must_use]
pub fn generate_literature_filename(options: &FilenameOptions, template: Option<&str>) -> String {
    let filename = if options.is_main {
        let base = if let Some(tmpl) = template {
            debug!("文件命名: 主文件 + 模板模式");
            generate_filename_from_template(tmpl, options)
        } else {
            debug!("文件命名: 主文件 + 默认模式");
            generate_filename(options)
        };
        // 在扩展名前插入随机后缀
        let suffix = &Uuid::new_v4().to_string()[..4];
        if let Some(dot_pos) = base.rfind('.') {
            format!("{}_{suffix}{}", &base[..dot_pos], &base[dot_pos..])
        } else {
            format!("{base}_{suffix}")
        }
    } else {
        debug!("文件命名: 附件模式");
        generate_attachment_filename(options)
    };
    debug!("文件命名: 最终文件名 => {filename}");
    filename
}

/// 计算一篇文献所有附件共享的安全模板前缀。
#[must_use]
pub(crate) fn batch_attachment_prefix(lit: &Literature, template: &str) -> String {
    let options = filename_options_from_literature(lit, "", false);
    sanitize_filename(&generate_filename_from_template(template, &options))
}

#[must_use]
pub(crate) fn generate_batch_attachment_filename_from_prefix(
    prefix: &str,
    old_name: &str,
    extension: &str,
    is_main: bool,
) -> String {
    let tail = existing_random_tail(old_name).unwrap_or_else(|| {
        let suffix = &Uuid::new_v4().to_string()[..4];
        if is_main {
            format!("_{suffix}")
        } else {
            format!("_att_{suffix}")
        }
    });
    if extension.is_empty() {
        format!("{prefix}{tail}")
    } else {
        format!("{prefix}{tail}.{extension}")
    }
}

fn existing_random_tail(old_name: &str) -> Option<String> {
    let stem = Path::new(old_name).file_stem()?.to_str()?;
    for marker in ["_att_", "_"] {
        if let Some(index) = stem.rfind(marker) {
            let suffix = &stem[index + marker.len()..];
            if suffix.len() == 4 && suffix.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(format!("{marker}{suffix}"));
            }
        }
    }
    None
}

#[cfg(test)]
mod batch_filename_tests {
    use super::{batch_attachment_prefix, generate_batch_attachment_filename_from_prefix};
    use models::{LiteratureType, constructors::create_literature};

    fn literature() -> models::Literature {
        let mut lit = create_literature("lit", "A Title", LiteratureType::Article);
        lit.year = Some(2024);
        lit
    }

    #[test]
    fn preserves_main_and_attachment_tails_and_extensions() {
        let lit = literature();
        assert_eq!(
            generate_batch_attachment_filename_from_prefix(
                &batch_attachment_prefix(&lit, "{author}-{year}"),
                "old_ab12.pdf",
                "pdf",
                true,
            ),
            "Unknown-2024_ab12.pdf"
        );
        assert_eq!(
            generate_batch_attachment_filename_from_prefix(
                &batch_attachment_prefix(&lit, "{author}-{year}"),
                "old_att_cd34.PNG",
                "PNG",
                false,
            ),
            "Unknown-2024_att_cd34.PNG"
        );
    }

    #[test]
    fn sanitizes_template_and_keeps_no_extension() {
        let lit = literature();
        let prefix = batch_attachment_prefix(&lit, "folder/{title}");
        let name = generate_batch_attachment_filename_from_prefix(
            &prefix,
            "old_att_ef56.zip",
            "zip",
            false,
        );
        assert_eq!(name, "folder_A Title_att_ef56.zip");
        assert!(
            generate_batch_attachment_filename_from_prefix(
                &batch_attachment_prefix(&lit, "{title}"),
                "old_1234",
                "",
                true,
            )
            .ends_with("_1234")
        );
    }

    #[test]
    fn adds_tail_only_when_old_name_has_no_standard_tail() {
        let lit = literature();
        let prefix = batch_attachment_prefix(&lit, "{title}");
        let first = generate_batch_attachment_filename_from_prefix(&prefix, "old.pdf", "pdf", true);
        let second = generate_batch_attachment_filename_from_prefix(&prefix, &first, "pdf", true);
        assert_eq!(first, second);
        let stem = first.strip_suffix(".pdf").unwrap();
        let tail = stem.rsplit_once('_').unwrap().1;
        assert_eq!(tail.len(), 4);
        assert!(tail.chars().all(|c| c.is_ascii_hexdigit()));

        let first =
            generate_batch_attachment_filename_from_prefix(&prefix, "old.png", "png", false);
        let second = generate_batch_attachment_filename_from_prefix(&prefix, &first, "png", false);
        assert_eq!(first, second);
        let stem = first.strip_suffix(".png").unwrap();
        let tail = stem.rsplit_once("_att_").unwrap().1;
        assert_eq!(tail.len(), 4);
        assert!(tail.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
