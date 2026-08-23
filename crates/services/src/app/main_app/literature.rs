use anyhow::Result;
use log::{debug, info, warn};
use models::constructors::*;
use models::{FolderType, Literature};
use uuid::Uuid;

use super::MainApp;

impl MainApp {
    pub fn add_literature(&self, lit: Literature) -> Result<()> {
        debug!(
            "MainApp: 添加文献 '{}' (id={})",
            lit.title.chars().take(40).collect::<String>(),
            lit.id
        );
        self.op_notify(|| {
            self.literature_service.save_literature(
                self.db.clone(),
                self.data_changed_notify(),
                lit,
            )
        })?;
        Ok(())
    }

    pub fn update_literature(&self, lit: Literature) -> Result<()> {
        debug!(
            "MainApp: 更新文献 '{}' (id={})",
            lit.title.chars().take(40).collect::<String>(),
            lit.id
        );
        self.op_notify(|| {
            let template = self.config.lock().unwrap().filename_template.clone();
            self.literature_service.update_literature_details(
                self.db.clone(),
                self.data_changed_notify(),
                &template,
                |id, old| self.sync_service.queue_remote_rename(id, old),
                lit,
            )
        })
    }

    /// 内部删除实现，不触发 notify（供批量方法复用）
    fn delete_literature_inner(&self, id: &str) -> Result<()> {
        let in_trash = self
            .db
            .get_literature(id)?
            .is_some_and(|lit| lit.folder_ids.contains(&"trash".to_string()));
        if in_trash {
            info!("MainApp: 物理删除文献 (id={id})");
            self.literature_service.delete_literature(
                &self.db,
                &self.local_state_manager,
                |p| self.file_manager.trash_file(p),
                id,
            )?;
        } else {
            info!("MainApp: 移动文献到回收站 (id={id})");
            self.literature_service.set_literature_folders(
                &self.db,
                id,
                vec!["trash".to_string()],
            )?;
        }
        Ok(())
    }

    /// 批量删除，只发一次 notify_data_changed
    pub fn batch_delete_literatures(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        info!("MainApp: 批量删除 {} 篇文献", ids.len());
        for id in ids {
            self.delete_literature_inner(id)?;
        }
        self.notify_data_changed();
        Ok(())
    }

    pub fn delete_literature_by_id(&self, id: &str) -> Result<()> {
        info!("MainApp: 删除单篇文献 (id={id})");
        self.op_notify(|| self.delete_literature_inner(id))
    }

    pub fn empty_trash(&self) -> Result<()> {
        let ids: Vec<String> = self
            .db
            .get_literatures_by_folder("trash")?
            .iter()
            .map(|l| l.id.clone())
            .collect();
        if ids.is_empty() {
            debug!("MainApp: 清空回收站，但回收站为空");
            return Ok(());
        }
        info!("MainApp: 清空回收站，共 {} 篇文献", ids.len());
        self.batch_delete_literatures(&ids)
    }

    /// 清理已同步的软删除数据（墓碑），并删除附件物理文件
    pub fn purge_synced_deletions(&self) -> Result<usize> {
        self.op_notify(|| {
            let mut total = 0;

            // 附件需先取出文件路径再清理记录
            let attachment_paths = self.db.purge_synced_attachments()?;
            for path in &attachment_paths {
                if let Err(e) = self.file_manager.trash_file(path) {
                    warn!("MainApp: 删除附件物理文件失败 '{path}': {e}");
                }
            }
            total += attachment_paths.len();

            total += self.db.purge_synced_deletions()?;
            total += self.db.purge_synced_folders()?;
            total += self.db.purge_synced_tags()?;
            total += self.db.purge_synced_feeds()?;
            total += self.db.purge_synced_feed_items()?;
            total += self.db.purge_synced_annotations()?;
            total += self.db.purge_synced_authors()?;
            total += self.db.purge_synced_publications()?;
            total += self.db.purge_synced_citations()?;

            info!("MainApp: 清理已同步的删除数据，共 {total} 条");
            Ok(total)
        })
    }

    pub fn add_folder(&self, parent_id: Option<String>, new_id: Option<String>) -> Result<()> {
        info!("MainApp: 添加文件夹 (parent={parent_id:?})");
        let folder = create_folder(
            new_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
            "",
            FolderType::Custom,
        );
        let mut folder = folder;
        folder.parent_id = parent_id;
        self.op_notify(|| self.folder_service.save_folder(&self.db, folder))
    }

    pub fn delete_folder(&self, id: &str) -> Result<()> {
        info!("MainApp: 删除文件夹 (id={id})");
        self.op_notify(|| {
            self.folder_service
                .delete_folder(&self.db, || self.notify_data_changed(), id, true)
        })
    }
    pub fn rename_folder(&self, id: &str, name: String) -> Result<()> {
        debug!("MainApp: 重命名文件夹 (id={id}) -> '{name}'");
        self.op_notify(|| self.folder_service.update_folder_name(&self.db, id, name))
    }
    pub fn move_folder(&self, id: &str, parent_id: Option<String>) -> Result<()> {
        info!("MainApp: 移动文件夹 (id={id}) -> parent={parent_id:?}");
        self.op_notify(|| self.folder_service.move_folder(&self.db, id, parent_id))
    }
    pub fn add_literature_to_folder(&self, lit_id: &str, f_id: &str) -> Result<()> {
        debug!("MainApp: 添加文献到文件夹 lit={lit_id}, folder={f_id}");
        self.op_notify(|| {
            self.literature_service
                .add_literature_to_folder(&self.db, lit_id, f_id)
        })
    }

    pub fn remove_literature_from_folder(&self, lit_id: &str, f_id: &str) -> Result<()> {
        debug!("MainApp: 从文件夹移除文献 lit={lit_id}, folder={f_id}");
        self.op_notify(|| {
            self.literature_service
                .remove_literature_from_folder(&self.db, lit_id, f_id)
        })
    }
    pub fn restore_literature(&self, lit_id: &str, target: Option<&str>) -> Result<()> {
        debug!("MainApp: 恢复文献 lit={lit_id}, target={target:?}");
        self.op_notify(|| {
            self.literature_service
                .remove_literature_from_folder(&self.db, lit_id, "trash")?;
            if let Some(f) = target {
                self.literature_service
                    .add_literature_to_folder(&self.db, lit_id, f)?;
            }
            Ok(())
        })
    }

    /// 批量重命名所有文献的主文件
    pub fn batch_rename_files(&self) -> Result<()> {
        warn!("MainApp: batch_rename_files 尚未实现");
        Ok(())
    }

    /// 删除指定的文献集合
    pub fn delete_selected_literatures(&self, ids: Vec<String>) -> Result<()> {
        debug!("MainApp: 删除选中文献集合 ({} 篇)", ids.len());
        self.batch_delete_literatures(&ids)
    }
}
