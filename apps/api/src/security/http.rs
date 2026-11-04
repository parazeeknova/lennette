use crate::{error::Result, state::AppState};
use axum::{
    extract::{ConnectInfo, State},
    http::{Request, header},
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;

pub async fn protect(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response> {
    if request.uri().path().starts_with("/health/") {
        return Ok(next.run(request).await);
    }
    // Only socket peers are trusted; never accept spoofable forwarding headers here.
    if let Some(ConnectInfo(peer)) = request.extensions().get::<ConnectInfo<SocketAddr>>() {
        let ip = if state.config.trusted_proxy_ips.contains(&peer.ip()) {
            request
                .headers()
                .get("cf-connecting-ip")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<std::net::IpAddr>().ok())
                .unwrap_or(peer.ip())
        } else {
            peer.ip()
        };
        let sensitive = request.uri().path().contains("/auth/");
        state
            .limit(
                format!("peer:{}:{}", ip, if sensitive { "auth" } else { "api" }),
                if sensitive { 30 } else { 1200 },
                60,
            )
            .await?;
    }
    if request.uri().path().contains("/auth/") || request.uri().path().contains("/browser/") {
        state.limit("public-auth-total".into(), 500, 60).await?;
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    response
        .headers_mut()
        .insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    response
        .headers_mut()
        .entry(header::CACHE_CONTROL)
        .or_insert("no-store".parse().unwrap());
    Ok(response)
}
