mod queries;
mod records;
mod rows;
mod schema;

pub use queries::MySqlSyncReader;
pub use records::MySqlSyncWriter;
pub use rows::{
    AttachmentRow, AuthorRow, FeedItemRow, FeedRow, FolderRow, LiteratureRow, PublicationRow,
    TagRow,
};

use anyhow::{Result, anyhow};
use log::{debug, info};
use models::DatabaseConfig;
use mysql_async::{Pool, prelude::*};
use std::sync::{Arc, Mutex, RwLock};

pub struct MySqlManager {
    pub(crate) config: RwLock<DatabaseConfig>,
    pub(crate) pool: Arc<Mutex<Option<Pool>>>,
    retired_pools: Arc<Mutex<Vec<Pool>>>,
}

impl MySqlManager {
    #[must_use]
    pub fn new(config: DatabaseConfig) -> Self {
        Self {
            config: RwLock::new(config),
            pool: Arc::new(Mutex::new(None)),
            retired_pools: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn update_config(&self, config: DatabaseConfig) -> bool {
        info!(
            "MySQL: 更新配置 (host: {}, port: {})",
            config.host, config.port
        );
        {
            let mut w = self.config.write().unwrap();
            *w = config;
        }
        let mut p = self.pool.lock().unwrap();
        let old = p.take();
        drop(p);
        if let Some(old) = old {
            info!("MySQL: 配置已更新，连接池待断开");
            self.retired_pools.lock().unwrap().push(old);
            true
        } else {
            false
        }
    }

    /// 丢弃当前连接池，让下次操作建立新连接。
    ///
    /// 旧池留在数据库层，稍后由 [`Self::disconnect_retired_pools`] 异步关闭。
    pub fn reset_pool(&self) -> bool {
        let old = self.pool.lock().unwrap().take();
        if let Some(old) = old {
            info!("MySQL: 已丢弃当前连接池，等待异步关闭");
            self.retired_pools.lock().unwrap().push(old);
            true
        } else {
            false
        }
    }

    /// 异步关闭此前被配置更新或网络错误替换的连接池。
    pub async fn disconnect_retired_pools(&self) -> Result<()> {
        let pools = {
            let mut retired = self.retired_pools.lock().unwrap();
            std::mem::take(&mut *retired)
        };

        let mut first_error = None;
        for pool in pools {
            if let Err(error) = pool.disconnect().await {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }

        if let Some(error) = first_error {
            return Err(error.into());
        }
        Ok(())
    }

    pub fn get_config(&self) -> DatabaseConfig {
        self.config.read().unwrap().clone()
    }

    pub(crate) async fn get_pool(&self) -> Result<Pool> {
        let mut pool_lock = self.pool.lock().unwrap();
        if let Some(pool) = &*pool_lock {
            debug!("MySQL: 复用已有连接池");
            return Ok(pool.clone());
        }

        debug!("MySQL: 创建新连接池");
        let config = self.config.read().unwrap().clone();
        let pool_opts = mysql_async::PoolOpts::new()
            // 强制连接在固定寿命后重建，避免使用切换网络前残留的死连接，
            // 也能规避服务端 wait_timeout 踢掉空闲连接后复用导致的底层 I/O 错误。
            .with_abs_conn_ttl(Some(std::time::Duration::from_secs(300)));
        let opts = mysql_async::OptsBuilder::default()
            .ip_or_hostname(config.host)
            .tcp_port(config.port)
            .db_name(Some(&config.database))
            .user(Some(&config.username))
            .pass(Some(&config.password))
            .pool_opts(pool_opts)
            .ssl_opts(if config.use_ssl {
                Some(mysql_async::SslOpts::default())
            } else {
                None
            });
        let pool = Pool::new(opts);
        *pool_lock = Some(pool.clone());
        Ok(pool)
    }

    pub async fn test_connection(&self) -> Result<()> {
        let config = self.config.read().unwrap().clone();
        if config.host.is_empty() {
            return Err(anyhow!("主机名为空"));
        }
        info!("MySQL: 正在测试连接到 {}:{}", config.host, config.port);
        let pool = self.get_pool().await?;
        let mut conn = pool.get_conn().await?;
        let _: Option<i32> = conn.query_first("SELECT 1").await?;
        self.ensure_remote_tables().await?;
        Ok(())
    }

    pub async fn ensure_remote_tables(&self) -> Result<()> {
        let pool = self.get_pool().await?;
        let mut conn = pool.get_conn().await?;
        schema::ensure_remote_tables(&mut conn).await
    }

    pub async fn clear_all_data(&self) -> Result<()> {
        schema::clear_all_data(self).await
    }

    pub async fn purge_deleted_data(&self) -> Result<usize> {
        schema::purge_deleted_data(self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(host: &str) -> DatabaseConfig {
        DatabaseConfig {
            use_remote: true,
            host: host.to_string(),
            port: 3306,
            database: "lumen".to_string(),
            username: "user".to_string(),
            password: "password".to_string(),
            use_ssl: false,
        }
    }

    #[test]
    fn config_update_and_reset_retire_pools_inside_database() {
        let manager = MySqlManager::new(config("first.example"));
        let first_pool = Pool::new(mysql_async::OptsBuilder::default());
        *manager.pool.lock().unwrap() = Some(first_pool);

        assert!(manager.update_config(config("second.example")));
        assert_eq!(manager.get_config().host, "second.example");
        assert!(manager.pool.lock().unwrap().is_none());
        assert_eq!(manager.retired_pools.lock().unwrap().len(), 1);

        let second_pool = Pool::new(mysql_async::OptsBuilder::default());
        *manager.pool.lock().unwrap() = Some(second_pool);
        assert!(manager.reset_pool());
        assert!(manager.pool.lock().unwrap().is_none());
        assert_eq!(manager.retired_pools.lock().unwrap().len(), 2);
    }
}
