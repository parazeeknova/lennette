use super::scenarios::{configured, serve};
use super::*;
use axum::{
    Json,
    extract::Form,
    http::header,
    routing::{get, post},
};
use std::collections::HashMap;

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    origin: Option<&str>,
) -> axum::response::Response {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    if let Some(origin) = origin {
        request = request.header(header::ORIGIN, origin);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn oauth_flow(
    app: &Router,
    path: &str,
    cookie: Option<&str>,
    origin: &str,
    provider: &str,
) -> (String, String) {
    let response = call(app, "POST", path, cookie, Some(origin)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let binding = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let url: url::Url = body["authorization_url"].as_str().unwrap().parse().unwrap();
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    (
        binding,
        format!("/v1/auth/{provider}/callback?state={state}&code=mock"),
    )
}

async fn browser_cookie(state: &AppState, tokens: &lennette_protocol::Tokens) -> (String, Uuid) {
    let p = auth::authenticate(state, &tokens.access_token)
        .await
        .unwrap();
    let token = auth::random_token();
    state
        .redis
        .set::<(), _, _>(
            format!("browser:{}", auth::digest(&token)),
            json!({"session":p.session_id,"created":Utc::now().timestamp()}).to_string(),
            Some(Expiration::EX(1800)),
            None,
            false,
        )
        .await
        .unwrap();
    (format!("lennette-dev={token}"), p.session_id)
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn provider_linking_preserves_account_and_rejects_conflicts_csrf_and_stale_sessions() {
    use lennette_server::entities::{identities, sessions};
    let (original, _, account, tokens) = fixture().await;
    let email = accounts::Entity::find_by_id(account)
        .one(&original.db)
        .await
        .unwrap()
        .unwrap()
        .email;
    let github_subject = (Uuid::new_v4().as_u128() as u64) / 2;
    let google_subject = Uuid::new_v4().to_string();
    let google =
        json!({"sub":google_subject,"email":email,"email_verified":true,"name":"Link test"});
    let github = json!({"id":github_subject,"name":"Link test"});
    let emails = json!([{"email":email,"verified":true,"primary":true}]);
    let mock = Router::new()
        .route(
            "/token",
            post(|| async { Json(json!({"access_token":"mock"})) }),
        )
        .route(
            "/google",
            get(move || {
                let value = google.clone();
                async move { Json(value) }
            }),
        )
        .route(
            "/github",
            get(move || {
                let value = github.clone();
                async move { Json(value) }
            }),
        )
        .route(
            "/emails",
            get(move || {
                let value = emails.clone();
                async move { Json(value) }
            }),
        );
    let (base, task) = serve(mock).await;
    let mut config = original.config.clone();
    config.google = Some(lennette_server::config::OAuthConfig {
        client_id: "test".into(),
        client_secret: "test".to_owned().into(),
        token_url: format!("{base}/token"),
        userinfo_url: format!("{base}/google"),
        emails_url: None,
    });
    config.github = Some(lennette_server::config::OAuthConfig {
        client_id: "test".into(),
        client_secret: "test".to_owned().into(),
        token_url: format!("{base}/token"),
        userinfo_url: format!("{base}/github"),
        emails_url: Some(format!("{base}/emails")),
    });
    let state = configured(&original, config);
    let app = routes::router(state.clone());
    let origin = state.config.website_url.origin().ascii_serialization();
    let (cookie, session) = browser_cookie(&state, &tokens).await;
    // Equal verified emails alone must not attach an unrelated provider identity.
    let (binding, callback) =
        oauth_flow(&app, "/v1/browser/auth/google", None, &origin, "google").await;
    let response = call(&app, "GET", &callback, Some(&binding), None).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let separate = identities::Entity::find()
        .filter(identities::Column::Provider.eq("google"))
        .filter(identities::Column::Subject.eq(&google_subject))
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(separate.account_id, account);
    accounts::Entity::delete_by_id(separate.account_id)
        .exec(&state.db)
        .await
        .unwrap();
    assert_eq!(
        call(&app, "POST", "/v1/browser/link/github", None, Some(&origin))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/browser/link/github",
            Some(&cookie),
            Some("https://evil.test")
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let (_, callback) = oauth_flow(
        &app,
        "/v1/browser/link/github",
        Some(&cookie),
        &origin,
        "github",
    )
    .await;
    assert_eq!(
        call(&app, "GET", &callback, Some(&cookie), None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(
        identities::Entity::find()
            .filter(identities::Column::Subject.eq(github_subject.to_string()))
            .one(&state.db)
            .await
            .unwrap()
            .is_none()
    );
    let (binding, callback) = oauth_flow(
        &app,
        "/v1/browser/link/github",
        Some(&cookie),
        &origin,
        "github",
    )
    .await;
    let response = call(
        &app,
        "GET",
        &callback,
        Some(&format!("{cookie}; {binding}")),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(
        response.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .ends_with("?link=connected")
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &callback,
            Some(&format!("{cookie}; {binding}")),
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    ); // single-use state
    // Connect the other provider and verify both normal sign-ins use this account.
    let (binding, callback) = oauth_flow(
        &app,
        "/v1/browser/link/google",
        Some(&cookie),
        &origin,
        "google",
    )
    .await;
    assert_eq!(
        call(
            &app,
            "GET",
            &callback,
            Some(&format!("{cookie}; {binding}")),
            None
        )
        .await
        .status(),
        StatusCode::SEE_OTHER
    );
    for provider in ["google", "github"] {
        let (binding, callback) = oauth_flow(
            &app,
            &format!("/v1/browser/auth/{provider}"),
            None,
            &origin,
            provider,
        )
        .await;
        let response = call(&app, "GET", &callback, Some(&binding), None).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let signed_cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find(|value| value.to_str().unwrap().starts_with("lennette-dev="))
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let me = call(&app, "GET", "/v1/me", Some(signed_cookie), None).await;
        let me: Value =
            serde_json::from_slice(&me.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(me["id"], account.to_string());
        assert_eq!(me["providers"].as_array().unwrap().len(), 2);
    }
    // Another account cannot steal an identity, even with a verified provider login.
    let (_, _, other, other_tokens) = fixture().await;
    let (other_cookie, _) = browser_cookie(&state, &other_tokens).await;
    let (binding, callback) = oauth_flow(
        &app,
        "/v1/browser/link/github",
        Some(&cookie),
        &origin,
        "github",
    )
    .await;
    assert_eq!(
        call(
            &app,
            "GET",
            &callback,
            Some(&format!("{other_cookie}; {binding}")),
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let (binding, callback) = oauth_flow(
        &app,
        "/v1/browser/link/github",
        Some(&other_cookie),
        &origin,
        "github",
    )
    .await;
    let response = call(
        &app,
        "GET",
        &callback,
        Some(&format!("{other_cookie}; {binding}")),
        None,
    )
    .await;
    assert!(
        response.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .ends_with("?link=conflict")
    );
    assert_eq!(
        identities::Entity::find()
            .filter(identities::Column::Subject.eq(github_subject.to_string()))
            .one(&state.db)
            .await
            .unwrap()
            .unwrap()
            .account_id,
        account
    );
    let mut row: sessions::ActiveModel = sessions::Entity::find_by_id(session)
        .one(&state.db)
        .await
        .unwrap()
        .unwrap()
        .into();
    row.created_at = Set(Utc::now() - Duration::minutes(11));
    row.update(&state.db).await.unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/browser/link/github",
            Some(&cookie),
            Some(&origin)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    task.abort();
    accounts::Entity::delete_by_id(other)
        .exec(&state.db)
        .await
        .unwrap();
    cleanup(original, account).await;
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn auth_abuse_is_limited_by_socket_peer_and_forwarded_headers_are_untrusted() {
    let (state, app, account, _) = fixture().await;
    let ip = std::net::Ipv6Addr::from(Uuid::new_v4().as_u128());
    for attempt in 0..31 {
        let mut req = Request::builder()
            .method("POST")
            .uri("/v1/browser/auth/google")
            .header("origin", "https://evil.test")
            .header("cf-connecting-ip", format!("10.0.0.{}", attempt + 1))
            .body(Body::empty())
            .unwrap();
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::new(
                ip.into(),
                5000,
            )));
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            response.status(),
            if attempt == 30 {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::UNAUTHORIZED
            }
        );
        if attempt == 30 {
            assert_eq!(response.headers()["retry-after"], "60");
        }
    }
    cleanup(state, account).await;
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn browser_oauth_cookie_csrf_expiry_revocation_and_events() {
    let (original, _, fixture_id, _) = fixture().await;
    let profile = json!({"sub":Uuid::new_v4().to_string(),"email":"browser@example.test","email_verified":true,"name":"Browser Test"});
    let mock = Router::new()
        .route(
            "/token",
            post(|Form(_): Form<HashMap<String, String>>| async {
                Json(json!({"access_token":"mock-provider"}))
            }),
        )
        .route(
            "/userinfo",
            get(move || {
                let profile = profile.clone();
                async move { Json(profile) }
            }),
        );
    let (base, task) = serve(mock).await;
    let mut config = original.config.clone();
    config.google = Some(lennette_server::config::OAuthConfig {
        client_id: "test".into(),
        client_secret: "test-secret".to_owned().into(),
        token_url: format!("{base}/token"),
        userinfo_url: format!("{base}/userinfo"),
        emails_url: None,
    });
    let state = configured(&original, config);
    let mut events_ready = state.events.subscribe();
    lennette_server::http::events::start(state.clone());
    tokio::time::timeout(std::time::Duration::from_secs(3), events_ready.recv())
        .await
        .unwrap()
        .unwrap();
    let app = routes::router(state.clone());
    let origin = state.config.website_url.origin().ascii_serialization();
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/browser/auth/google",
            None,
            Some("https://evil.test")
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let started = call(&app, "POST", "/v1/browser/auth/google", None, Some(&origin)).await;
    assert_eq!(started.status(), StatusCode::OK);
    let binding = started.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let value: Value =
        serde_json::from_slice(&started.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let authorization: url::Url = value["authorization_url"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let params: HashMap<_, _> = authorization.query_pairs().into_owned().collect();
    let callback = format!(
        "/v1/auth/google/callback?state={}&code=mock",
        params["state"]
    );
    let response = call(&app, "GET", &callback, Some(&binding), None).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let cookie = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .find(|v| v.to_str().unwrap().starts_with("lennette-dev="))
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        cookie.contains("HttpOnly")
            && cookie.contains("SameSite=Lax")
            && !cookie.contains("Domain=")
    );
    let cookie = cookie.split(';').next().unwrap().to_owned();
    assert!(
        !response.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .contains("ticket")
    );
    let me = call(&app, "GET", "/v1/me", Some(&cookie), None).await;
    assert_eq!(me.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&me.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let account: Uuid = value["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/browser/activity",
            Some(&cookie),
            Some("https://evil.test")
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/browser/activity",
            Some(&cookie),
            Some(&origin)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let response = call(
        &app,
        "GET",
        "/v1/account/events",
        Some(&cookie),
        Some(&origin),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    use futures_util::StreamExt;
    let connected = stream.next().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&connected).contains("changed"));
    accounts::Entity::update_many()
        .col_expr(
            accounts::Column::DisplayName,
            sea_orm::sea_query::Expr::value("Changed by a second device"),
        )
        .filter(accounts::Column::Id.eq(account))
        .exec(&state.db)
        .await
        .unwrap();
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&event).contains("account"));
    drop(stream);
    assert_eq!(
        call(&app, "DELETE", "/v1/me", Some(&cookie), Some(&origin))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    let token = cookie.split_once('=').unwrap().1;
    let key = format!("browser:{}", auth::digest(token));
    let raw: String = state.redis.get(&key).await.unwrap();
    let mut session: Value = serde_json::from_str(&raw).unwrap();
    session["created"] = json!(Utc::now().timestamp() - 13 * 3600);
    state
        .redis
        .set::<(), _, _>(
            &key,
            session.to_string(),
            Some(Expiration::EX(1800)),
            None,
            false,
        )
        .await
        .unwrap();
    assert_eq!(
        call(&app, "GET", "/v1/me", Some(&cookie), None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    state.shutdown.cancel();
    session["created"] = json!(Utc::now().timestamp());
    state
        .redis
        .set::<(), _, _>(
            &key,
            session.to_string(),
            Some(Expiration::EX(1800)),
            None,
            false,
        )
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/browser/logout",
            Some(&cookie),
            Some(&origin)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "GET", "/v1/me", Some(&cookie), None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    task.abort();
    accounts::Entity::delete_by_id(account)
        .exec(&state.db)
        .await
        .unwrap();
    cleanup(original, fixture_id).await;
}
