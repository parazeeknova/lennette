//! Structured business-activity logging for OpenObserve.
//!
//! Every event carries `event = "<domain>.<object>.<outcome>"` so dashboards
//! can filter with `WHERE event = '...'` without parsing messages. IDs are
//! always opaque UUIDs (`account_id`, `device_id`, `session_id`, ...);
//! durations use `*_secs`/`duration_ms`, sizes `*_bytes`, money integer
//! microdollars.
//!
//! # Secure-logging contract (enforced by review, not by types)
//!
//! NEVER log: emails, display names, device names, OAuth subjects, tokens or
//! token digests, authorization codes, transcript/prompt/message/file content,
//! search queries, attachment filenames, SQL text, secrets, or IP addresses.
//! Peer IPs are rate-limit keys but must stay out of the log stream.
//!
//! SAFE to log: account/device/session/meeting/conversation/stream/capture/
//! attachment/quote/reservation IDs (opaque UUIDs), counts, sizes, durations,
//! model/provider/plan/kind/status/error codes, and boolean flags.
//!
//! # Collection boundary
//!
//! Events flow through the standard tracing pipeline, which only leaves the
//! machine when the operator configures `OTEL_EXPORTER_OTLP_ENDPOINT`
//! (see `telemetry.rs`; without it everything is console-only). There is no
//! separate phone-home: the desktop client ships no telemetry SDK and talks
//! only to its configured API URL, and this API never dials out except to
//! its configured providers (Deepgram/Gemini/DeepSeek/Dodo/R2/ClamAV).
//!
//! On top of transport, events are split by purpose:
//!
//! * **Security / service-integrity** events always emit. They carry no
//!   content and exist to detect abuse and outages (auth failures, token
//!   replay, webhook forgery, rate limits, upstream/provider failures,
//!   quota blocks, 5xx outcomes).
//! * **Business** events (presence, durations, usage, uploads, billing flow)
//!   emit only when `LENNETTE_ACTIVITY_BUSINESS` is not `0`/`false`/`off`/`no`
//!   (default: on). Local and private distributions set it off to silence
//!   product analytics. This is not anonymization: security, audit, and
//!   service-integrity events still attribute account/session IDs, because
//!   abuse response and incident diagnosis are impossible without them.
//!
//! Debug-level events are diagnostic detail and stay out of the default
//! `info` stream.

use std::sync::OnceLock;
use uuid::Uuid;

static BUSINESS_ENABLED: OnceLock<bool> = OnceLock::new();

fn business_enabled() -> bool {
    *BUSINESS_ENABLED.get_or_init(|| match std::env::var("LENNETTE_ACTIVITY_BUSINESS") {
        Ok(value) => !matches!(
            value.to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ),
        Err(_) => true,
    })
}

// ---------------------------------------------------------------------------
// auth
// ---------------------------------------------------------------------------

/// OAuth authorization URL minted (after validation + rate limits).
pub fn oauth_started(provider: &str, link: bool) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "auth.oauth.started",
        provider,
        link,
        "oauth authorization started"
    );
}

/// User denied access at the identity provider (user-driven, not an attack).
pub fn oauth_denied(provider: &str) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "auth.oauth.denied",
        provider,
        "oauth denied at provider"
    );
}

/// OAuth callback arrived without a matching pending login (expired CSRF state
/// or probing). No account context exists on this path by design.
pub fn oauth_invalid_state(provider: &str) {
    tracing::warn!(
        event = "auth.oauth.invalid_state",
        provider,
        "oauth callback with unknown or expired state"
    );
}

/// OAuth login finished: new or returning account, native or browser flow.
pub fn oauth_completed(provider: &str, new_account: bool, browser: bool, account_id: Uuid) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "auth.oauth.completed",
        provider,
        new_account,
        browser,
        account_id = %account_id,
        "oauth login completed"
    );
}

/// New device session created (native exchange, browser login, or link flow).
pub fn session_issued(account_id: Uuid, device_id: Uuid, session_id: Uuid) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "auth.session.issued",
        account_id = %account_id,
        device_id = %device_id,
        session_id = %session_id,
        "device session started"
    );
}

/// Access/refresh pair rotated successfully.
pub fn token_refreshed(account_id: Uuid, session_id: Uuid) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "auth.token.refreshed",
        account_id = %account_id,
        session_id = %session_id,
        "session tokens rotated"
    );
}

/// A rotated-out refresh token was presented again: possible theft. The
/// device session is revoked by the caller; this records the signal.
pub fn token_replay_detected(account_id: Uuid, session_id: Uuid) {
    tracing::warn!(
        event = "auth.token.replay_detected",
        account_id = %account_id,
        session_id = %session_id,
        "stale refresh token replayed; device session revoked"
    );
}

