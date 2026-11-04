use super::scenarios::{configured, serve};
use super::*;
use axum::{
    Json,
    response::IntoResponse,
    routing::{get, post},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::RwLock;

struct Provider {
    current: Arc<RwLock<Value>>,
    amount: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
    mode: Arc<AtomicUsize>,
}
async fn setup() -> (
    AppState,
    Router,
    Uuid,
    lennette_protocol::Tokens,
    Provider,
    tokio::task::JoinHandle<()>,
) {
    let (original, _, account, tokens) = fixture().await;
    subscribe(&original, account).await;
    let sub = subscriptions::Entity::find_by_id(account)
        .one(&original.db)
        .await
        .unwrap()
        .unwrap();
    let current = Arc::new(RwLock::new(
        json!({"subscription_id":sub.subscription_id,"product_id":"pdt_standard","status":"active","customer":{"customer_id":sub.customer_id},"metadata":{"account_id":account.to_string()},"previous_billing_date":sub.period_start.to_rfc3339(),"next_billing_date":sub.period_end.to_rfc3339()}),
    ));
    let amount = Arc::new(AtomicUsize::new(2290));
    let calls = Arc::new(AtomicUsize::new(0));
    let mode = Arc::new(AtomicUsize::new(0));
    let get_current = current.clone();
    let price = amount.clone();
    let changes = calls.clone();
    let update = current.clone();
    let outcome = mode.clone();
    let mock=Router::new()
        .route("/subscriptions/{id}",get(move||{let value=get_current.clone();async move{Json(value.read().await.clone())}}))
        .route("/subscriptions/{id}/change-plan/preview",post(move|Json(body):Json<Value>|{let price=price.clone();async move{assert_eq!(body["proration_billing_mode"],"difference_immediately");assert_eq!(body["on_payment_failure"],"prevent_change");Json(json!({"immediate_charge":{"summary":{"total_amount":price.load(Ordering::SeqCst),"currency":"USD"}}}))}}))
        .route("/subscriptions/{id}/change-plan",post(move|headers:axum::http::HeaderMap,Json(body):Json<Value>|{let calls=changes.clone();let value=update.clone();let mode=outcome.clone();async move{
            calls.fetch_add(1,Ordering::SeqCst);
            assert!(headers.contains_key("idempotency-key"));
            assert_eq!(body["product_id"],"pdt_plus");assert_eq!(body["quantity"],1);assert_eq!(body["proration_billing_mode"],"difference_immediately");assert_eq!(body["on_payment_failure"],"prevent_change");
            match mode.load(Ordering::SeqCst) {
                1 => (StatusCode::PAYMENT_REQUIRED,Json(json!({"error":"declined"}))).into_response(),
                2 => (StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":"uncertain"}))).into_response(),
                3 => Json(json!({"payment_id":"pay_pending"})).into_response(),
                _ => {let mut current=value.write().await; current["product_id"]=json!("pdt_plus");current["previous_billing_date"]=json!(Utc::now().to_rfc3339());current["next_billing_date"]=json!((Utc::now()+Duration::days(30)).to_rfc3339());Json(json!({"payment_id":"pay_success"})).into_response()}
            }
        }}));
    let (base, task) = serve(mock).await;
    let mut config = original.config.clone();
    config.dodo_url = format!("{base}/").parse().unwrap();
    config.dodo_key = "mock".to_owned().into();
    let state = configured(&original, config);
    let app = routes::router(state.clone());
    (
        state,
        app,
        account,
        tokens,
        Provider {
            current,
            amount,
            calls,
            mode,
        },
        task,
    )
}
async fn quote(app: &Router, token: &str) -> Value {
    let (status, value) = request(
        app,
        "POST",
        "/v1/billing/upgrade/quote",
        Some(token),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    value
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn difference_upgrade_requires_quote_confirmation_and_charges_once() {
    let (state, app, account, tokens, provider, task) = setup().await;
    let q = quote(&app, &tokens.access_token).await;
    assert_eq!(q["amount"], 2290);
    assert_eq!(q["base_difference_cents"], 2290);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    let (_, _, other, other_tokens) = fixture().await;
    let body = json!({"quote_id":q["quote_id"]});
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/billing/upgrade",
            Some(&other_tokens.access_token),
            body.clone()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    provider.amount.store(2500, Ordering::SeqCst);
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/billing/upgrade",
            Some(&tokens.access_token),
            body
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    let q = quote(&app, &tokens.access_token).await;
    use lennette_server::entities::plan_upgrades;
    let id = q["quote_id"].as_str().unwrap().parse::<Uuid>().unwrap();
    let mut expired: plan_upgrades::ActiveModel = plan_upgrades::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .unwrap()
        .unwrap()
        .into();
    expired.expires_at = Set(Utc::now() - Duration::seconds(1));
    expired.update(&state.db).await.unwrap();
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/billing/upgrade",
            Some(&tokens.access_token),
            json!({"quote_id":id})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    provider.amount.store(2290, Ordering::SeqCst);
    let q = quote(&app, &tokens.access_token).await;
    let body = json!({"quote_id":q["quote_id"]});
    let (a, b) = tokio::join!(
        request(
            &app,
            "POST",
            "/v1/billing/upgrade",
            Some(&tokens.access_token),
            body.clone()
        ),
        request(
            &app,
            "POST",
            "/v1/billing/upgrade",
            Some(&tokens.access_token),
            body.clone()
        )
    );
    assert!([a.0, b.0].contains(&StatusCode::OK));
    assert!(
        [a.0, b.0]
            .iter()
            .all(|status| [StatusCode::OK, StatusCode::BAD_REQUEST].contains(status))
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        subscriptions::Entity::find_by_id(account)
            .one(&state.db)
            .await
            .unwrap()
            .unwrap()
            .plan,
        "managed_plus"
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/billing/upgrade",
            Some(&tokens.access_token),
            body
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    task.abort();
    accounts::Entity::delete_by_id(other)
        .exec(&state.db)
        .await
        .unwrap();
    cleanup(state, account).await;
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn failed_and_uncertain_upgrades_never_grant_plus_or_repeat_a_charge() {
    let (state, app, account, tokens, provider, task) = setup().await;
    provider.mode.store(1, Ordering::SeqCst);
    let q = quote(&app, &tokens.access_token).await;
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/billing/upgrade",
            Some(&tokens.access_token),
            json!({"quote_id":q["quote_id"]})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    // A definitive rejection can be retried with a newly reviewed quote.
    provider.mode.store(2, Ordering::SeqCst);
    let q = quote(&app, &tokens.access_token).await;
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/billing/upgrade",
            Some(&tokens.access_token),
            json!({"quote_id":q["quote_id"]})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let q = quote(&app, &tokens.access_token).await;
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/billing/upgrade",
            Some(&tokens.access_token),
            json!({"quote_id":q["quote_id"]})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        subscriptions::Entity::find_by_id(account)
            .one(&state.db)
            .await
            .unwrap()
            .unwrap()
            .plan,
        "managed"
    );
    assert_eq!(provider.current.read().await["product_id"], "pdt_standard");
    task.abort();
    cleanup(state, account).await;
}

#[tokio::test]
#[ignore = "requires local Postgres and Redis"]
async fn pending_upgrade_keeps_managed_access_until_payment_is_confirmed() {
    let (state, app, account, tokens, provider, task) = setup().await;
    provider.mode.store(3, Ordering::SeqCst);
    let q = quote(&app, &tokens.access_token).await;
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/billing/upgrade",
            Some(&tokens.access_token),
            json!({"quote_id":q["quote_id"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        subscriptions::Entity::find_by_id(account)
            .one(&state.db)
            .await
            .unwrap()
            .unwrap()
            .plan,
        "managed"
    );
    task.abort();
    cleanup(state, account).await;
}
