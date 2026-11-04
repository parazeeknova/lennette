use lennette_server::{config::Config, migration::Migrator, state::AppState};
use sea_orm_migration::MigratorTrait;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let telemetry = lennette_server::telemetry::init()?;
    let state = AppState::connect(Config::from_env()?).await?;
    // Explicit migration command allows a single release job before replicas start.
    if std::env::args().any(|s| s == "migrate") {
        Migrator::up(&state.db, None).await?;
        return Ok(());
    }
    let listener = tokio::net::TcpListener::bind(state.config.bind).await?;
    tracing::info!(address = %listener.local_addr()?, "API listening");
    let shutdown = state.shutdown.clone();
    lennette_server::storage::cleanup::start(state.clone());
    lennette_server::http::events::start(state.clone());
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
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=terminate=>{}}
        shutdown.cancel();
    })
    .await?;
    if let Some(provider) = telemetry {
        provider.shutdown()?;
    }
    Ok(())
}
