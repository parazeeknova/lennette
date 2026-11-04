//! Bounded dictation proxy. Audio exists only in request memory and the upstream call.
use crate::{
    auth::Principal,
    billing,
    error::{ApiError, Result},
    state::AppState,
};
use axum::{Extension, Json, body::Bytes, extract::State};
use secrecy::ExposeSecret;
use serde_json::Value;

pub async fn transcribe(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    body: Bytes,
) -> Result<Json<Value>> {
    if state.config.deepgram_key.expose_secret().is_empty() {
        return Err(ApiError::Unavailable);
    }
    let reader = hound::WavReader::new(std::io::Cursor::new(&body))
        .map_err(|_| ApiError::BadRequest("invalid WAV audio".into()))?;
    let spec = reader.spec();
    if spec.sample_rate != 16000
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
        || !(1..=2).contains(&spec.channels)
    {
        return Err(ApiError::BadRequest(
            "send 16kHz 16-bit PCM WAV with one or two channels".into(),
        ));
    }
    let frames = reader.duration() as i64;
    let seconds = (frames + 15999) / 16000;
    if !(1..=120).contains(&seconds) {
        return Err(ApiError::BadRequest(
            "dictation must be at most two minutes".into(),
        ));
    }
    let samples = reader
        .into_samples::<i16>()
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| ApiError::BadRequest("truncated WAV audio".into()))?;
    if samples.len() as i64 != frames * spec.channels as i64 {
        return Err(ApiError::BadRequest("invalid WAV sample count".into()));
    }
    state
        .limit(format!("dictation:{}", p.account_id), 30, 60)
        .await?;
    let reservation = billing::reserve(&state, p.account_id, "speech", seconds).await?;
    let mut url = state.config.deepgram_url.clone();
    let scheme = if url.scheme() == "wss" {
        "https"
    } else {
        "http"
    };
    url.set_scheme(scheme).map_err(|_| ApiError::Unavailable)?;
    url.set_query(None);
    url.query_pairs_mut().extend_pairs([
        ("model", "nova-3"),
        ("language", "multi"),
        ("smart_format", "true"),
        ("punctuate", "true"),
        ("mip_opt_out", "true"),
    ]);
    let response = state
        .http
        .post(url)
        .header(
            "authorization",
            format!("Token {}", state.config.deepgram_key.expose_secret()),
        )
        .header("content-type", "audio/wav")
        .body(body)
        .send()
        .await?;
    if !response.status().is_success() {
        billing::settle(&state, reservation, 0).await?;
        return Err(ApiError::Unavailable);
    }
    let value: Value = response.json().await?;
    billing::settle(&state, reservation, seconds).await?;
    Ok(Json(value))
}
