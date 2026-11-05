use super::scenarios::{body_request, configured, serve};
use super::*;
use axum::{extract::State, response::IntoResponse};
use futures_util::StreamExt;
use lennette_server::{config::R2Config, entities::attachments, storage};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

pub(super) async fn scanner() -> String {
    scanner_reply(None).await
}
async fn scanner_reply(forced: Option<&'static [u8]>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut command = [0u8; 10];
                stream.read_exact(&mut command).await.unwrap();
                assert_eq!(&command, b"zINSTREAM\0");
                let mut data = Vec::new();
                loop {
                    let size = stream.read_u32().await.unwrap();
                    if size == 0 {
                        break;
                    }
                    assert!(size <= 32 * 1024);
                    let start = data.len();
                    data.resize(start + size as usize, 0);
                    stream.read_exact(&mut data[start..]).await.unwrap();
                }
                let response = if let Some(response) = forced {
                    response
                } else if data.windows(22).any(|s| s == b"scanner-test-detection") {
                    b"stream: Mock.Detection FOUND\0".as_slice()
                } else {
                    b"stream: OK\0".as_slice()
                };
                stream.write_all(response).await.unwrap();
            });
        }
    });
    address
}

#[derive(Default)]
struct MockS3 {
    objects: Mutex<HashMap<String, bytes::Bytes>>,
    fail: AtomicBool,
    requests: AtomicUsize,
    fail_next: AtomicUsize,
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn removing_the_last_record_reference_queues_erasure_and_refunds_storage() {
    let (state, app, account, tokens) = storage_fixture().await;
    subscribe(&state, account).await;
    let blob = Uuid::new_v4();
    let path = format!("/v1/attachments?id={blob}&name=notes.txt&content_type=text%2Fplain");
    assert_eq!(
        body_request(&app, &path, &tokens.access_token, b"Private notes".to_vec())
            .await
            .0,
        StatusCode::OK
    );
    let conversation = Uuid::new_v4();
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/sync/push",
            Some(&tokens.access_token),
            json!({"mutations":[mutation(conversation,0,"Meeting")]})
        )
        .await
        .0,
        StatusCode::OK
    );
    let context = Uuid::new_v4();
    let context_data = json!({"conversation_id":conversation,"context_json":json!({"sources":[{"remote_blob_id":blob.to_string()}]}).to_string()});
    assert_eq!(request(&app,"POST","/v1/sync/push",Some(&tokens.access_token),json!({"mutations":[{"id":context,"mutation_id":Uuid::new_v4(),"kind":"context","base_revision":0,"deleted":false,"data":context_data}]})).await.0,StatusCode::OK);
    assert_eq!(request(&app,"POST","/v1/sync/push",Some(&tokens.access_token),json!({"mutations":[{"id":conversation,"mutation_id":Uuid::new_v4(),"kind":"conversation","base_revision":1,"deleted":true,"data":{}}]})).await.0,StatusCode::OK);
    assert!(
        attachments::Entity::find_by_id(blob)
            .one(&state.db)
            .await
            .unwrap()
            .is_none()
    );
    storage::cleanup::reconcile(&state).await.unwrap();
    use object_store::ObjectStoreExt;
    assert!(
        state
            .attachments
            .as_ref()
            .unwrap()
            .get(&storage::key(account, blob))
            .await
            .is_err()
    );
    cleanup(state, account).await;
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn unsafe_or_unscanned_uploads_never_reach_storage_or_quota() {
    use object_store::ObjectStore;
    let (state, app, account, tokens) = storage_fixture().await;
    subscribe(&state, account).await;
    assert_eq!(
        body_request(
            &app,
            "/v1/attachments?name=notes.txt&content_type=text%2Fplain",
            &tokens.access_token,
            b"scanner-test-detection".to_vec()
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        body_request(
            &app,
            "/v1/attachments?name=payload.exe&content_type=text%2Fplain",
            &tokens.access_token,
            b"hello".to_vec()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        body_request(
            &app,
            "/v1/attachments?name=picture.png&content_type=image%2Fpng",
            &tokens.access_token,
            b"not an image".to_vec()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        body_request(
            &app,
            "/v1/attachments?name=notes.pdf&content_type=application%2Fpdf",
            &tokens.access_token,
            b"%PDF-1.7\ninvalid\n%%EOF".to_vec()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let mut config = state.config.clone();
    config.clamd_address = None;
    let unavailable = routes::router(configured(&state, config));
    assert_eq!(
        body_request(
            &unavailable,
            "/v1/attachments?name=notes.txt&content_type=text%2Fplain",
            &tokens.access_token,
            b"Private notes".to_vec()
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let mut config = state.config.clone();
    config.clamd_address = Some(scanner_reply(Some(b"stream: scan size exceeded ERROR\0")).await);
    let unavailable = routes::router(configured(&state, config));
    assert_eq!(
        body_request(
            &unavailable,
            "/v1/attachments?name=notes.txt&content_type=text%2Fplain",
            &tokens.access_token,
            b"Private notes".to_vec()
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
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
    assert_eq!(
        accounts::Entity::find_by_id(account)
            .one(&state.db)
            .await
            .unwrap()
            .unwrap()
            .storage_bytes,
        0
    );
    assert!(
        lennette_server::entities::object_uploads::Entity::find()
            .filter(lennette_server::entities::object_uploads::Column::AccountId.eq(account))
            .all(&state.db)
            .await
            .unwrap()
            .is_empty()
    );
    cleanup(state, account).await;
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn durable_upload_journal_reaps_crash_orphans_and_preserves_committed_files() {
    use lennette_server::entities::object_uploads;
    use object_store::ObjectStoreExt;
    let (state, app, account, tokens) = storage_fixture().await;
    subscribe(&state, account).await;
    let orphan = Uuid::new_v4();
    storage::cleanup::begin(&state, account, orphan)
        .await
        .unwrap();
    state
        .attachments
        .as_ref()
        .unwrap()
        .put(
            &storage::key(account, orphan),
            bytes::Bytes::from_static(b"orphan").into(),
        )
        .await
        .unwrap();
    let committed = Uuid::new_v4();
    let path = format!("/v1/attachments?id={committed}&name=notes.txt&content_type=text%2Fplain");
    assert_eq!(
        body_request(&app, &path, &tokens.access_token, b"Private notes".to_vec())
            .await
            .0,
        StatusCode::OK
    );
    storage::cleanup::begin(&state, account, committed)
        .await
        .unwrap();
    object_uploads::Entity::update_many()
        .col_expr(
            object_uploads::Column::CreatedAt,
            sea_orm::sea_query::Expr::value(Utc::now() - Duration::hours(2)),
        )
        .filter(object_uploads::Column::AccountId.eq(account))
        .exec(&state.db)
        .await
        .unwrap();
    storage::cleanup::reconcile(&state).await.unwrap();
    assert!(
        state
            .attachments
            .as_ref()
            .unwrap()
            .get(&storage::key(account, orphan))
            .await
            .is_err()
    );
    assert!(
        state
            .attachments
            .as_ref()
            .unwrap()
            .get(&storage::key(account, committed))
            .await
            .is_ok()
    );
    assert!(
        object_uploads::Entity::find()
            .filter(object_uploads::Column::AccountId.eq(account))
            .all(&state.db)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        accounts::Entity::find_by_id(account)
            .one(&state.db)
            .await
            .unwrap()
            .unwrap()
            .storage_bytes,
        13
    );
    cleanup(state, account).await;
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn unconfigured_storage_fails_closed_without_charging_quota() {
    let (state, _, account, tokens) = storage_fixture().await;
    subscribe(&state, account).await;
    let mut unavailable = configured(&state, state.config.clone());
    Arc::get_mut(&mut unavailable.0).unwrap().attachments = None;
    let app = routes::router(unavailable);
    let (status, response) = body_request(
        &app,
        "/v1/attachments?name=notes.txt&content_type=text%2Fplain",
        &tokens.access_token,
        b"Private notes".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(!String::from_utf8_lossy(&response).contains("Private notes"));
    assert_eq!(
        accounts::Entity::find_by_id(account)
            .one(&state.db)
            .await
            .unwrap()
            .unwrap()
            .storage_bytes,
        0
    );
    cleanup(state, account).await;
}

async fn s3(State(s): State<Arc<MockS3>>, request: Request<Body>) -> axum::response::Response {
    s.requests.fetch_add(1, Ordering::SeqCst);
    let authorization = request
        .headers()
        .get("authorization")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(authorization.starts_with("AWS4-HMAC-SHA256 "));
    assert!(authorization.contains("Credential=local-test-access/"));
    assert!(authorization.contains("/auto/s3/aws4_request"));
    assert!(request.headers().contains_key("x-amz-date"));
    assert!(
        request
            .uri()
            .path()
            .starts_with("/test-attachments/accounts/")
    );
    if s.fail.load(Ordering::SeqCst)
        || s.fail_next
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "<Error><Code>ServiceUnavailable</Code></Error>",
        )
            .into_response();
    }
    let path = request.uri().path().to_owned();
    let method = request.method().clone();
    match method.as_str() {
        "PUT" => {
            let bytes = axum::body::to_bytes(request.into_body(), 11 * 1024 * 1024)
                .await
                .unwrap();
            s.objects.lock().await.insert(path, bytes);
            ([("etag", "\"mock-etag\"")], "").into_response()
        }
        "GET" => match s.objects.lock().await.get(&path).cloned() {
            Some(bytes) => (
                [
                    ("etag", "\"mock-etag\""),
                    ("last-modified", "Wed, 04 Nov 2026 00:00:00 GMT"),
                ],
                bytes,
            )
                .into_response(),
            None => (
                StatusCode::NOT_FOUND,
                "<Error><Code>NoSuchKey</Code></Error>",
            )
                .into_response(),
        },
        "DELETE" => {
            s.objects.lock().await.remove(&path);
            StatusCode::NO_CONTENT.into_response()
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn signed_r2_roundtrip_failures_quota_and_account_deletion() {
    let mock = Arc::new(MockS3::default());
    let (endpoint, task) = serve(Router::new().fallback(s3).with_state(mock.clone())).await;
    let (original, _, account_id, tokens) = storage_fixture().await;
    let mut config = original.config.clone();
    config.r2 = Some(R2Config {
        endpoint: endpoint.parse().unwrap(),
        bucket: "test-attachments".into(),
        access_key_id: "local-test-access".to_owned().into(),
        secret_access_key: "local-test-secret".to_owned().into(),
    });
    let store = storage::connect(config.r2.as_ref().unwrap()).unwrap();
    let mut state = configured(&original, config);
    Arc::get_mut(&mut state.0).unwrap().attachments = Some(store);
    let app = routes::router(state.clone());
    subscribe(&state, account_id).await;
    let id = Uuid::new_v4();
    let path = format!("/v1/attachments?id={id}&name=notes.txt&content_type=text%2Fplain");
    let object_key = format!("/test-attachments/{}", storage::key(account_id, id));
    mock.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        body_request(&app, &path, &tokens.access_token, b"Private notes".to_vec())
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        accounts::Entity::find_by_id(account_id)
            .one(&state.db)
            .await
            .unwrap()
            .unwrap()
            .storage_bytes,
        0
    );
    assert!(
        attachments::Entity::find_by_id(id)
            .one(&state.db)
            .await
            .unwrap()
            .is_none()
    );
    mock.fail.store(false, Ordering::SeqCst);
    assert_eq!(
        mock.requests.load(Ordering::SeqCst),
        3,
        "initial attempt plus two bounded retries"
    );
    mock.fail_next.store(1, Ordering::SeqCst);
    assert_eq!(
        body_request(&app, &path, &tokens.access_token, b"Private notes".to_vec())
            .await
            .0,
        StatusCode::OK
    );
    let count = mock.requests.load(Ordering::SeqCst);
    assert_eq!(
        body_request(&app, &path, &tokens.access_token, b"Private notes".to_vec())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(mock.requests.load(Ordering::SeqCst), count);
    let download = format!("/v1/attachments/{id}");
    mock.fail_next.store(1, Ordering::SeqCst);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&download)
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
        "Private notes"
    );
    // Same-length corruption is detected against the immutable database checksum.
    mock.objects.lock().await.insert(
        object_key.clone(),
        bytes::Bytes::from_static(b"Corrupt notes"),
    );
    assert_eq!(
        request(
            &app,
            "GET",
            &download,
            Some(&tokens.access_token),
            json!({})
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    mock.objects
        .lock()
        .await
        .insert(object_key, bytes::Bytes::from_static(b"Private notes"));
    accounts::Entity::update_many()
        .col_expr(
            accounts::Column::StorageBytes,
            sea_orm::sea_query::Expr::value(lennette_server::plans::STANDARD.storage_bytes),
        )
        .filter(accounts::Column::Id.eq(account_id))
        .exec(&state.db)
        .await
        .unwrap();
    let count = mock.requests.load(Ordering::SeqCst);
    assert_eq!(
        body_request(
            &app,
            "/v1/attachments?name=new.txt&content_type=text%2Fplain",
            &tokens.access_token,
            b"x".to_vec()
        )
        .await
        .0,
        StatusCode::PAYMENT_REQUIRED
    );
    assert_eq!(mock.requests.load(Ordering::SeqCst), count);
    accounts::Entity::update_many()
        .col_expr(
            accounts::Column::StorageBytes,
            sea_orm::sea_query::Expr::value(13i64),
        )
        .filter(accounts::Column::Id.eq(account_id))
        .exec(&state.db)
        .await
        .unwrap();
    mock.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        request(
            &app,
            "DELETE",
            &download,
            Some(&tokens.access_token),
            json!({})
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(
        attachments::Entity::find_by_id(id)
            .one(&state.db)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        accounts::Entity::find_by_id(account_id)
            .one(&state.db)
            .await
            .unwrap()
            .unwrap()
            .storage_bytes,
        13
    );
    subscriptions::Entity::update_many()
        .col_expr(
            subscriptions::Column::Status,
            sea_orm::sea_query::Expr::value("expired"),
        )
        .filter(subscriptions::Column::AccountId.eq(account_id))
        .exec(&state.db)
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            "DELETE",
            "/v1/me",
            Some(&tokens.access_token),
            json!({})
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    assert!(
        accounts::Entity::find_by_id(account_id)
            .one(&state.db)
            .await
            .unwrap()
            .is_none()
    );
    // The account is gone immediately; private blobs remain inaccessible and cleanup is durable.
    assert!(storage::cleanup::reconcile(&state).await.is_err());
    assert!(
        !lennette_server::entities::object_uploads::Entity::find()
            .filter(lennette_server::entities::object_uploads::Column::AccountId.eq(account_id))
            .all(&state.db)
            .await
            .unwrap()
            .is_empty()
    );
    mock.fail.store(false, Ordering::SeqCst);
    mock.fail_next.store(1, Ordering::SeqCst);
    // The DELETE handler also starts a sweep. A concurrent sweep can skip its
    // advisory lock, so wait for durable erasure rather than one sweep's return.
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            storage::cleanup::reconcile(&state).await.unwrap();
            if mock.objects.lock().await.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("account objects must be erased after storage recovers");
    assert!(mock.objects.lock().await.is_empty());
    task.abort();
    cleanup(state, account_id).await;
}