/// Session ended. `reason`: `logout` (self), `device` (revoked another
/// device), `replay` (see above).
pub fn session_revoked(account_id: Uuid, session_id: Uuid, reason: &'static str) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "auth.session.revoked",
        account_id = %account_id,
        session_id = %session_id,
        reason,
        "device session ended"
    );
}

/// A second sign-in method was attached to the account.
pub fn account_linked(account_id: Uuid, provider: &str) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "auth.account.linked",
        account_id = %account_id,
        provider,
        "sign-in method connected"
    );
}

/// Linking refused: identity belongs to another account or provider exists.
pub fn account_link_conflict(account_id: Uuid, provider: &str) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "auth.account.link_conflict",
        account_id = %account_id,
        provider,
        "sign-in method link refused"
    );
}

/// Account deletion completed. Destructive-action audit: always emitted, even
/// with business events disabled.
pub fn account_deleted(account_id: Uuid, session_id: Uuid) {
    tracing::info!(
        event = "account.deleted",
        account_id = %account_id,
        session_id = %session_id,
        "account deleted; durable object erasure scheduled"
    );
}

/// Account deletion blocked because the billing subscription did not confirm
/// cancellation. Rare and support-relevant: always emitted.
pub fn cancel_unconfirmed(account_id: Uuid) {
    tracing::warn!(
        event = "billing.cancel.unconfirmed",
        account_id = %account_id,
        "account deletion blocked by unconfirmed subscription cancellation"
    );
}

// ---------------------------------------------------------------------------
// billing
// ---------------------------------------------------------------------------

/// Hosted checkout session created. The checkout URL/session ID are secrets
/// of the payment flow and are intentionally not logged.
pub fn checkout_created(account_id: Uuid, plan: &str) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "billing.checkout.created",
        account_id = %account_id,
        plan,
        "billing checkout created"
    );
}

/// Customer-portal session opened.
pub fn portal_opened(account_id: Uuid) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "billing.portal.opened",
        account_id = %account_id,
        "billing portal opened"
    );
}

/// Webhook signature verified and payload parsed. `webhook_id` is the
/// provider's idempotency key (`evt_...`), safe to log.
pub fn webhook_received(event: &str, webhook_id: &str) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "billing.webhook.received",
        webhook_event = webhook_token(event),
        webhook_id = webhook_token(webhook_id),
        "billing webhook received"
    );
}

/// Webhook drove a state change. `hold` marks refund/dispute holds.
pub fn webhook_applied(event: &str, webhook_id: &str, account_id: Option<Uuid>, hold: bool) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "billing.webhook.applied",
        webhook_event = webhook_token(event),
        webhook_id = webhook_token(webhook_id),
        account_id = ?account_id,
        hold,
        "billing webhook applied"
    );
}

/// Webhook acknowledged without state change (duplicate, non-subscription
/// event, or missing subscription). Debug: high-frequency no-op path.
pub fn webhook_ignored(event: &str, webhook_id: &str) {
    if !business_enabled() {
        return;
    }
    tracing::debug!(
        event = "billing.webhook.ignored",
        webhook_event = webhook_token(event),
        webhook_id = webhook_token(webhook_id),
        "billing webhook acknowledged without changes"
    );
}

/// Provider event names (`subscription.*`, `refund.succeeded`, `dispute.*`)
/// and ids (`wh_*`, `evt_*`) are safe to log; anything else is replaced.
fn webhook_token(value: &str) -> &str {
    if !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        value
    } else {
        "invalid"
    }
}

/// Logs only the MIME essence: parameters such as `name="..."` can smuggle
/// the uploaded filename into the log stream.
fn mime_essence(content_type: &str) -> &str {
    content_type.split(';').next().unwrap_or("").trim()
}
/// Webhook signature check failed. `webhook_id` is best-effort (may be absent
/// or forged on malformed requests) and never a secret; untrusted values are
/// replaced before logging.
pub fn webhook_rejected(reason: &'static str, webhook_id: Option<&str>) {
    tracing::warn!(
        event = "billing.webhook.rejected",
        reason,
        webhook_id = webhook_id.map(webhook_token).unwrap_or("absent"),
        "billing webhook rejected"
    );
}

/// Upgrade price preview stored (5-minute quote window).
pub fn upgrade_quoted(account_id: Uuid, quote_id: Uuid, amount: i64, currency: &str) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "billing.upgrade.quoted",
        account_id = %account_id,
        quote_id = %quote_id,
        amount,
        currency,
        "upgrade quote issued"
    );
}

