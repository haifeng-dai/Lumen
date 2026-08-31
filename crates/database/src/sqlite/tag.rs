use super::Database;
use chrono::Local;
use log::{debug, info, warn};
use models::Tag;
use models::constructors::*;
use rusqlite::{Connection, OptionalExtension, Result, params};

impl Database {
    // --- Internal conn-based helpers ---

    /// 在事务内根据名称查找有效标签 ID
    pub fn find_tag_id_by_name(conn: &Connection, name: &str) -> Result<Option<String>> {
        conn.query_row(
            "SELECT id FROM tags WHERE name = ?1 AND is_deleted = 0",
            [name],
            |row| row.get(0),
        )
        .optional()
    }

    /// 在事务内插入标签（用于 set_tags 等内部上下文）
    pub fn upsert_tag(
        conn: &Connection,
        id: &str,
        name: &str,
        color: &str,
        now: i64,
    ) -> Result<()> {
        conn.execute(
            "INSERT INTO tags (id, name, color, is_dirty, is_deleted, version, created_at, updated_at) VALUES (?1, ?2, ?3, 1, 0, 1, ?4, ?4)",
            params![id, name, color, now],
        )?;
        Ok(())
    }

    // --- Basic CRUD ---

    /// 创建或获取标签
    pub fn create_tag(&self, name: &str, color: Option<String>) -> Result<Tag> {
        info!("数据库: 准备创建或获取标签 '{name}'");
        let now = Local::now().timestamp();
        self.with_conn(|conn| {
            // 1. 检查是否存在（包括已软删除的）
            // 优先查找未删除的
            let existing: Option<(String, bool)> = conn
                .query_row(
                    "SELECT id, is_deleted FROM tags WHERE name = ?1 ORDER BY is_deleted ASC LIMIT 1",
                    [name],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;

            if let Some((id, is_deleted)) = existing {
                if is_deleted {
                    info!("数据库: 发现已删除的同名标签 (ID: {id}), 正在恢复");
                    // 复活标签
                    conn.execute(
                        "UPDATE tags SET is_deleted = 0, version = version + 1, updated_at = ?1, is_dirty = 1 WHERE id = ?2",
                        params![now, id],
                    )?;
                }
                // 如果提供了颜色且不一致，则更新颜色
                if let Some(c) = color {
                    let updated = conn.execute(
                        "UPDATE tags SET color = ?1, version = version + 1, updated_at = ?2, is_dirty = 1 WHERE id = ?3 AND (color IS NULL OR color != ?1)",
                        params![c, now, id],
                    )?;
                    if updated > 0 {
                        debug!("数据库: 更新标签 '{name}' (ID: {id}) 的颜色为 {c}");
                    }
                }

                return Self::get_tag_by_id_conn(conn, &id);
            }

            // 2. 创建新标签
            let new_tag = if let Some(c) = color {
                create_tag_with_color(name, c)
            } else {
                create_tag(name)
            };

            info!("数据库: 创建新标签 '{}' (ID: {})", name, new_tag.id);
            conn.execute(
                "INSERT INTO tags (id, name, color, is_dirty, is_deleted, version, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    new_tag.id,
                    new_tag.name,
                    new_tag.color,
                    new_tag.is_dirty,
                    new_tag.is_deleted,
                    new_tag.version,
                    new_tag.created_at,
                    new_tag.updated_at
                ],
            )?;

            Ok(new_tag)
        })
    }

