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
use uuid::Uuid;

pub fn start(state: AppState) {
    tokio::spawn(async move {
        loop {
            let result: anyhow::Result<()> = async {
                let mut listener = sqlx::postgres::PgListener::connect(state.config.database_url.expose_secret()).await?;
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
                tracing::warn!("account event listener disconnected; reconnecting");
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
    // A per-session lease prevents unbounded long-lived HTTP connections.
    let key = format!("dashboard-stream:{}", p.session_id);
    let lease = auth::random_token();
    let acquired: Option<String> = state
        .redis
        .set(
            &key,
            &lease,
            Some(Expiration::EX(45)),
            Some(SetOptions::NX),
            false,
        )
        .await?;
    if acquired.is_none() {
        return Err(ApiError::RateLimited);
    }
    let mut receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        yield Ok(Event::default().event("changed").data("connected"));
        let mut timer = tokio::time::interval(Duration::from_secs(15));
        loop {
            tokio::select! {
                _ = state.shutdown.cancelled() => break,
                _ = timer.tick() => {
                    if auth::browser::authenticate_cookie(&state,&headers,false).await.is_err() {
                        yield Ok(Event::default().event("expired").data("sign_in_required")); break;
                    }
                    let renewed: std::result::Result<i64,_> = state.redis.eval("if redis.call('GET',KEYS[1])==ARGV[1] then return redis.call('EXPIRE',KEYS[1],45) else return 0 end",vec![key.clone()],vec![lease.clone()]).await;
                    if !matches!(renewed,Ok(1)) { break; }
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
        let _: std::result::Result<i64,_> = state.redis.eval("if redis.call('GET',KEYS[1])==ARGV[1] then return redis.call('DEL',KEYS[1]) else return 0 end",vec![key],vec![lease]).await;
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default().interval(Duration::from_secs(10))))
}
