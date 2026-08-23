use crate::runtime::RUNTIME;
use crate::utils::filename;
use anyhow::{Result, anyhow};
use log::{debug, error, info, warn};
use models::FetchSource;
use models::config::AppConfig;
use models::{Attachment, Literature};
use std::{path::Path, process::Command};
use uuid::Uuid;

use super::MainApp;

impl MainApp {
    pub fn import_file_to_literature(
        &self,
        lit_id: &str,
        path: &Path,
        is_main: bool,
    ) -> Result<()> {
        info!(
            "MainApp: 导入文件到文献 lit={lit_id}, path='{}', is_main={is_main}",
            path.display()
        );
        let lit = self.db.get_literature(lit_id)?.ok_or_else(|| {
            warn!("MainApp: 导入失败，找不到文献 (id={lit_id})");
            anyhow!("找不到文献")
        })?;
        let (last, first) = lit.authors.first().map_or_else(
            || ("Unknown".to_string(), String::new()),
            |a| (a.last_name.clone(), a.first_name.clone()),
        );
        let opts = filename::filename_options_from_path(
            &last,
            &first,
            lit.year,
            &lit.title,
            &lit.publication
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            path,
            is_main,
        );
        let name = filename::generate_literature_filename(
            &opts,
            Some(&self.config.lock().unwrap().filename_template.clone()),
        );
        let mut new_lit = lit.clone();
        if is_main {
            for a in &new_lit.attachments {
                if a.is_main
                    && let Err(e) = self.file_manager.trash_file(&a.file_path)
                {
                    warn!("文件系统: 移入回收站失败 [{}]: {e}", a.file_path);
                }
            }
            new_lit.attachments.retain(|a| !a.is_main);
        }
        let result = self.file_manager.upload_file_with_name(path, &name)?;
        let mut att = models::constructors::create_attachment(
            Uuid::new_v4().to_string(),
            lit_id.to_string(),
            result.final_path.to_string_lossy().to_string(),
            result.final_name,
            result.size,
        );
        att.is_main = is_main;
        new_lit.attachments.push(att);
        new_lit.version += 1;
        new_lit.updated_at = chrono::Local::now().timestamp();
        self.op_notify(|| {
            self.literature_service.save_literature(
                self.db.clone(),
                self.data_changed_notify(),
                new_lit,
            )
        })
    }

    pub fn open_attachment(&self, id: &str) -> Result<()> {
        if let Some(att) = self.get_attachment_by_id(id) {
            let path = Path::new(&att.file_path);
            let config = self.config.lock().unwrap().clone();

            // 捕获通知发送端
            let refresh_tx = self.refresh_tx.lock().unwrap().clone();

            if path.exists() {
                info!("MainApp: 打开附件 (id={id}, path='{}')", att.file_path);
                Self::open_file_with_config(&att.file_path, &config)?;
            } else {
                let att_id = att.id.clone();
                info!(
                    "MainApp: 附件本地不存在，触发远程下载 (id={att_id}, name='{}')",
                    att.file_name
                );
                let sync = self.sync_service.clone();
                let db = self.db.clone();
                // 异步执行下载/修复任务
                RUNTIME.spawn(async move {
                    match sync.download_single_file(&att).await {
                        Ok(changed) => {
                            if changed {
                                info!("MainApp: 附件下载成功并已更新本地记录");
                                if let Some(tx) = &refresh_tx {
                                    let _ = tx.send(crate::notify::RefreshMsg::DataChanged);
                                }
                                // 同时请求一次后台同步以保持状态一致
                                sync.request_sync();
                            } else {
                                debug!("MainApp: 附件下载返回无变更");
                            }
                            // 最稳妥的方式：重新获取 attachment
                            if let Ok(Some(new_att)) = db.get_attachment(&att.id) {
                                let _ = Self::open_file_with_config(&new_att.file_path, &config);
                            }
                        }
                        Err(e) => {
                            error!("下载/打开附件失败 (id={att_id}): {e}");
                        }
                    }
                });
            }
        } else {
            warn!("MainApp: 打开附件失败，未找到 (id={id})");
        }
        Ok(())
    }

    pub fn open_literature_main_file(&self, id: &str) -> Result<()> {
        debug!("MainApp: 打开文献主文件 (lit_id={id})");
        let att_id = self.db.get_literature(id)?.and_then(|l| {
            l.attachments
                .iter()
                .find(|a| a.is_main)
                .map(|a| a.id.clone())
        });
        if let Some(aid) = att_id {
            self.open_attachment(&aid)?;
        } else {
            debug!("MainApp: 文献无主文件 (lit_id={id})");
        }
        Ok(())
    }

