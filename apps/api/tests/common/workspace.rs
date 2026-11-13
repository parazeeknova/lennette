use super::*;
use futures_util::{SinkExt, StreamExt};
use std::time::{Duration as Wait, Instant};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
const ORIGIN: &str = "https://lennette.itssingularity.com";

async fn connect(
    base: &str,
    cookie: Option<&str>,
    origin: Option<&str>,
    after: &str,
) -> std::result::Result<Socket, tokio_tungstenite::tungstenite::Error> {
    let mut request = format!(
        "{}/v1/workspace/ws?live_after={after}",
        base.replacen("http", "ws", 1)
    )
    .into_client_request()
    .unwrap();
    if let Some(cookie) = cookie {
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
    }
    if let Some(origin) = origin {
        request
            .headers_mut()
            .insert("origin", origin.parse().unwrap());
    }
    Ok(tokio_tungstenite::connect_async(request).await?.0)
}
async fn event(ws: &mut Socket, kind: &str) -> Value {
    tokio::time::timeout(Wait::from_secs(12), async {
        loop {
            match ws.next().await.unwrap().unwrap() {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    if value["type"] == kind {
                        return value;
                    }
                }
                Message::Ping(data) => ws.send(Message::Pong(data)).await.unwrap(),
                Message::Close(_) => panic!("socket closed before {kind}"),
                _ => {}
            }
        }
    })
    .await
    .expect("workspace event deadline")
}
fn item(id: Uuid, kind: &str, data: Value) -> Value {
    json!({"mutation_id":Uuid::new_v4(),"id":id,"kind":kind,"base_revision":0,"deleted":false,"data":data})
}

