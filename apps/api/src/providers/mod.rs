mod batch;
mod chat;
use crate::{
    auth::{self, Principal},
    billing,
    error::{ApiError, Result},
    plans,
    state::AppState,
};
use axum::{
    Extension, Json,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::header,
    response::{IntoResponse, Response},
};
pub use batch::transcribe;
use fred::prelude::*;
use futures_util::{SinkExt, StreamExt};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{Message as UpstreamMessage, client::IntoClientRequest};
use tracing::Instrument;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatMessage {
    pub role: String,
    pub content: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatRequest {
    pub conversation_id: Option<Uuid>,
    pub relay_live: Option<bool>,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: Option<bool>,
    pub max_tokens: Option<i64>,
    pub meeting_id: Option<Uuid>,
    pub reasoning_effort: Option<String>,
}
fn validate_messages(messages: &[ChatMessage]) -> Result<i64> {
    if messages.is_empty() || messages.len() > 100 {
        return Err(ApiError::BadRequest("send 1 to 100 messages".into()));
    }
    let mut budget = 0i64;
    for m in messages {
        if !matches!(m.role.as_str(), "system" | "user" | "assistant") {
            return Err(ApiError::BadRequest("unsupported role".into()));
        }
        match &m.content {
            Value::String(text) => budget += text.len() as i64 + 16,
            Value::Array(parts) => {
                if parts.len() > 9 {
                    return Err(ApiError::BadRequest("too many content parts".into()));
                }
                for part in parts {
                    match part["type"].as_str() {
                        Some("text") => {
                            budget += part["text"]
                                .as_str()
                                .ok_or_else(|| ApiError::BadRequest("invalid text".into()))?
                                .len() as i64
                                + 16
                        }
                        Some("image_url") => {
                            let image = part["image_url"]["url"]
                                .as_str()
                                .ok_or_else(|| ApiError::BadRequest("invalid image".into()))?;
                            // No remote URLs: prevent provider-side URL fetches/SSRF. Fixed small images only.
                            let encoded = image
                                .strip_prefix("data:image/png;base64,")
                                .or_else(|| image.strip_prefix("data:image/jpeg;base64,"))
                                .ok_or_else(|| {
                                    ApiError::BadRequest("inline PNG/JPEG images only".into())
                                })?;
                            use base64::Engine;
                            let bytes = base64::engine::general_purpose::STANDARD
                                .decode(encoded)
                                .map_err(|_| {
                                ApiError::BadRequest("invalid image encoding".into())
                            })?;
                            if bytes.len() > 512 * 1024 || !valid_image_dimensions(&bytes) {
                                return Err(ApiError::BadRequest(
                                    "image must be at most 1024×1024 and 512 KiB".into(),
                                ));
                            }
                            budget += 16000;
                        }
                        _ => return Err(ApiError::BadRequest("unsupported content part".into())),
                    }
                }
            }
            _ => return Err(ApiError::BadRequest("invalid message content".into())),
        }
    }
    if budget > 96000 {
        return Err(ApiError::BadRequest(
            "context exceeds managed request limit".into(),
        ));
    }
    Ok(budget)
}
fn valid_image_dimensions(bytes: &[u8]) -> bool {
    // PNG header dimensions. JPEG is supported after decoding through image in a future API revision.
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return false;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    width > 0 && height > 0 && width <= 1024 && height <= 1024
}
async fn own_meeting(state: &AppState, p: &Principal, id: Option<Uuid>) -> Result<()> {
    if let Some(id) = id {
        use sea_orm::EntityTrait;
        let row = crate::entities::records::Entity::find_by_id((p.account_id, id))
            .one(&state.db)
            .await?
            .ok_or(ApiError::NotFound)?;
        if row.deleted || row.kind != "meeting" {
            return Err(ApiError::NotFound);
        }
    }
    Ok(())
}
pub async fn live(state: &AppState, p: &Principal, meeting: Option<Uuid>, event: Value) {
    crate::workspace::realtime::publish(state, p, meeting, event).await;
}

fn usage_cost(model: plans::Model, usage: &Value) -> Option<i64> {
    let input = usage["prompt_tokens"].as_i64().filter(|n| *n >= 0)?;
    let completion = usage["completion_tokens"].as_i64().filter(|n| *n >= 0)?;
    // Gemini can omit thinking tokens from completion_tokens while including
    // them in total_tokens. Bill all generated tokens without double counting.
    let output = usage["total_tokens"]
        .as_i64()
        .filter(|total| *total >= input)
        .map_or(completion, |total| completion.max(total - input));
    let cached = usage["prompt_tokens_details"]["cached_tokens"]
        .as_i64()
        .unwrap_or(0)
        .max(usage["prompt_cache_hit_tokens"].as_i64().unwrap_or(0));
    Some(model.cost_with_cached(input, output, cached))
}

pub use chat::chat;

#[derive(Deserialize)]
pub struct SpeechQuery {
    pub meeting_id: Uuid,
    #[serde(default)]
    pub offset_seconds: f64,
    /// Older clients keep live web relay; low latency clients explicitly opt out.
    #[serde(default = "default_live_relay")]
    pub relay_live: bool,
}
fn default_live_relay() -> bool {
    true
}

pub async fn speech(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Query(q): Query<SpeechQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response> {
    if !q.offset_seconds.is_finite() || !(0.0..=604800.0).contains(&q.offset_seconds) {
        return Err(ApiError::BadRequest("invalid meeting offset".into()));
    }
    if state.config.deepgram_key.expose_secret().is_empty() {
        return Err(ApiError::Unavailable);
    }
    // A non-relayed stream may refer to an unsynced local meeting. It never
    // reads or writes that meeting or publishes its id to the workspace.
    if q.relay_live {
        own_meeting(&state, &p, Some(q.meeting_id)).await?;
    }
    let key = format!("speech:{}", p.account_id);
    let lease = auth::random_token();
    let acquired: Option<String> = state
        .redis
        .set(
            &key,
            &lease,
            Some(Expiration::EX(90)),
            Some(SetOptions::NX),
            false,
        )
        .await?;
    if acquired.is_none() {
        return Err(ApiError::RateLimited);
    }
    let reservation = match billing::reserve(&state, p.account_id, "speech", 30).await {
        Ok(r) => r,
        Err(e) => {
            let _: std::result::Result<(), _> = state.redis.del(&key).await;
            return Err(e);
        }
    };
    let mut url = state.config.deepgram_url.clone();
    url.query_pairs_mut().extend_pairs([
        ("model", "nova-3"),
        ("language", "multi"),
        ("encoding", "linear16"),
        ("sample_rate", "16000"),
        ("channels", "2"),
        ("multichannel", "true"),
        ("diarize", "true"),
        ("interim_results", "true"),
        ("smart_format", "true"),
        ("punctuate", "true"),
        ("vad_events", "true"),
        ("endpointing", "300"),
        ("utterance_end_ms", "1000"),
        ("mip_opt_out", "true"),
    ]);
    let mut upstream_request = url
        .as_str()
        .into_client_request()
        .map_err(anyhow::Error::from)?;
    upstream_request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Token {}", state.config.deepgram_key.expose_secret())
            .parse()
            .map_err(anyhow::Error::from)?,
    );
    let upstream = tokio_tungstenite::connect_async(upstream_request)
        .instrument(tracing::info_span!(
            "deepgram.connect",
            otel.kind = "client",
            server.address = state.config.deepgram_url.host_str().unwrap_or("deepgram")
        ))
        .await;
    let (upstream, _) = match upstream {
        Ok(u) => u,
        Err(error) => {
            let status = match &error {
                tokio_tungstenite::tungstenite::Error::Http(response) => {
                    Some(response.status().as_u16())
                }
                _ => None,
            };
            tracing::warn!(status, "Managed speech upstream connection failed");
            billing::settle(&state, reservation, 0).await?;
            let _: std::result::Result<(), _> = state.redis.del(&key).await;
            return Err(ApiError::Unavailable);
        }
    };
    let relay_header = if q.relay_live { "true" } else { "false" };
    let mut response = ws.max_message_size(64*1024).on_upgrade(move |ws|async move{
        if q.relay_live { live(&state,&p,Some(q.meeting_id),json!({"type":"meeting_active"})).await; }
        if let Err(error)=pipe_speech(&state,&p,q.meeting_id,q.offset_seconds,q.relay_live,ws,upstream,reservation,&key,&lease).await{tracing::debug!(%error,"speech stream ended")}
        if q.relay_live { live(&state,&p,Some(q.meeting_id),json!({"type":"meeting_ended"})).await; }
        let _:std::result::Result<i64,_>=state.redis.eval("if redis.call('GET',KEYS[1])==ARGV[1] then return redis.call('DEL',KEYS[1]) else return 0 end",vec![key],vec![lease]).await;
    });
    response.headers_mut().insert(
        "x-lennette-relay-live",
        axum::http::HeaderValue::from_static(relay_header),
    );
    Ok(response)
}
/// Only Deepgram's documented client controls pass through the managed proxy.
/// Canonicalize them rather than forwarding arbitrary client JSON upstream.
fn speech_control(text: &str) -> Option<&'static str> {
    if text.len() > 128 {
        return None;
    }
    let value: Value = serde_json::from_str(text).ok()?;
    let object = value.as_object()?;
    if object.len() != 1 {
        return None;
    }
    match object.get("type")?.as_str()? {
        "KeepAlive" => Some(r#"{"type":"KeepAlive"}"#),
        "Finalize" => Some(r#"{"type":"Finalize"}"#),
        "CloseStream" => Some(r#"{"type":"CloseStream"}"#),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
async fn pipe_speech(
    state: &AppState,
    p: &Principal,
    meeting: Uuid,
    offset: f64,
    relay_live: bool,
    mut ws: WebSocket,
    mut upstream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    mut reservation: Uuid,
    key: &str,
    lease: &str,
) -> Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    interval.tick().await;
    let mut bytes = 0u64;
    let mut chunk_bytes = 0u64;
    let mut control_frames = 0u64;
    let started = tokio::time::Instant::now();
    let mut chunk_started = tokio::time::Instant::now();
    let capture = Uuid::new_v4();
    loop {
        tokio::select! {
            _=state.shutdown.cancelled()=>break,
            _=interval.tick()=>{
                if relay_live { live(state,p,Some(meeting),json!({"type":"meeting_active"})).await; }
                billing::settle(state,reservation,30).await?;
                reservation=match billing::reserve(state,p.account_id,"speech",30).await{Ok(id)=>id,Err(_)=>break};
                chunk_started=tokio::time::Instant::now();
                chunk_bytes=0;
                let current=crate::entities::sessions::Entity::find_by_id(p.session_id).one(&state.db).await?;
                if current.is_none_or(|s|s.revoked_at.is_some()||s.refresh_expires_at<=chrono::Utc::now()) || started.elapsed()>Duration::from_secs(4*3600){break}
                let renewed:i64=state.redis.eval("if redis.call('GET',KEYS[1])==ARGV[1] then return redis.call('EXPIRE',KEYS[1],90) else return 0 end",vec![key],vec![lease]).await?;
                if renewed==0{break}
            },
            incoming=ws.next()=>match incoming{
                Some(Ok(Message::Binary(data)))=>{
                    bytes+=data.len() as u64;
                    chunk_bytes+=data.len() as u64;
                    // 16kHz, 16-bit stereo: at most real-time audio plus one second burst.
                    if data.len()%4!=0 || bytes>(started.elapsed().as_secs()+1)*64000{break}
                    if upstream.send(UpstreamMessage::Binary(data)).await.is_err(){break}
                },
                Some(Ok(Message::Text(text)))=>{
                    let Some(control) = speech_control(&text) else { break };
                    control_frames += 1;
                    if control_frames > (started.elapsed().as_secs() + 1) * 4 { break }
                    if upstream.send(UpstreamMessage::Text(control.into())).await.is_err(){break}
                },
                Some(Ok(Message::Ping(data)))=>{ws.send(Message::Pong(data)).await.ok();},
                Some(Ok(Message::Pong(_)))=>{},
                _=>break,
            },
            response=upstream.next()=>match response{
                Some(Ok(UpstreamMessage::Text(text)))=>{
                    if text.len()>256*1024{break}
                    if relay_live && let Ok(event)=serde_json::from_str::<Value>(&text) && event["type"]=="Results" {
                        let words=event["channel"]["alternatives"][0]["transcript"].as_str().unwrap_or("").chars().take(8000).collect::<String>();
                        live(state,p,Some(meeting),json!({"type":"transcript","capture_id":capture,"result":{"channel_index":event["channel_index"],"start":event["start"].as_f64().unwrap_or(0.0)+offset,"is_final":event["is_final"],"channel":{"alternatives":[{"transcript":words}]}}})).await;
                    }
                    if ws.send(Message::Text(text.to_string().into())).await.is_err(){break}
                },
                Some(Ok(UpstreamMessage::Ping(data)))=>{upstream.send(UpstreamMessage::Pong(data)).await.ok();},
                Some(Ok(UpstreamMessage::Pong(_)))=>{},
                _=>break,
            }
        }
    }
    upstream.close(None).await.ok();
    ws.close().await.ok();
    let elapsed = chunk_started.elapsed().as_millis().div_ceil(1000) as u64;
    billing::settle(
        state,
        reservation,
        elapsed.max(chunk_bytes.div_ceil(64000)).clamp(1, 30) as i64,
    )
    .await?;
    Ok(())
}
use sea_orm::EntityTrait;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn speech_relay_remains_compatible_and_can_be_disabled_for_local_meetings() {
        let meeting = Uuid::new_v4();
        let old: SpeechQuery = serde_json::from_value(json!({"meeting_id": meeting})).unwrap();
        assert!(old.relay_live);
        let local: SpeechQuery =
            serde_json::from_value(json!({"meeting_id": meeting, "relay_live": false})).unwrap();
        assert!(!local.relay_live);
    }
    #[test]
    fn gemini_usage_includes_thinking_without_counting_it_twice() {
        let model = plans::MODELS[2];
        assert_eq!(
            usage_cost(
                model,
                &json!({"prompt_tokens":1000,"completion_tokens":100,"total_tokens":1600})
            ),
            Some(model.cost(1000, 600))
        );
        assert_eq!(
            usage_cost(
                model,
                &json!({"prompt_tokens":1000,"completion_tokens":600,"total_tokens":1600})
            ),
            Some(model.cost(1000, 600))
        );
        assert_eq!(
            usage_cost(
                model,
                &json!({"prompt_tokens":1000,"completion_tokens":100})
            ),
            Some(model.cost(1000, 100))
        );
        assert_eq!(
            usage_cost(model, &json!({"prompt_tokens":-1,"completion_tokens":10})),
            None
        );
    }
    #[test]
    fn deepseek_cache_tokens_are_discounted_for_both_usage_formats() {
        let model = *plans::MODELS.last().unwrap();
        let expected = Some(model.cost_with_cached(1000, 200, 500));
        assert_eq!(
            usage_cost(
                model,
                &json!({"prompt_tokens":1000,"completion_tokens":200,"total_tokens":1200,"prompt_tokens_details":{"cached_tokens":500}})
            ),
            expected
        );
        assert_eq!(
            usage_cost(
                model,
                &json!({"prompt_tokens":1000,"completion_tokens":200,"total_tokens":1200,"prompt_cache_hit_tokens":500})
            ),
            expected
        );
    }
    #[test]
    fn speech_controls_allow_keepalive_and_close_without_arbitrary_payloads() {
        assert_eq!(
            speech_control(r#"{ "type": "KeepAlive" }"#),
            Some(r#"{"type":"KeepAlive"}"#)
        );
        assert_eq!(
            speech_control(r#"{"type":"CloseStream"}"#),
            Some(r#"{"type":"CloseStream"}"#)
        );
        assert_eq!(
            speech_control(r#"{"type":"Finalize"}"#),
            Some(r#"{"type":"Finalize"}"#)
        );
        for value in [
            r#"{"type":"KeepAlive","extra":true}"#,
            r#"{"type":"Results"}"#,
            "null",
            "invalid",
        ] {
            assert_eq!(speech_control(value), None);
        }
    }

    #[test]
    fn proxy_rejects_roles_urls_and_unbounded_context() {
        assert!(
            validate_messages(&[ChatMessage {
                role: "tool".into(),
                content: json!("x")
            }])
            .is_err()
        );
        assert!(validate_messages(&[ChatMessage{role:"user".into(),content:json!([{"type":"image_url","image_url":{"url":"http://169.254.169.254/latest/meta-data"}}])}]).is_err());
        assert!(
            validate_messages(&[ChatMessage {
                role: "user".into(),
                content: json!("x".repeat(96000))
            }])
            .is_err()
        );
        assert!(
            validate_messages(&[ChatMessage {
                role: "user".into(),
                content: json!("hello")
            }])
            .is_ok()
        );
    }
}
