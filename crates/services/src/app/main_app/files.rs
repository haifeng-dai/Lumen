use crate::runtime::RUNTIME;
use crate::utils::filename;
use anyhow::{Result, anyhow};
use log::{debug, error, info, warn};
use models::FetchSource;
use models::config::AppConfig;
use models::{Attachment, Literature};
use std::{path::Path, process::Command};
use tokio::sync::broadcast;
use uuid::Uuid;

use super::MainApp;
use crate::notify::{AttachmentOpenNotice, RefreshMsg};
use crate::sync::attachments::{
    PrepareAttachmentErrorKind, PreparedAttachment, classify_prepare_attachment_error,
};

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
        if self.get_attachment_by_id(id).is_none() {
            warn!("MainApp: 打开附件失败，未找到附件记录");
            return Err(anyhow!("未找到附件"));
        }

        let config = self.config.lock().unwrap().clone();
        let att_id = id.to_string();
        info!("MainApp: 触发附件打开准备流程");
        let sync = self.sync_service.clone();
        let refresh_tx = self.refresh_tx.lock().unwrap().clone();

        RUNTIME.spawn(async move {
            // 同步外壳：返回 Ok(()) 仅表示异步任务已成功提交，
            // 不代表附件已经准备或打开成功（§ PLAN 3.2.5）。
            // 错误分类只在此处进行一次，编排函数只接收类型化结果。
            let prepared = sync
                .prepare_attachment_for_open(&att_id)
                .await
                .map_err(|error| classify_prepare_attachment_error(&error));
            if let Ok(p) = &prepared {
                if let Some(tx) = &refresh_tx {
                    let _ = tx.send(RefreshMsg::DataChanged);
                }
                if p.issue.is_none() {
                    sync.request_sync();
                }
            }
            Self::dispatch_open_result(prepared, &refresh_tx, |p| {
                Self::open_file_with_config(p, &config)
            });
        });

        Ok(())
    }

    /// 打开编排的可测试纯函数：类型化 prepare 结果 → 调用最终打开回调 → 广播至多一条通知。
    ///
    /// 唯一通知优先级：`prepare 失败 > open 失败 > issue > 无通知`。
    /// 不依赖 GPUI、不启动真实系统程序（最终打开由注入的 `open` 回调执行）。
    /// - 准备失败：广播 `PrepareFailed(kind)`，不调用 `open`（不回退旧路径）。
    /// - 最终打开失败：只广播 `OpenFailed`，不再发 issue（避免双 Toast）。
    /// - 最终打开成功且有 `issue`：只广播 `Issue`（issue 只读，不阻止打开）。
    /// - 通知发送失败仅表示 UI 接收端已关闭，不影响已完成的准备/打开。
    fn dispatch_open_result(
        prepared: Result<PreparedAttachment, PrepareAttachmentErrorKind>,
        refresh_tx: &Option<broadcast::Sender<RefreshMsg>>,
        open: impl FnOnce(&Path) -> Result<()>,
    ) {
        match prepared {
            Ok(prepared) => {
                if open(prepared.local_path.as_path()).is_err() {
                    // 仅记录固定类别，不输出路径/文件名/版本（§ PLAN 3.4）
                    error!("MainApp: 系统打开附件失败 (category=open_failed)");
                    Self::send_open_notice(refresh_tx, AttachmentOpenNotice::OpenFailed);
                    return;
                }
                if let Some(issue) = prepared.issue {
                    Self::send_open_notice(refresh_tx, AttachmentOpenNotice::Issue(issue));
                }
            }
            Err(kind) => {
                // 仅记录固定类别，不输出路径/object key/版本/hash（§ PLAN 3.4）
                error!("MainApp: 准备附件失败 (category={kind:?})");
                Self::send_open_notice(refresh_tx, AttachmentOpenNotice::PrepareFailed(kind));
            }
        }
    }

    fn send_open_notice(
        refresh_tx: &Option<broadcast::Sender<RefreshMsg>>,
        notice: AttachmentOpenNotice,
    ) {
        if let Some(tx) = refresh_tx {
            let _ = tx.send(RefreshMsg::AttachmentOpenNotice(notice));
        }
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

    fn open_file_with_config(path: &Path, config: &AppConfig) -> Result<()> {
        debug!("MainApp: 使用系统打开文件 (path='{}')", path.display());
        let is_pdf = path.to_string_lossy().to_lowercase().ends_with(".pdf");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::RefreshMsg;
    use crate::sync::attachments::{AttachmentSyncIssue, PreparedAttachment};
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::rc::Rc;

    /// 与数据库旧路径明显不同的可信准备路径。
    fn trusted_path() -> PathBuf {
        PathBuf::from("/tmp/attachment-open-trusted/paper-v2.pdf")
    }

    fn prepared(
        issue: Option<AttachmentSyncIssue>,
    ) -> Result<PreparedAttachment, PrepareAttachmentErrorKind> {
        Ok(PreparedAttachment {
            attachment_id: "att-1".to_string(),
            local_path: trusted_path(),
            issue,
        })
    }

    struct OpenRun {
        notices: Vec<AttachmentOpenNotice>,
        open_count: usize,
        opened_paths: Vec<PathBuf>,
    }

    /// 运行编排并回收所有 `AttachmentOpenNotice` 事件、打开次数与每次收到的路径。
    fn run_and_collect(
        prepared: Result<PreparedAttachment, PrepareAttachmentErrorKind>,
        open: impl FnOnce(&Path) -> Result<()>,
    ) -> OpenRun {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<RefreshMsg>(16);
        let open_count = Rc::new(Cell::new(0usize));
        let opened_paths = Rc::new(std::cell::RefCell::new(Vec::<PathBuf>::new()));
        let open_count_for_closure = open_count.clone();
        let opened_paths_for_closure = opened_paths.clone();
        let open = move |p: &Path| {
            open_count_for_closure.set(open_count_for_closure.get() + 1);
            opened_paths_for_closure.borrow_mut().push(p.to_path_buf());
            open(p)
        };
        MainApp::dispatch_open_result(prepared, &Some(tx), open);
        let mut notices = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            if let RefreshMsg::AttachmentOpenNotice(n) = msg {
                notices.push(n);
            }
        }
        OpenRun {
            notices,
            open_count: open_count.get(),
            opened_paths: opened_paths.borrow().clone(),
        }
    }

    #[test]
    fn attachment_open_success_without_issue_emits_no_notice() {
        let run = run_and_collect(prepared(None), |_| Ok(()));
        assert!(run.notices.is_empty());
        assert_eq!(run.open_count, 1);
    }

    #[test]
    fn attachment_open_success_with_issue_emits_exactly_one_issue_notice() {
        let cases = [
            AttachmentSyncIssue::FileConflict,
            AttachmentSyncIssue::UnknownDivergence,
            AttachmentSyncIssue::PendingDownload,
        ];
        for issue in cases {
            let run = run_and_collect(prepared(Some(issue.clone())), |_| Ok(()));
            assert_eq!(run.notices.len(), 1, "{issue:?}: 不得双 Toast");
            assert_eq!(run.notices[0], AttachmentOpenNotice::Issue(issue.clone()));
            assert_eq!(run.open_count, 1, "{issue:?}: issue 只读，不阻止打开");
        }
    }

    #[test]
    fn attachment_open_failure_suppresses_issue_and_emits_only_open_failed() {
        let cases = [
            AttachmentSyncIssue::FileConflict,
            AttachmentSyncIssue::UnknownDivergence,
            AttachmentSyncIssue::PendingDownload,
        ];
        for issue in cases {
            let run = run_and_collect(prepared(Some(issue.clone())), |_| {
                Err(anyhow!("cannot spawn"))
            });
            assert_eq!(
                run.notices.len(),
                1,
                "{issue:?}: issue+open 失败只允许一条通知"
            );
            assert_eq!(run.notices[0], AttachmentOpenNotice::OpenFailed);
            assert_eq!(run.open_count, 1);
        }
    }

    #[test]
    fn attachment_open_prepare_failure_propagates_every_kind_without_opening() {
        let kinds = [
            PrepareAttachmentErrorKind::UnrecoverableMissing,
            PrepareAttachmentErrorKind::Conflict,
            PrepareAttachmentErrorKind::IdentityMismatch,
            PrepareAttachmentErrorKind::BackendUnavailable,
            PrepareAttachmentErrorKind::LocalWrite,
            PrepareAttachmentErrorKind::InvalidState,
        ];
        for kind in kinds {
            let run = run_and_collect(Err(kind), |_| {
                panic!("prepare 失败时不得调用最终打开回调");
            });
            assert_eq!(run.notices.len(), 1, "{kind:?}: 只允许一条通知");
            assert_eq!(run.notices[0], AttachmentOpenNotice::PrepareFailed(kind));
            assert_eq!(run.open_count, 0, "{kind:?}: 不得打开");
        }
    }

    #[test]
    fn attachment_open_callback_receives_exactly_prepared_local_path() {
        let run = run_and_collect(prepared(None), |_| Ok(()));
        assert_eq!(run.open_count, 1);
        assert_eq!(run.opened_paths.len(), 1);
        assert_eq!(run.opened_paths[0], trusted_path());
    }

    #[test]
    fn attachment_open_receiver_closed_on_success_does_not_panic() {
        let (tx, rx) = tokio::sync::broadcast::channel::<RefreshMsg>(16);
        drop(rx); // 接收端已关闭
        let open_count = Rc::new(Cell::new(0usize));
        let open_count_clone = open_count.clone();
        MainApp::dispatch_open_result(prepared(None), &Some(tx), move |_| {
            open_count_clone.set(open_count_clone.get() + 1);
            Ok(())
        });
        assert_eq!(open_count.get(), 1, "通知通道失败不改变打开行为");
    }

    #[test]
    fn attachment_open_receiver_closed_on_open_failure_does_not_panic() {
        let (tx, rx) = tokio::sync::broadcast::channel::<RefreshMsg>(16);
        drop(rx); // 发送 OpenFailed 也会失败
        let open_count = Rc::new(Cell::new(0usize));
        let open_count_clone = open_count.clone();
        MainApp::dispatch_open_result(prepared(None), &Some(tx), move |_| {
            open_count_clone.set(open_count_clone.get() + 1);
            Err(anyhow!("cannot spawn"))
        });
        assert_eq!(open_count.get(), 1, "通知通道失败不改变打开行为");
    }
}
