use crate::{
    config::Config,
    error::{ApiError, Result},
};
use fred::prelude::*;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use secrecy::ExposeSecret;
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub struct AppState(pub Arc<Inner>);
pub struct Inner {
    pub config: Config,
    pub db: DatabaseConnection,
    pub redis: Client,
    pub http: reqwest::Client,
    pub shutdown: tokio_util::sync::CancellationToken,
    pub attachments: Option<Arc<dyn object_store::ObjectStore>>,
    pub upload_permits: Arc<tokio::sync::Semaphore>,
    pub events: tokio::sync::broadcast::Sender<uuid::Uuid>,
}
impl std::ops::Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}
impl AppState {
    pub async fn connect(config: Config) -> anyhow::Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut options = ConnectOptions::new(config.database_url.expose_secret());
        options
            .max_connections(20)
            .min_connections(1)
            .connect_timeout(Duration::from_secs(10))
            .sqlx_logging(false);
        let db = Database::connect(options).await?;
        let redis = Builder::from_config(fred::types::config::Config::from_url(
            config.redis_url.expose_secret(),
        )?)
        .build()?;
        redis.init().await?;
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(180))
            .pool_max_idle_per_host(8)
            .user_agent("Lennette-managed/0.1")
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let attachments = config
            .r2
            .as_ref()
            .map(crate::storage::connect)
            .transpose()?;
        Ok(Self(Arc::new(Inner {
            config,
            db,
            redis,
            http,
            shutdown: tokio_util::sync::CancellationToken::new(),
            attachments,
            upload_permits: Arc::new(tokio::sync::Semaphore::new(2)),
            events: tokio::sync::broadcast::channel(256).0,
        })))
    }
    pub async fn limit(&self, key: String, max: i64, seconds: i64) -> Result<()> {
        let count: i64 = self.redis.eval("local n=redis.call('INCR',KEYS[1]); if n==1 then redis.call('EXPIRE',KEYS[1],ARGV[1]) end; return n", vec![format!("rate:{key}")], vec![seconds]).await?;
        if count > max {
            Err(ApiError::RateLimited)
        } else {
            Ok(())
        }
    }
}