/// Upgrade submitted to the payment provider (idempotent on `quote_id`).
pub fn upgrade_submitted(account_id: Uuid, quote_id: Uuid, amount: i64, currency: &str) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "billing.upgrade.submitted",
        account_id = %account_id,
        quote_id = %quote_id,
        amount,
        currency,
        "upgrade submitted to payment provider"
    );
}

/// Provider confirmed the charge and the new plan took effect locally.
pub fn upgrade_applied(account_id: Uuid, quote_id: Uuid) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "billing.upgrade.applied",
        account_id = %account_id,
        quote_id = %quote_id,
        "upgrade subscription applied"
    );
}

/// Provider did not confirm the upgrade. Always emitted: provider failures
/// are service-integrity signals, not business analytics. `outcome`:
/// `provider_rejected` (definitive 4xx) or `provider_unreachable`
/// (uncertain; never auto-retried).
pub fn upgrade_failed(account_id: Uuid, quote_id: Uuid, outcome: &'static str) {
    tracing::warn!(
        event = "billing.upgrade.failed",
        account_id = %account_id,
        quote_id = %quote_id,
        outcome,
        "upgrade not confirmed by payment provider"
    );
}

// ---------------------------------------------------------------------------
// speech (live proxy) + dictation (batch)
// ---------------------------------------------------------------------------

/// Live transcription session accepted: upstream connected, quota reserved.
pub fn speech_started(
    account_id: Uuid,
    device_id: Uuid,
    session_id: Uuid,
    meeting_id: Uuid,
    relay_live: bool,
) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "speech.session.started",
        account_id = %account_id,
        device_id = %device_id,
        session_id = %session_id,
        meeting_id = %meeting_id,
        relay_live,
        "live transcription session started"
    );
}

/// Upstream Deepgram connection failed before any audio flowed. `status` is
/// the HTTP status when the handshake produced one.
pub fn speech_upstream_failed(account_id: Uuid, meeting_id: Uuid, status: Option<u16>) {
    tracing::warn!(
        event = "speech.session.upstream_failed",
        account_id = %account_id,
        meeting_id = %meeting_id,
        status,
        "managed speech upstream connection failed"
    );
}

/// Pipe broke with an infrastructure/billing error (DB settle failed, etc.).
/// Normal endings go through `speech_ended`.
pub fn speech_failed(account_id: Uuid, meeting_id: Uuid, reason: &'static str) {
    tracing::error!(
        event = "speech.session.failed",
        account_id = %account_id,
        meeting_id = %meeting_id,
        reason,
        "live transcription session failed"
    );
}

/// Live session summary. Byte/character counts only — never audio or
/// transcript content. `speakers` counts distinct diarization labels seen,
/// `finals` finalized result messages, `upstream_errors` provider error
/// frames; all three are relayed-session only. `reason`: `client_closed`,
/// `upstream_closed`, `superseded` (newer socket took the single-flight
/// lease), `quota_exhausted`, `session_revoked`, `session_expired`,
/// `audio_pacing`, `control_flood`, `oversize_frame`, `shutdown`.
#[allow(clippy::too_many_arguments)]
pub fn speech_ended(
    account_id: Uuid,
    session_id: Uuid,
    meeting_id: Uuid,
    relay_live: bool,
    duration_secs: u64,
    audio_secs: u64,
    audio_bytes: u64,
    billed_secs: i64,
    chunks: u64,
    transcript_chars: u64,
    results: u64,
    speakers: u64,
    finals: u64,
    upstream_errors: u64,
    reason: &'static str,
) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "speech.session.ended",
        account_id = %account_id,
        session_id = %session_id,
        meeting_id = %meeting_id,
        relay_live,
        duration_secs,
        audio_secs,
        audio_bytes,
        billed_secs,
        chunks,
        transcript_chars,
        results,
        speakers,
        finals,
        upstream_errors,
        reason,
        "live transcription session ended"
    );
}

/// One-shot dictation transcribed and settled.
pub fn dictation_completed(
    account_id: Uuid,
    audio_secs: i64,
    audio_bytes: usize,
    billed_secs: i64,
    latency_ms: u64,
) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "speech.dictation.completed",
        account_id = %account_id,
        audio_secs,
        audio_bytes,
        billed_secs,
        latency_ms,
        "dictation transcribed"
    );
}

/// Provider transport broke mid-stream. Always emitted, independent of the
/// business gate; graceful upstream closure stays in `speech.session.ended`.
pub fn speech_upstream_error(account_id: Uuid, meeting_id: Uuid) {
    tracing::warn!(
        event = "speech.session.upstream_error",
        account_id = %account_id,
        meeting_id = %meeting_id,
        "live transcription upstream transport failed"
    );
}

