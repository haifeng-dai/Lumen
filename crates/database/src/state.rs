use anyhow::Result;
use log::{debug, error, info};
use models::chat::{ChatMessage, ChatSession};
use models::local_state::{AppUiState, WindowState};
use rusqlite::{Connection, params};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use uuid::Uuid;

/// 本地状态数据库：负责 `state.db`（UI 状态 / PDF 阅读进度 / AI 对话）。
/// 该结构只提供 SQLite 持久化原语，业务编排仍由 services 调用。
pub struct LocalStateManager {
    db_path: PathBuf,
}

impl LocalStateManager {
    #[must_use]
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            db_path: config_dir.join("state.db"),
        }
    }

    /// 初始化数据库表 (如果不存在)
    pub fn init(&self) -> Result<()> {
        info!("本地状态管理: 初始化状态数据库 (路径: {:?})", self.db_path);
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&self.db_path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS ui_state (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS pdf_state (
                id TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                page_index INTEGER NOT NULL,
                zoom_level REAL NOT NULL,
                offset_y REAL NOT NULL DEFAULT 0.0,
                fit_to_width INTEGER NOT NULL DEFAULT 0,
                is_left_sidebar_open INTEGER NOT NULL DEFAULT 0,
                is_right_sidebar_open INTEGER NOT NULL DEFAULT 0,
                left_sidebar_width REAL NOT NULL DEFAULT 240.0,
                right_sidebar_width REAL NOT NULL DEFAULT 300.0,
                last_read_at INTEGER NOT NULL,
                auto_translate INTEGER NOT NULL DEFAULT 1
            )",
            [],
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS chat_sessions (
                id TEXT PRIMARY KEY,
                literature_id TEXT NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                system_prompt TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                compressed_summary TEXT NOT NULL DEFAULT '',
                active_message_id TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_chat_sessions_lit_id
                ON chat_sessions(literature_id, updated_at DESC);
            CREATE TABLE IF NOT EXISTS chat_messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL DEFAULT '',
                attachments TEXT NOT NULL DEFAULT '[]',
                created_at INTEGER NOT NULL,
                reasoning TEXT,
                parent_id TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_chat_messages_session
                ON chat_messages(session_id, created_at ASC);",
        )?;

        Ok(())
    }

    /// 从数据库加载所有状态到内存
    pub fn load_all(&self) -> Result<AppUiState> {
        debug!("本地状态管理: 正在从数据库加载所有 UI 状态");
        let conn = Connection::open(&self.db_path)?;
        let mut stmt = conn.prepare("SELECT key, value FROM ui_state")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut state = AppUiState::default();
        let mut window_state = WindowState::default();

        for row in rows {
            let (key, value) = row?;
            match key.as_str() {
                "expanded_folder_ids" => {
                    if let Ok(ids) = serde_json::from_str::<HashSet<String>>(&value) {
                        state.expanded_folder_ids = ids;
                    }
                }
                "selected_sidebar_item" => {
                    state.selected_sidebar_item = Some(value);
                }
                "sort_field" => {
                    state.sort_field = Some(value);
                }
                "sort_asc" => {
                    state.sort_asc = value == "true";
                }
                "window_width" => {
                    window_state.width = value.parse().ok();
                }
                "window_height" => {
                    window_state.height = value.parse().ok();
                }
                "window_x" => {
                    window_state.x = value.parse().ok();
                }
                "window_y" => {
                    window_state.y = value.parse().ok();
                }
                "window_maximized" => {
                    window_state.is_maximized = value == "true";
                }
                "window_fullscreen" => {
                    window_state.is_fullscreen = value == "true";
                }
                "left_sidebar_width" => {
                    state.left_sidebar_width = value.parse().ok();
                }
                "right_sidebar_width" => {
                    state.right_sidebar_width = value.parse().ok();
                }
                "translation_keys" => {
                    if let Ok(keys) = serde_json::from_str::<HashMap<String, String>>(&value) {
                        state.translation_keys = keys;
                    }
                }
                "translation_original_expanded" => {
                    state.translation_original_expanded = value == "true";
                }
                "google_drive_refresh_token" => {
                    state.google_drive_refresh_token = value;
                }
                "webdav_password" => {
                    state.webdav_password = value;
                }
                "pdf_page_color_mode" => {
                    state.pdf_page_color_mode = Some(value);
                }
                _ => {}
            }
        }

        state.window_state = window_state;
        debug!("本地状态管理: UI 状态加载完成");
        Ok(state)
    }

    /// 将内存中的状态保存到数据库
    /// 使用事务确保原子性
    pub fn save_all(&self, state: &AppUiState) -> Result<()> {
        debug!("本地状态管理: 正在保存 UI 状态到数据库");
        let mut conn = Connection::open(&self.db_path)?;
        let tx = conn.transaction()?;

        // Helper closure to upsert
        {
            let mut upsert = tx.prepare(
                "INSERT INTO ui_state (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            )?;

            // 1. Expanded Folders
            let expanded_json = serde_json::to_string(&state.expanded_folder_ids)?;
            upsert.execute(params!["expanded_folder_ids", expanded_json])?;

            // 2. Selected Sidebar Item
            if let Some(ref item) = state.selected_sidebar_item {
                upsert.execute(params!["selected_sidebar_item", item])?;
            } else {
                // 如果为 None，可能需要删除？或者存空字符串
                // 这里选择删除，或者忽略
                tx.execute(
                    "DELETE FROM ui_state WHERE key = ?1",
                    params!["selected_sidebar_item"],
                )?;
            }

            // 4. Sort Config
            if let Some(ref field) = state.sort_field {
                upsert.execute(params!["sort_field", field])?;
            }
            upsert.execute(params!["sort_asc", state.sort_asc.to_string()])?;

            // 5. Window State
            if let Some(w) = state.window_state.width {
                upsert.execute(params!["window_width", w.to_string()])?;
            }
            if let Some(h) = state.window_state.height {
                upsert.execute(params!["window_height", h.to_string()])?;
            }
            if let Some(x) = state.window_state.x {
                upsert.execute(params!["window_x", x.to_string()])?;
            }
            if let Some(y) = state.window_state.y {
                upsert.execute(params!["window_y", y.to_string()])?;
            }
            upsert.execute(params![
                "window_maximized",
                state.window_state.is_maximized.to_string()
            ])?;
            upsert.execute(params![
                "window_fullscreen",
                state.window_state.is_fullscreen.to_string()
            ])?;

            if let Some(w) = state.left_sidebar_width {
                upsert.execute(params!["left_sidebar_width", w.to_string()])?;
            }
            if let Some(w) = state.right_sidebar_width {
                upsert.execute(params!["right_sidebar_width", w.to_string()])?;
            }
            let keys_json = serde_json::to_string(&state.translation_keys)?;
            upsert.execute(params!["translation_keys", keys_json])?;
            upsert.execute(params![
                "translation_original_expanded",
                state.translation_original_expanded.to_string()
            ])?;
            upsert.execute(params![
                "google_drive_refresh_token",
                state.google_drive_refresh_token
            ])?;
            upsert.execute(params!["webdav_password", state.webdav_password])?;

            if let Some(ref mode) = state.pdf_page_color_mode {
                upsert.execute(params!["pdf_page_color_mode", mode])?;
            } else {
                tx.execute(
                    "DELETE FROM ui_state WHERE key = ?1",
                    params!["pdf_page_color_mode"],
                )?;
            }
        }

        tx.commit()?;
        debug!("本地状态管理: UI 状态保存完成");
        Ok(())
    }

    /// 从 state.db 加载 config JSON blob
    pub fn load_config(&self) -> Result<Option<String>> {
        debug!("本地状态管理: 正在加载配置");
        let conn = Connection::open(&self.db_path)?;
        let mut stmt = conn.prepare("SELECT value FROM ui_state WHERE key = ?1")?;
        let mut rows = stmt.query(params!["config"])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// 将 config JSON blob 保存到 state.db
    pub fn save_config(&self, config_json: &str) -> Result<()> {
        debug!("本地状态管理: 正在保存配置");
        let conn = Connection::open(&self.db_path)?;
        conn.execute(
            "INSERT INTO ui_state (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params!["config", config_json],
        )?;
        Ok(())
    }

    pub fn get_pdf_state(&self, id: &str) -> Result<Option<models::local_state::PdfState>> {
        debug!("本地状态管理: 正在加载 PDF 阅读器状态 (ID: {id})");
        let conn = Connection::open(&self.db_path)?;

        // 1. 获取全局 UI 状态
        let global_id = "global_pdf_ui_state";
        let mut global_stmt = conn.prepare(
            "SELECT zoom_level, fit_to_width, is_left_sidebar_open, is_right_sidebar_open,
                    left_sidebar_width, right_sidebar_width, COALESCE(auto_translate, 1) as auto_translate
             FROM pdf_state WHERE id = ?1",
        )?;
        let mut global_rows = global_stmt.query(params![global_id])?;
        let global_ui = if let Some(row) = global_rows.next()? {
            Some((
                row.get::<_, f32>(0)?,
                row.get::<_, i32>(1)? != 0,
                row.get::<_, i32>(2)? != 0,
                row.get::<_, i32>(3)? != 0,
                row.get::<_, f32>(4)?,
                row.get::<_, f32>(5)?,
                row.get::<_, i32>(6)? != 0,
            ))
        } else {
            None
        };

        // 2. 获取专属 PDF 进度及降级 UI 状态
        let mut stmt = conn.prepare(
            "SELECT path, page_index, zoom_level, offset_y, fit_to_width,
                    is_left_sidebar_open, is_right_sidebar_open,
                    left_sidebar_width, right_sidebar_width, last_read_at,
                    COALESCE(auto_translate, 1) as auto_translate
             FROM pdf_state WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            let (
                zoom_level,
                fit_to_width,
                is_left_sidebar_open,
                is_right_sidebar_open,
                left_sidebar_width,
                right_sidebar_width,
                auto_translate,
            ) = global_ui.unwrap_or_else(|| {
                (
                    row.get::<_, f32>(2).unwrap_or(1.0),
                    row.get::<_, i32>(4).unwrap_or(0) != 0,
                    row.get::<_, i32>(5).unwrap_or(1) != 0,
                    row.get::<_, i32>(6).unwrap_or(0) != 0,
                    row.get::<_, f32>(7).unwrap_or(250.0),
                    row.get::<_, f32>(8).unwrap_or(300.0),
                    row.get::<_, i32>(10).unwrap_or(1) != 0,
                )
            });

            Ok(Some(models::local_state::PdfState {
                path: row.get(0)?,
                page_index: row.get(1)?,
                zoom_level,
                offset_y: row.get(3)?,
                fit_to_width,
                is_left_sidebar_open,
                is_right_sidebar_open,
                left_sidebar_width,
                right_sidebar_width,
                last_read_at: row.get::<_, i64>(9)? as u64,
                auto_translate,
                id: id.to_string(),
            }))
        } else {
            // 如果此文档无专属记录，但存在全局 UI 状态，则以全局 UI 状态初始化
            if let Some((
                zoom_level,
                fit_to_width,
                is_left_sidebar_open,
                is_right_sidebar_open,
                left_sidebar_width,
                right_sidebar_width,
                auto_translate,
            )) = global_ui
            {
                Ok(Some(models::local_state::PdfState {
                    path: String::new(),
                    page_index: 0,
                    zoom_level,
                    offset_y: 0.0,
                    fit_to_width,
                    is_left_sidebar_open,
                    is_right_sidebar_open,
                    left_sidebar_width,
                    right_sidebar_width,
                    last_read_at: 0,
                    auto_translate,
                    id: id.to_string(),
                }))
            } else {
                Ok(None)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_pdf_state(
        &self,
        id: &str,
        path: &str,
        page_index: u16,
        zoom_level: f32,
        offset_y: f32,
        fit_to_width: bool,
        is_left_sidebar_open: bool,
        is_right_sidebar_open: bool,
        left_sidebar_width: f32,
        right_sidebar_width: f32,
        auto_translate: bool,
    ) -> Result<()> {
        debug!(
            "本地状态管理: 保存 PDF 阅读器状态 (ID: {id}, page: {page_index}, zoom: {zoom_level})"
        );
        let conn = Connection::open(&self.db_path)?;
        let last_read_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // 1. 保存专属进度及 UI 状态
        conn.execute(
            "INSERT INTO pdf_state (
                id, path, page_index, zoom_level, offset_y, fit_to_width,
                is_left_sidebar_open, is_right_sidebar_open,
                left_sidebar_width, right_sidebar_width, last_read_at,
                auto_translate
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO UPDATE SET
                path = excluded.path,
                page_index = excluded.page_index,
                zoom_level = excluded.zoom_level,
                offset_y = excluded.offset_y,
                fit_to_width = excluded.fit_to_width,
                auto_translate = excluded.auto_translate,
                is_left_sidebar_open = excluded.is_left_sidebar_open,
                is_right_sidebar_open = excluded.is_right_sidebar_open,
                left_sidebar_width = excluded.left_sidebar_width,
                right_sidebar_width = excluded.right_sidebar_width,
                last_read_at = excluded.last_read_at",
            params![
                id,
                path,
                page_index,
                zoom_level,
                offset_y,
                if fit_to_width { 1 } else { 0 },
                if is_left_sidebar_open { 1 } else { 0 },
                if is_right_sidebar_open { 1 } else { 0 },
                left_sidebar_width,
                right_sidebar_width,
                last_read_at as i64,
                if auto_translate { 1 } else { 0 }
            ],
        )?;

        // 2. 保存全局共享的 UI 状态
        let global_id = "global_pdf_ui_state";
        conn.execute(
            "INSERT INTO pdf_state (
                id, path, page_index, zoom_level, offset_y, fit_to_width,
                is_left_sidebar_open, is_right_sidebar_open,
                left_sidebar_width, right_sidebar_width, last_read_at,
                auto_translate
             )
             VALUES (?1, '', 0, ?2, 0.0, ?3, ?4, ?5, ?6, ?7, 0, ?8)
             ON CONFLICT(id) DO UPDATE SET
                zoom_level = excluded.zoom_level,
                fit_to_width = excluded.fit_to_width,
                auto_translate = excluded.auto_translate,
                is_left_sidebar_open = excluded.is_left_sidebar_open,
                is_right_sidebar_open = excluded.is_right_sidebar_open,
                left_sidebar_width = excluded.left_sidebar_width,
                right_sidebar_width = excluded.right_sidebar_width",
            params![
                global_id,
                zoom_level,
                if fit_to_width { 1 } else { 0 },
                if is_left_sidebar_open { 1 } else { 0 },
                if is_right_sidebar_open { 1 } else { 0 },
                left_sidebar_width,
                right_sidebar_width,
                if auto_translate { 1 } else { 0 }
            ],
        )?;

        Ok(())
    }

    // ── AI 对话 ──────────────────────────────────────────

    pub fn list_chat_sessions(&self, literature_id: &str) -> Result<Vec<ChatSession>> {
        let conn = Connection::open(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT id, literature_id, title, system_prompt, created_at, updated_at, compressed_summary
             FROM chat_sessions
             WHERE literature_id = ?1
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map(params![literature_id], |row| {
            Ok(ChatSession {
                id: row.get(0)?,
                literature_id: row.get(1)?,
                title: row.get(2)?,
                system_prompt: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                compressed_summary: row.get(6)?,
            })
        })?;
        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }
        Ok(sessions)
    }

    pub fn create_chat_session(
        &self,
        literature_id: &str,
        title: &str,
        system_prompt: &str,
    ) -> Result<String> {
        debug!("本地状态管理: 新建对话 (literature_id={literature_id})");
        let conn = Connection::open(&self.db_path)?;
        let now = chrono::Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO chat_sessions (id, literature_id, title, system_prompt, created_at, updated_at, compressed_summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, '')",
            params![id, literature_id, title, system_prompt, now],
        )
        .map_err(|e| {
            error!("create_chat_session INSERT 失败: {e}");
            e
        })?;
        Ok(id)
    }

    pub fn delete_chat_session(&self, session_id: &str) -> Result<bool> {
        debug!("本地状态管理: 删除对话 (id={session_id})");
        let conn = Connection::open(&self.db_path)?;
        conn.execute(
            "DELETE FROM chat_messages WHERE session_id = ?1",
            params![session_id],
        )?;
        let rows = conn.execute(
            "DELETE FROM chat_sessions WHERE id = ?1",
            params![session_id],
        )?;
        Ok(rows > 0)
    }

    pub fn update_chat_session(
        &self,
        session_id: &str,
        title: Option<&str>,
        system_prompt: Option<&str>,
    ) -> Result<bool> {
        let conn = Connection::open(&self.db_path)?;
        let now = chrono::Utc::now().timestamp();
        let rows = conn.execute(
            "UPDATE chat_sessions
             SET title = COALESCE(?2, title),
                 system_prompt = COALESCE(?3, system_prompt),
                 updated_at = ?4
             WHERE id = ?1",
            params![session_id, title, system_prompt, now],
        )?;
        Ok(rows > 0)
    }

    pub fn get_chat_session_summary(&self, session_id: &str) -> Result<String> {
        let conn = Connection::open(&self.db_path)?;
        let summary: String = conn.query_row(
            "SELECT compressed_summary FROM chat_sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(summary)
    }

    pub fn update_chat_session_summary(&self, session_id: &str, summary: &str) -> Result<bool> {
        let conn = Connection::open(&self.db_path)?;
        let rows = conn.execute(
            "UPDATE chat_sessions SET compressed_summary = ?1, updated_at = ?2 WHERE id = ?3",
            params![summary, chrono::Utc::now().timestamp(), session_id],
        )?;
        Ok(rows > 0)
    }

    pub fn list_chat_messages(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
        let conn = Connection::open(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, attachments, created_at, reasoning, parent_id
             FROM chat_messages
             WHERE session_id = ?1
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            let attachments_str: String = row.get(4)?;
            let attachments: Vec<String> =
                serde_json::from_str(&attachments_str).unwrap_or_default();
            Ok(ChatMessage {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                attachments,
                created_at: row.get(5)?,
                reasoning: row.get(6)?,
                parent_id: row.get(7)?,
            })
        })?;
        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }
        Ok(messages)
    }

    pub fn add_chat_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        attachments: &[String],
        reasoning: Option<&str>,
    ) -> Result<String> {
        self.add_chat_message_with_parent(session_id, role, content, attachments, reasoning, None)
    }

    pub fn add_chat_message_with_parent(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        attachments: &[String],
        reasoning: Option<&str>,
        parent_id: Option<&str>,
    ) -> Result<String> {
        let conn = Connection::open(&self.db_path)?;
        let now = chrono::Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();
        let attachments_json = serde_json::to_string(attachments)?;
        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content, attachments, created_at, reasoning, parent_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, session_id, role, content, attachments_json, now, reasoning, parent_id],
        )?;
        conn.execute(
            "UPDATE chat_sessions SET updated_at = ?1, active_message_id = ?3 WHERE id = ?2",
            params![now, session_id, id],
        )?;
        Ok(id)
    }

    /// 根据活跃叶子节点追溯会话当前分支的消息链
    pub fn get_chat_message_chain(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
        let conn = Connection::open(&self.db_path)?;

        // 1. 获取会话的 active_message_id
        let active_id: Option<String> = conn
            .query_row(
                "SELECT active_message_id FROM chat_sessions WHERE id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .unwrap_or(None);

        let leaf_id = match active_id {
            Some(id) if !id.is_empty() => id,
            _ => {
                // 如果没有设置 active_message_id，默认取最新的一条消息作为叶子节点
                let latest_id: Option<String> = conn.query_row(
                    "SELECT id FROM chat_messages WHERE session_id = ?1 ORDER BY created_at DESC LIMIT 1",
                    params![session_id],
                    |row| row.get(0),
                ).ok();
                match latest_id {
                    Some(id) => id,
                    None => return Ok(Vec::new()), // 会话中没有消息
                }
            }
        };

        // 2. 自底向上沿 parent_id 追溯
        let mut chain = Vec::new();
        let mut current_id = Some(leaf_id);

        while let Some(id) = current_id {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, role, content, attachments, created_at, reasoning, parent_id
                 FROM chat_messages WHERE id = ?1"
            )?;
            let msg_res = stmt.query_row(params![id], |row| {
                let attachments_str: String = row.get(4)?;
                let attachments: Vec<String> =
                    serde_json::from_str(&attachments_str).unwrap_or_default();
                Ok(ChatMessage {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    attachments,
                    created_at: row.get(5)?,
                    reasoning: row.get(6)?,
                    parent_id: row.get(7)?,
                })
            });

            match msg_res {
                Ok(msg) => {
                    let next_parent = msg.parent_id.clone();
                    chain.push(msg);
                    current_id = next_parent;
                }
                Err(_) => {
                    break;
                }
            }
        }

        // 3. 反序以符合时间正序
        chain.reverse();
        Ok(chain)
    }

    /// 切换会话活跃的叶子节点
    pub fn switch_active_message(&self, session_id: &str, leaf_message_id: &str) -> Result<()> {
        let conn = Connection::open(&self.db_path)?;
        conn.execute(
            "UPDATE chat_sessions SET active_message_id = ?1, updated_at = ?2 WHERE id = ?3",
            params![leaf_message_id, chrono::Utc::now().timestamp(), session_id],
        )?;
        Ok(())
    }

    /// 沿着某个分支一直向下，找到最新的叶子节点
    pub fn find_deepest_leaf(&self, start_message_id: &str) -> Result<String> {
        let conn = Connection::open(&self.db_path)?;
        let mut current_id = start_message_id.to_string();

        loop {
            let next_child: Option<String> = conn.query_row(
                "SELECT id FROM chat_messages WHERE parent_id = ?1 ORDER BY created_at DESC LIMIT 1",
                params![current_id],
                |row| row.get(0),
            ).ok();

            match next_child {
                Some(child_id) => {
                    current_id = child_id;
                }
                None => {
                    break;
                }
            }
        }

        Ok(current_id)
    }

    /// 级联回退：删除指定消息及其之后的所有消息，并将该消息的 parent_id 设为当前活跃节点
    pub fn truncate_chat_messages_after(
        &self,
        session_id: &str,
        target_message_id: &str,
    ) -> Result<()> {
        let conn = Connection::open(&self.db_path)?;

        // 1. 获取目标消息的创建时间戳和 parent_id
        let (target_time, parent_id): (i64, Option<String>) = conn.query_row(
            "SELECT created_at, parent_id FROM chat_messages WHERE id = ?1 AND session_id = ?2",
            params![target_message_id, session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        // 2. 删除该时间戳及其之后的所有消息 (包含 target 自身)
        conn.execute(
            "DELETE FROM chat_messages WHERE session_id = ?1 AND created_at >= ?2",
            params![session_id, target_time],
        )?;

        // 3. 将会话的活跃节点更新为目标消息的 parent_id
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "UPDATE chat_sessions SET active_message_id = ?1, updated_at = ?2 WHERE id = ?3",
            params![parent_id, now, session_id],
        )?;

        Ok(())
    }

    /// 获取当前消息的所有兄弟节点 ID 列表（包含它自己，按创建时间排序）
    pub fn get_message_siblings(&self, message_id: &str) -> Result<Vec<String>> {
        let conn = Connection::open(&self.db_path)?;
        // 1. 获取该消息的 parent_id 和 session_id
        let (parent_id, session_id): (Option<String>, String) = conn.query_row(
            "SELECT parent_id, session_id FROM chat_messages WHERE id = ?1",
            params![message_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        // 2. 使用 IS 统一处理 Option 类型，并查询兄弟节点 ID
        let mut stmt = conn.prepare(
            "SELECT id FROM chat_messages 
             WHERE session_id = ?1 AND parent_id IS ?2 
             ORDER BY created_at ASC",
        )?;

        let rows = stmt.query_map(params![session_id, parent_id], |row| {
            row.get::<_, String>(0)
        })?;

        let mut siblings = Vec::new();
        for row in rows {
            siblings.push(row?);
        }
        Ok(siblings)
    }

    pub fn delete_chat_sessions_for_literature(&self, literature_id: &str) -> Result<usize> {
        debug!("本地状态管理: 清理文献的所有对话 (literature_id={literature_id})");
        let conn = Connection::open(&self.db_path)?;
        conn.execute(
            "DELETE FROM chat_messages WHERE session_id IN (SELECT id FROM chat_sessions WHERE literature_id = ?1)",
            params![literature_id],
        )?;
        let rows = conn.execute(
            "DELETE FROM chat_sessions WHERE literature_id = ?1",
            params![literature_id],
        )?;
        Ok(rows)
    }
}
