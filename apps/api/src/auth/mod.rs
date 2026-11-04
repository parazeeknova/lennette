use crate::{
    entities::{accounts, identities, sessions},
    error::{ApiError, Result},
    state::AppState,
};
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::{Request, header},
    middleware::Next,
    response::{Redirect, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration, Utc};
use fred::prelude::*;
use lennette_protocol::{AuthExchange, AuthStart, Tokens};
use rand::RngExt;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QuerySelect, Set, TransactionTrait,
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use url::Url;
use uuid::Uuid;
use validator::Validate;

#[derive(Clone, Debug)]
pub struct Principal {
    pub account_id: Uuid,
    pub device_id: Uuid,
    pub session_id: Uuid,
}
pub fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::rng().random::<[u8; 32]>())
}
pub fn digest(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}
fn equal(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

pub fn validate_redirect(uri: &str, mobile: &[String]) -> Result<Url> {
    let url: Url = uri
        .parse()
        .map_err(|_| ApiError::BadRequest("invalid redirect URI".into()))?;
    let loopback = url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some_and(|p| p >= 1024)
        && url.path() == "/oauth/callback";
    let allowed_mobile = url.scheme() == "https" && mobile.iter().any(|s| s == uri);
    if (!loopback && !allowed_mobile)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        return Err(ApiError::BadRequest(
            "redirect must be a loopback callback or registered mobile link".into(),
        ));
    }
    Ok(url)
}

#[derive(Serialize, Deserialize)]
struct Pending {
    request: AuthStart,
    provider: String,
    provider_verifier: String,
}
#[derive(Serialize, Deserialize)]
struct Ticket {
    account_id: Uuid,
    request: AuthStart,
}

pub async fn start(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(request): Json<AuthStart>,
) -> Result<Json<Value>> {
    request
        .validate()
        .map_err(|_| ApiError::BadRequest("invalid device or PKCE challenge".into()))?;
    if URL_SAFE_NO_PAD
        .decode(&request.code_challenge)
        .ok()
        .is_none_or(|b| b.len() != 32)
    {
        return Err(ApiError::BadRequest("PKCE must use S256".into()));
    }
    validate_redirect(&request.redirect_uri, &state.config.mobile_redirects)?;
    state.limit("oauth-start".into(), 200, 60).await?;
    state
        .limit(format!("oauth:{}", request.device_id), 10, 60)
        .await?;
    let (config, endpoint, scope) = match provider.as_str() {
        "google" => (
            state.config.google.as_ref(),
            "https://accounts.google.com/o/oauth2/v2/auth",
            "openid email profile",
        ),
        "github" => (
            state.config.github.as_ref(),
            "https://github.com/login/oauth/authorize",
            "read:user user:email",
        ),
        _ => return Err(ApiError::BadRequest("unsupported identity provider".into())),
    };
    let config = config.ok_or(ApiError::Unavailable)?;
    let oauth_state = random_token();
    let verifier = random_token();
    let mut url: Url = endpoint.parse().unwrap();
    url.query_pairs_mut().extend_pairs([
        ("client_id", config.client_id.as_str()),
        ("response_type", "code"),
        ("redirect_uri", callback_url(&state, &provider)?.as_str()),
        ("scope", scope),
        ("state", &oauth_state),
        ("code_challenge", &digest(&verifier)),
        ("code_challenge_method", "S256"),
    ]);
    let pending = serde_json::to_string(&Pending {
        request,
        provider,
        provider_verifier: verifier,
    })
    .map_err(anyhow::Error::from)?;
    state
        .redis
        .set::<(), _, _>(
            format!("oauth:{}", digest(&oauth_state)),
            pending,
            Some(Expiration::EX(300)),
            None,
            false,
        )
        .await?;
    Ok(Json(
        json!({"authorization_url":url.as_str(), "expires_in":300}),
    ))
}
fn callback_url(state: &AppState, provider: &str) -> Result<Url> {
    state
        .config
        .public_url
        .join(&format!("/v1/auth/{provider}/callback"))
        .map_err(|e| ApiError::Internal(e.into()))
}
#[derive(Deserialize)]
pub struct Callback {
    state: String,
    code: Option<String>,
    error: Option<String>,
}

