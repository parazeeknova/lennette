use super::*;
use axum::body::{Body, Bytes};
use plans::{Model, ModelProvider};

const HEADERS_TIMEOUT: Duration = Duration::from_secs(5);
const TOKEN_TIMEOUT: Duration = Duration::from_secs(12);
const DEEPSEEK: &str = "deepseek-ai/DeepSeek-V4.1-Flash-1017";
const GEMINI: &str = "gemini-3.5-flash-lite";

fn credentials(state: &AppState, model: Model) -> (&url::Url, &str) {
    match model.provider {
        ModelProvider::Gemini => (
            &state.config.gemini_url,
            state.config.gemini_key.expose_secret(),
        ),
        ModelProvider::DeepSeek => (
            &state.config.deepseek_url,
            state.config.deepseek_key.expose_secret(),
        ),
    }
}

fn payload(model: Model, messages: &[Value], max_tokens: i64, effort: &str) -> Value {
    let effort = if matches!(model.provider, ModelProvider::Gemini) && effort == "none" {
        "low"
    } else {
        effort
    };
    json!({"model":model.id,"messages":messages,"stream":true,"max_tokens":max_tokens,
        "reasoning_effort":effort,"stream_options":{"include_usage":true}})
}

struct Attempt {
    model: Model,
    reservation: Uuid,
    reserved: i64,
    response: reqwest::Response,
}

async fn open(
    state: &AppState,
    principal: &Principal,
    model: Model,
    messages: &[Value],
    input: i64,
    max_tokens: i64,
    effort: &str,
) -> Result<Option<Attempt>> {
    let (url, key) = credentials(state, model);
    if key.is_empty() {
        return Ok(None);
    }
    // Reserve uncached input/output; cached tokens are discounted at settlement.
    let reserved = model.cost(input, max_tokens);
    let reservation = billing::reserve(state, principal.account_id, "ai", reserved).await?;
    let request = state
        .http
        .post(url.clone())
        .bearer_auth(key)
        .json(&payload(model, messages, max_tokens, effort))
        .send();
    let response = tokio::time::timeout(HEADERS_TIMEOUT, request)
        .instrument(tracing::info_span!(
            "managed.chat",
            provider = model.provider.name(),
            model = model.id,
            otel.kind = "client",
            server.address = url.host_str().unwrap_or("")
        ))
        .await;
    if let Ok(Ok(response)) = response
        && response.status().is_success()
    {
        return Ok(Some(Attempt {
            model,
            reservation,
            reserved,
            response,
        }));
    }
    billing::settle(state, reservation, 0).await?;
    Ok(None)
}

fn continuation(messages: &[Value], partial: &str) -> Vec<Value> {
    let mut messages = messages.to_vec();
    if !partial.is_empty() {
        messages.push(json!({"role":"assistant","content":partial}));
        messages.push(json!({"role":"user","content":"The response was interrupted. Continue exactly where it stopped, without repeating the text already written or adding a preamble. Preserve the requested format and finish the original answer."}));
    }
    messages
}

fn sse(value: Value) -> Bytes {
    Bytes::from(format!("data: {value}\n\n"))
}

