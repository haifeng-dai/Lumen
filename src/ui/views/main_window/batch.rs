use gpui::{AsyncApp, WeakEntity, prelude::*};
use i18n::{I18nKey, tf};

use super::*;

impl super::MainWindow {
    pub fn handle_batch_fetch_metadata(
        &mut self,
        lit_ids: Vec<String>,
        source_type: crate::ui::views::main_window::types::BatchSource,
        cx: &mut Context<Self>,
    ) {
        if lit_ids.is_empty() {
            return;
        }

        let app = self.app.clone();
        let data_store = self.data_store.clone();
        let total = lit_ids.len();
        let lang = self.app.current_language();
        self.loading_modal = Some(tf(
            I18nKey::BatchUpdatingMetadata,
            lang,
            &["0", &total.to_string()],
        ));
        cx.notify();

        cx.spawn(move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx_inner = cx.clone();
            async move {
                let mut results = Vec::new();
                {
                    for (i, id) in lit_ids.into_iter().enumerate() {
                        // 更新进度提示
                        let _ = this.update(&mut cx_inner, |this, cx: &mut Context<Self>| {
                            this.loading_modal = Some(tf(
                                I18nKey::BatchUpdatingMetadata,
                                lang,
                                &[&(i + 1).to_string(), &total.to_string()],
                            ));
                            cx.notify();
                        });

                        let lit_opt = cx_inner.update(|cx| {
                            data_store
                                .read(cx)
                                .literatures
                                .iter()
                                .find(|l| l.id == id)
                                .cloned()
                        });

                        if let Some(lit) = lit_opt {
                            let app_clone = app.clone();
                            let lit_for_fetch = lit.clone();
                            let handle = crate::RUNTIME.spawn(async move {
                                if source_type == BatchSource::OpenAlex {
                                    app_clone
                                        .fetcher_service
                                        .resolve_openalex_auto(&lit_for_fetch)
                                        .await
                                } else {
                                    let source = match source_type {
                                        BatchSource::ArXiv => {
                                            crate::ui::views::main_window::utils::extract_arxiv_id(
                                                &lit_for_fetch,
                                            )
                                            .map(FetchSource::ArXiv)
                                        }
                                        BatchSource::Doi => lit_for_fetch
                                            .doi
                                            .as_ref()
                                            .filter(|d| !d.trim().is_empty())
                                            .map(|d| FetchSource::Doi(d.clone())),
                                        BatchSource::Dblp => {
                                            if lit_for_fetch.title.is_empty() {
                                                None
                                            } else {
                                                Some(FetchSource::Dblp(lit_for_fetch.title.clone()))
                                            }
                                        }
                                        BatchSource::OpenAlex => None,
                                    };

                                    match source {
                                        Some(source) => app_clone
                                            .fetch_metadata_from_source(source)
                                            .await
                                            .map(Some),
                                        None => Ok(None),
                                    }
                                }
                            });

                            match handle.await {
                                Ok(Ok(Some(remote_lit))) => {
                                    results.push((lit, remote_lit));
                                }
                                Ok(Ok(None)) => {
                                    log::debug!("Batch fetch skipped for {id}: no usable source");
                                }
                                Ok(Err(e)) => {
                                    log::error!("Batch fetch failed for {id}: {e}");
                                }
                                Err(e) => {
                                    log::error!("Tokio task join failed for {id}: {e}");
                                }
                            }
                        }
                    }
                }

                let _ = this.update(&mut cx_inner, |this, cx: &mut Context<Self>| {
                    this.loading_modal = None;
                    this.pending_compares.extend(results);
                    this.process_next_batch_compare(cx);
                    this.app.notify_data_changed();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 处理对比队列中的下一项
    pub fn process_next_batch_compare(&mut self, cx: &mut Context<Self>) {
        if self.pending_compares.is_empty() {
            return;
        }

        let (original, remote) = self.pending_compares.remove(0);
        self.show_literature_compare_with_callback(original, remote, cx, |this, cx| {
            this.process_next_batch_compare(cx);
        });
    }

    // =========================================================================
    // UI 状态变更方法（写入 UiState Global + LocalState）
    // =========================================================================
}
