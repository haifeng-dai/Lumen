use gpui::Context;

impl super::PdfReaderView {
    pub(crate) fn clear_thumbnail_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected_thumbnails.is_empty() && self.last_anchor_page.is_none() {
            return;
        }
        self.selected_thumbnails.clear();
        self.last_anchor_page = None;
        cx.notify();
    }

    pub(crate) fn select_thumbnail(&mut self, page: u16, cx: &mut Context<Self>) {
        self.selected_thumbnails.clear();
        self.selected_thumbnails.insert(page);
        self.last_anchor_page = Some(page);
        cx.notify();
    }

    /// Cmd/Ctrl 点击：切换该页选中态，不影响其他。
    pub(crate) fn toggle_thumbnail_selection(&mut self, page: u16, cx: &mut Context<Self>) {
        if self.selected_thumbnails.contains(&page) {
            self.selected_thumbnails.remove(&page);
            if self.last_anchor_page == Some(page) {
                self.last_anchor_page = None;
            }
        } else {
            self.selected_thumbnails.insert(page);
            self.last_anchor_page = Some(page);
        }
        cx.notify();
    }

    /// Shift 点击：以 last_anchor_page 为起点到当前页做范围选（含两端）。
    pub(crate) fn range_select_thumbnails(&mut self, page: u16, cx: &mut Context<Self>) {
        let start = match self.last_anchor_page {
            Some(p) => p,
            None => {
                self.select_thumbnail(page, cx);
                return;
            }
        };
        let (lo, hi) = if page >= start {
            (start, page)
        } else {
            (page, start)
        };
        for p in lo..=hi {
            self.selected_thumbnails.insert(p);
        }
        cx.notify();
    }

    /// 删除给定页（降序发送，避免索引前移错位），并清空选中集。
    pub(crate) fn delete_pages(&mut self, pages: &[u16], cx: &mut Context<Self>) {
        if pages.is_empty() {
            return;
        }
        let mut sorted: Vec<u16> = pages.to_vec();
        sorted.sort_unstable_by(|a, b| b.cmp(a)); // 降序
        for p in sorted {
            self.pdf_service.send_delete_page(p);
        }
        self.selected_thumbnails.clear();
        self.last_anchor_page = None;
        cx.notify();
    }

    /// 把给定页导出为新 PDF：弹系统保存对话框，用户选路径后发给 worker。
    pub(crate) fn export_pages(&mut self, pages: &[u16], cx: &mut Context<Self>) {
        if pages.is_empty() {
            return;
        }
        let mut sorted: Vec<u16> = pages.to_vec();
        sorted.sort_unstable();

        // 原文档目录 + 建议文件名
        let dir = self.document_path.parent().map(|p| p.to_path_buf());
        let suggested_name = self.suggest_extract_name(&sorted);

        let dir_ref = dir.as_deref().unwrap_or_else(|| std::path::Path::new("."));
        // 同步触发系统保存对话框，拿接收端后在后台任务中等待用户选择
        let receiver = cx.prompt_for_new_path(dir_ref, Some(suggested_name.as_str()));

        let service = self.pdf_service.clone();
        let pages_for_task = sorted.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(dest_path))) = receiver.await {
                service.send_extract_pages(pages_for_task, dest_path);
                let _ = this.update(cx, |this, cx| {
                    this.clear_thumbnail_selection(cx);
                });
            }
        })
        .detach();
    }

    /// 根据选中页生成建议文件名，如 `<原名>_pages_1-3.pdf`（单页 `<原名>_p2.pdf`）。
    fn suggest_extract_name(&self, pages: &[u16]) -> String {
        let stem = self
            .document_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("document")
            .to_string();
        if pages.len() == 1 {
            format!("{}_p{}.pdf", stem, pages[0] + 1)
        } else {
            format!(
                "{}_pages_{}-{}.pdf",
                stem,
                pages.first().map(|p| p + 1).unwrap_or(1),
                pages.last().map(|p| p + 1).unwrap_or(1)
            )
        }
    }
}
