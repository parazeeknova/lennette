use anyhow::Context;
use lennette_server::{config::Config, migration::Migrator, state::AppState};
use sea_orm::ConnectionTrait;
use sea_orm_migration::MigratorTrait;
use secrecy::ExposeSecret;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let telemetry = lennette_server::telemetry::init()?;
    let config = Config::from_env()?;
    let migrate_only = std::env::args().any(|s| s == "migrate");
    // A single production replica migrates before it becomes ready. A separate
    // release job can still use `migrate` without needing Redis to be available.
    if migrate_only || std::env::args().any(|s| s == "--migrate") {
        let url = config
            .database_url_unpooled
            .as_ref()
            .unwrap_or(&config.database_url);
        let mut options = sea_orm::ConnectOptions::new(url.expose_secret());
        options
            .max_connections(1)
            .min_connections(1)
            .connect_timeout(std::time::Duration::from_secs(30))
            .sqlx_logging(false);
        let db = sea_orm::Database::connect(options).await?;
        let result = async {
            // This dedicated one-connection pool keeps the session lock through
            // pending-migration discovery and every migration transaction.
            db.execute_unprepared("SELECT pg_advisory_lock(7354221, 0)")
                .await?;
            Migrator::up(&db, None).await
        }
        .await;
        // Closing the session releases the lock on success and on failure.
        let closed = db.close().await;
        result?;
        closed?;
        tracing::info!("database migrations complete");
    }
    if migrate_only {
        if let Some(telemetry) = telemetry {
            telemetry.shutdown()?;
        }
        return Ok(());
    }
    let state = AppState::connect(config).await?;
    let listener = tokio::net::TcpListener::bind(state.config.bind)
        .await
        .with_context(|| format!("Cannot bind API to {}. Stop the previous API/dev process or configure a different API_BIND.", state.config.bind))?;
    tracing::info!(address = %listener.local_addr()?, "API listening");
    let shutdown = state.shutdown.clone();
    lennette_server::storage::cleanup::start(state.clone());
    lennette_server::http::events::start(state.clone());
    lennette_server::runtime::infrastructure_metrics::start(state.clone());
    axum::serve(
        listener,
        lennette_server::routes::router(state)
            .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        #[cfg(unix)]
        let terminate = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler")
                .recv()
                .await;
        };
        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();
        let lease = async {
            let Ok(path) = std::env::var("LENNETTE_DEV_LEASE") else {
                return std::future::pending::<()>().await;
            };
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let alive = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age.as_secs() < 8);
                if !alive {
                    break;
                }
            }
        };
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=terminate=>{},_=lease=>{}}
        shutdown.cancel();
    })
    .await?;
    if let Some(provider) = telemetry
        && let Err(error) = provider.shutdown()
    {
        tracing::warn!(%error,"telemetry flush incomplete during shutdown");
    }
    Ok(())
}
