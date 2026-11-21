use crate::{
    auth::{self, Principal},
    error::{ApiError, Result},
    state::AppState,
};
use axum::{
    Extension,
    extract::State,
    http::HeaderMap,
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
};
use fred::prelude::*;
use secrecy::ExposeSecret;
use std::{convert::Infallible, time::Duration};
use tracing::{Instrument, Span};
use uuid::Uuid;

struct StreamLease {
    redis: Client,
    key: String,
    token: String,
    span: Span,
}
impl Drop for StreamLease {
    fn drop(&mut self) {
        let redis = self.redis.clone();
        let key = self.key.clone();
        let token = self.token.clone();
        // Response bodies are dropped on navigation/disconnect, without polling
        // the generator to completion. Release capacity on that path too.
        tokio::spawn(
            async move {
                if redis.zrem::<i64, _, _>(key, token).await.is_err() {
                    tracing::warn!(
                        event = "account.events.lease_release_failed",
                        dependency = "redis",
                        "Account event stream lease will expire automatically"
                    );
                }
            }
            .instrument(self.span.clone()),
        );
    }
}

pub fn start(state: AppState) {
    tokio::spawn(async move {
        loop {
            let result: anyhow::Result<()> = async {
                // LISTEN needs a session connection; transaction poolers can
                // leak notifications into unrelated requests and lose events.
                let url = state.config.database_url_unpooled.as_ref()
                    .unwrap_or(&state.config.database_url);
                let mut listener = sqlx::postgres::PgListener::connect(url.expose_secret()).await?;
                listener.listen("lennette_account_events").await?;
                state.events.send(Uuid::nil()).ok(); // Reconnect invalidates potentially missed snapshots.
                loop {
                    tokio::select! {
                        _ = state.shutdown.cancelled() => return Ok(()),
                        message = listener.recv() => {
                            if let Ok(id) = message?.payload().parse() { state.events.send(id).ok(); }
                        }
                    }
                }
            }.await;
            if state.shutdown.is_cancelled() {
                break;
            }
            if result.is_err() {
                tracing::warn!(
                    event = "account.events.listener_failed",
                    dependency = "database",
                    "Account event listener disconnected; reconnecting"
                );
            }
            tokio::select! { _ = state.shutdown.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
        }
    });
}

pub async fn stream(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = std::result::Result<Event, Infallible>>>> {
    // Three tabs per browser session, with expiring cross-replica leases.
    let key = format!("dashboard-streams:{}", p.session_id);
    let lease = auth::random_token();
    let now = chrono::Utc::now().timestamp();
    let acquired: i64 = state.redis.eval("redis.call('ZREMRANGEBYSCORE',KEYS[1],'-inf',ARGV[1]); if redis.call('ZCARD',KEYS[1])>=3 then return 0 end; redis.call('ZADD',KEYS[1],ARGV[2],ARGV[3]); redis.call('EXPIRE',KEYS[1],45); return 1", vec![key.clone()], vec![now.to_string(),(now+45).to_string(),lease.clone()]).await?;
    if acquired == 0 {
        return Err(ApiError::RateLimited);
    }
    let span = Span::current();
    let guard = StreamLease {
        redis: state.redis.clone(),
        key: key.clone(),
        token: lease.clone(),
        span: span.clone(),
    };
    let mut receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        let _guard = guard;
        yield Ok(Event::default().event("changed").data("connected"));
        let mut timer = tokio::time::interval(Duration::from_secs(15));
        loop {
            tokio::select! {
                _ = state.shutdown.cancelled() => break,
                _ = timer.tick() => {
                    if let Err(error) = auth::browser::authenticate_cookie(&state,&headers,false).await {
                        if !matches!(error, ApiError::Unauthorized) {
                            tracing::warn!(event = "account.events.auth_refresh_failed", session_id = %p.session_id, reason = error.diagnostic_code(), "Cannot revalidate account event stream session");
                        }
                        yield Ok(Event::default().event("expired").data("sign_in_required")); break;
                    }
                    let renewed: std::result::Result<i64,_> = state.redis.eval("if redis.call('ZSCORE',KEYS[1],ARGV[1]) then redis.call('ZADD',KEYS[1],ARGV[2],ARGV[1]); redis.call('EXPIRE',KEYS[1],45); return 1 else return 0 end",vec![key.clone()],vec![lease.clone(),(chrono::Utc::now().timestamp()+45).to_string()]).await;
                    match renewed {
                        Ok(1) => {}
                        Ok(_) => break,
                        Err(_) => {
                            tracing::warn!(event = "account.events.lease_renewal_failed", session_id = %p.session_id, dependency = "redis", "Could not renew account event stream lease");
                            break;
                        }
                    }
                }
                event = receiver.recv() => {
                    match event {
                        Ok(id) if id == p.account_id || id.is_nil() => yield Ok(Event::default().event("changed").data("account")),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => yield Ok(Event::default().event("changed").data("resync")),
                        Err(_) => break,
                        _ => {}
                    }
                }
            }
        }
    };
    Ok(Sse::new(crate::telemetry::instrument_stream(stream, span))
        .keep_alive(KeepAlive::default().interval(Duration::from_secs(10))))
}
