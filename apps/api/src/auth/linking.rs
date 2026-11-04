//! Explicit provider linking, bound to a recent browser session and OAuth cookie.
use super::*;
use axum::{http::HeaderMap, response::IntoResponse};

#[derive(Serialize, Deserialize)]
pub(super) struct Target {
    account: Uuid,
    session: Uuid,
}

pub async fn begin(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
) -> Result<Response> {
    browser::origin(&state, &headers)?;
    let p = browser::authenticate_cookie(&state, &headers, false).await?;
    let session = sessions::Entity::find_by_id(p.session_id)
        .one(&state.db)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if session.created_at < Utc::now() - Duration::minutes(10) {
        return Err(ApiError::BadRequest(
            "Sign out and sign in again before connecting a sign-in method.".into(),
        ));
    }
    state.limit(format!("link:{}", p.account_id), 5, 60).await?;
    browser::begin_flow(
        state,
        provider,
        headers,
        Some(Target {
            account: p.account_id,
            session: p.session_id,
        }),
    )
    .await
}

pub(super) async fn complete(
    state: &AppState,
    headers: &HeaderMap,
    request: &AuthStart,
    target: Target,
    provider: &str,
    subject: &str,
) -> Result<Response> {
    let p = browser::authenticate_cookie(state, headers, false).await?;
    if p.account_id != target.account || p.session_id != target.session {
        return Err(ApiError::Unauthorized);
    }
    let expected: Option<String> = state
        .redis
        .getdel(format!("browser-login:{}", request.code_challenge))
        .await?;
    let binding =
        browser::cookie(headers, browser::oauth_name(state)).ok_or(ApiError::Unauthorized)?;
    if !expected.is_some_and(|expected| equal(&expected, &digest(&binding))) {
        return Err(ApiError::Unauthorized);
    }
    let outcome = attach(state, &p, provider, subject).await;
    let status = match outcome {
        Ok(()) => "connected",
        Err(ApiError::Conflict) => "conflict",
        Err(error) => return Err(error),
    };
    let mut url = state
        .config
        .website_url
        .join("/account")
        .map_err(anyhow::Error::from)?;
    url.query_pairs_mut().append_pair("link", status);
    let mut response = Redirect::to(url.as_str()).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        browser::set_cookie(state, browser::oauth_name(state), "", 0)
            .parse()
            .map_err(anyhow::Error::from)?,
    );
    Ok(response)
}

async fn attach(state: &AppState, p: &Principal, provider: &str, subject: &str) -> Result<()> {
    use sea_orm::ConnectionTrait;
    let tx = state.db.begin().await?;
    // Same identity lock used by ordinary sign-in: link and signup cannot race.
    tx.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
        [format!("{provider}:{subject}").into()],
    ))
    .await?;
    let account = accounts::Entity::find_by_id(p.account_id)
        .lock_exclusive()
        .one(&tx)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    let session = sessions::Entity::find_by_id(p.session_id)
        .lock_exclusive()
        .one(&tx)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if session.account_id != account.id
        || session.revoked_at.is_some()
        || session.refresh_expires_at <= Utc::now()
        || session.created_at < Utc::now() - Duration::minutes(10)
    {
        return Err(ApiError::Unauthorized);
    }
    if let Some(identity) = identities::Entity::find()
        .filter(identities::Column::Provider.eq(provider))
        .filter(identities::Column::Subject.eq(subject))
        .one(&tx)
        .await?
    {
        if identity.account_id != account.id {
            return Err(ApiError::Conflict);
        }
    } else {
        // One identity per provider on an account; never replace an existing login.
        if identities::Entity::find()
            .filter(identities::Column::AccountId.eq(account.id))
            .filter(identities::Column::Provider.eq(provider))
            .one(&tx)
            .await?
            .is_some()
        {
            return Err(ApiError::Conflict);
        }
        identities::ActiveModel {
            id: Set(Uuid::new_v4()),
            account_id: Set(account.id),
            provider: Set(provider.into()),
            subject: Set(subject.into()),
        }
        .insert(&tx)
        .await?;
    }
    tx.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Postgres,
        "SELECT pg_notify('lennette_account_events',$1)",
        [account.id.to_string().into()],
    ))
    .await?;
    tx.commit().await?;
    Ok(())
}
