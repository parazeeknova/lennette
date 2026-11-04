//! Host-only HttpOnly browser sessions; OAuth tokens never enter browser JavaScript.
use super::*;
use axum::{
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};

const IDLE: i64 = 30 * 60;
const ABSOLUTE: i64 = 12 * 60 * 60;
#[derive(Serialize, Deserialize)]
struct BrowserSession {
    session: Uuid,
    created: i64,
}

pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| {
            let (key, value) = part.trim().split_once('=')?;
            (key == name && value.len() <= 128).then(|| value.to_owned())
        })
}
fn name(state: &AppState) -> &'static str {
    if state.config.public_url.scheme() == "https" {
        "__Host-lennette"
    } else {
        "lennette-dev"
    }
}
fn oauth_name(state: &AppState) -> &'static str {
    if state.config.public_url.scheme() == "https" {
        "__Host-lennette-oauth"
    } else {
        "lennette-oauth"
    }
}
fn set_cookie(state: &AppState, name: &str, value: &str, seconds: i64) -> String {
    format!(
        "{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={seconds}{}",
        if state.config.public_url.scheme() == "https" {
            "; Secure"
        } else {
            ""
        }
    )
}
pub fn origin(state: &AppState, headers: &HeaderMap) -> Result<()> {
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok())
        != Some(
            state
                .config
                .website_url
                .origin()
                .ascii_serialization()
                .as_str(),
        )
    {
        return Err(ApiError::Unauthorized);
    }
    Ok(())
}
pub fn redirect(state: &AppState, uri: &str) -> Result<Url> {
    let expected = state
        .config
        .website_url
        .join("/account")
        .map_err(anyhow::Error::from)?;
    if uri == expected.as_str() {
        Ok(expected)
    } else {
        super::validate_redirect(uri, &state.config.mobile_redirects)
    }
}

pub async fn begin(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
) -> Result<Response> {
    origin(&state, &headers)?;
    let binding = random_token();
    let challenge = digest(&binding);
    let request = AuthStart {
        device_id: Uuid::new_v4(),
        device_name: "Web browser".into(),
        redirect_uri: state
            .config
            .website_url
            .join("/account")
            .map_err(anyhow::Error::from)?
            .to_string(),
        code_challenge: challenge.clone(),
    };
    let Json(result) = super::start(State(state.clone()), Path(provider), Json(request)).await?;
    state
        .redis
        .set::<(), _, _>(
            format!("browser-login:{challenge}"),
            digest(&binding),
            Some(Expiration::EX(300)),
            None,
            false,
        )
        .await?;
    Ok((
        [(
            header::SET_COOKIE,
            set_cookie(&state, oauth_name(&state), &binding, 300),
        )],
        Json(result),
    )
        .into_response())
}

pub async fn complete(
    state: &AppState,
    headers: &HeaderMap,
    account: Uuid,
    request: &AuthStart,
) -> Result<Option<Response>> {
    let raw: Option<String> = state
        .redis
        .getdel(format!("browser-login:{}", request.code_challenge))
        .await?;
    let Some(expected) = raw else {
        return Ok(None);
    };
    let binding = cookie(headers, oauth_name(state)).ok_or(ApiError::Unauthorized)?;
    if !equal(&expected, &digest(&binding)) {
        return Err(ApiError::Unauthorized);
    }
    let tokens = issue(state, account, request.device_id, "Web browser").await?;
    let row = sessions::Entity::find()
        .filter(sessions::Column::AccessHash.eq(digest(&tokens.access_token)))
        .one(&state.db)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    let mut row: sessions::ActiveModel = row.into();
    row.refresh_expires_at = Set(Utc::now() + Duration::seconds(IDLE));
    let row = row.update(&state.db).await?;
    let session = random_token();
    state
        .redis
        .set::<(), _, _>(
            format!("browser:{}", digest(&session)),
            serde_json::to_string(&BrowserSession {
                session: row.id,
                created: Utc::now().timestamp(),
            })
            .map_err(anyhow::Error::from)?,
            Some(Expiration::EX(IDLE)),
            None,
            false,
        )
        .await?;
    let mut response = Redirect::to(
        state
            .config
            .website_url
            .join("/account")
            .map_err(anyhow::Error::from)?
            .as_str(),
    )
    .into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        set_cookie(state, name(state), &session, ABSOLUTE)
            .parse()
            .map_err(anyhow::Error::from)?,
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        set_cookie(state, oauth_name(state), "", 0)
            .parse()
            .map_err(anyhow::Error::from)?,
    );
    Ok(Some(response))
}

pub async fn authenticate_cookie(
    state: &AppState,
    headers: &HeaderMap,
    touch: bool,
) -> Result<Principal> {
    let token = cookie(headers, name(state)).ok_or(ApiError::Unauthorized)?;
    let key = format!("browser:{}", digest(&token));
    let raw: Option<String> = state.redis.get(&key).await?;
    let browser: BrowserSession =
        serde_json::from_str(&raw.ok_or(ApiError::Unauthorized)?).map_err(anyhow::Error::from)?;
    let remaining = browser.created + ABSOLUTE - Utc::now().timestamp();
    let session = sessions::Entity::find_by_id(browser.session)
        .one(&state.db)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if remaining <= 0 || session.revoked_at.is_some() || session.refresh_expires_at <= Utc::now() {
        return Err(ApiError::Unauthorized);
    }
    if touch {
        state
            .redis
            .expire::<bool, _>(&key, remaining.min(IDLE), None)
            .await?;
        let mut session: sessions::ActiveModel = session.clone().into();
        session.refresh_expires_at = Set(Utc::now() + Duration::seconds(remaining.min(IDLE)));
        session.update(&state.db).await?;
    }
    Ok(Principal {
        account_id: session.account_id,
        device_id: session.device_id,
        session_id: session.id,
    })
}

pub async fn logout_cookie(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    origin(&state, &headers)?;
    if let Ok(p) = authenticate_cookie(&state, &headers, false).await {
        let _ = super::logout(State(state.clone()), Extension(p)).await?;
    }
    if let Some(value) = cookie(&headers, name(&state)) {
        state
            .redis
            .del::<(), _>(format!("browser:{}", digest(&value)))
            .await?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, set_cookie(&state, name(&state), "", 0))],
    )
        .into_response())
}

pub async fn activity(State(state): State<AppState>, headers: HeaderMap) -> Result<StatusCode> {
    origin(&state, &headers)?;
    authenticate_cookie(&state, &headers, true).await?;
    Ok(StatusCode::NO_CONTENT)
}