pub async fn callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(query): Query<Callback>,
) -> Result<Redirect> {
    let raw: Option<String> = state
        .redis
        .getdel(format!("oauth:{}", digest(&query.state)))
        .await?;
    let pending: Pending =
        serde_json::from_str(&raw.ok_or(ApiError::Unauthorized)?).map_err(anyhow::Error::from)?;
    if provider != pending.provider || query.error.is_some() {
        return Err(ApiError::Unauthorized);
    }
    let code = query.code.ok_or(ApiError::Unauthorized)?;
    let config = match provider.as_str() {
        "google" => state.config.google.as_ref(),
        "github" => state.config.github.as_ref(),
        _ => return Err(ApiError::Unauthorized),
    };
    let config = config.ok_or(ApiError::Unavailable)?;
    let token: Value = state
        .http
        .post(&config.token_url)
        .header(header::ACCEPT, "application/json")
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", &config.client_id),
            ("client_secret", config.client_secret.expose_secret()),
            ("code", &code),
            ("redirect_uri", callback_url(&state, &provider)?.as_str()),
            ("code_verifier", &pending.provider_verifier),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let token = token["access_token"]
        .as_str()
        .ok_or(ApiError::Unauthorized)?;
    let (subject, email, display_name) = if provider == "google" {
        let user: Value = state
            .http
            .get(&config.userinfo_url)
            .bearer_auth(token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if user["email_verified"] != true {
            return Err(ApiError::Unauthorized);
        }
        (
            user["sub"]
                .as_str()
                .ok_or(ApiError::Unauthorized)?
                .to_owned(),
            user["email"]
                .as_str()
                .ok_or(ApiError::Unauthorized)?
                .to_owned(),
            user["name"].as_str().unwrap_or("Lennette user").to_owned(),
        )
    } else {
        let user: Value = state
            .http
            .get(&config.userinfo_url)
            .header(header::USER_AGENT, "Lennette")
            .bearer_auth(token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let emails: Vec<Value> = state
            .http
            .get(config.emails_url.as_deref().ok_or(ApiError::Unavailable)?)
            .header(header::USER_AGENT, "Lennette")
            .bearer_auth(token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let email = emails
            .iter()
            .find(|e| e["verified"] == true && e["primary"] == true)
            .and_then(|e| e["email"].as_str())
            .ok_or(ApiError::Unauthorized)?;
        (
            user["id"]
                .as_u64()
                .ok_or(ApiError::Unauthorized)?
                .to_string(),
            email.to_owned(),
            user["name"]
                .as_str()
                .or_else(|| user["login"].as_str())
                .unwrap_or("Lennette user")
                .to_owned(),
        )
    };
    let account_id = find_or_create(&state, &provider, &subject, &email, &display_name).await?;
    let ticket = random_token();
    let value = serde_json::to_string(&Ticket {
        account_id,
        request: pending.request.clone(),
    })
    .map_err(anyhow::Error::from)?;
    state
        .redis
        .set::<(), _, _>(
            format!("ticket:{}", digest(&ticket)),
            value,
            Some(Expiration::EX(60)),
            None,
            false,
        )
        .await?;
    let mut redirect = validate_redirect(
        &pending.request.redirect_uri,
        &state.config.mobile_redirects,
    )?;
    redirect
        .query_pairs_mut()
        .append_pair("ticket", &ticket)
        .append_pair("state", &pending.request.code_challenge);
    Ok(Redirect::to(redirect.as_str()))
}

async fn find_or_create(
    state: &AppState,
    provider: &str,
    subject: &str,
    email: &str,
    name: &str,
) -> Result<Uuid> {
    let tx = state.db.begin().await?;
    // Advisory lock serializes simultaneous first sign-ins for the same provider identity.
    use sea_orm::ConnectionTrait;
    tx.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
        [format!("{provider}:{subject}").into()],
    ))
    .await?;
    if let Some(identity) = identities::Entity::find()
        .filter(identities::Column::Provider.eq(provider))
        .filter(identities::Column::Subject.eq(subject))
        .one(&tx)
        .await?
    {
        tx.commit().await?;
        return Ok(identity.account_id);
    }
    let id = Uuid::new_v4();
    accounts::ActiveModel {
        id: Set(id),
        email: Set(email.into()),
        display_name: Set(name.into()),
        created_at: Set(Utc::now()),
        sync_sequence: Set(0),
        storage_bytes: Set(0),
    }
    .insert(&tx)
    .await?;
    identities::ActiveModel {
        id: Set(Uuid::new_v4()),
        account_id: Set(id),
        provider: Set(provider.into()),
        subject: Set(subject.into()),
    }
    .insert(&tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn exchange(
    State(state): State<AppState>,
    Json(request): Json<AuthExchange>,
) -> Result<Json<Tokens>> {
    if !(43..=128).contains(&request.code_verifier.len()) {
        return Err(ApiError::Unauthorized);
    }
    let raw: Option<String> = state
        .redis
        .getdel(format!("ticket:{}", digest(&request.ticket)))
        .await?;
    let ticket: Ticket =
        serde_json::from_str(&raw.ok_or(ApiError::Unauthorized)?).map_err(anyhow::Error::from)?;
    if !equal(
        &digest(&request.code_verifier),
        &ticket.request.code_challenge,
    ) {
        return Err(ApiError::Unauthorized);
    }
    Ok(Json(
        issue(
            &state,
            ticket.account_id,
            ticket.request.device_id,
            &ticket.request.device_name,
        )
        .await?,
    ))
}

pub async fn issue(
    state: &AppState,
    account_id: Uuid,
    device_id: Uuid,
    name: &str,
) -> Result<Tokens> {
    let tx = state.db.begin().await?;
    let _account = accounts::Entity::find_by_id(account_id)
        .lock_exclusive()
        .one(&tx)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    sessions::Entity::update_many()
        .col_expr(
            sessions::Column::RevokedAt,
            sea_orm::sea_query::Expr::value(Utc::now()),
        )
        .filter(sessions::Column::AccountId.eq(account_id))
        .filter(sessions::Column::DeviceId.eq(device_id))
        .exec(&tx)
        .await?;
    // Unsubscribed users can still authenticate and read/export data.
    let count = sessions::Entity::find()
        .filter(sessions::Column::AccountId.eq(account_id))
        .filter(sessions::Column::RevokedAt.is_null())
        .filter(sessions::Column::RefreshExpiresAt.gt(Utc::now()))
        .all(&tx)
        .await?
        .len();
    let allowed = crate::billing::entitlement(&tx, account_id)
        .await
        .map(|(plan, _)| plan.max_devices)
        .unwrap_or(3);
    if count as u64 >= allowed {
        return Err(ApiError::BadRequest(
            "revoke a device before adding another".into(),
        ));
    }
    let id = Uuid::new_v4();
    let access = format!("{id}.{}", random_token());
    let refresh = format!("{id}.{}", random_token());
    sessions::ActiveModel {
        id: Set(id),
        account_id: Set(account_id),
        device_id: Set(device_id),
        device_name: Set(name.into()),
        access_hash: Set(digest(&access)),
        refresh_hash: Set(digest(&refresh)),
        access_expires_at: Set(Utc::now() + Duration::minutes(15)),
        refresh_expires_at: Set(Utc::now() + Duration::days(30)),
        revoked_at: Set(None),
        created_at: Set(Utc::now()),
    }
    .insert(&tx)
    .await?;
    tx.commit().await?;
    Ok(Tokens {
        access_token: access,
        refresh_token: refresh,
        expires_in: 900,
        account_id,
    })
}

#[derive(Deserialize)]
pub struct Refresh {
    pub refresh_token: String,
}
pub async fn refresh(
    State(state): State<AppState>,
    Json(request): Json<Refresh>,
) -> Result<Json<Tokens>> {
    let id = token_id(&request.refresh_token)?;
    state.limit(format!("refresh:{id}"), 30, 60).await?;
    let tx = state.db.begin().await?;
    let session = sessions::Entity::find_by_id(id)
        .lock_exclusive()
        .one(&tx)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if session.revoked_at.is_some() || session.refresh_expires_at <= Utc::now() {
        return Err(ApiError::Unauthorized);
    }
    if !equal(&session.refresh_hash, &digest(&request.refresh_token)) {
        // A stale refresh token is a replay: revoke the entire device session.
        let mut model: sessions::ActiveModel = session.into();
        model.revoked_at = Set(Some(Utc::now()));
        model.update(&tx).await?;
        tx.commit().await?;
        return Err(ApiError::Unauthorized);
    }
    let access = format!("{id}.{}", random_token());
    let refresh = format!("{id}.{}", random_token());
    let account_id = session.account_id;
    let mut model: sessions::ActiveModel = session.into();
    model.access_hash = Set(digest(&access));
    model.refresh_hash = Set(digest(&refresh));
    model.access_expires_at = Set(Utc::now() + Duration::minutes(15));
    model.refresh_expires_at = Set(Utc::now() + Duration::days(30));
    model.update(&tx).await?;
    tx.commit().await?;
    Ok(Json(Tokens {
        access_token: access,
        refresh_token: refresh,
        expires_in: 900,
        account_id,
    }))
}
fn token_id(token: &str) -> Result<Uuid> {
    token
        .split_once('.')
        .and_then(|(id, _)| id.parse().ok())
        .ok_or(ApiError::Unauthorized)
}
pub async fn authenticate(state: &AppState, token: &str) -> Result<Principal> {
    let session = sessions::Entity::find_by_id(token_id(token)?)
        .one(&state.db)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if session.revoked_at.is_some()
        || session.access_expires_at <= Utc::now()
        || !equal(&session.access_hash, &digest(token))
    {
        return Err(ApiError::Unauthorized);
    }
    Ok(Principal {
        account_id: session.account_id,
        device_id: session.device_id,
        session_id: session.id,
    })
}
pub async fn guard(
    State(state): State<AppState>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response> {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)?;
    let principal = authenticate(&state, token).await?;
    state
        .limit(format!("api:{}", principal.account_id), 300, 60)
        .await?;
    request.extensions_mut().insert(principal);
    Ok(next.run(request).await)
}
pub async fn logout(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
) -> Result<Json<Value>> {
    sessions::Entity::update_many()
        .col_expr(
            sessions::Column::RevokedAt,
            sea_orm::sea_query::Expr::value(Utc::now()),
        )
        .filter(sessions::Column::Id.eq(p.session_id))
        .exec(&state.db)
        .await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn devices(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
) -> Result<Json<Value>> {
    let devices = sessions::Entity::find()
        .filter(sessions::Column::AccountId.eq(p.account_id))
        .filter(sessions::Column::RevokedAt.is_null())
        .filter(sessions::Column::RefreshExpiresAt.gt(Utc::now()))
        .all(&state.db)
        .await?;
    Ok(Json(json!(devices.into_iter().map(|s|json!({"session_id":s.id,"device_id":s.device_id,"name":s.device_name,"created_at":s.created_at,"current":s.id==p.session_id})).collect::<Vec<_>>())))
}
pub async fn revoke(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    sessions::Entity::update_many()
        .col_expr(
            sessions::Column::RevokedAt,
            sea_orm::sea_query::Expr::value(Utc::now()),
        )
        .filter(sessions::Column::Id.eq(id))
        .filter(sessions::Column::AccountId.eq(p.account_id))
        .exec(&state.db)
        .await?;
    Ok(Json(json!({"ok":true})))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redirects_are_exact_and_never_arbitrary() {
        assert!(validate_redirect("http://127.0.0.1:49152/oauth/callback", &[]).is_ok());
        for bad in [
            "https://evil.test/oauth/callback",
            "http://localhost:5000/oauth/callback",
            "http://127.0.0.1.evil.test:5000/oauth/callback",
            "http://127.0.0.1:5000/oauth/callback?next=evil",
            "file:///etc/passwd",
        ] {
            assert!(validate_redirect(bad, &[]).is_err(), "{bad}")
        }
    }
    #[test]
    fn tokens_are_random_and_hashes_deterministic() {
        let a = random_token();
        assert_eq!(a.len(), 43);
        assert_ne!(a, random_token());
        assert_eq!(digest("hello"), digest("hello"));
        assert!(!equal("x", "y"));
    }
}
