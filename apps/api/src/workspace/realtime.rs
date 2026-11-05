//! Browser companion transport: shared Redis connection, bounded fanout, no URL credentials.
use crate::{
    auth::{self, Principal},
    entities::accounts,
    error::{ApiError, Result},
    state::AppState,
};
use axum::{
    Extension,
    extract::{
        Query, State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::Response,
};
use fred::prelude::*;
use futures_util::StreamExt;
use sea_orm::EntityTrait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, broadcast};
use uuid::Uuid;

pub(crate) fn keys(account: Uuid) -> Vec<String> {
    vec![
        format!("live-events:{{{account}}}"),
        format!("live-state:{{{account}}}"),
    ]
}

const MAX_FRAME_BYTES: usize = 32 * 1024;

/// A bounded, ordered worker keeps Redis delays out of provider output loops.
#[derive(Clone)]
pub struct Publisher(tokio::sync::mpsc::Sender<(Uuid, String)>);
impl Publisher {
    pub fn new(redis: Client, shutdown: tokio_util::sync::CancellationToken) -> Self {
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<(Uuid, String)>(1024);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    message = receiver.recv() => {
                        let Some((account, raw)) = message else { break };
                        let result = tokio::time::timeout(Duration::from_secs(2), redis.eval::<String, _, _, _>(
                            super::redis::PUBLISH, keys(account), vec![raw, format!("live:{account}")],
                        )).await;
                        if !matches!(result, Ok(Ok(_))) {
                            tracing::warn!("live relay unavailable; durable sync will recover saved content");
                        }
                    }
                }
            }
        });
        Self(sender)
    }
}

fn frames(p: &Principal, meeting: Option<Uuid>, event: Value) -> Vec<String> {
    let mut frame = json!({"type":"live","meeting_id":meeting,"device_id":p.device_id,"event":event,"emitted_at":chrono::Utc::now().timestamp_millis()});
    let raw = frame.to_string();
    if raw.len() <= MAX_FRAME_BYTES {
        return vec![raw];
    }
    if frame["event"]["type"] != "ai_delta" {
        return Vec::new();
    }
    let Some(delta) = frame["event"]["delta"].as_str().map(str::to_owned) else {
        return Vec::new();
    };
    let offset = frame["event"]["offset"].as_u64().unwrap_or(0);
    let mut result = Vec::new();
    let mut start = 0;
    while start < delta.len() {
        // A JSON-escaped byte takes at most six bytes. Reserve ample envelope space.
        let mut end = (start + 4096).min(delta.len());
        while !delta.is_char_boundary(end) {
            end -= 1;
        }
        frame["event"]["delta"] = json!(&delta[start..end]);
        frame["event"]["offset"] = json!(offset + start as u64);
        let raw = frame.to_string();
        if raw.len() > MAX_FRAME_BYTES {
            return Vec::new();
        }
        result.push(raw);
        start = end;
    }
    result
}

pub async fn publish(state: &AppState, p: &Principal, meeting: Option<Uuid>, event: Value) {
    for raw in frames(p, meeting, event) {
        if state
            .workspace_publisher
            .0
            .try_send((p.account_id, raw))
            .is_err()
        {
            tracing::warn!("live relay queue full; durable sync will recover saved content");
            break;
        }
    }
}

