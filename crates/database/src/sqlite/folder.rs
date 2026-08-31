use super::Database;
use log::{debug, info, warn};
use models::{Folder, FolderType};
use rusqlite::{Connection, Result, Row, params};
use serde_json;

impl Database {
    pub fn insert_folder(&self, folder: &Folder) -> Result<()> {
        info!(
            "数据库: 准备写入文件夹 '{}' (ID: {})",
            folder.name, folder.id
        );
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO folders (id, name, folder_type, parent_id, is_dirty, is_deleted, version, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    folder.id,
                    folder.name,
                    serde_json::to_string(&folder.folder_type).unwrap_or_default().trim_matches('"'),
                    folder.parent_id,
                    folder.is_dirty,
                    folder.is_deleted,
                    folder.version,
                    folder.created_at,
                    folder.updated_at,
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_folder_name(&self, id: &str, name: &str) -> Result<()> {
        info!("数据库: 正在重命名文件夹 (ID: {id}) 为 '{name}'");
        self.with_conn(|conn| {
            let rows = conn.execute(
                "UPDATE folders SET name = ?1, updated_at = ?2, is_dirty = 1, version = version + 1 WHERE id = ?3",
                params![name, chrono::Local::now().timestamp(), id],
            )?;
            if rows == 0 {
                warn!("数据库: 重命名文件夹失败，未找到 ID 为 {id} 的记录");
            }
            Ok(())
        })
    }

    pub fn move_folder(&self, id: &str, parent_id: Option<String>) -> Result<()> {
        debug!("数据库: 正在移动文件夹 (ID: {id}) 到父文件夹 {parent_id:?}");
        self.with_conn(|conn| {
            let rows = conn.execute(
                "UPDATE folders SET parent_id = ?1, updated_at = ?2, is_dirty = 1, version = version + 1 WHERE id = ?3",
                params![parent_id, chrono::Local::now().timestamp(), id],
            )?;
            if rows == 0 {
                warn!("数据库: 移动文件夹失败，未找到 ID 为 {id} 的记录");
            }
            Ok(())
        })
    }

    pub fn get_all_folders(&self) -> Result<Vec<Folder>> {
        debug!("数据库: 正在获取所有文件夹列表");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT id, name, folder_type, parent_id, is_dirty, is_deleted, version, created_at, updated_at FROM folders WHERE is_deleted = 0")?;
            let folder_iter = stmt.query_map([], |row| {
                let folder_type_str: String = row.get(2)?;
                let folder_type: FolderType = serde_json::from_str(&format!("\"{folder_type_str}\"")).unwrap_or(FolderType::Custom);

                Ok(Folder {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    folder_type,
                    parent_id: row.get(3)?,
                    literature_count: 0, // 之后由 DataManager 计算
                    is_dirty: row.get(4)?,
                    is_deleted: row.get(5)?,
                    version: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })?;

            let mut folders = Vec::new();
            for folder in folder_iter {
                folders.push(folder?);
            }
            debug!("数据库: 成功获取 {} 个文件夹", folders.len());
            Ok(folders)
        })
    }

    /// 删除文件夹及其所有子文件夹 (递归删除逻辑)
    /// 关联文献不会被删除，但 literature_folders 关联会被软删除，
    /// 使文献自动出现在「未分类」视图中
    pub fn delete_folder_recursive(&self, id: &str) -> Result<()> {
        info!("数据库: 准备递归删除文件夹 (ID: {id})");
        self.with_transaction(|tx| {
            // 1. 查找所有子文件夹
            let mut stmt =
                tx.prepare("SELECT id FROM folders WHERE parent_id = ?1 AND is_deleted = 0")?;
            let child_ids: Vec<String> = stmt
                .query_map([id], |row| row.get(0))?
                .collect::<Result<Vec<String>>>()?;

            if !child_ids.is_empty() {
                debug!(
                    "数据库: 文件夹 {} 包含 {} 个子文件夹，将递归删除",
                    id,
                    child_ids.len()
                );
            }

            // 2. 递归删除子文件夹
            for child_id in child_ids {
                Self::delete_folder_raw(tx, &child_id)?;
            }

            // 3. 删除当前文件夹
            Self::delete_folder_raw(tx, id)?;

            info!("数据库: 文件夹 (ID: {id}) 及其子文件夹已成功删除");
            Ok(())
        })
    }

    pub fn delete_folder(&self, id: &str) -> Result<()> {
        info!("数据库: 正在删除文件夹 (ID: {id})");
        self.with_conn(|conn| Self::delete_folder_raw(conn, id))
    }

    // --- 同步支持方法 ---

    pub fn get_dirty_folders(&self) -> Result<Vec<Folder>> {
        debug!("数据库: 正在获取待同步文件夹记录");
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, folder_type, parent_id, is_dirty, is_deleted, version, created_at, updated_at FROM folders WHERE is_dirty = 1"
            )?;
            let iter = stmt.query_map([], |row| self.map_folder_row(row))?;
            let folders = iter.collect::<Result<Vec<_>>>()?;
            debug!("数据库: 获取到 {} 个待同步文件夹", folders.len());
            Ok(folders)
        })
    }

    pub fn mark_folder_synced(&self, id: &str) -> Result<()> {
        debug!("数据库: 标记文件夹为已同步 (ID: {id})");
        self.with_conn(|conn| {
            let rows = conn.execute("UPDATE folders SET is_dirty = 0 WHERE id = ?1", [id])?;
            if rows == 0 {
                warn!("数据库: 标记文件夹同步失败，未找到 ID 为 {id} 的记录");
            }
            Ok(())
        })
    }

    /// 原子原语：把远程文件夹盲目 upsert 到本地（覆盖写或插入）。
    pub fn apply_remote_folder(&self, remote: &Folder) -> Result<()> {
        self.with_conn(|conn| self.insert_folder_internal(conn, remote))
    }

    /// 原子原语：版本一致且本地无修改时，仅刷新时间戳并清脏标记。
    pub fn mark_folder_up_to_date(&self, remote: &Folder) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE folders SET updated_at = ?1, is_dirty = 0 WHERE id = ?2",
                params![remote.updated_at, remote.id],
            )?;
            Ok(())
        })
    }

    fn insert_folder_internal(&self, conn: &Connection, folder: &Folder) -> Result<()> {
        conn.execute(
            "INSERT OR REPLACE INTO folders (id, name, folder_type, parent_id, is_dirty, is_deleted, version, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                folder.id,
                folder.name,
                serde_json::to_string(&folder.folder_type).unwrap_or_default().trim_matches('"'),
                folder.parent_id,
                0,
                folder.is_deleted,
                folder.version,
                folder.created_at,
                folder.updated_at,
            ],
        )?;
        Ok(())
    }

    // --- 内部辅助函数 ---

    fn map_folder_row(&self, row: &Row) -> Result<Folder> {
        let id: String = row.get(0)?;
        let name: String = row.get(1)?;
        let type_str: String = row.get(2)?;
        let folder_type: FolderType =
            serde_json::from_str(&format!("\"{type_str}\"")).unwrap_or(FolderType::Custom);

        Ok(Folder {
            id,
            name,
            folder_type,
            parent_id: row.get(3)?,
            literature_count: 0,
            is_dirty: row.get(4)?,
            is_deleted: row.get(5)?,
            version: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }

    fn delete_folder_raw(conn: &Connection, id: &str) -> Result<()> {
        let now = chrono::Local::now().timestamp();
        debug!("数据库: 执行软删除文件夹记录 (ID: {id})");
        conn.execute(
            "UPDATE folders SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE id = ?2",
            params![now, id]
        )?;
        // 软删除该文件夹下所有文献的归属关联，使文献回到「未分类」
        conn.execute(
            "UPDATE literature_folders SET is_deleted = 1, is_dirty = 1, version = version + 1, updated_at = ?1 WHERE folder_id = ?2",
            params![now, id]
        )?;
        Ok(())
    }

    pub fn purge_synced_folders(&self) -> Result<usize> {
        info!("数据库: 正在清理已同步的删除文件夹记录");
        self.with_conn(|conn| {
            let count = conn.execute(
                "DELETE FROM folders WHERE is_deleted = 1 AND is_dirty = 0",
                [],
            )?;
            info!("数据库: 已清理 {count} 个已同步删除的文件夹");
            Ok(count)
        })
    }
}
