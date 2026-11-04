use crate::{
    auth::{self, Principal},
    billing,
    error::Result,
    plans, providers,
    state::AppState,
    sync,
};
use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderValue, StatusCode, header},
    middleware,
    routing::{delete, get, post},
};
use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use tower::ServiceBuilder;
use tower_http::{
    cors::CorsLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::SetSensitiveRequestHeadersLayer,
    trace::TraceLayer,
};

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/browser/link/{provider}", post(auth::linking::begin))
        .route("/me", get(me).delete(sync::delete_account))
        .route("/account/events", get(super::events::stream))
        .route("/browser/activity", post(auth::browser::activity))
        .route("/auth/logout", post(auth::logout))
        .route("/devices", get(auth::devices))
        .route("/devices/{id}", delete(auth::revoke))
        .route("/sync/push", post(sync::push))
        .route("/sync/pull", get(sync::pull))
        .route("/sync/ws", get(sync::socket))
        .route(
            "/attachments",
            post(sync::upload).layer(DefaultBodyLimit::max(10 * 1024 * 1024)),
        )
        .route(
            "/attachments/{id}",
            get(sync::download).delete(sync::delete_attachment),
        )
        .route("/billing/usage", get(billing::usage))
        .route("/billing/checkout", post(billing::checkout))
        .route("/billing/portal", post(billing::portal))
        .route("/billing/upgrade/quote", post(billing::upgrade::quote))
        .route("/billing/upgrade", post(billing::upgrade::confirm))
        .route("/chat/completions", post(providers::chat))
        .route("/speech/live", get(providers::speech))
        .route(
            "/speech/transcribe",
            post(providers::transcribe).layer(DefaultBodyLimit::max(10 * 1024 * 1024)),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::guard));
    let api=Router::new().merge(protected)
        .route("/browser/auth/{provider}",post(auth::browser::begin))
        .route("/browser/logout",post(auth::browser::logout_cookie))
        .route("/auth/{provider}/start",post(auth::start)).route("/auth/{provider}/callback",get(auth::callback))
        .route("/auth/exchange",post(auth::exchange)).route("/auth/refresh",post(auth::refresh))
        .route("/billing/webhook",post(billing::webhook))
        .route("/catalog",get(||async{Json(json!({"plans":[plans::STANDARD,plans::PLUS],"models":plans::MODELS,"api_version":1}))}));
    Router::new().nest("/v1",api)
        .route("/health/live",get(||async{StatusCode::NO_CONTENT})).route("/health/ready",get(ready))
        .layer(DefaultBodyLimit::max(4*1024*1024))
        .layer(middleware::from_fn_with_state(state.clone(), crate::security::http::protect))
        .layer(ServiceBuilder::new().layer(tower::limit::ConcurrencyLimitLayer::new(64))
            .layer(SetRequestIdLayer::new(header::HeaderName::from_static("x-request-id"),MakeRequestUuid))
            .layer(SetSensitiveRequestHeadersLayer::new([header::AUTHORIZATION,header::COOKIE]))
            .layer(TraceLayer::new_for_http().on_response(tower_http::trace::DefaultOnResponse::new().level(tracing::Level::DEBUG)).make_span_with(|request:&axum::http::Request<axum::body::Body>|tracing::info_span!("http_request",otel.kind="server",method=%request.method(),path=request.uri().path())))
            .layer(middleware::from_fn(crate::telemetry::context))
            .layer(PropagateRequestIdLayer::new(header::HeaderName::from_static("x-request-id")))
            .layer(CorsLayer::new().allow_credentials(true).allow_origin(state.config.website_url.origin().ascii_serialization().parse::<HeaderValue>().expect("validated website origin")).allow_headers([header::AUTHORIZATION,header::CONTENT_TYPE,header::HeaderName::from_static("x-delete-confirmation")]).allow_methods([axum::http::Method::GET,axum::http::Method::POST,axum::http::Method::DELETE])))
        .with_state(state)
}
async fn ready(State(state): State<AppState>) -> StatusCode {
    use fred::prelude::*;
    if state
        .db
        .execute_unprepared("SELECT 1 FROM accounts LIMIT 1; SELECT 1 FROM billing_holds LIMIT 1; SELECT 1 FROM object_uploads LIMIT 1; SELECT 1 FROM attachment_refs LIMIT 1")
        .await
        .is_err()
        || state.redis.ping::<String>(None).await.is_err()
    {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::NO_CONTENT
    }
}
async fn me(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
) -> Result<Json<Value>> {
    use sea_orm::EntityTrait;
    let a = crate::entities::accounts::Entity::find_by_id(p.account_id)
        .one(&state.db)
        .await?
        .ok_or(crate::error::ApiError::Unauthorized)?;
    use sea_orm::{ColumnTrait, QueryFilter};
    let providers: Vec<String> = crate::entities::identities::Entity::find()
        .filter(crate::entities::identities::Column::AccountId.eq(a.id))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|identity| identity.provider)
        .collect();
    Ok(Json(
        json!({"id":a.id,"email":a.email,"name":a.display_name,"device_id":p.device_id,"providers":providers}),
    ))
}
