use anyhow::Result;

use super::MainApp;
use std::collections::HashSet;

impl MainApp {
    /// 根据 target_id 是否属于选中的 ID 集合，决定批量操作的目标集合。
    pub fn resolve_smart_targets(target_id: &str, selected_ids: &HashSet<String>) -> Vec<String> {
        if selected_ids.contains(target_id) {
            selected_ids.iter().cloned().collect()
        } else {
            vec![target_id.to_string()]
        }
    }

    pub fn smart_delete_literature(&self, id: &str, selected_ids: &HashSet<String>) -> Result<()> {
        let targets = Self::resolve_smart_targets(id, selected_ids);
        self.batch_delete_literatures(&targets)
    }
    pub fn smart_add_literatures_to_folder(
        &self,
        id: &str,
        f: &str,
        selected_ids: &HashSet<String>,
    ) -> Result<()> {
        self.op_notify(|| {
            for aid in Self::resolve_smart_targets(id, selected_ids) {
                self.literature_service
                    .add_literature_to_folder(&self.db, &aid, f)?;
            }
            Ok(())
        })
    }
    pub fn smart_remove_literatures_from_folder(
        &self,
        id: &str,
        f: &str,
        selected_ids: &HashSet<String>,
    ) -> Result<()> {
        self.op_notify(|| {
            for aid in Self::resolve_smart_targets(id, selected_ids) {
                self.literature_service
                    .remove_literature_from_folder(&self.db, &aid, f)?;
            }
            Ok(())
        })
    }
    pub fn smart_restore_literatures(
        &self,
        id: &str,
        f: Option<&str>,
        selected_ids: &HashSet<String>,
    ) -> Result<()> {
        for aid in Self::resolve_smart_targets(id, selected_ids) {
            self.restore_literature(&aid, f)?;
        }
        Ok(())
    }
    pub fn smart_toggle_feed_items_read(
        &self,
        id: &str,
        read: bool,
        selected_ids: &HashSet<String>,
    ) -> Result<()> {
        self.op_notify(|| {
            for aid in Self::resolve_smart_targets(id, selected_ids) {
                self.feed_service
                    .update_feed_item_read_status(&self.db, &aid, read)?;
            }
            Ok(())
        })
    }
}