/// Dictation reached the provider but failed there.
pub fn dictation_failed(account_id: Uuid, audio_secs: i64, status: Option<u16>) {
    tracing::warn!(
        event = "speech.dictation.failed",
        account_id = %account_id,
        audio_secs,
        status,
        "dictation upstream failed"
    );
}

// ---------------------------------------------------------------------------
// chat (managed AI proxy)
// ---------------------------------------------------------------------------

/// One upstream attempt failed (primary or fallback). Each attempt settles
/// its own reservation, so this pairs with billing records.
pub fn chat_upstream_failed(
    account_id: Uuid,
    provider: &str,
    model: &str,
    reason: &'static str,
    status: Option<u16>,
) {
    tracing::warn!(
        event = "ai.chat.upstream_failed",
        account_id = %account_id,
        provider,
        model,
        reason,
        status,
        "managed chat upstream attempt failed"
    );
}

/// Streaming answer started. `input_chars` is the validated context budget,
/// never message content; `images` counts inline image parts.
#[allow(clippy::too_many_arguments)]
pub fn chat_started(
    account_id: Uuid,
    model: &str,
    provider: &str,
    input_chars: i64,
    max_tokens: i64,
    effort: &str,
    meeting: bool,
    conversation: bool,
    switched: bool,
    images: usize,
) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "ai.chat.started",
        account_id = %account_id,
        model,
        provider,
        input_chars,
        max_tokens,
        effort,
        meeting,
        conversation,
        switched,
        images,
        "managed chat started"
    );
}

/// Inline chat image pointed at a remote URL (SSRF probe). The request is
/// still rejected by validation; this records the attempt. Always emitted:
/// probing attempts must stay visible with business logging off.
pub fn chat_remote_image_blocked(account_id: Uuid) {
    tracing::warn!(
        event = "ai.chat.remote_image_blocked",
        account_id = %account_id,
        "inline chat image with remote URL refused"
    );
}

/// Background recovery settled a reservation the stream summary missed: the
/// in-stream settle failed, the cleanup stayed armed, and the retry after
/// the response drop succeeded. Always emitted so billed totals stay
/// auditable; correlate with `ai.chat.completed` via the stream's account.
pub fn chat_settlement_recovered(account_id: Uuid, reservation_id: Uuid) {
    tracing::info!(
        event = "ai.chat.settlement_recovered",
        account_id = %account_id,
        reservation_id = %reservation_id,
        "cancelled continuation settled on retry"
    );
}

/// Mid-answer failover to the alternate provider.
pub fn chat_switched(
    stream_id: Uuid,
    conversation_id: Option<Uuid>,
    model: &str,
    provider: &str,
    continued_chars: usize,
) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "ai.chat.switched",
        stream_id = %stream_id,
        conversation_id = ?conversation_id,
        model,
        provider,
        continued_chars,
        "managed chat failed over to alternate provider"
    );
}

/// Stream finished (complete answer or provider-exhausted fallback message).
/// `billed_cost` is settled microdollars across all attempts.
#[allow(clippy::too_many_arguments)]
pub fn chat_completed(
    account_id: Uuid,
    stream_id: Uuid,
    conversation_id: Option<Uuid>,
    model: &str,
    provider: &str,
    output_chars: usize,
    billed_cost: i64,
    switched: bool,
    complete: bool,
) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "ai.chat.completed",
        account_id = %account_id,
        stream_id = %stream_id,
        conversation_id = ?conversation_id,
        model,
        provider,
        output_chars,
        billed_cost,
        switched,
        complete,
        "managed chat completed"
    );
}

// ---------------------------------------------------------------------------
// sync
// ---------------------------------------------------------------------------

/// Push batch committed: mutation counts in, quota/storage effects out.
/// `kinds` is a compact `kind=count` breakdown (meeting, transcript, context,
/// ...). Individual record data is never logged.
#[allow(clippy::too_many_arguments)]
pub fn push_completed(
    account_id: Uuid,
    device_id: Uuid,
    mutations: usize,
    applied: usize,
    kinds: &str,
    storage_delta_bytes: i64,
    duration_ms: u64,
) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "sync.push.completed",
        account_id = %account_id,
        device_id = %device_id,
        mutations,
        applied,
        kinds,
        storage_delta_bytes,
        duration_ms,
        "sync push committed"
    );
}

