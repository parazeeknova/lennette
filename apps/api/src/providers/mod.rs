mod batch;
use crate::{
    auth::{self, Principal},
    billing,
    error::{ApiError, Result},
    plans::MODELS,
    state::AppState,
};
use axum::{
    Extension, Json,
    body::{Body, Bytes},
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
    let payload = json!({"type":"live","meeting_id":meeting,"device_id":p.device_id,"event":event});
    if let Ok(serialized) = serde_json::to_string(&payload) {
        let _: std::result::Result<(), _> = state
            .redis
            .publish(format!("live:{}", p.account_id), serialized)
            .await;
    }
}

pub async fn chat(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Json(request): Json<ChatRequest>,
) -> Result<Response> {
    if state.config.merge_key.expose_secret().is_empty() {
        return Err(ApiError::Unavailable);
    }
    if request.stream == Some(false) {
        return Err(ApiError::BadRequest(
            "managed chat requires streaming".into(),
        ));
    }
    let model = *MODELS
        .iter()
        .find(|m| m.id == request.model)
        .ok_or_else(|| ApiError::BadRequest("model is not in the managed catalog".into()))?;
    let (plan, _) = billing::entitlement(&state.db, p.account_id).await?;
    if model.plus_only && plan.id != "managed_plus" {
        return Err(ApiError::PaymentRequired);
    }
    own_meeting(&state, &p, request.meeting_id).await?;
    let input = validate_messages(&request.messages)?;
    let max_tokens = request.max_tokens.unwrap_or(2048);
    if !(1..=4096).contains(&max_tokens) {
        return Err(ApiError::BadRequest("max_tokens must be 1 to 4096".into()));
    }
    state
        .limit(format!("chat:{}", p.account_id), 30, 60)
        .await?;
    let reservation =
        billing::reserve(&state, p.account_id, "ai", model.cost(input, max_tokens)).await?;
    let messages: Vec<Value> = request
        .messages
        .into_iter()
        .map(|m| json!({"role":m.role,"content":m.content}))
        .collect();
    let mut payload = json!({"model":model.id,"messages":messages,"stream":true,"max_tokens":max_tokens,"stream_options":{"include_usage":true}});
    if let Some(effort) = request.reasoning_effort {
        if !matches!(effort.as_str(), "none" | "low" | "medium") {
            billing::settle(&state, reservation, 0).await?;
            return Err(ApiError::BadRequest("unsupported reasoning effort".into()));
        }
        payload["reasoning_effort"] = json!(effort);
    }
    let upstream = state
        .http
        .post(state.config.merge_url.clone())
        .bearer_auth(state.config.merge_key.expose_secret())
        .json(&payload)
        .send()
        .instrument(tracing::info_span!(
            "merge.chat",
            otel.kind = "client",
            server.address = state.config.merge_url.host_str().unwrap_or("merge")
        ))
        .await?;
    if !upstream.status().is_success() {
        billing::settle(&state, reservation, 0).await?;
        return Err(ApiError::Unavailable);
    }
    let stream = async_stream::stream! {
        let mut upstream=upstream.bytes_stream();let mut pending=Vec::new();let mut usage=None;let mut done=false;let mut output=String::new();let stream_id=Uuid::new_v4();
        while let Some(chunk)=tokio::select!{chunk=upstream.next()=>chunk,_=state.shutdown.cancelled()=>None}{
            let chunk=match chunk{Ok(c)=>c,Err(_)=>break};
            pending.extend_from_slice(&chunk);
            if pending.len()>512*1024{break}
            while let Some(end)=pending.iter().position(|b|*b==b'\n'){
                let line:Vec<u8>=pending.drain(..=end).collect();
                if let Ok(text)=std::str::from_utf8(&line) && let Some(data)=text.trim().strip_prefix("data:"){
                    if data.trim()=="[DONE]"{done=true;}
                    else if let Ok(event)=serde_json::from_str::<Value>(data){
                        if let (Some(input),Some(output))=(event["usage"]["prompt_tokens"].as_i64(),event["usage"]["completion_tokens"].as_i64()) && input>=0 && output>=0{usage=Some(model.cost(input,output));}
                        if let Some(delta)=event["choices"][0]["delta"]["content"].as_str(){
                            output.push_str(delta);
                            live(&state,&p,request.meeting_id,json!({"type":"ai_delta","stream_id":stream_id,"delta":delta})).await;
                        }
                    }
                }
                if done && let Some(cost)=usage && let Err(e)=billing::settle(&state,reservation,cost).await{tracing::error!(%e,"AI usage settlement failed")}
                yield Ok::<Bytes,std::io::Error>(Bytes::from(line));
            }
            if output.len()>1024*1024{break}
        }
        if done && let Some(cost)=usage && let Err(e)=billing::settle(&state,reservation,cost).await{tracing::error!(%e,"AI usage settlement failed")}
        live(&state,&p,request.meeting_id,json!({"type":"ai_done","stream_id":stream_id,"complete":done})).await;
    };
    Ok((
        [
            (header::CONTENT_TYPE, "text/event-stream"),
            (header::CACHE_CONTROL, "no-cache, no-transform"),
            (header::HeaderName::from_static("x-accel-buffering"), "no"),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct SpeechQuery {
    pub meeting_id: Uuid,
}
pub async fn speech(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Query(q): Query<SpeechQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response> {
    if state.config.deepgram_key.expose_secret().is_empty() {
        return Err(ApiError::Unavailable);
    }
    own_meeting(&state, &p, Some(q.meeting_id)).await?;
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
    Ok(ws.max_message_size(64*1024).on_upgrade(move |ws|async move{
        if let Err(error)=pipe_speech(&state,&p,q.meeting_id,ws,upstream,reservation,&key,&lease).await{tracing::debug!(%error,"speech stream ended")}
        let _:std::result::Result<i64,_>=state.redis.eval("if redis.call('GET',KEYS[1])==ARGV[1] then return redis.call('DEL',KEYS[1]) else return 0 end",vec![key],vec![lease]).await;
    }))
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
        "CloseStream" => Some(r#"{"type":"CloseStream"}"#),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
async fn pipe_speech(
    state: &AppState,
    p: &Principal,
    meeting: Uuid,
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
    loop {
        tokio::select! {
            _=state.shutdown.cancelled()=>break,
            _=interval.tick()=>{
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
                    if let Ok(event)=serde_json::from_str::<Value>(&text) && event["type"]=="Results"{live(state,p,Some(meeting),json!({"type":"transcript","result":event})).await;}
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
    fn speech_controls_allow_keepalive_and_close_without_arbitrary_payloads() {
        assert_eq!(
            speech_control(r#"{ "type": "KeepAlive" }"#),
            Some(r#"{"type":"KeepAlive"}"#)
        );
        assert_eq!(
            speech_control(r#"{"type":"CloseStream"}"#),
            Some(r#"{"type":"CloseStream"}"#)
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
