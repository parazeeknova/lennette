//! Run with: cargo test -p lennette-server --test integration -- --ignored
//! Uses real PostgreSQL 18 + Redis from infra/compose.yaml, mock billable providers.
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{Duration, Utc};
use fred::prelude::*;
use http_body_util::BodyExt;
use lennette_server::{
    auth, billing,
    config::Config,
    entities::{accounts, subscriptions, usage_buckets},
    migration::Migrator,
    routes,
    state::AppState,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use sea_orm_migration::MigratorTrait;
use secrecy::SecretString;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;
#[path = "common/browser.rs"]
mod browser;
#[path = "common/r2.rs"]
mod r2;
#[path = "common/scenarios.rs"]
mod scenarios;

async fn fixture() -> (AppState, Router, Uuid, lennette_protocol::Tokens) {
    static MIGRATION: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    let empty = || SecretString::from(String::new());
    let mut state = AppState::connect(Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        public_url: "http://127.0.0.1:8090".parse().unwrap(),
        website_url: "https://lennette.itssingularity.com".parse().unwrap(),
        database_url: SecretString::from(std::env::var("TEST_DATABASE_URL").unwrap_or(
            "postgres://lennette:local-development-only@127.0.0.1:55432/lennette".into(),
        )),
        redis_url: SecretString::from(
            std::env::var("TEST_REDIS_URL")
                .unwrap_or("redis://:local-development-only@127.0.0.1:56379/1".into()),
        ),
        google: None,
        github: None,
        merge_key: empty(),
        merge_url: "http://127.0.0.1:1/chat".parse().unwrap(),
        deepgram_key: empty(),
        deepgram_url: "ws://127.0.0.1:1/listen".parse().unwrap(),
        dodo_key: empty(),
        dodo_webhook_secret: empty(),
        dodo_url: "http://127.0.0.1:1/".parse().unwrap(),
        dodo_standard_product: "pdt_standard".into(),
        dodo_plus_product: "pdt_plus".into(),
        r2: None,
        clamd_address: Some(r2::scanner().await),
        trusted_proxy_ips: vec![],
        mobile_redirects: vec![],
    })
    .await
    .unwrap();
    std::sync::Arc::get_mut(&mut state.0).unwrap().attachments =
        Some(std::sync::Arc::new(object_store::memory::InMemory::new()));
    MIGRATION
        .get_or_init(|| async { Migrator::up(&state.db, None).await.unwrap() })
        .await;
    let id = Uuid::new_v4();
    accounts::ActiveModel {
        id: Set(id),
        email: Set(format!("{id}@example.test")),
        display_name: Set("Test".into()),
        created_at: Set(Utc::now()),
        sync_sequence: Set(0),
        storage_bytes: Set(0),
    }
    .insert(&state.db)
    .await
    .unwrap();
    let token = auth::issue(&state, id, Uuid::new_v4(), "Test device")
        .await
        .unwrap();
    let app = routes::router(state.clone());
    (state, app, id, token)
}
async fn subscribe(state: &AppState, id: Uuid) {
    subscriptions::ActiveModel {
        account_id: Set(id),
        subscription_id: Set(format!("sub_{id}")),
        customer_id: Set(format!("cus_{id}")),
        product_id: Set("pdt_standard".into()),
        plan: Set("managed".into()),
        status: Set("active".into()),
        period_start: Set(Utc::now() - Duration::days(1)),
        period_end: Set(Utc::now() + Duration::days(29)),
        updated_at: Set(Utc::now()),
    }
    .insert(&state.db)
    .await
    .unwrap();
}
async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    value: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(value.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}
fn mutation(id: Uuid, revision: i64, text: &str) -> Value {
    json!({"mutation_id":Uuid::new_v4(),"id":id,"kind":"conversation","base_revision":revision,"deleted":false,"data":{"title":text}})
}
async fn cleanup(state: AppState, id: Uuid) {
    accounts::Entity::delete_by_id(id)
        .exec(&state.db)
        .await
        .unwrap();
    state.redis.quit().await.ok();
    state.db.clone().close().await.ok();
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn auth_rotation_replay_revocation_and_health() {
    let (s, app, id, t) = fixture().await;
    assert_eq!(
        request(&app, "GET", "/health/ready", None, json!({}))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&app, "GET", "/v1/me", None, json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "GET", "/v1/me", Some(&t.access_token), json!({}))
            .await
            .1["id"],
        id.to_string()
    );
    let (status, value) = request(
        &app,
        "POST",
        "/v1/auth/refresh",
        None,
        json!({"refresh_token":t.refresh_token}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rotated: lennette_protocol::Tokens = serde_json::from_value(value).unwrap();
    assert_eq!(
        request(&app, "GET", "/v1/me", Some(&t.access_token), json!({}))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/v1/me",
            Some(&rotated.access_token),
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/auth/refresh",
            None,
            json!({"refresh_token":t.refresh_token})
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
            Some(&rotated.access_token),
            json!({})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    cleanup(s, id).await;
}
#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn sync_idempotency_conflicts_tenant_isolation_and_expiry() {
    let (s, app, id, t) = fixture().await;
    subscribe(&s, id).await;
    let record = Uuid::new_v4();
    let first = mutation(record, 0, "Private meeting");
    let (status, results) = request(
        &app,
        "POST",
        "/v1/sync/push",
        Some(&t.access_token),
        json!({"mutations":[first]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{results}");
    assert_eq!(results[0]["change"]["sequence"], 1);
    let (status, replayed) = request(
        &app,
        "POST",
        "/v1/sync/push",
        Some(&t.access_token),
        json!({"mutations":[first]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(results, replayed);
    let mut tampered = first.clone();
    tampered["data"]["title"] = json!("Different request under same id");
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/sync/push",
            Some(&t.access_token),
            json!({"mutations":[tampered]})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/sync/push",
            Some(&t.access_token),
            json!({"mutations":[mutation(record,0,"stale")]})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let other = Uuid::new_v4();
    let other_token = auth::issue(&s, id, other, "Mobile").await.unwrap();
    let (_, pull) = request(
        &app,
        "GET",
        "/v1/sync/pull?after=0",
        Some(&other_token.access_token),
        json!({}),
    )
    .await;
    assert_eq!(pull["changes"][0]["data"]["title"], "Private meeting");
    let (s2, app2, id2, t2) = fixture().await;
    let (_, pull) = request(
        &app2,
        "GET",
        "/v1/sync/pull?after=0",
        Some(&t2.access_token),
        json!({}),
    )
    .await;
    assert_eq!(pull["changes"].as_array().unwrap().len(), 0);
    subscriptions::Entity::update_many()
        .col_expr(
            subscriptions::Column::PeriodEnd,
            sea_orm::sea_query::Expr::value(Utc::now() - Duration::seconds(1)),
        )
        .filter(subscriptions::Column::AccountId.eq(id))
        .exec(&s.db)
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/sync/push",
            Some(&t.access_token),
            json!({"mutations":[mutation(record,1,"expired")]})
        )
        .await
        .0,
        StatusCode::PAYMENT_REQUIRED
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/v1/sync/pull?after=0",
            Some(&t.access_token),
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    cleanup(s2, id2).await;
    cleanup(s, id).await;
}
#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn batch_atomicity_tombstones_and_pagination() {
    let (s, app, id, t) = fixture().await;
    subscribe(&s, id).await;
    let one = Uuid::new_v4();
    let two = Uuid::new_v4();
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/sync/push",
            Some(&t.access_token),
            json!({"mutations":[mutation(one,0,"one"),mutation(two,9,"invalid")]})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, pull) = request(
        &app,
        "GET",
        "/v1/sync/pull?after=0",
        Some(&t.access_token),
        json!({}),
    )
    .await;
    assert_eq!(pull["cursor"], 0);
    let (status, _) = request(
        &app,
        "POST",
        "/v1/sync/push",
        Some(&t.access_token),
        json!({"mutations":[mutation(one,0,"one"),mutation(two,0,"two")]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, pull) = request(
        &app,
        "GET",
        "/v1/sync/pull?after=0&limit=1",
        Some(&t.access_token),
        json!({}),
    )
    .await;
    assert_eq!(pull["has_more"], true);
    assert_eq!(pull["cursor"], 1);
    let mut deleted = mutation(one, 1, "");
    deleted["deleted"] = json!(true);
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/sync/push",
            Some(&t.access_token),
            json!({"mutations":[deleted]})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, pull) = request(
        &app,
        "GET",
        "/v1/sync/pull?after=2",
        Some(&t.access_token),
        json!({}),
    )
    .await;
    assert_eq!(pull["changes"][0]["deleted"], true);
    cleanup(s, id).await;
}
#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn simultaneous_usage_reservations_cannot_overspend() {
    let (s, _app, id, _t) = fixture().await;
    subscribe(&s, id).await;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let s = s.clone();
        tasks.spawn(async move { billing::reserve(&s, id, "ai", 500_000).await });
    }
    let mut ids = Vec::new();
    while let Some(r) = tasks.join_next().await {
        if let Ok(id) = r.unwrap() {
            ids.push(id)
        }
    }
    assert_eq!(ids.len(), 4);
    billing::settle(&s, ids[0], 125_000).await.unwrap();
    billing::settle(&s, ids[0], 0).await.unwrap();
    let bucket = usage_buckets::Entity::find()
        .filter(usage_buckets::Column::AccountId.eq(id))
        .one(&s.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bucket.ai_spent, 1_625_000);
    assert!(billing::reserve(&s, id, "ai", 375_000).await.is_ok());
    assert!(billing::reserve(&s, id, "ai", 1).await.is_err());
    cleanup(s, id).await;
}
#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn secrets_and_unconfigured_providers_fail_closed() {
    let (s, app, id, t) = fixture().await;
    subscribe(&s, id).await;
    let mut secret = mutation(Uuid::new_v4(), 0, "");
    secret["kind"] = json!("settings");
    secret["data"] = json!({"api_key":"never upload"});
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/sync/push",
            Some(&t.access_token),
            json!({"mutations":[secret]})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let body = json!({"model":"openai/gpt-6-luna","messages":[{"role":"user","content":"hello"}],"stream":true});
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/chat/completions",
            Some(&t.access_token),
            body
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
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
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/billing/webhook",
            None,
            json!({"type":"subscription.active"})
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    cleanup(s, id).await;
}