pub async fn chat(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Json(request): Json<ChatRequest>,
) -> Result<Response> {
    if request.stream == Some(false) {
        return Err(ApiError::BadRequest(
            "managed chat requires streaming".into(),
        ));
    }
    let catalog = plans::models();
    let selected = *catalog
        .iter()
        .find(|m| m.id == request.model)
        .ok_or_else(|| ApiError::BadRequest("model is not in the managed catalog".into()))?;
    let (plan, _) = billing::entitlement(&state.db, p.account_id).await?;
    if selected.plus_only && plan.id != "managed_plus" {
        return Err(ApiError::PaymentRequired);
    }
    own_meeting(&state, &p, request.meeting_id).await?;
    let input = validate_messages(&request.messages)?;
    let max_tokens = request.max_tokens.unwrap_or(2048);
    if !(1..=4096).contains(&max_tokens) {
        return Err(ApiError::BadRequest("max_tokens must be 1 to 4096".into()));
    }
    let effort = request.reasoning_effort.as_deref().unwrap_or("low");
    if !matches!(effort, "none" | "low" | "medium" | "high") {
        return Err(ApiError::BadRequest("unsupported reasoning effort".into()));
    }
    state
        .limit(format!("chat:{}", p.account_id), 30, 60)
        .await?;
    let messages: Vec<Value> = request
        .messages
        .iter()
        .map(|m| json!({"role":m.role,"content":m.content}))
        .collect();
    let alternate_id = match selected.provider {
        ModelProvider::Gemini => DEEPSEEK,
        ModelProvider::DeepSeek => GEMINI,
    };
    let alternate = catalog
        .into_iter()
        .find(|m| m.id == alternate_id && !credentials(&state, *m).1.is_empty());
    let mut first = open(&state, &p, selected, &messages, input, max_tokens, effort).await?;
    let mut backup = alternate;
    if first.is_none()
        && let Some(model) = backup.take()
    {
        first = open(&state, &p, model, &messages, input, max_tokens, effort).await?;
    }
    let first = first.ok_or(ApiError::Unavailable)?;
    let switched = first.model.id != selected.id;
    let relay = request.relay_live.unwrap_or(true);
    let prompt = messages
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .and_then(|m| m["content"].as_str())
        .unwrap_or("")
        .chars()
        .take(4000)
        .collect::<String>();
    let effort = effort.to_owned();
    let stream = async_stream::stream! {
        let stream_id = Uuid::new_v4();
        let mut output = String::new();
        let mut current = Some(first);
        let mut switched = switched;
        let mut complete = false;
        if relay { live(&state,&p,request.meeting_id,json!({"type":"ai_start","stream_id":stream_id,"conversation_id":request.conversation_id,"prompt":prompt})).await; }
        while let Some(attempt) = current.take() {
            let model = attempt.model;
            if switched {
                let event = json!({"type":"ai_provider_switch","stream_id":stream_id,"conversation_id":request.conversation_id,"model":model.id,"name":model.name,"provider":model.provider.name(),"continued":!output.is_empty()});
                if relay { live(&state,&p,request.meeting_id,event.clone()).await; }
                yield Ok::<Bytes,std::io::Error>(sse(json!({"lennette":event})));
            }
            let mut upstream = attempt.response.bytes_stream();
            let output_start = output.len();
            let mut pending = Vec::new();
            let mut usage = None;
            let mut done = false;
            let mut failed = false;
            let mut activity = tokio::time::Instant::now();
            loop {
                let next = tokio::select! {
                    next = tokio::time::timeout_at(activity + TOKEN_TIMEOUT, upstream.next()) => next,
                    _ = state.shutdown.cancelled() => { failed=true; break; }
                };
                let chunk = match next { Ok(Some(Ok(chunk))) => chunk, _ => { failed=true; break; } };
                pending.extend_from_slice(&chunk);
                if pending.len()>512*1024 { failed=true; break; }
                while let Some(end) = pending.iter().position(|b|*b==b'\n') {
                    let line: Vec<_> = pending.drain(..=end).collect();
                    let Ok(text) = std::str::from_utf8(&line) else { failed=true; break; };
                    let Some(data) = text.trim().strip_prefix("data:").map(str::trim) else { continue; };
                    if data=="[DONE]" { done=true; break; }
                    let event: Value = match serde_json::from_str(data) { Ok(v)=>v, Err(_)=>{failed=true;break;} };
                    if event.get("error").is_some() {failed=true;break;}
                    if let Some(cost)=usage_cost(model,&event["usage"]) {usage=Some(cost);}
                    if let Some(delta)=event["choices"][0]["delta"]["content"].as_str() && !delta.is_empty() {
                        activity=tokio::time::Instant::now();
                        if output.len()+delta.len()>1024*1024 {failed=true;break;}
                        let offset=output.len();
                        output.push_str(delta);
                        if relay {live(&state,&p,request.meeting_id,json!({"type":"ai_delta","stream_id":stream_id,"conversation_id":request.conversation_id,"offset":offset,"delta":delta})).await;}
                    }
                    yield Ok(sse(event));
                }
                if done || failed {break;}
            }
            // Missing usage after an accepted request is an unknown billable
            // outcome, so keep its reserved amount instead of granting free AI.
            if let Err(error)=billing::settle(&state,attempt.reservation,usage.unwrap_or(attempt.reserved)).await {
                tracing::error!(%error,"AI usage settlement failed");
                failed=true;
            }
            if done && !failed && output.len()>output_start && !output.trim().is_empty() {complete=true;break;}
            if state.shutdown.is_cancelled() || output.len()>64*1024 {break;}
            if let Some(model)=backup.take() {
                let continued=continuation(&messages,&output);
                let continued_input=input.saturating_add(output.len() as i64).saturating_add(256);
                match open(&state,&p,model,&continued,continued_input,max_tokens,&effort).await {
                    Ok(attempt)=>current=attempt,
                    Err(_)=>current=None,
                }
                switched=true;
            }
        }
        if relay {live(&state,&p,request.meeting_id,json!({"type":"ai_done","stream_id":stream_id,"conversation_id":request.conversation_id,"total_bytes":output.len(),"complete":complete})).await;}
        if complete {
            yield Ok(Bytes::from_static(b"data: [DONE]\n\n"));
        } else {
            yield Ok(sse(json!({"error":{"message":"AI providers could not finish this answer. Check your remaining allowance or try again shortly."}})));
        }
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