/// Socket ended abnormally. Always emitted so service failures stay visible
/// when business events are disabled; the gated `sync.socket.closed` keeps
/// the full detail (duration, delivered count). Normal `client_closed` and
/// `shutdown` endings never call this.
pub fn socket_aborted(
    kind: &'static str,
    account_id: Uuid,
    session_id: Uuid,
    reason: &'static str,
) {
    tracing::warn!(
        event = "sync.socket.aborted",
        kind,
        account_id = %account_id,
        session_id = %session_id,
        reason,
        "realtime socket ended abnormally"
    );
}

/// Long-lived socket opened. `kind`: `sync`, `workspace`, or `events`.
/// This is the server-visible "client instance started" signal.
pub fn socket_opened(kind: &'static str, account_id: Uuid, session_id: Uuid) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "sync.socket.opened",
        kind,
        account_id = %account_id,
        session_id = %session_id,
        "realtime socket opened"
    );
}

/// Socket ended. `delivered` counts messages pushed to this socket;
/// `reason`: `client_closed`, `session_revoked`, `lease_lost`, `shutdown`.
pub fn socket_closed(
    kind: &'static str,
    account_id: Uuid,
    session_id: Uuid,
    duration_secs: u64,
    delivered: u64,
    reason: &'static str,
) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "sync.socket.closed",
        kind,
        account_id = %account_id,
        session_id = %session_id,
        duration_secs,
        delivered,
        reason,
        "realtime socket closed"
    );
}

// ---------------------------------------------------------------------------
// storage
// ---------------------------------------------------------------------------

/// Attachment stored (or deduplicated). Filename is user content: only the
/// MIME type and size are logged.
pub fn upload_completed(
    account_id: Uuid,
    attachment_id: Uuid,
    size_bytes: i64,
    content_type: &str,
    dedup: bool,
    duration_ms: u64,
) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "storage.upload.completed",
        account_id = %account_id,
        attachment_id = %attachment_id,
        size_bytes,
        content_type = mime_essence(content_type),
        dedup,
        duration_ms,
        "attachment stored"
    );
}

/// Upload refused by safety scanning. `reason`: `malware` or `scanner`.
pub fn upload_blocked(
    account_id: Uuid,
    reason: &'static str,
    content_type: &str,
    size_bytes: usize,
) {
    tracing::warn!(
        event = "storage.upload.blocked",
        account_id = %account_id,
        reason,
        content_type = mime_essence(content_type),
        size_bytes,
        "attachment refused by safety scanning"
    );
}

/// Attachment file deleted (metadata + quota follow in the same commit).
pub fn attachment_deleted(account_id: Uuid, attachment_id: Uuid, size_bytes: i64) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "storage.attachment.deleted",
        account_id = %account_id,
        attachment_id = %attachment_id,
        size_bytes,
        "attachment deleted"
    );
}

/// Background sweeper did real work. Silent when idle (runs every 60s).
pub fn cleanup_completed(jobs: u64, reclaimed_bytes: u64) {
    if !business_enabled() {
        return;
    }
    tracing::info!(
        event = "storage.cleanup.completed",
        jobs,
        reclaimed_bytes,
        "object cleanup sweep completed"
    );
}

// ---------------------------------------------------------------------------
// HTTP outcome (protected routes; runs inside the auth guard)
// ---------------------------------------------------------------------------

/// Protected client errors with account context. Server failures are logged
/// by the HTTP telemetry middleware for both public and protected routes.
pub fn protected_outcome(
    method: &str,
    path: &str,
    status: u16,
    account_id: Uuid,
    device_id: Uuid,
    session_id: Uuid,
    latency_ms: u64,
) {
    if status == 429 {
        tracing::warn!(
            event = "http.request.rate_limited",
            method,
            path,
            status,
            account_id = %account_id,
            device_id = %device_id,
            session_id = %session_id,
            latency_ms,
            "protected request rate limited"
        );
    } else if status == 402 {
        tracing::info!(
            event = "http.request.quota_exceeded",
            method,
            path,
            status,
            account_id = %account_id,
            device_id = %device_id,
            session_id = %session_id,
            latency_ms,
            "protected request blocked by subscription or quota"
        );
    } else if (400..500).contains(&status) {
        tracing::debug!(
            event = "http.request.client_error",
            method,
            path,
            status,
            account_id = %account_id,
            device_id = %device_id,
            session_id = %session_id,
            latency_ms,
            "protected request rejected"
        );
    }
}

/// Public-endpoint rate limit hit (no account context by definition). Callers
/// pass static route classes (`/auth/*`, `/api/*`), never raw paths: the
/// limit key may contain an IP and the path attacker-controlled segments.
pub fn public_rate_limited(path: &str, scope: &'static str) {
    tracing::warn!(
        event = "http.request.rate_limited",
        path,
        scope,
        "public request rate limited"
    );
}
