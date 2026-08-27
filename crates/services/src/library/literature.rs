use anyhow::Result;
use database::Database;
use database::state::LocalStateManager;
use log::{debug, error, info, warn};
/// 数据库操作单例管理器
///
/// 负责协调持久化存储与内存数据的同步
use models::{Author, Literature, LiteratureNote, Publication, PublicationType};
use parser::normalize::sanitize_arxiv_identifiers;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::{collections, fs};

use crate::analysis::CCFService;
use crate::utils::filename::{FilenameOptions, generate_literature_filename};

pub struct LiteratureService;

impl LiteratureService {
    #[must_use]
    pub fn new() -> Self {
        debug!("文献服务: 初始化");
        Self
    }
}

impl Default for LiteratureService {
    fn default() -> Self {
        Self::new()
    }
}

/// 标题模糊匹配的相似度阈值（0~1）。越高越严格（误报少、漏报多）。
const TITLE_SIMILARITY_THRESHOLD: f64 = 0.85;

/// 标题模糊匹配归一化：转小写，非字母数字统一替换为空格并折叠空白。
fn normalize_title_for_match(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut prev_space = false;
    for c in title.chars() {
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

/// 归一化编辑距离相似度：1 - 距离 / max(长度)，区间 [0,1]。
/// 长度差异超过 50% 直接剪枝返回 0，避免无谓计算。
fn levenshtein_ratio(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    if n == 0 && m == 0 {
        return 1.0;
    }
    if n == 0 || m == 0 {
        return 0.0;
    }
    let max_len = n.max(m);
    let min_len = n.min(m);
    if max_len - min_len > max_len / 2 {
        return 0.0;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut cur = vec![0usize; m + 1];
    for i in 1..=n {
        cur[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let dist = prev[m];
    1.0 - (dist as f64) / (max_len as f64)
}

/// 词重叠系数（Szymkiewicz-Simpson）：|A∩B| / min(|A|,|B|)。
/// 能捕捉“一个标题是另一个的子集”的情况（如带副标题的变体）。
fn token_overlap(a: &str, b: &str) -> f64 {
    let sa: HashSet<&str> = a.split_whitespace().collect();
    let sb: HashSet<&str> = b.split_whitespace().collect();
    if sa.is_empty() || sb.is_empty() {
        return 0.0;
    }
    let inter = sa.intersection(&sb).count();
    inter as f64 / sa.len().min(sb.len()) as f64
}

/// 标题相似度：取编辑距离与词重叠系数的最大值。
fn title_similarity(a: &str, b: &str) -> f64 {
    let na = normalize_title_for_match(a);
    let nb = normalize_title_for_match(b);
    if na.is_empty() || nb.is_empty() {
        return 0.0;
    }
    levenshtein_ratio(&na, &nb).max(token_overlap(&na, &nb))
}

/// 若期刊的 abbreviation 缺失或为空，自动用词表计算并填充。
fn fill_publication_abbreviation(pub_info: &mut Publication) {
    if pub_info.publication_type == PublicationType::Journal
        && pub_info
            .abbreviation
            .as_deref()
            .is_none_or(|a| a.trim().is_empty())
    {
        pub_info.abbreviation = Some(parser::abbreviate_journal_name(&pub_info.name));
    }
}

impl LiteratureService {
    /// 寻找重复文献
    pub fn find_duplicates(&self, db: &Database) -> Vec<Vec<Literature>> {
        let literatures = db
            .get_all_literatures()
            .unwrap_or_default()
            .into_iter()
            .filter(|l| !l.folder_ids.iter().any(|s| s == "trash"))
            .collect::<Vec<_>>();

        debug!("查重: 扫描 {} 篇非回收站文献", literatures.len());

        if literatures.len() < 2 {
            debug!("查重: 文献数量不足，跳过");
            return Vec::new();
        }

        let mut groups: Vec<HashSet<String>> = Vec::new();

        // 1. 按 DOI 分组
        let mut doi_map: collections::HashMap<String, Vec<String>> = collections::HashMap::new();
        for lit in &literatures {
            if let Some(doi) = &lit.doi {
                let normalized_doi = doi.trim().to_lowercase();
                if !normalized_doi.is_empty() {
                    doi_map
                        .entry(normalized_doi)
                        .or_default()
                        .push(lit.id.clone());
                }
            }
        }
        let doi_dup_count = doi_map.values().filter(|ids| ids.len() > 1).count();
        for ids in doi_map.values() {
            if ids.len() > 1 {
                groups.push(ids.iter().cloned().collect());
            }
        }
        debug!("查重: DOI 分组发现 {} 组重复", doi_dup_count);

        // 2. 按标题模糊匹配分组（忽略年份，不使用作者）
        let norm_titles: Vec<(String, String)> = literatures
            .iter()
            .map(|lit| (lit.id.clone(), normalize_title_for_match(&lit.title)))
            .filter(|(_, t)| !t.is_empty())
            .collect();
        let mut title_pair_count = 0usize;
        for (i, (id_a, title_a)) in norm_titles.iter().enumerate() {
            for (id_b, title_b) in &norm_titles[(i + 1)..] {
                if title_similarity(title_a, title_b) >= TITLE_SIMILARITY_THRESHOLD {
                    let mut pair = HashSet::new();
                    pair.insert(id_a.clone());
                    pair.insert(id_b.clone());
                    groups.push(pair);
                    title_pair_count += 1;
                }
            }
        }
        debug!("查重: 标题模糊匹配发现 {} 对相似", title_pair_count);

        // 合并相交的组 (Union-Find 思想)
        let mut merged_groups: Vec<HashSet<String>> = Vec::new();
        for group in groups {
            let mut found = false;
            for merged in &mut merged_groups {
                if !merged.is_disjoint(&group) {
                    merged.extend(group.iter().cloned());
                    found = true;
                    break;
                }
            }
            if !found {
                merged_groups.push(group);
            }
        }
        debug!("查重: 初步合并后 {} 组", merged_groups.len());

        // 再次检查是否有可进一步合并的组
        let mut final_groups: Vec<HashSet<String>> = Vec::new();
        for mut group in merged_groups {
            let mut i = 0;
            while i < final_groups.len() {
                if final_groups[i].is_disjoint(&group) {
                    i += 1;
                } else {
                    let other = final_groups.remove(i);
                    group.extend(other);
                }
            }
            final_groups.push(group);
        }
        debug!("查重: 最终合并后 {} 组", final_groups.len());

        info!("查重完成: 发现 {} 组可能重复的文献", final_groups.len());

        final_groups
            .into_iter()
            .map(|ids| {
                ids.into_iter()
                    .filter_map(|id| literatures.iter().find(|l| l.id == id).cloned())
                    .collect()
            })
            .collect()
    }

    pub fn save_literature(
        &self,
        db: Arc<Database>,
        _notify: Arc<dyn Fn() + Send + Sync>,
        mut lit: Literature,
    ) -> Result<()> {
        sanitize_arxiv_identifiers(&mut lit);
        if let Some(ref mut pub_info) = lit.publication {
            fill_publication_abbreviation(pub_info);
        }
        // 自动补充 CCF 分级信息 (如果缺失)
        let mut new_ccf_rank = None;
        let mut pub_name_for_update = None;

        if let Some(ref mut pub_info) = lit.publication
            && pub_info.ccf_rank.is_none()
            && let Some(rank) = CCFService::new().get_rank(&pub_info.name)
        {
            info!("CCF: 自动识别文献 '{}' 的分级: {}", pub_info.name, rank);
            pub_info.ccf_rank = Some(rank.clone());
            new_ccf_rank = Some(rank);
            pub_name_for_update = Some(pub_info.name.clone());
        }

        info!(
            "数据库管理: 正在保存文献: '{}' (ID: {}, 附件数: {})",
            lit.title,
            lit.id,
            lit.attachments.len()
        );

        let was_new = db.get_literature(&lit.id)?.is_none();
        debug!(
            "保存文献: '{}' 为{}记录",
            lit.title,
            if was_new { "新" } else { "已有" }
        );
        let old_pub_name = if !was_new {
            db.get_literature(&lit.id)?
                .and_then(|l| l.publication.map(|p| p.name))
        } else {
            None
        };

        db.insert_literature(&lit)
            .inspect_err(|e| error!("数据库管理: 保存文献到数据库失败: {e}"))?;

        // --- 批量更新同名期刊的 CCF 分级 ---
        if let (Some(rank), Some(pub_name)) = (new_ccf_rank, pub_name_for_update) {
            let all_lits = db.get_all_literatures()?;
            let mut updates = Vec::new();
            for mut other_lit in all_lits {
                if other_lit.id == lit.id {
                    continue;
                }
                if let Some(ref mut other_pub) = other_lit.publication
                    && other_pub.name == pub_name
                    && other_pub.ccf_rank.as_ref() != Some(&rank)
                {
                    other_pub.ccf_rank = Some(rank.clone());
                    updates.push(other_lit);
                }
            }
            if !updates.is_empty() {
                info!(
                    "CCF: 检测到同名期刊 '{pub_name}'，正在批量更新 {} 篇文献的 CCF 分级为 '{rank}'",
                    updates.len()
                );
                for updated_lit in updates {
                    if let Err(e) = db.insert_literature(&updated_lit) {
                        error!("CCF: 批量更新文献[{}]失败: {}", updated_lit.id, e);
                    }
                }
            }
        }

        // 重新从数据库加载以获取规范化后的作者 ID 等信息
        let reloaded_lit = db
            .get_literature(&lit.id)?
            .ok_or_else(|| anyhow::anyhow!("保存后无法重新获取文献记录 (ID: {})", lit.id))?;

        let name_changed = if !was_new {
            let new_name = reloaded_lit.publication.as_ref().map(|p| p.name.as_str());
            old_pub_name.as_deref() != new_name
        } else {
            false
        };
        if name_changed {
            info!("文献服务: 检测到刊名变化，将触发分级查询");
        }

        debug!("保存文献流程完成 (ID: {})", lit.id);
        Ok(())
    }

    /// 更新文献详细信息（包含智能重命名和云端清理逻辑）
    pub fn update_literature_details(
        &self,
        db: Arc<Database>,
        notify: Arc<dyn Fn() + Send + Sync>,
        filename_template: &str,
        queue_remote_rename: impl Fn(&str, &str),
        mut lit: Literature,
    ) -> Result<()> {
        sanitize_arxiv_identifiers(&mut lit);
        let lit_id = lit.id.clone();
        info!("更新文献[{lit_id}]: {}", lit.title);

        // --- 智能重命名逻辑 ---
        let template = filename_template.to_string();

        // 预先提取元数据
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
        let lit_title_clone = lit.title.clone();

        let att_count = lit.attachments.len();
        debug!("更新文献详情: {} 个附件待处理", att_count);
        for att in lit.attachments.iter_mut() {
            let path = Path::new(&att.file_path);
            if !path.exists() {
                continue;
            }

            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if extension.is_empty() {
                continue;
            }

            let options = FilenameOptions::new(
                &last_name,
                &first_name,
                &year_str,
                &lit_title_clone,
                &publication,
                extension,
                att.is_main,
            );
            let new_filename = generate_literature_filename(&options, Some(&template));

            if new_filename != att.file_name {
                let parent = path.parent().unwrap_or_else(|| Path::new("."));
                let new_path = parent.join(&new_filename);
                let old_filename = att.file_name.clone();

                info!("自动重命名: {old_filename} -> {new_filename}");

                if let Err(e) = fs::rename(path, &new_path) {
                    error!("重命名失败 {path:?} -> {new_path:?}: {e}");
                    continue;
                }

                queue_remote_rename(&att.id, &old_filename);

                att.file_name = new_filename;
                att.file_path = new_path.to_string_lossy().to_string();

                att.is_dirty = true;
            }
        }

        // 更新版本和时间
        lit.version += 1;
        lit.updated_at = chrono::Local::now().timestamp();

        self.save_literature(db, notify, lit)?;

        info!("文献详细信息更新流程完成 (ID: {lit_id})");
        Ok(())
    }

    pub fn update_literature_reading_status(
        &self,
        db: &Database,
        id: &str,
        status: models::ReadingStatus,
    ) -> Result<()> {
        info!("数据库管理: 正在更新文献阅读状态 (ID: {id}, status: {status:?})");
        db.update_reading_status(id, status)
            .inspect_err(|e| error!("数据库管理: 更新阅读状态失败: {e}"))?;
        Ok(())
    }

    pub fn delete_literature(
        &self,
        db: &Database,
        local_state_manager: &LocalStateManager,
        trash_file: impl Fn(&str) -> std::io::Result<()>,
        id: &str,
    ) -> Result<()> {
        info!("数据库管理: 准备删除文献 (ID: {id})");

        // 1. 获取文献附件并移入系统回收站
        if let Ok(Some(lit)) = db.get_literature(id) {
            info!(
                "数据库管理: 正在清理文献 '{}' 的 {} 个附件",
                lit.title,
                lit.attachments.len()
            );
            for att in &lit.attachments {
                if let Err(e) = trash_file(&att.file_path) {
                    warn!("文件系统: 移入回收站失败 [{}]: {e}", att.file_path);
                }
            }
        } else {
            warn!("数据库管理: 未找到待删除文献 (ID: {id})");
        }

        // 2. 清理关联的 AI 对话
        if let Err(e) = local_state_manager.delete_chat_sessions_for_literature(id) {
            warn!("本地状态管理: 清理文献的对话记录失败: {e}");
        }

        // 3. 删除数据库记录
        db.delete_literature(id)
            .inspect_err(|e| error!("数据库管理: 删除数据库记录失败: {e}"))?;
        info!("数据库管理: 数据库记录已删除 (ID: {id})");
        Ok(())
    }

    // --- Relationship Operations ---

    pub fn set_literature_authors(
        &self,
        db: &Database,
        literature_id: &str,
        authors: Vec<Author>,
    ) -> Result<()> {
        info!(
            "数据库管理: 正在更新文献作者关联 (文献ID: {}, 作者数: {})",
            literature_id,
            authors.len()
        );
        db.set_literature_authors(literature_id, &authors)
            .inspect_err(|e| error!("数据库管理: 更新文献作者关联失败: {e}"))?;
        Ok(())
    }

    pub fn add_literature_to_folder(
        &self,
        db: &Database,
        literature_id: &str,
        folder_id: &str,
    ) -> Result<()> {
        info!("数据库: 开始添加文献[{literature_id}]到文件夹[{folder_id}]");
        let mut folder_ids = db
            .get_folders_for_literature(literature_id)
            .unwrap_or_default();

        if folder_ids.iter().any(|s| s == folder_id) {
            info!("数据库: 文献[{literature_id}]已在文件夹[{folder_id}]中，无需重复添加");
        } else {
            folder_ids.push(folder_id.to_string());
            db.set_literature_folders(literature_id, &folder_ids)
                .inspect_err(|e| error!("数据库: 保存文件夹关系失败: {e}"))?;
            info!("数据库: 文件夹关系已保存到数据库");
        }
        Ok(())
    }

    pub fn remove_literature_from_folder(
        &self,
        db: &Database,
        literature_id: &str,
        folder_id: &str,
    ) -> Result<()> {
        info!("数据库: 开始从文件夹[{folder_id}]移除文献[{literature_id}]");
        let mut folder_ids = db
            .get_folders_for_literature(literature_id)
            .unwrap_or_default();

        if folder_ids.iter().any(|s| s == folder_id) {
            folder_ids.retain(|id| id != folder_id);
            db.set_literature_folders(literature_id, &folder_ids)
                .inspect_err(|e| error!("数据库: 移除文件夹关系失败: {e}"))?;
            info!("数据库: 文件夹关系已从数据库移除");
        } else {
            info!("数据库: 文献[{literature_id}]不在文件夹[{folder_id}]中，无需移除");
        }
        Ok(())
    }

    pub fn set_literature_folders(
        &self,
        db: &Database,
        literature_id: &str,
        folder_ids: Vec<String>,
    ) -> Result<()> {
        info!(
            "数据库管理: 正在更新文献文件夹关联 (文献ID: {}, 文件夹数: {})",
            literature_id,
            folder_ids.len()
        );
        db.set_literature_folders(literature_id, &folder_ids)
            .inspect_err(|e| error!("数据库管理: 保存文献文件夹关联失败: {e}"))?;
        Ok(())
    }

    pub fn set_literature_tags(
        &self,
        db: &Database,
        notify: impl Fn(),
        literature_id: &str,
        tags: Vec<String>,
    ) -> Result<()> {
        info!(
            "数据库管理: 正在更新文献标签关联 (文献ID: {}, 标签数: {})",
            literature_id,
            tags.len()
        );
        db.set_literature_tags(literature_id, &tags)
            .inspect_err(|e| error!("数据库管理: 保存文献标签关联失败: {e}"))?;
        notify();
        Ok(())
    }

    // ── 笔记 ────────────────────────────────────────────

    pub fn list_notes(&self, db: &Database, literature_id: &str) -> Vec<LiteratureNote> {
        db.list_notes(literature_id).unwrap_or_default()
    }

    pub fn create_note(&self, db: &Database, literature_id: &str, title: &str) -> Option<String> {
        db.create_note(literature_id, title).ok()
    }

    pub fn update_note(
        &self,
        db: &Database,
        note_id: &str,
        title: Option<&str>,
        content: Option<&str>,
    ) -> bool {
        db.update_note(note_id, title, content).unwrap_or(false)
    }

    pub fn delete_note(&self, db: &Database, note_id: &str) -> bool {
        db.delete_note(note_id).unwrap_or(false)
    }
}