    pub fn delete_attachment_file(&self, id: &str) -> Result<()> {
        let att = self.db.get_attachment(id)?.ok_or_else(|| {
            warn!("MainApp: 删除附件失败，未找到 (id={id})");
            anyhow!("找不到附件")
        })?;
        info!("MainApp: 删除附件文件 (id={id}, name='{}')", att.file_name);
        let path = att.file_path;
        self.op_notify(|| {
            if let Err(e) = self.file_manager.trash_file(&path) {
                warn!("文件系统: 移入回收站失败 [{}]: {e}", path);
            }
            self.db.delete_attachment(id)?;
            Ok(())
        })
    }

    pub fn get_attachment_by_id(&self, id: &str) -> Option<Attachment> {
        self.db.get_attachment(id).unwrap_or(None)
    }

    /// 判断文件是否应使用外部程序打开（非PDF或启用了外置阅读器时为true）
    pub fn should_use_external_viewer(&self, path: &str) -> bool {
        let is_pdf = path.to_lowercase().ends_with(".pdf");
        if !is_pdf {
            return true;
        }
        let config = self.config.lock().unwrap();
        config.pdf_viewer.use_custom
    }

    fn open_file_with_config(path: &str, config: &AppConfig) -> Result<()> {
        debug!("MainApp: 使用系统打开文件 (path='{path}')");
        let is_pdf = path.to_lowercase().ends_with(".pdf");
        if is_pdf && config.pdf_viewer.use_custom {
            #[cfg(target_os = "macos")]
            if !config.pdf_viewer.macos_app.is_empty() {
                return Ok(Command::new("open")
                    .arg("-a")
                    .arg(&config.pdf_viewer.macos_app)
                    .arg(path)
                    .spawn()
                    .map(|_| ())?);
            }
            #[cfg(target_os = "windows")]
            if !config.pdf_viewer.windows_app.is_empty() {
                return Ok(Command::new(&config.pdf_viewer.windows_app)
                    .arg(path)
                    .spawn()
                    .map(|_| ())?);
            }
        }
        #[cfg(target_os = "macos")]
        Command::new("open").arg(path).spawn()?;
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            std::process::Command::new("cmd")
                .arg("/c")
                .arg("start")
                .arg("")
                .arg(path)
                .creation_flags(0x08000000)
                .spawn()?;
        }
        #[cfg(target_os = "linux")]
        Command::new("xdg-open").arg(path).spawn()?;
        Ok(())
    }

    pub async fn fetch_metadata_from_source(&self, source: FetchSource) -> Result<Literature> {
        debug!("MainApp: 从外部源获取元数据");
        match source {
            FetchSource::Doi(doi) => self.fetcher_service.parse_doi(&doi).await,
            FetchSource::ArXiv(id) => self.fetcher_service.parse_arxiv(&id).await,
            FetchSource::Dblp(query) => self.fetcher_service.resolve_dblp_best_match(&query).await,
            FetchSource::OpenAlexDoi(doi) => self.fetcher_service.parse_openalex(&doi).await,
            FetchSource::OpenAlexTitle(title) => {
                self.fetcher_service
                    .resolve_openalex_best_match(&title)
                    .await
            }
        }
    }

    pub fn find_duplicates(&self) -> Vec<Vec<Literature>> {
        let result = self.literature_service.find_duplicates(&self.db);
        let total_dup: usize = result.iter().map(|g| g.len()).sum();
        debug!(
            "MainApp: 查重完成, 发现 {} 组共 {} 篇重复文献",
            result.len(),
            total_dup
        );
        result
    }

    pub fn merge_literature_relations(&self, source_id: &str, target_id: &str) -> Result<()> {
        info!("MainApp: 合并文献关系 source={source_id} -> target={target_id}");
        self.op_notify(|| {
            self.db.merge_literature_relations(source_id, target_id)?;
            Ok(())
        })
    }

    pub fn cleanup_orphaned_files(&self) -> Result<()> {
        info!("MainApp: 清理孤立文件...");
        let att_dir = self.file_manager.get_attachments_dir();
        self.attachment_service
            .cleanup_orphaned_files(&self.db, &att_dir, |p| self.file_manager.trash_file(p))
    }
}