    /// 获取标签详情 (Internal Helper)
    fn get_tag_by_id_conn(conn: &Connection, id: &str) -> Result<Tag> {
        conn.query_row(
            "SELECT id, name, color, created_at, updated_at, version, is_deleted FROM tags WHERE id = ?1",
            [id],
            |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    version: row.get(5)?,
                    is_deleted: row.get(6)?,
                    is_dirty: false,
                })
            },
        )
    }

    /// 获取所有标签及其使用频次 (Dashboard)
    pub fn get_all_tags_with_counts(&self) -> Result<Vec<(Tag, usize)>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT t.id, t.name, t.color, t.created_at, t.updated_at, t.version, t.is_deleted,
                        COUNT(lt.literature_id) as count
                 FROM tags t
                 LEFT JOIN literature_tags lt ON t.id = lt.tag_id AND lt.is_deleted = 0
                 WHERE t.is_deleted = 0
                   AND (lt.literature_id IS NULL OR lt.literature_id NOT IN (
                       SELECT literature_id FROM literature_folders WHERE folder_id = 'trash' AND is_deleted = 0
                   ))
                 GROUP BY t.id
                 ORDER BY count DESC, t.name ASC",
            )?;
            let tag_iter = stmt.query_map([], |row| {
                let tag = Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    version: row.get(5)?,
                    is_deleted: row.get(6)?,
                    is_dirty: false,
                };
                let count: i64 = row.get(7)?;
                Ok((tag, count as usize))
            })?;

            let mut tags = Vec::new();
            for tag in tag_iter {
                tags.push(tag?);
            }
            Ok(tags)
        })
    }

    /// 更新标签颜色
    pub fn update_tag_color(&self, id: &str, color: &str) -> Result<()> {
        debug!("数据库: 正在更新标签颜色 (ID: {id}, Color: {color})");
        let now = Local::now().timestamp();
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE tags SET color = ?1, version = version + 1, updated_at = ?2, is_dirty = 1 WHERE id = ?3",
                params![color, now, id],
            )?;
            Ok(())
        })
    }

    /// 重命名标签 (逻辑升级：支持同步与合并检测)
    pub fn update_tag_name(&self, id: &str, new_name: &str) -> Result<()> {
        info!("数据库: 准备重命名标签 (ID: {id}) 为 '{new_name}'");
        let now = Local::now().timestamp();

        self.with_transaction(|tx| {
            // 1. 检查名称冲突
            let conflict_id: Option<String> = tx
                .query_row(
                    "SELECT id FROM tags WHERE name = ?1 AND is_deleted = 0 AND id != ?2",
                    [new_name, id],
                    |row| row.get(0),
                )
                .optional()?;

            if let Some(target_id) = conflict_id {
                info!("数据库: 重命名冲突，发现已存在名称为 '{new_name}' 的标签 (ID: {target_id}), 将触发合并");
                // 如果名称已存在，触发合并逻辑
                return self.merge_tags_internal(tx, id, &target_id);
            }

            // 2. 直接更新
            tx.execute(
                "UPDATE tags SET name = ?1, version = version + 1, updated_at = ?2, is_dirty = 1 WHERE id = ?3",
                params![new_name, now, id],
            )?;
            debug!("数据库: 标签 (ID: {id}) 重命名成功");
            Ok(())
        })
    }

    /// 删除标签 (软删除)
    pub fn delete_tag(&self, id: &str) -> Result<()> {
        info!("数据库: 准备删除标签 (ID: {id})");
        let now = Local::now().timestamp();
        self.with_conn(|conn| {
            // 1. 软删除标签本身
            let rows = conn.execute(
                "UPDATE tags SET is_deleted = 1, version = version + 1, updated_at = ?1, is_dirty = 1 WHERE id = ?2",
                params![now, id],
            )?;

            if rows > 0 {
                // 2. 软删除关联关系
                let rel_rows = conn.execute(
                    "UPDATE literature_tags SET is_deleted = 1, version = version + 1, updated_at = ?1, is_dirty = 1 WHERE tag_id = ?2",
                    params![now, id],
                )?;
                info!("数据库: 标签 (ID: {id}) 及其 {rel_rows} 个关联关系已标记为删除");
            } else {
                warn!("数据库: 删除标签失败，未找到 ID 为 {id} 的记录");
            }

            Ok(())
        })
    }

    // --- Internal Helpers ---

    /// 内部合并逻辑 (Transactional Context)
    fn merge_tags_internal(
        &self,
        conn: &Connection,
        source_id: &str,
        target_id: &str,
    ) -> Result<()> {
        debug!("数据库: 正在执行标签合并逻辑 ({source_id} -> {target_id})");
        let now = Local::now().timestamp();

        // 1. 迁移关联关系
        let mut stmt = conn.prepare(
            "SELECT literature_id FROM literature_tags WHERE tag_id = ?1 AND is_deleted = 0",
        )?;
        let lit_ids: Vec<String> = stmt
            .query_map([source_id], |row| row.get(0))?
            .collect::<Result<Vec<String>>>()?;

        let mut moved_count = 0;
        for lit_id in lit_ids {
            // 检查目标标签是否已经关联该文献
            let already_has_target: bool = conn
                .query_row(
                    "SELECT 1 FROM literature_tags WHERE literature_id = ?1 AND tag_id = ?2 AND is_deleted = 0",
                    params![lit_id, target_id],
                    |_| Ok(true),
                )
                .optional()?
                .unwrap_or(false);

            if already_has_target {
                // 如果目标已存在，则只需删除源关联 (软删除)
                conn.execute(
                    "UPDATE literature_tags SET is_deleted = 1, version = version + 1, updated_at = ?1, is_dirty = 1 WHERE literature_id = ?2 AND tag_id = ?3",
                    params![now, lit_id, source_id],
                )?;
            } else {
                // 如果目标不存在，则将源关联"移动"到目标 (通过创建新关联并删除旧关联)
                // Step A: 删除旧关联
                conn.execute(
                    "UPDATE literature_tags SET is_deleted = 1, version = version + 1, updated_at = ?1, is_dirty = 1 WHERE literature_id = ?2 AND tag_id = ?3",
                    params![now, lit_id, source_id],
                )?;

                // Step B: 创建新关联 (或复活已删除的目标关联)
                let target_relation_exists: Option<bool> = conn.query_row(
                    "SELECT is_deleted FROM literature_tags WHERE literature_id = ?1 AND tag_id = ?2",
                    params![lit_id, target_id],
                    |row| row.get(0)
                ).optional()?;

                match target_relation_exists {
                    Some(_) => {
                        // 关联记录已存在 (可能是删除状态)，更新复活
                        conn.execute(
                            "UPDATE literature_tags SET is_deleted = 0, version = version + 1, updated_at = ?1, is_dirty = 1 WHERE literature_id = ?2 AND tag_id = ?3",
                            params![now, lit_id, target_id],
                        )?;
                    }
                    None => {
                        // 关联记录不存在，插入
                        conn.execute(
                            "INSERT INTO literature_tags (literature_id, tag_id, is_dirty, is_deleted, version, updated_at) VALUES (?1, ?2, 1, 0, 1, ?3)",
                            params![lit_id, target_id, now],
                        )?;
                    }
                }
                moved_count += 1;
            }
        }

        // 2. 软删除源标签
        conn.execute(
            "UPDATE tags SET is_deleted = 1, version = version + 1, updated_at = ?1, is_dirty = 1 WHERE id = ?2",
            params![now, source_id],
        )?;

        info!(
            "数据库: 标签合并完成 (Source: {source_id} -> Target: {target_id}), 迁移了 {moved_count} 个关联关系"
        );
        Ok(())
    }

    // --- Sync Support ---

    pub fn get_dirty_tags(&self) -> Result<Vec<Tag>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, color, created_at, updated_at, version, is_deleted
                 FROM tags WHERE is_dirty = 1",
            )?;
            let iter = stmt.query_map([], |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    version: row.get(5)?,
                    is_deleted: row.get(6)?,
                    is_dirty: true,
                })
            })?;

            let mut tags = Vec::new();
            for t in iter {
                tags.push(t?);
            }
            Ok(tags)
        })
    }

    /// 原子原语：把远程标签盲目 upsert 到本地（覆盖写）。
    pub fn apply_remote_tag(&self, remote: &Tag) -> Result<()> {
        self.with_conn(|conn| self.insert_tag_internal(conn, remote))
    }

    /// 原子原语：版本一致本地无修改时，仅刷新时间戳并清脏标记。
    pub fn mark_tag_up_to_date(&self, remote: &Tag) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE tags SET updated_at = ?1, is_dirty = 0 WHERE id = ?2",
                params![remote.updated_at, remote.id],
            )?;
            Ok(())
        })
    }

    fn insert_tag_internal(&self, conn: &Connection, tag: &Tag) -> Result<()> {
        conn.execute(
            "INSERT INTO tags (id, name, color, is_dirty, is_deleted, version, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
             name = excluded.name,
             color = excluded.color,
             is_dirty = 0,
             is_deleted = excluded.is_deleted,
             version = excluded.version,
             updated_at = excluded.updated_at",
            params![
                tag.id,
                tag.name,
                tag.color,
                tag.is_deleted,
                tag.version,
                tag.created_at,
                tag.updated_at
            ],
        )?;
        Ok(())
    }

    /// 清理已同步的软删除标签
    pub fn purge_synced_tags(&self) -> Result<usize> {
        info!("数据库: 正在清理已同步的软删除标签记录");
        self.with_conn(|conn| {
            let mut total = 0;
            // 先清理关联表
            total += conn.execute(
                "DELETE FROM literature_tags WHERE is_deleted = 1 AND is_dirty = 0",
                [],
            )?;
            // 再清理主表
            total += conn.execute("DELETE FROM tags WHERE is_deleted = 1 AND is_dirty = 0", [])?;
            info!("数据库: 标签清理完成，共物理删除 {total} 条记录");
            Ok(total)
        })
    }
}
