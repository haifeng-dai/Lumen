pub mod rows;
pub mod schema;

use crate::DatabaseConfig;
use anyhow::{Result, anyhow};
use log::{debug, info};
use mysql_async::{Pool, prelude::*};
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;

pub struct MySqlManager {
    pub(crate) config: RwLock<DatabaseConfig>,
    pub(crate) pool: Arc<Mutex<Option<Pool>>>,
}

impl MySqlManager {
    #[must_use]
    pub fn new(config: DatabaseConfig) -> Self {
        Self {
            config: RwLock::new(config),
            pool: Arc::new(Mutex::new(None)),
        }
    }

    pub fn update_config(&self, config: DatabaseConfig) -> Option<Pool> {
        info!(
            "MySQL: 更新配置 (host: {}, port: {})",
            config.host, config.port
        );
        {
            let mut w = self.config.write().unwrap();
            *w = config;
        }
        let mut p = self.pool.blocking_lock();
        let old = p.take();
        drop(p);
        if old.is_some() {
            info!("MySQL: 配置已更新，连接池待断开");
        }
        old
    }

    pub fn get_config(&self) -> DatabaseConfig {
        self.config.read().unwrap().clone()
    }

    pub async fn get_pool(&self) -> Result<Pool> {
        let mut pool_lock = self.pool.lock().await;
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
        self.ensure_remote_tables(&mut conn).await?;
        crate::migration::remote::run_remote_migrations(&mut conn).await?;
        Ok(())
    }

    pub async fn ensure_remote_tables(&self, conn: &mut mysql_async::Conn) -> Result<()> {
        schema::ensure_remote_tables(conn).await
    }

    pub async fn clear_all_data(&self) -> Result<()> {
        schema::clear_all_data(self).await
    }

    pub async fn purge_deleted_data(&self) -> Result<usize> {
        schema::purge_deleted_data(self).await
    }
}
