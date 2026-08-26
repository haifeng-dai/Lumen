use anyhow::Result;
use i18n::{I18nKey, Language, t};
use log::info;
use models::{FeedType, FolderType};

use super::MainApp;

impl MainApp {
    pub fn refresh_all_data(&self) -> Result<()> {
        self.notify_data_changed();
        Ok(())
    }

    pub fn check_local_files(&self) -> Result<(usize, Vec<String>)> {
        self.attachment_service.check_local_files(&self.db)
    }

    pub fn clear_local_database(&self) -> Result<()> {
        info!("MainApp: 开始清空本地数据库...");
        self.db.rebuild_schema()?;
        let lang = self
            .config
            .lock()
            .unwrap()
            .ui
            .language
            .parse::<Language>()
            .unwrap_or_default();
        info!("MainApp: 重建默认文件夹和订阅源...");
        for f in [
            models::constructors::create_folder(
                "all",
                t(I18nKey::AllLiterature, lang),
                FolderType::All,
            ),
            models::constructors::create_folder(
                "uncategorized",
                t(I18nKey::Uncategorized, lang),
                FolderType::Uncategorized,
            ),
            models::constructors::create_folder(
                "trash",
                t(I18nKey::Trash, lang),
                FolderType::Trash,
            ),
        ] {
            let _ = self.db.insert_folder(&f);
        }
        for f in [
            models::constructors::create_feed(
                "all_subs",
                t(I18nKey::AllSubscription, lang),
                FeedType::Rss,
            ),
            models::constructors::create_feed("unread", t(I18nKey::Unread, lang), FeedType::Rss),
        ] {
            let _ = self.db.insert_feed(&f);
        }
        info!("MainApp: 本地数据库清空完成");
        Ok(())
    }

    pub async fn purge_deleted_data(&self) -> Result<usize> {
        let total = self.sync_service.purge_deleted_data().await?;
        self.notify_data_changed();
        self.notify_ui_changed();
        Ok(total)
    }
}
