// Shared fixtures are compiled from integration.rs; each scenario uses isolated accounts.
use super::*;
use axum::{
    Json,
    extract::{
        Form, Query,
        ws::{Message, WebSocketUpgrade},
    },
    http::{HeaderMap, header},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration as StdDuration,
};
use tokio::sync::RwLock;

pub(super) fn configured(state: &AppState, config: Config) -> AppState {
    AppState(Arc::new(lennette_server::state::Inner {
        config,
        db: state.db.clone(),
        redis: state.redis.clone(),
        http: state.http.clone(),
        shutdown: state.shutdown.clone(),
        attachments: state.attachments.clone(),
        upload_permits: state.upload_permits.clone(),
        events: state.events.clone(),
    }))
}
pub(super) async fn serve(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, task)
}
pub(super) async fn body_request(
    app: &Router,
    path: &str,
    token: &str,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    (
        response.status(),
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn native_oauth_browser_return_and_pkce_exchange_are_single_use() {
    let (state, _, fixture_id, _) = fixture().await;
    let subject = Uuid::new_v4().to_string();
    let profile = json!({"sub":subject,"email":"verified@example.test","email_verified":true,"name":"OAuth Test"});
    let token_calls = Arc::new(AtomicUsize::new(0));
    let calls = token_calls.clone();
    let mock = Router::new()
        .route(
            "/token",
            post(move |Form(form): Form<HashMap<String, String>>| {
                let calls = calls.clone();
                async move {
                    assert_eq!(form["client_id"], "test-client");
                    assert_eq!(form["client_secret"], "test-only-secret");
                    assert_eq!(form["grant_type"], "authorization_code");
                    assert_eq!(form["code_verifier"].len(), 43);
                    calls.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"access_token":"test-provider-token"}))
                }
            }),
        )
        .route(
            "/userinfo",
            get(move |headers: HeaderMap| {
                let profile = profile.clone();
                async move {
                    assert_eq!(headers["authorization"], "Bearer test-provider-token");
                    Json(profile)
                }
            }),
        );
    let (provider, provider_task) = serve(mock).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = format!("http://{}", listener.local_addr().unwrap());
    let mut config = state.config.clone();
    config.public_url = api.parse().unwrap();
    config.google = Some(lennette_server::config::OAuthConfig {
        client_id: "test-client".into(),
        client_secret: SecretString::from("test-only-secret"),
        token_url: format!("{provider}/token"),
        userinfo_url: format!("{provider}/userinfo"),
        emails_url: None,
    });
    let state = configured(&state, config);
    let app = routes::router(state.clone());
    let app_clone = app.clone();
    let api_task = tokio::spawn(async move { axum::serve(listener, app_clone).await.unwrap() });
    let login = lennette_sync_client::Login::begin(&api, "google", Uuid::new_v4())
        .await
        .unwrap();
    let authorization: url::Url = login.authorization_url.parse().unwrap();
    let query = authorization
        .query_pairs()
        .into_owned()
        .collect::<HashMap<_, _>>();
    assert_eq!(query["code_challenge_method"], "S256");
    let callback = format!(
        "/v1/auth/google/callback?state={}&code=test-auth-code",
        query["state"]
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&callback)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let return_url = response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_owned();
    let finished = tokio::spawn(login.finish());
    reqwest::get(&return_url)
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let client = finished.await.unwrap().unwrap();
    let me: Value = client
        .request(reqwest::Method::GET, "/v1/me", &())
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(me["email"], "verified@example.test");
    assert_eq!(token_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        request(&app, "GET", &callback, None, json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
    let account = client.account_id().await;
    accounts::Entity::delete_by_id(account)
        .exec(&state.db)
        .await
        .unwrap();
    api_task.abort();
    provider_task.abort();
    cleanup(state, fixture_id).await;
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn attachments_are_idempotent_scoped_bounded_and_readable_after_expiry() {
    let (state, app, id, t) = fixture().await;
    subscribe(&state, id).await;
    let blob = Uuid::new_v4();
    let path = format!("/v1/attachments?id={blob}&name=notes.txt&content_type=text%2Fplain");
    let (status, body) =
        body_request(&app, &path, &t.access_token, b"Private notes".to_vec()).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(
        body_request(&app, &path, &t.access_token, b"Private notes".to_vec())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        body_request(&app, &path, &t.access_token, b"changed bytes".to_vec())
            .await
            .0,
        StatusCode::CONFLICT
    );
    let account = accounts::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.storage_bytes, 13);
    let (state2, app2, id2, t2) = fixture().await;
    subscribe(&state2, id2).await;
    assert_eq!(
        request(
            &app2,
            "GET",
            &format!("/v1/attachments/{blob}"),
            Some(&t2.access_token),
            json!({})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        body_request(&app2, &path, &t2.access_token, b"Private notes".to_vec())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        body_request(
            &app,
            "/v1/attachments?name=raw.wav&content_type=audio%2Fwav",
            &t.access_token,
            vec![1; 32]
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        body_request(&app, &path, &t.access_token, vec![1; 10 * 1024 * 1024 + 1])
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    subscriptions::Entity::update_many()
        .col_expr(
            subscriptions::Column::Status,
            sea_orm::sea_query::Expr::value("on_hold"),
        )
        .filter(subscriptions::Column::AccountId.eq(id))
        .exec(&state.db)
        .await
        .unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/attachments/{blob}"))
                .header("authorization", format!("Bearer {}", t.access_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "Private notes"
    );
    assert_eq!(
        request(
            &app,
            "DELETE",
            &format!("/v1/attachments/{blob}"),
            Some(&t.access_token),
            json!({})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    cleanup(state2, id2).await;
    cleanup(state, id).await;
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn deleting_a_conversation_erases_replay_content_and_children_even_after_expiry() {
    let (state, app, id, t) = fixture().await;
    subscribe(&state, id).await;
    let parent = Uuid::new_v4();
    let child = Uuid::new_v4();
    let mut message = mutation(child, 0, "");
    message["kind"] = json!("message");
    message["data"] =
        json!({"conversation_id":parent,"content":"Erase this secret content","role":"user"});
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/sync/push",
            Some(&t.access_token),
            json!({"mutations":[mutation(parent,0,"Private title"),message]})
        )
        .await
        .0,
        StatusCode::OK
    );
    subscriptions::Entity::update_many()
        .col_expr(
            subscriptions::Column::Status,
            sea_orm::sea_query::Expr::value("expired"),
        )
        .filter(subscriptions::Column::AccountId.eq(id))
        .exec(&state.db)
        .await
        .unwrap();
    let mut deletion = mutation(parent, 1, "");
    deletion["deleted"] = json!(true);
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/sync/push",
            Some(&t.access_token),
            json!({"mutations":[deletion]})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, page) = request(
        &app,
        "GET",
        "/v1/sync/pull?after=0",
        Some(&t.access_token),
        json!({}),
    )
    .await;
    assert!(!page.to_string().contains("Erase this secret content"));
    assert!(!page.to_string().contains("Private title"));
    assert!(
        page["changes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["deleted"] == true)
    );
    cleanup(state, id).await;
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn merge_streaming_settles_usage_before_done_and_never_exposes_provider_key() {
    let (state, _, id, t) = fixture().await;
    subscribe(&state, id).await;
    let mock=Router::new().route("/chat",post(|headers:HeaderMap,Json(body):Json<Value>|async move{
        assert_eq!(headers["authorization"],"Bearer mock-merge-key");assert_eq!(body["model"],"openai/gpt-6-luna");assert_eq!(body["stream_options"]["include_usage"],true);
        ([(header::CONTENT_TYPE,"text/event-stream")],"data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":20,\"completion_tokens\":3}}\n\ndata: [DONE]\n\n")
    }));
    let (base, task) = serve(mock).await;
    let mut config = state.config.clone();
    config.merge_key = SecretString::from("mock-merge-key");
    config.merge_url = format!("{base}/chat").parse().unwrap();
    let state = configured(&state, config);
    let app = routes::router(state.clone());
    let payload = json!({"model":"openai/gpt-6-luna","messages":[{"role":"user","content":"hello"}],"stream":true});
    let (status, bytes) = body_request(
        &app,
        "/v1/chat/completions",
        &t.access_token,
        serde_json::to_vec(&payload).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("Hello"));
    assert!(text.contains("[DONE]"));
    assert!(!text.contains("mock-merge-key"));
    let bucket = usage_buckets::Entity::find()
        .filter(usage_buckets::Column::AccountId.eq(id))
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bucket.ai_spent, 4);
    let premium = json!({"model":"anthropic/claude-sonnet-5-5","messages":[{"role":"user","content":"hello"}],"stream":true});
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/chat/completions",
            Some(&t.access_token),
            premium
        )
        .await
        .0,
        StatusCode::PAYMENT_REQUIRED
    );
    task.abort();
    cleanup(state, id).await;
}

fn signed_headers(id: &str, body: &[u8], secret: &[u8]) -> HeaderMap {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;
    let timestamp = Utc::now().timestamp();
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret).unwrap();
    mac.update(format!("{id}.{timestamp}.").as_bytes());
    mac.update(body);
    let mut headers = HeaderMap::new();
    headers.insert("webhook-id", id.parse().unwrap());
    headers.insert("webhook-timestamp", timestamp.to_string().parse().unwrap());
    headers.insert(
        "webhook-signature",
        format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()))
            .parse()
            .unwrap(),
    );
    headers
}
async fn send_webhook(app: &Router, event: &str, body: Value) -> StatusCode {
    let bytes = serde_json::to_vec(&body).unwrap();
    let headers = signed_headers(event, &bytes, b"test-webhook-secret");
    let mut request = Request::builder().method("POST").uri("/v1/billing/webhook");
    *request.headers_mut().unwrap() = headers;
    app.clone()
        .oneshot(request.body(Body::from(bytes)).unwrap())
        .await
        .unwrap()
        .status()
}
#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn dodo_checkout_signed_webhooks_reconcile_current_state_and_deduplicate() {
    let (state, _, id, t) = fixture().await;
    let sub_id = format!("sub_{}", id.simple());
    let current = Arc::new(RwLock::new(
        json!({"subscription_id":sub_id,"product_id":"pdt_standard","customer":{"customer_id":"cus_test"},"metadata":{"account_id":id.to_string()},"status":"active","previous_billing_date":(Utc::now()-Duration::days(1)).to_rfc3339(),"next_billing_date":(Utc::now()+Duration::days(29)).to_rfc3339()}),
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    let current_clone = current.clone();
    let calls_clone = calls.clone();
    let mock=Router::new().route("/checkouts",post(move|Json(body):Json<Value>|async move{assert_eq!(body["metadata"]["account_id"],id.to_string());assert_eq!(body["product_cart"][0]["product_id"],"pdt_standard");Json(json!({"session_id":"cks_test","checkout_url":"https://test.checkout.dodopayments.com/session/cks_test"}))}))
        .route("/subscriptions/{id}",get(move||{let current=current_clone.clone();let calls=calls_clone.clone();async move{calls.fetch_add(1,Ordering::SeqCst);Json(current.read().await.clone())}}));
    let (base, task) = serve(mock).await;
    let mut config = state.config.clone();
    config.dodo_url = format!("{base}/").parse().unwrap();
    config.dodo_key = SecretString::from("mock-dodo-key");
    use base64::{Engine, engine::general_purpose::STANDARD};
    config.dodo_webhook_secret =
        SecretString::from(format!("whsec_{}", STANDARD.encode(b"test-webhook-secret")));
    let state = configured(&state, config);
    let app = routes::router(state.clone());
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/billing/checkout",
            Some(&t.access_token),
            json!({"plan":"managed"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let event = format!("evt_{}", Uuid::new_v4());
    let body = json!({"type":"subscription.active","data":{"subscription_id":sub_id}});
    assert_eq!(
        send_webhook(&app, &event, body.clone()).await,
        StatusCode::OK
    );
    assert!(billing::entitlement(&state.db, id).await.is_ok());
    assert_eq!(
        send_webhook(&app, &event, body.clone()).await,
        StatusCode::OK
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    current.write().await["status"] = json!("on_hold");
    assert_eq!(
        send_webhook(&app, &format!("evt_{}", Uuid::new_v4()), body).await,
        StatusCode::OK
    );
    assert!(billing::entitlement(&state.db, id).await.is_err());
    current.write().await["status"] = json!("active");
    let refund = json!({"type":"refund.succeeded","data":{"subscription_id":sub_id}});
    assert_eq!(
        send_webhook(&app, &format!("evt_{}", Uuid::new_v4()), refund).await,
        StatusCode::OK
    );
    assert!(billing::entitlement(&state.db, id).await.is_err());
    let old_activation = json!({"type":"subscription.active","data":{"subscription_id":sub_id}});
    assert_eq!(
        send_webhook(&app, &format!("evt_{}", Uuid::new_v4()), old_activation).await,
        StatusCode::OK
    );
    assert!(billing::entitlement(&state.db, id).await.is_err());
    task.abort();
    cleanup(state, id).await;
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn remote_device_revocation_notifies_sync_socket_immediately() {
    use tokio_tungstenite::tungstenite::{Message as WireMessage, client::IntoClientRequest};
    let (state, app, account, desktop) = fixture().await;
    let browser = auth::issue(&state, account, Uuid::new_v4(), "Browser")
        .await
        .unwrap();
    let principal = auth::authenticate(&state, &desktop.access_token)
        .await
        .unwrap();
    let (api, task) = serve(app.clone()).await;
    let mut request_ws = format!("{}/v1/sync/ws?after=0", api.replace("http://", "ws://"))
        .into_client_request()
        .unwrap();
    request_ws.headers_mut().insert(
        "authorization",
        format!("Bearer {}", desktop.access_token).parse().unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request_ws).await.unwrap();
    // The first heartbeat proves the Redis subscription has been installed.
    tokio::time::timeout(StdDuration::from_secs(5), async {
        loop {
            if let WireMessage::Ping(payload) = socket.next().await.unwrap().unwrap() {
                socket.send(WireMessage::Pong(payload)).await.unwrap();
                break;
            }
        }
    })
    .await
    .unwrap();
    let started = std::time::Instant::now();
    assert_eq!(
        request(
            &app,
            "DELETE",
            &format!("/v1/devices/{}", principal.session_id),
            Some(&browser.access_token),
            json!(null)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(next_json(&mut socket).await["type"], "session_revoked");
    assert!(
        started.elapsed() < StdDuration::from_secs(2),
        "Revocation must not wait for the periodic authentication check"
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/v1/me",
            Some(&desktop.access_token),
            json!(null)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/v1/me",
            Some(&browser.access_token),
            json!(null)
        )
        .await
        .0,
        StatusCode::OK
    );
    task.abort();
    cleanup(state, account).await;
}

async fn next_json(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    tokio::time::timeout(StdDuration::from_secs(5), async {
        loop {
            match socket.next().await.unwrap().unwrap() {
                tokio_tungstenite::tungstenite::Message::Text(text) => {
                    return serde_json::from_str(&text).unwrap();
                }
                tokio_tungstenite::tungstenite::Message::Ping(data) => {
                    socket
                        .send(tokio_tungstenite::tungstenite::Message::Pong(data))
                        .await
                        .unwrap();
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn live_transcripts_reach_another_replica_and_audio_is_not_stored() {
    let (state, _, id, t) = fixture().await;
    subscribe(&state, id).await;
    let mock=Router::new().route("/listen",get(|headers:HeaderMap,Query(query):Query<HashMap<String,String>>,ws:WebSocketUpgrade|async move{
        assert_eq!(headers["authorization"],"Token mock-deepgram-key");assert_eq!(query["channels"],"2");assert_eq!(query["mip_opt_out"],"true");
        ws.on_upgrade(|mut socket|async move{
            while let Some(Ok(message))=socket.recv().await{match message{
                Message::Binary(_)=>{socket.send(Message::Text(json!({"type":"Results","channel_index":[0,2],"is_final":true,"speech_final":true,"channel":{"alternatives":[{"transcript":"Live meeting words","words":[]}]}}).to_string().into())).await.ok();},
                Message::Text(text) if text==r#"{"type":"KeepAlive"}"# => { socket.send(Message::Text(json!({"type":"Metadata","keepalive_received":true}).to_string().into())).await.unwrap(); },
                Message::Close(_)=>break,_=>{},
            }}
        })
    }));
    let (provider, provider_task) = serve(mock).await;
    let mut config = state.config.clone();
    config.deepgram_key = SecretString::from("mock-deepgram-key");
    config.deepgram_url = format!("{}/listen", provider.replace("http://", "ws://"))
        .parse()
        .unwrap();
    let state = configured(&state, config);
    let app = routes::router(state.clone());
    let meeting = Uuid::new_v4();
    let mut create = mutation(meeting, 0, "Meeting");
    create["kind"] = json!("meeting");
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/sync/push",
            Some(&t.access_token),
            json!({"mutations":[create]})
        )
        .await
        .0,
        StatusCode::OK
    );
    let device = auth::issue(&state, id, Uuid::new_v4(), "Mobile")
        .await
        .unwrap();
    let replica = AppState::connect(state.config.clone()).await.unwrap();
    let (replica_url, replica_task) = serve(routes::router(replica.clone())).await;
    let (api, api_task) = serve(app).await;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut sync_request = format!(
        "{}/v1/sync/ws?after=0",
        replica_url.replace("http://", "ws://")
    )
    .into_client_request()
    .unwrap();
    sync_request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", device.access_token).parse().unwrap(),
    );
    let (mut mobile, _) = tokio_tungstenite::connect_async(sync_request)
        .await
        .unwrap();
    assert_eq!(
        next_json(&mut mobile).await["changes"][0]["id"],
        meeting.to_string()
    );
    let speech_url = format!(
        "{}/v1/speech/live?meeting_id={meeting}",
        api.replace("http://", "ws://")
    );
    let mut speech_request = speech_url.clone().into_client_request().unwrap();
    speech_request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", t.access_token).parse().unwrap(),
    );
    let (mut desktop, _) = tokio_tungstenite::connect_async(speech_request)
        .await
        .unwrap();
    // The desktop emits this every three seconds, including before speech.
    // The proxy must forward it rather than closing the paid session.
    desktop
        .send(tokio_tungstenite::tungstenite::Message::Text(
            r#"{"type":"KeepAlive"}"#.into(),
        ))
        .await
        .unwrap();
    assert_eq!(next_json(&mut desktop).await["keepalive_received"], true);
    desktop
        .send(tokio_tungstenite::tungstenite::Message::Binary(
            vec![0; 3200].into(),
        ))
        .await
        .unwrap();
    assert_eq!(
        next_json(&mut desktop).await["channel"]["alternatives"][0]["transcript"],
        "Live meeting words"
    );
    let event = next_json(&mut mobile).await;
    assert_eq!(event["type"], "live");
    assert_eq!(event["event"]["type"], "transcript");
    assert_eq!(event["meeting_id"], meeting.to_string());
    let mut duplicate = speech_url.into_client_request().unwrap();
    duplicate.headers_mut().insert(
        "authorization",
        format!("Bearer {}", t.access_token).parse().unwrap(),
    );
    assert!(
        matches!(tokio_tungstenite::connect_async(duplicate).await,Err(tokio_tungstenite::tungstenite::Error::Http(response)) if response.status()==StatusCode::TOO_MANY_REQUESTS)
    );
    desktop.close(None).await.ok();
    mobile.close(None).await.ok();
    assert!(
        state
            .attachments
            .as_ref()
            .unwrap()
            .list(None)
            .next()
            .await
            .is_none()
    );
    api_task.abort();
    replica_task.abort();
    provider_task.abort();
    replica.redis.quit().await.ok();
    replica.db.clone().close().await.ok();
    cleanup(state, id).await;
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn dictation_validates_pcm_before_forwarding_and_accounts_for_duration() {
    let (state, _, id, t) = fixture().await;
    subscribe(&state, id).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mock=Router::new().route("/listen",post(move|headers:HeaderMap,Query(query):Query<HashMap<String,String>>,body:axum::body::Bytes|{let calls=count.clone();async move{
        assert_eq!(headers["authorization"],"Token mock-deepgram-key");assert_eq!(query["mip_opt_out"],"true");assert!(body.starts_with(b"RIFF"));calls.fetch_add(1,Ordering::SeqCst);
        Json(json!({"results":{"channels":[{"alternatives":[{"transcript":"Dictated words"}]}]}}))
    }}));
    let (base, task) = serve(mock).await;
    let mut config = state.config.clone();
    config.deepgram_key = SecretString::from("mock-deepgram-key");
    config.deepgram_url = format!("{}/listen", base.replace("http://", "ws://"))
        .parse()
        .unwrap();
    let state = configured(&state, config);
    let app = routes::router(state.clone());
    assert_eq!(
        body_request(
            &app,
            "/v1/speech/transcribe",
            &t.access_token,
            b"not WAV".to_vec()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut wav = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(
            &mut wav,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..16000 {
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();
    }
    let (status, body) = body_request(
        &app,
        "/v1/speech/transcribe",
        &t.access_token,
        wav.into_inner(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).contains("Dictated words"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let bucket = usage_buckets::Entity::find()
        .filter(usage_buckets::Column::AccountId.eq(id))
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bucket.speech_seconds, 1);
    task.abort();
    cleanup(state, id).await;
}