type Channel = broadcast::Sender<Arc<str>>;
pub struct Hub {
    subscriber: Client,
    accounts: Mutex<HashMap<Uuid, Weak<Channel>>>,
}
impl Hub {
    async fn new(
        subscriber: Client,
        shutdown: tokio_util::sync::CancellationToken,
    ) -> Result<Arc<Self>> {
        let mut messages = subscriber.message_rx();
        subscriber.init().await?;
        let mut reconnects = subscriber.reconnect_rx();
        let hub = Arc::new(Self {
            subscriber,
            accounts: Mutex::new(HashMap::new()),
        });
        let reader = hub.clone();
        tokio::spawn(async move {
            let mut cleanup = tokio::time::interval(Duration::from_secs(15));
            loop {
                tokio::select! {
                    _=shutdown.cancelled()=>break,
                    _=cleanup.tick()=>{
                        let mut accounts=reader.accounts.lock().await;
                        let dead=accounts.iter().filter(|(_,tx)|tx.strong_count()==0).map(|(id,_)|*id).collect::<Vec<_>>();
                        for account in dead {
                            accounts.remove(&account);
                            let _=reader.subscriber.unsubscribe(vec![format!("sync:{account}"),format!("live:{account}")]).await;
                        }
                    },
                    _=reconnects.recv()=>{
                        let accounts=reader.accounts.lock().await;
                        let channels=accounts.keys().flat_map(|id|[format!("sync:{id}"),format!("live:{id}")]).collect::<Vec<_>>();
                        if !channels.is_empty() { let _=reader.subscriber.subscribe(channels).await; }
                        for tx in accounts.values().filter_map(Weak::upgrade) { let _=tx.send(Arc::from(r#"{"type":"resync"}"#)); }
                    },
                    event=messages.recv()=>{
                        match event {
                            Ok(message)=>{
                                let Some((kind,id))=message.channel.split_once(':') else {continue};
                                let Ok(account)=id.parse::<Uuid>() else {continue};
                                let tx=reader.accounts.lock().await.get(&account).and_then(Weak::upgrade);
                                if let Some(tx)=tx {
                                    if kind=="sync" { let _=tx.send(Arc::from(r#"{"type":"changed"}"#)); }
                                    else if kind=="live" && let Ok(raw)=message.value.convert::<String>() && raw.len()<=64*1024 { let _=tx.send(Arc::from(raw)); }
                                }
                            },
                            Err(broadcast::error::RecvError::Lagged(_))=>{
                                for tx in reader.accounts.lock().await.values().filter_map(Weak::upgrade) { let _=tx.send(Arc::from(r#"{"type":"resync"}"#)); }
                            },
                            Err(_)=>break,
                        }
                    }
                }
            }
            reader.subscriber.quit().await.ok();
        });
        Ok(hub)
    }
    async fn subscribe(&self, account: Uuid) -> Result<Arc<Channel>> {
        // Serialize subscribe/unsubscribe so a disconnect cannot remove a new viewer's channels.
        let mut accounts = self.accounts.lock().await;
        if let Some(tx) = accounts.get(&account).and_then(Weak::upgrade) {
            return Ok(tx);
        }
        let tx = Arc::new(broadcast::channel(32).0);
        self.subscriber
            .subscribe(vec![format!("sync:{account}"), format!("live:{account}")])
            .await?;
        accounts.insert(account, Arc::downgrade(&tx));
        Ok(tx)
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SocketQuery {
    pub live_after: Option<String>,
    pub account: Option<Uuid>,
}
fn valid_cursor(cursor: &str) -> bool {
    cursor.len() <= 42
        && cursor.split_once('-').is_some_and(|(a, b)| {
            !a.is_empty()
                && !b.is_empty()
                && a.bytes().all(|c| c.is_ascii_digit())
                && b.bytes().all(|c| c.is_ascii_digit())
                && a.parse::<u64>().is_ok()
                && b.parse::<u64>().is_ok()
        })
}

struct Lease {
    state: AppState,
    key: String,
    token: String,
    _permit: OwnedSemaphorePermit,
}
impl Drop for Lease {
    fn drop(&mut self) {
        let (redis, key, token) = (
            self.state.redis.clone(),
            self.key.clone(),
            self.token.clone(),
        );
        tokio::spawn(async move {
            let _: std::result::Result<i64, _> = redis.zrem(key, token).await;
        });
    }
}
pub async fn socket(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Query(q): Query<SocketQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response> {
    auth::browser::origin(&state, &headers)?;
    if q.account.is_some_and(|id| id != p.account_id) {
        return Err(ApiError::Unauthorized);
    }
    let browser = auth::browser::authenticate_cookie(&state, &headers, false).await?;
    if browser.session_id != p.session_id {
        return Err(ApiError::Unauthorized);
    }
    let after = q.live_after.unwrap_or_else(|| "0-0".into());
    if !valid_cursor(&after) {
        return Err(ApiError::BadRequest("invalid live cursor".into()));
    }
    state
        .limit(format!("workspace-connect:{}", p.session_id), 20, 60)
        .await?;
    let permit = state
        .workspace_permits
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::RateLimited)?;
    let hub = state
        .workspace_hub
        .get_or_try_init(|| Hub::new(state.redis.clone_new(), state.shutdown.clone()))
        .await?;
    let tx = hub.subscribe(p.account_id).await?;
    let receiver = tx.subscribe();
    let key = format!("workspace-streams:{}", p.account_id);
    let token = format!("{}:{}", p.session_id, auth::random_token());
    let now = chrono::Utc::now().timestamp();
    let acquired:i64=state.redis.eval("redis.call('ZREMRANGEBYSCORE',KEYS[1],'-inf',ARGV[1]); if redis.call('ZCARD',KEYS[1])>=8 then return 0 end; redis.call('ZADD',KEYS[1],ARGV[2],ARGV[3]); redis.call('EXPIRE',KEYS[1],45); return 1",vec![key.clone()],vec![now.to_string(),(now+45).to_string(),token.clone()]).await?;
    if acquired == 0 {
        return Err(ApiError::RateLimited);
    }
    let lease = Lease {
        state: state.clone(),
        key,
        token,
        _permit: permit,
    };
    Ok(ws
        .read_buffer_size(4096)
        .write_buffer_size(0)
        .max_write_buffer_size(1024 * 1024)
        .max_message_size(4096)
        .max_frame_size(4096)
        .on_upgrade(move |ws| async move {
            let _subscription = tx;
            if stream(&state, &p, &headers, ws, receiver, &after, &lease)
                .await
                .is_err()
            {
                tracing::debug!("workspace socket ended; client can reconnect");
            }
        }))
}
async fn send(ws: &mut WebSocket, message: Message) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(3), ws.send(message))
        .await
        .map_err(anyhow::Error::from)?
        .map_err(anyhow::Error::from)?;
    Ok(())
}
async fn sequence(state: &AppState, account: Uuid) -> Result<i64> {
    Ok(accounts::Entity::find_by_id(account)
        .one(&state.db)
        .await?
        .ok_or(ApiError::Unauthorized)?
        .sync_sequence)
}
#[allow(clippy::too_many_arguments)]
async fn stream(
    state: &AppState,
    p: &Principal,
    headers: &HeaderMap,
    mut ws: WebSocket,
    mut receiver: broadcast::Receiver<Arc<str>>,
    after: &str,
    lease: &Lease,
) -> Result<()> {
    let mut revision = sequence(state, p.account_id).await?;
    let replay: String = state
        .redis
        .eval(super::redis::REPLAY, keys(p.account_id), vec![after])
        .await?;
    send(&mut ws, Message::Text(replay.into())).await?;
    send(
        &mut ws,
        Message::Text(
            json!({"type":"ready","server_time":chrono::Utc::now().timestamp_millis()})
                .to_string()
                .into(),
        ),
    )
    .await?;
    let mut timer = tokio::time::interval(Duration::from_secs(10));
    let mut last_pong = tokio::time::Instant::now();
    let mut incoming_count = 0u16;
    let mut incoming_window = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _=state.shutdown.cancelled()=>break,
            _=timer.tick()=>{
                if last_pong.elapsed()>Duration::from_secs(35) { break; }
                match auth::browser::authenticate_cookie(state,headers,false).await {
                    Ok(current) if current.session_id==p.session_id=>{},
                    Ok(_)|Err(ApiError::Unauthorized)=>{send(&mut ws,Message::Text(r#"{"type":"session_revoked"}"#.into())).await?; break;}
                    Err(error)=>return Err(error),
                }
                let now=chrono::Utc::now().timestamp();
                let renewed:i64=state.redis.eval("if redis.call('ZSCORE',KEYS[1],ARGV[1]) then redis.call('ZADD',KEYS[1],ARGV[2],ARGV[1]); redis.call('EXPIRE',KEYS[1],45); return 1 else return 0 end",vec![lease.key.clone()],vec![lease.token.clone(),(now+45).to_string()]).await?;
                if renewed!=1 { break; }
                let current=sequence(state,p.account_id).await?;
                if current!=revision {revision=current;send(&mut ws,Message::Text(r#"{"type":"changed"}"#.into())).await?;}
                send(&mut ws,Message::Ping(axum::body::Bytes::new())).await?;
            },
            event=receiver.recv()=>match event {
                Ok(text)=>{send(&mut ws,Message::Text(text.to_string().into())).await?; if text.as_ref() == r#"{"type":"resync"}"# {break;}}
                Err(_)=>{send(&mut ws,Message::Text(r#"{"type":"resync"}"#.into())).await?;break;}
            },
            incoming=ws.next()=>{
                if incoming_window.elapsed()>Duration::from_secs(60) {incoming_count=0;incoming_window=tokio::time::Instant::now();}
                incoming_count+=1;if incoming_count>120 {break;}
                match incoming {
                    Some(Ok(Message::Pong(_)))=>last_pong=tokio::time::Instant::now(),
                    Some(Ok(Message::Ping(data)))=>{send(&mut ws,Message::Pong(data)).await?;},
                    _=>break, // Read-only transport: reject client data frames.
                }
            }
        }
    }
    send(
        &mut ws,
        Message::Close(Some(CloseFrame {
            code: 1000,
            reason: "reconnect or sign in again".into(),
        })),
    )
    .await
    .ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_deltas_keep_utf8_json_limits_and_byte_offsets() {
        let p = Principal {
            account_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
        };
        let delta = "é🌍\n\"\u{0000}".repeat(10000);
        let raw = frames(
            &p,
            Some(Uuid::new_v4()),
            json!({"type":"ai_delta","stream_id":Uuid::new_v4(),"offset":17,"delta":delta}),
        );
        assert!(raw.len() > 1);
        let mut restored = String::new();
        for raw in raw {
            assert!(raw.len() <= MAX_FRAME_BYTES);
            let frame: Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(frame["event"]["offset"], 17 + restored.len());
            restored.push_str(frame["event"]["delta"].as_str().unwrap());
        }
        assert_eq!(restored, delta);
    }

    #[test]
    fn cursors_are_bounded_redis_stream_ids() {
        for value in ["0-0", "123456789-42"] {
            assert!(valid_cursor(value));
        }
        for value in ["", "-1", "1-", "a-b", "1-2-3", "(1-0", "1\n-2"] {
            assert!(!valid_cursor(value));
        }
        assert!(!valid_cursor(&"1".repeat(100)));
    }
}
