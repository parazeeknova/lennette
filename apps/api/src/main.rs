use anyhow::Context;
use lennette_server::{config::Config, migration::Migrator, state::AppState};
use sea_orm_migration::MigratorTrait;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let telemetry = lennette_server::telemetry::init()?;
    let state = AppState::connect(Config::from_env()?).await?;
    // Explicit migration command allows a single release job before replicas start.
    if std::env::args().any(|s| s == "migrate") {
        Migrator::up(&state.db, None).await?;
        if let Some(telemetry) = telemetry {
            telemetry.shutdown()?;
        }
        return Ok(());
    }
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