#[tokio::test]
#[ignore = "requires isolated Postgres and Redis"]
async fn workspace_reads_are_paginated_private_and_include_document_files() {
    let (state, app, account, tokens) = fixture().await;
    let (other, _, other_account, other_tokens) = fixture().await;
    subscribe(&state, account).await;
    let (conversation, meeting, document) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let (status, upload) = scenarios::body_request(
        &app,
        "/v1/attachments?name=agenda.txt&content_type=text/plain",
        &tokens.access_token,
        b"Meeting agenda\nSafe document.".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let uploaded: Value = serde_json::from_slice(&upload).unwrap();
    let mut mutations = vec![
        item(
            conversation,
            "conversation",
            json!({"title":"Planning chat","created_at":1,"updated_at":1}),
        ),
        item(
            meeting,
            "meeting",
            json!({"title":"Planning meeting","started_at":"2026-11-05 10:00:00"}),
        ),
        item(
            document,
            "attachment",
            json!({"name":"Imported notes","ocr_text":"Readable document text"}),
        ),
        item(
            Uuid::new_v4(),
            "context",
            json!({"conversation_id":conversation,"context_json":json!({"sources":[{"remote_blob_id":uploaded["id"]}]}).to_string()}),
        ),
    ];
    for i in 0..25 {
        mutations.push(item(Uuid::new_v4(),"message",json!({"conversation_id":conversation,"role":"assistant","content":format!("Answer {i}"),"timestamp":i})));
    }
    mutations.push(item(
        Uuid::new_v4(),
        "transcript",
        json!({"meeting_id":meeting,"text":"A saved transcript","who":"You","start_time":12.5}),
    ));
    mutations.push(item(Uuid::new_v4(), "analysis", json!({"meeting_id":meeting,"status":"ready","content":"{\"segments\":[]}","source_lines":1})));
    mutations.push(item(
        Uuid::new_v4(),
        "speaker",
        json!({"meeting_id":meeting,"speaker_key":"them:0","name":"Sam"}),
    ));
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/sync/push",
            Some(&tokens.access_token),
            json!({"mutations":mutations})
        )
        .await
        .0,
        StatusCode::OK
    );
    for kind in ["conversation", "meeting", "attachment", "file"] {
        let path = format!("/v1/workspace/items?kind={kind}&limit=1");
        assert_eq!(
            request(&app, "GET", &path, None, json!({})).await.0,
            StatusCode::UNAUTHORIZED
        );
        let (status, page) =
            request(&app, "GET", &path, Some(&tokens.access_token), json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        let (_, foreign) = request(
            &app,
            "GET",
            &path,
            Some(&other_tokens.access_token),
            json!({}),
        )
        .await;
        assert!(foreign["items"].as_array().unwrap().is_empty());
    }
    let path = format!("/v1/workspace/items/{conversation}?limit=10000");
    let (status, first) = request(&app, "GET", &path, Some(&tokens.access_token), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["items"].as_array().unwrap().len(), 20);
    assert_eq!(first["total_items"], 25);
    assert_eq!(first["items"][0]["data"]["content"], "Answer 24");
    assert_eq!(first["files"][0]["id"], uploaded["id"]);
    // A stable keyset cursor still works after its boundary row is deleted.
    let boundary = first["items"][19]["id"].clone();
    assert_eq!(request(&app,"POST","/v1/sync/push",Some(&tokens.access_token),json!({"mutations":[{"mutation_id":Uuid::new_v4(),"id":boundary,"kind":"message","base_revision":1,"deleted":true,"data":{}}]})).await.0,StatusCode::OK);
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/v1/workspace/items?kind=meeting&account={other_account}"),
            Some(&tokens.access_token),
            json!({})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "GET",
            &path,
            Some(&other_tokens.access_token),
            json!({})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (_, second) = request(
        &app,
        "GET",
        &format!(
            "/v1/workspace/items/{conversation}?after={}",
            first["next"].as_str().unwrap()
        ),
        Some(&tokens.access_token),
        json!({}),
    )
    .await;
    assert_eq!(second["items"].as_array().unwrap().len(), 5);
    assert!(second["next"].is_null());
    assert_eq!(second["files"], first["files"]);
    let (_, transcript) = request(
        &app,
        "GET",
        &format!("/v1/workspace/items/{meeting}"),
        Some(&tokens.access_token),
        json!({}),
    )
    .await;
    assert_eq!(transcript["items"][0]["data"]["text"], "A saved transcript");
    assert!(
        transcript["extras"]
            .as_array()
            .unwrap()
            .iter()
            .any(|extra| extra["kind"] == "analysis" && extra["data"]["status"] == "ready")
    );
    let (_, searched) = request(
        &app,
        "GET",
        &format!("/v1/workspace/items/{conversation}?q=Answer%2024"),
        Some(&tokens.access_token),
        json!({}),
    )
    .await;
    assert_eq!(searched["items"].as_array().unwrap().len(), 1);
    assert_eq!(searched["items"][0]["data"]["content"], "Answer 24");
    let (_, absent) = request(
        &app,
        "GET",
        &format!("/v1/workspace/items/{meeting}?q=unmatched"),
        Some(&tokens.access_token),
        json!({}),
    )
    .await;
    assert!(absent["items"].as_array().unwrap().is_empty());
    assert_eq!(absent["total_items"], 1);
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/v1/workspace/items/{meeting}?q={}", "x".repeat(201)),
            Some(&tokens.access_token),
            json!({})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, doc) = request(
        &app,
        "GET",
        &format!("/v1/workspace/items/{document}"),
        Some(&tokens.access_token),
        json!({}),
    )
    .await;
    assert_eq!(doc["record"]["data"]["ocr_text"], "Readable document text");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/attachments/{}",
                    uploaded["id"].as_str().unwrap()
                ))
                .header("authorization", format!("Bearer {}", tokens.access_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "Meeting agenda\nSafe document."
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/v1/workspace/items?kind=settings",
            Some(&tokens.access_token),
            json!({})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(request(&app,"POST","/v1/sync/push",Some(&tokens.access_token),json!({"mutations":[{"mutation_id":Uuid::new_v4(),"id":conversation,"kind":"conversation","base_revision":1,"deleted":true,"data":{}}]})).await.0,StatusCode::OK);
    assert_eq!(
        request(&app, "GET", &path, Some(&tokens.access_token), json!({}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    state.shutdown.cancel();
    other.shutdown.cancel();
    cleanup(state, account).await;
    cleanup(other, other_account).await;
}

#[tokio::test]
#[ignore = "requires isolated Postgres and Redis"]
async fn workspace_websockets_fan_out_across_replicas_resume_and_revoke() {
    let (state, app, account, tokens) = fixture().await;
    let (other, _, other_account, other_tokens) = fixture().await;
    let (other_cookie, _) = browser::browser_cookie(&other, &other_tokens).await;
    let replica = scenarios::configured(&state, state.config.clone());
    let (base, server) = scenarios::serve(app.clone()).await;
    let (base2, server2) = scenarios::serve(routes::router(replica.clone())).await;
    let (cookie, _) = browser::browser_cookie(&state, &tokens).await;
    let mut laptop = connect(&base, Some(&cookie), Some(ORIGIN), "0-0")
        .await
        .unwrap();
    let mut mobile = connect(&base2, Some(&cookie), Some(ORIGIN), "0-0")
        .await
        .unwrap();
    event(&mut laptop, "ready").await;
    event(&mut mobile, "ready").await;
    let mut foreign = connect(&base, Some(&other_cookie), Some(ORIGIN), "0-0")
        .await
        .unwrap();
    event(&mut foreign, "ready").await;
    let principal = auth::authenticate(&state, &tokens.access_token)
        .await
        .unwrap();
    let stream = Uuid::new_v4();
    let meeting = Uuid::new_v4();
    let conversation = Uuid::new_v4();
    lennette_server::providers::live(&state,&principal,Some(meeting),json!({"type":"ai_start","stream_id":stream,"conversation_id":conversation,"prompt":"What next?"})).await;
    event(&mut laptop, "live").await;
    event(&mut mobile, "live").await;
    let started = Instant::now();
    lennette_server::providers::live(&state,&principal,Some(meeting),json!({"type":"ai_delta","stream_id":stream,"conversation_id":conversation,"delta":{"content":"Hello 🌍"}})).await;
    let first = event(&mut laptop, "live").await;
    let second = event(&mut mobile, "live").await;
    assert_eq!(first["event"], second["event"]);
    assert_eq!(first["event"]["delta"]["content"], "Hello 🌍");
    assert!(
        tokio::time::timeout(Wait::from_millis(100), event(&mut foreign, "live"))
            .await
            .is_err()
    );
    assert!(
        started.elapsed() < Wait::from_secs(2),
        "cross-replica local delivery took {:?}",
        started.elapsed()
    );
    println!(
        "Workspace cross-replica live delivery: {:?}",
        started.elapsed()
    );
    mobile.close(None).await.unwrap();
    for part in [" One.", " Two."] {
        lennette_server::providers::live(&state,&principal,Some(meeting),json!({"type":"ai_delta","stream_id":stream,"conversation_id":conversation,"delta":{"content":part}})).await;
    }
    let mut resumed = connect(
        &base2,
        Some(&cookie),
        Some(ORIGIN),
        first["live_cursor"].as_str().unwrap(),
    )
    .await
    .unwrap();
    let replay = event(&mut resumed, "replay").await;
    assert_eq!(replay["frames"].as_array().unwrap().len(), 2);
    event(&mut resumed, "ready").await;
    let mut fresh = connect(&base, Some(&cookie), Some(ORIGIN), "0-0")
        .await
        .unwrap();
    let snapshot = event(&mut fresh, "snapshot").await;
    let answer = snapshot["frames"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["event"]["stream_id"] == stream.to_string())
        .unwrap();
    assert_eq!(answer["event"]["content"], "Hello 🌍 One. Two.");
    assert_eq!(answer["event"]["prompt"], "What next?");
    event(&mut fresh, "ready").await;
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/auth/logout",
            Some(&tokens.access_token),
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        event(&mut resumed, "session_revoked").await["type"],
        "session_revoked"
    );
    drop((laptop, resumed, fresh, foreign));
    state.shutdown.cancel();
    server.abort();
    server2.abort();
    drop(replica);
    cleanup(state, account).await;
    other.shutdown.cancel();
    cleanup(other, other_account).await;
}

#[tokio::test]
#[ignore = "requires isolated Postgres and Redis"]
async fn workspace_websocket_origin_capacity_and_snapshot_bounds() {
    let (state, app, account, tokens) = fixture().await;
    let (base, server) = scenarios::serve(app).await;
    let (cookie, _) = browser::browser_cookie(&state, &tokens).await;
    for origin in [None, Some("https://attacker.example")] {
        let error = connect(&base, Some(&cookie), origin, "0-0")
            .await
            .unwrap_err();
        assert!(
            matches!(error,tokio_tungstenite::tungstenite::Error::Http(ref r) if r.status()==401)
        );
    }
    let mut sockets = Vec::new();
    for _ in 0..8 {
        let mut ws = connect(&base, Some(&cookie), Some(ORIGIN), "0-0")
            .await
            .unwrap();
        event(&mut ws, "ready").await;
        sockets.push(ws);
    }
    let error = connect(&base, Some(&cookie), Some(ORIGIN), "0-0")
        .await
        .unwrap_err();
    assert!(matches!(error,tokio_tungstenite::tungstenite::Error::Http(ref r) if r.status()==429));
    sockets.pop().unwrap().close(None).await.unwrap();
    tokio::time::timeout(Wait::from_secs(2), async {
        loop {
            let n: i64 = state
                .redis
                .zcard(format!("workspace-streams:{account}"))
                .await
                .unwrap();
            if n == 7 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let principal = auth::authenticate(&state, &tokens.access_token)
        .await
        .unwrap();
    let stream = Uuid::new_v4();
    for _ in 0..6 {
        lennette_server::providers::live(
            &state,
            &principal,
            None,
            json!({"type":"ai_delta","stream_id":stream,"delta":{"content":"🌍".repeat(1000)}}),
        )
        .await;
    }
    lennette_server::providers::live(
        &state,
        &principal,
        None,
        json!({"type":"ai_done","stream_id":stream,"complete":false}),
    )
    .await;
    let mut fresh = connect(&base, Some(&cookie), Some(ORIGIN), "0-0")
        .await
        .unwrap();
    let snapshot = event(&mut fresh, "snapshot").await;
    let answer = &snapshot["frames"][0]["event"];
    assert!(answer["content"].as_str().unwrap().len() <= 16384);
    assert_eq!(answer["truncated"], true);
    assert_eq!(answer["complete"], false);
    assert_eq!(answer["done"], true);
    let ttl: i64 = state
        .redis
        .ttl(format!("live-state:{{{account}}}"))
        .await
        .unwrap();
    assert!((1..=180).contains(&ttl));
    fresh
        .send(Message::Text("unexpected client data".into()))
        .await
        .unwrap();
    tokio::time::timeout(Wait::from_secs(2), async {
        while let Some(Ok(message)) = fresh.next().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
        }
    })
    .await
    .unwrap();
    drop(sockets);
    state.shutdown.cancel();
    server.abort();
    cleanup(state, account).await;
}

#[tokio::test]
#[ignore = "requires isolated Postgres and Redis"]
async fn workspace_chat_stream_uses_real_provider_frames_and_hides_background_turns() {
    use axum::routing::post;
    let (state, _, account, tokens) = fixture().await;
    subscribe(&state, account).await;
    let provider=Router::new().route("/chat",post(||async {
        ([ ("content-type","text/event-stream") ], "data: {\"choices\":[{\"delta\":{\"content\":\"Hello \"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"world\"}}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n")
    }));
    let (provider_url, provider_task) = scenarios::serve(provider).await;
    let mut config = state.config.clone();
    config.gemini_key = SecretString::from("mock-key");
    config.gemini_url = format!("{provider_url}/chat").parse().unwrap();
    let state = scenarios::configured(&state, config);
    let app = routes::router(state.clone());
    let (base, server) = scenarios::serve(app.clone()).await;
    let (cookie, _) = browser::browser_cookie(&state, &tokens).await;
    let mut socket = connect(&base, Some(&cookie), Some(ORIGIN), "0-0")
        .await
        .unwrap();
    event(&mut socket, "ready").await;
    let conversation = Uuid::new_v4();
    let mut body = json!({"model":"gemini-3.5-flash-lite","messages":[{"role":"user","content":"Question?"}],"max_tokens":16,"conversation_id":conversation,"relay_live":true});
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/chat/completions",
            Some(&tokens.access_token),
            body.clone()
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        event(&mut socket, "live").await["event"]["type"],
        "ai_start"
    );
    let first = event(&mut socket, "live").await;
    assert_eq!(first["event"]["delta"], "Hello ");
    assert_eq!(first["event"]["offset"], 0);
    let second = event(&mut socket, "live").await;
    assert_eq!(second["event"]["delta"], "world");
    assert_eq!(second["event"]["offset"], 6);
    assert_eq!(event(&mut socket, "live").await["event"]["complete"], true);
    body["relay_live"] = json!(false);
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/chat/completions",
            Some(&tokens.access_token),
            body
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(
        tokio::time::timeout(Wait::from_millis(150), event(&mut socket, "live"))
            .await
            .is_err()
    );
    let mut fresh = connect(&base, Some(&cookie), Some(ORIGIN), "0-0")
        .await
        .unwrap();
    let snapshot = event(&mut fresh, "snapshot").await;
    assert_eq!(snapshot["frames"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["frames"][0]["event"]["content"], "Hello world");
    assert_eq!(
        snapshot["frames"][0]["event"]["conversation_id"],
        conversation.to_string()
    );
    drop((socket, fresh));
    state.shutdown.cancel();
    server.abort();
    provider_task.abort();
    cleanup(state, account).await;
}

#[tokio::test]
#[ignore = "requires isolated Postgres and Redis"]
async fn workspace_numeric_order_speakers_and_files_do_not_truncate() {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    let (state, app, account, tokens) = fixture().await;
    subscribe(&state, account).await;
    let meeting = Uuid::new_v4();
    let mut mutations = vec![item(
        meeting,
        "meeting",
        json!({"title":"Many participants","started_at":"2026-11-05 10:00:00"}),
    )];
    for timestamp in [9, 10, 100] {
        mutations.push(item(
            Uuid::new_v4(),
            "conversation",
            json!({"title":timestamp.to_string(),"updated_at":timestamp}),
        ));
    }
    for i in 0..40 {
        mutations.push(item(Uuid::new_v4(), "speaker", json!({"meeting_id":meeting,"speaker_key":format!("them:{i}"),"name":format!("Speaker {i}")})));
        mutations.push(item(
            Uuid::new_v4(),
            "analysis",
            json!({"meeting_id":meeting,"content":"{}"}),
        ));
    }
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/sync/push",
            Some(&tokens.access_token),
            json!({"mutations":mutations})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, first) = request(
        &app,
        "GET",
        "/v1/workspace/items?kind=conversation&limit=1",
        Some(&tokens.access_token),
        json!({}),
    )
    .await;
    assert_eq!(first["items"][0]["data"]["title"], "100");
    let (_, second) = request(
        &app,
        "GET",
        &format!(
            "/v1/workspace/items?kind=conversation&limit=1&after={}",
            first["next"].as_str().unwrap()
        ),
        Some(&tokens.access_token),
        json!({}),
    )
    .await;
    assert_eq!(second["items"][0]["data"]["title"], "10");
    state.db.execute_raw(Statement::from_sql_and_values(DbBackend::Postgres,
        "WITH added AS (INSERT INTO attachments(id,account_id,name,content_type,size_bytes,sha256) SELECT gen_random_uuid(),$1,'file-' || n,'text/plain',1,'test' FROM generate_series(1,33) n RETURNING id) INSERT INTO attachment_refs(account_id,record_id,attachment_id) SELECT $1,$2,id FROM added", [account.into(),meeting.into()])).await.unwrap();
    let (_, first) = request(
        &app,
        "GET",
        &format!("/v1/workspace/items/{meeting}"),
        Some(&tokens.access_token),
        json!({}),
    )
    .await;
    assert_eq!(
        first["extras"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["kind"] == "speaker")
            .count(),
        40
    );
    assert_eq!(
        first["extras"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["kind"] == "analysis")
            .count(),
        32
    );
    assert_eq!(first["files"].as_array().unwrap().len(), 32);
    let (_, second) = request(
        &app,
        "GET",
        &format!(
            "/v1/workspace/items/{meeting}?files_after={}",
            first["files_next"].as_str().unwrap()
        ),
        Some(&tokens.access_token),
        json!({}),
    )
    .await;
    assert_eq!(second["files"].as_array().unwrap().len(), 1);
    assert!(second["files_next"].is_null());
    assert!(
        !first["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["id"] == second["files"][0]["id"])
    );
    cleanup(state, account).await;
}
