use crate::feed::SubscriptionRefreshResult;
use crate::runtime::RUNTIME;
use anyhow::{Result, anyhow};
use log::{debug, error, info, warn};
use models::FeedType;
use models::constructors::*;
use parser::normalize::*;
use parser::text;
use std::sync::Arc;
use uuid::Uuid;

use super::MainApp;

impl MainApp {
    pub fn add_feed(self: Arc<Self>, name: String, url: String, interval: u32) -> Result<()> {
        let id = Uuid::new_v4().to_string();
        info!("MainApp: 添加订阅 name='{name}', url='{url}', interval={interval}h, id={id}");
        let mut feed = create_feed(id.clone(), name, FeedType::Rss);
        feed.url = Some(url);
        feed.update_interval = interval;
        self.op_notify(|| self.feed_service.save_feed(&self.db, feed))?;
        let feed_mgr = self.feed_service.clone();
        let db = self.db.clone();
        RUNTIME.spawn(async move {
            info!("MainApp: 启动新订阅的首次刷新 (id={id})");
            let _ = feed_mgr.refresh_feed(db, id).await;
        });
        self.notify_data_changed();
        Ok(())
    }

    pub fn update_feed(&self, id: String, name: String, url: String, interval: u32) -> Result<()> {
        info!("MainApp: 更新订阅 (id={id})");
        let mut feed = self.db.get_feed(&id)?.ok_or_else(|| {
            warn!("MainApp: 更新订阅失败，未找到 (id={id})");
            anyhow!("订阅不存在")
        })?;
        feed.name = name;
        feed.url = Some(url);
        feed.update_interval = interval;
        self.op_notify(|| self.feed_service.save_feed(&self.db, feed))
    }

    /// 立即刷新单个订阅（手动触发，不等待后台轮询周期）。
    ///
    /// 异步执行；发起后立即返回，UI 无需等待抓取完成。
    pub fn refresh_feed(&self, id: &str) -> Result<()> {
        info!("MainApp: 手动刷新订阅 (id={id})");
        if self.db.get_feed(id)?.is_none() {
            warn!("MainApp: 刷新订阅失败，未找到 (id={id})");
            return Err(anyhow!("订阅不存在"));
        }
        let feed_mgr = self.feed_service.clone();
        let db = self.db.clone();
        let id_owned = id.to_string();
        RUNTIME.spawn(async move {
            if let Err(e) = feed_mgr.refresh_feed(db, id_owned).await {
                error!("MainApp: 手动刷新订阅失败: {e}");
            }
        });
        Ok(())
    }

    /// 刷新所有真实订阅源（不含 all_subs / unread 虚拟节点）。
    ///
    /// 内部用 channel 收集每个订阅的刷新结果，返回 `Receiver` 供 UI 在 `cx.spawn`
    /// 中监听并逐条弹通知（成功/失败各一次，失败不中断其他订阅）。
    /// 整轮结束后经 `refresh_tx` 发 `DataChanged` 触发 UI 重绘（等价于 `notify_data_changed` 的刷新半部分）。
    pub fn refresh_all_subscriptions(
        &self,
    ) -> Result<tokio::sync::mpsc::Receiver<SubscriptionRefreshResult>> {
        info!("MainApp: 手动刷新所有订阅");
        let (tx, rx) = tokio::sync::mpsc::channel::<SubscriptionRefreshResult>(64);
        let on_result: Arc<dyn Fn(SubscriptionRefreshResult) + Send + Sync> = Arc::new(move |r| {
            // 通道满时丢弃最新结果，避免阻塞刷新循环
            let _ = tx.try_send(r);
        });
        let feed_mgr = self.feed_service.clone();
        let db = self.db.clone();
        let refresh_tx = self.refresh_tx.clone();
        RUNTIME.spawn(async move {
            let _ = feed_mgr.refresh_all(db, on_result).await;
            // 整轮结束 → 触发 UI 重绘（仅 DataChanged，不触发同步请求）
            if let Some(tx) = &*refresh_tx.lock().unwrap() {
                let _ = tx.send(crate::notify::RefreshMsg::DataChanged);
            }
        });
        Ok(rx)
    }

    pub fn delete_feed(&self, id: &str) -> Result<()> {
        info!("MainApp: 删除订阅 (id={id})");
        self.op_notify(|| self.feed_service.delete_feed(&self.db, id))
    }

    /// 删除指定的订阅条目集合
    pub fn delete_selected_feed_items(&self, ids: Vec<String>) -> Result<()> {
        debug!("MainApp: 批量删除订阅项 ({} 条)", ids.len());
        for id in ids {
            self.feed_service.delete_feed_item(&self.db, &id)?;
        }
        self.notify_data_changed();
        Ok(())
    }

    pub fn add_feed_item_to_library(&self, id: &str) -> Result<String> {
        let item = self.db.get_feed_item(id)?.ok_or_else(|| {
            warn!("MainApp: 添加订阅项到文献库失败，未找到 (id={id})");
            anyhow!("订阅项不存在")
        })?;
        if item.is_added_to_library {
            debug!("MainApp: 订阅项已添加过 (id={id}), 尝试查找已有文献");
            if let Some(lit) = self
                .db
                .get_all_literatures()?
                .iter()
                .find(|l| l.title == item.title)
                .cloned()
            {
                debug!("MainApp: 找到已有文献 id={}", lit.id);
                return Ok(lit.id);
            }
            warn!("MainApp: 订阅项已标记添加但未找到对应文献 (id={id})");
            return Err(anyhow!("文献已添加但未找到对应记录"));
        }
        info!(
            "MainApp: 从订阅项创建文献 (id={id}, title='{}')",
            item.title.chars().take(40).collect::<String>()
        );
        let lit_id = Uuid::new_v4().to_string();
        let mut lit = create_literature(
            lit_id.clone(),
            item.title.clone(),
            item.literature_type.clone(),
        );
        lit.authors = item.authors.clone();
        lit.year = item.year;
        lit.abstract_text = item.abstract_text.clone();
        lit.doi = item.doi.clone();
        lit.url = item.url.clone();
        sanitize_arxiv_identifiers(&mut lit);
        lit.volume = item.volume.clone();
        lit.issue = item.issue.clone();
        lit.pages = item.pages.clone();
        if let Some(ref j) = item.journal {
            let cleaned = text::clean_publication_name(j);
            if !cleaned.is_empty() {
                let pub_type = if item.literature_type == models::LiteratureType::Conference {
                    models::PublicationType::Conference
                } else {
                    models::PublicationType::Journal
                };
                lit.publication = Some(create_publication(cleaned, pub_type));
            }
        }
        self.op_notify(|| {
            self.literature_service.save_literature(
                self.db.clone(),
                self.data_changed_notify(),
                lit,
            )?;
            self.feed_service
                .update_feed_item_added_status(&self.db, id, true)
        })?;
        Ok(lit_id)
    }
}
