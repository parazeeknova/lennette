use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("authentication required")]
    Unauthorized,
    #[error("subscription required or quota exhausted")]
    PaymentRequired,
    #[error("{0}")]
    BadRequest(String),
    #[error("record changed on another device; pull and merge before retrying")]
    Conflict,
    #[error("not found")]
    NotFound,
    #[error("rate limit exceeded")]
    RateLimited,
    #[error("integration is not configured")]
    Unavailable,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}
impl From<sea_orm::DbErr> for ApiError {
    fn from(e: sea_orm::DbErr) -> Self {
        Self::Internal(e.into())
    }
}
impl From<fred::error::Error> for ApiError {
    fn from(e: fred::error::Error) -> Self {
        Self::Internal(e.into())
    }
}
impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        Self::Internal(e.without_url().into())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::PaymentRequired => (StatusCode::PAYMENT_REQUIRED, "quota_or_subscription"),
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "invalid_request"),
            Self::Conflict => (StatusCode::CONFLICT, "revision_conflict"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "not_configured"),
            Self::Internal(error) => {
                tracing::error!(error = %error, "request failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
            }
        };
        let message = if matches!(self, Self::Internal(_)) {
            "request failed".to_owned()
        } else {
            self.to_string()
        };
        (
            status,
            Json(json!({"error":{"code":code,"message":message}})),
        )
            .into_response()
    }
}
pub type Result<T> = std::result::Result<T, ApiError>;
