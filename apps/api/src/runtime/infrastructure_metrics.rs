//! Low-cardinality infrastructure and cleanup gauges; no user data or SQL text exported.
use crate::state::AppState;
use fred::prelude::*;
use sea_orm::ConnectionTrait;

pub async fn sample(state: &AppState) {
    let meter = opentelemetry::global::meter("lennette-infrastructure");
    let connections = meter.u64_gauge("postgresql.backends").build();
    let size = meter
        .u64_gauge("postgresql.db_size")
        .with_unit("By")
        .build();
    let clients = meter.u64_gauge("redis.clients.connected").build();
    let memory = meter.u64_gauge("redis.memory.used").with_unit("By").build();
    let pending = meter.u64_gauge("lennette.storage.cleanup.pending").build();
    let oldest = meter
        .u64_gauge("lennette.storage.cleanup.oldest")
        .with_unit("s")
        .build();
    let result=state.db.query_one_raw(sea_orm::Statement::from_string(sea_orm::DbBackend::Postgres,
    "SELECT (SELECT count(*) FROM pg_stat_activity WHERE datname=current_database())::bigint AS connections,pg_database_size(current_database())::bigint AS bytes,(SELECT count(*) FROM object_uploads)::bigint AS pending,(SELECT coalesce(greatest(0,extract(epoch from now()-min(created_at))),0)::bigint FROM object_uploads) AS oldest")).await;
    match result {
        Ok(Some(row)) => {
            for (field, gauge) in [
                ("connections", &connections),
                ("bytes", &size),
                ("pending", &pending),
                ("oldest", &oldest),
            ] {
                if let Ok(value) = row.try_get::<i64>("", field) {
                    gauge.record(value.max(0) as u64, &[]);
                }
            }
        }
        _ => tracing::warn!("PostgreSQL monitoring sample failed; retrying next interval"),
    }
    match state.redis.info::<String>(None).await {
        Ok(info) => {
            for line in info.lines() {
                if let Some((key, value)) = line.split_once(':')
                    && let Ok(value) = value.trim().parse::<u64>()
                {
                    match key {
                        "connected_clients" => clients.record(value, &[]),
                        "used_memory" => memory.record(value, &[]),
                        _ => {}
                    }
                }
            }
        }
        Err(_) => tracing::warn!("Redis monitoring sample failed; retrying next interval"),
    }
}

pub fn start(state: AppState) {
    if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_none() {
        return;
    }
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! { _=state.shutdown.cancelled()=>break,_=timer.tick()=>{} }
            sample(&state).await;
        }
    });
}
