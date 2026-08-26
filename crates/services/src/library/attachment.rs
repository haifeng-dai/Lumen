/// 数据库操作单例管理器
///
/// 负责协调持久化存储与内存数据的同步
use anyhow::Result;
use database::Database;
use log::{debug, info, warn};
use models::Attachment;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub struct AttachmentService;

impl AttachmentService {
    #[must_use]
    pub fn new() -> Self {
        debug!("附件服务: 初始化");
        Self
    }

    /// 获取某篇文献的全部附件
    pub fn literature_attachments(&self, db: &Database, literature_id: &str) -> Vec<Attachment> {
        db.get_literature(literature_id)
            .ok()
            .flatten()
            .map(|l| l.attachments)
            .unwrap_or_default()
    }

    /// 校对所有未删除文献的有效附件是否仍存在于本地文件系统。
    pub fn check_local_files(&self, db: &Database) -> Result<(usize, Vec<String>)> {
        let literatures = db.get_all_literatures()?;
        let mut checked = 0;
        let mut missing = Vec::new();

        for literature in literatures
            .iter()
            .filter(|literature| !literature.is_deleted)
        {
            for attachment in literature
                .attachments
                .iter()
                .filter(|attachment| !attachment.is_deleted)
            {
                checked += 1;
                let path = Path::new(&attachment.file_path);
                if !path.is_file() {
                    warn!(
                        "附件校对: 未找到文献附件 '{}' ({})",
                        attachment.file_name, attachment.file_path
                    );
                    missing.push(attachment.file_path.clone());
                }
            }
        }

        info!(
            "附件校对完成: 检查 {} 个有效附件，缺失 {} 个",
            checked,
            missing.len()
        );
        Ok((checked, missing))
    }

    /// 清理孤立附件（未被数据库引用的物理文件）
    pub fn cleanup_orphaned_files(
        &self,
        db: &Database,
        attachments_dir: &Path,
        trash_file: impl Fn(&str) -> std::io::Result<()>,
    ) -> Result<()> {
        info!("开始清理孤立附件...");
        if !attachments_dir.exists() {
            debug!("附件目录不存在，跳过清理: {}", attachments_dir.display());
            return Ok(());
        }
        debug!("扫描附件目录: {}", attachments_dir.display());

        // 1. 收集数据库中引用的所有文件路径
        let referenced_paths = {
            let mut paths = HashSet::new();
            if let Ok(lits) = db.get_all_literatures() {
                for lit in &lits {
                    for att in &lit.attachments {
                        paths.insert(att.file_path.clone());
                    }
                }
            }
            paths
        };

        // 2. 递归扫描目录，找出不在引用列表中的文件
        let mut orphaned_count = 0;
        self.scan_and_cleanup_orphaned(
            &trash_file,
            attachments_dir,
            &referenced_paths,
            &mut orphaned_count,
        )?;

        info!("清理完成，共发现并移至回收站 {orphaned_count} 个孤立文件");
        Ok(())
    }

    fn scan_and_cleanup_orphaned(
        &self,
        trash_file: &impl Fn(&str) -> std::io::Result<()>,
        dir: &Path,
        referenced_paths: &HashSet<String>,
        count: &mut usize,
    ) -> Result<()> {
        let entries = fs::read_dir(dir)?;
        debug!("扫描目录: {}", dir.display());
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                // 递归处理子目录
                self.scan_and_cleanup_orphaned(trash_file, &path, referenced_paths, count)?;

                // 如果子目录变空了，可以考虑删除它
                if fs::read_dir(&path)?.next().is_none() {
                    let _ = fs::remove_dir(&path);
                }
            } else {
                let path_str = path.to_string_lossy().to_string();
                if !referenced_paths.contains(&path_str) {
                    debug!("发现孤立文件，移至回收站: {path_str}");
                    if let Err(e) = trash_file(&path_str) {
                        warn!("文件系统: 移入回收站失败 [{}]: {e}", path_str);
                    }
                    *count += 1;
                }
            }
        }
        Ok(())
    }
}

impl Default for AttachmentService {
    fn default() -> Self {
        Self::new()
    }
}
