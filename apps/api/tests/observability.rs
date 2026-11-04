//! End-to-end SDK -> OpenObserve Enterprise smoke test.
use serde_json::{Value, json};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the local infrastructure containers"]
async fn logs_metrics_and_traces_reach_openobserve() {
    let started = chrono::Utc::now().timestamp_micros();
    let telemetry = lennette_server::telemetry::init()
        .unwrap()
        .expect("set OTEL_EXPORTER_OTLP_ENDPOINT");
    let marker = format!("lennette_smoke_{}", uuid::Uuid::new_v4().simple());
    let span = tracing::info_span!("observability_smoke",smoke_id=%marker);
    span.in_scope(|| tracing::info!(smoke_id=%marker,"local observability smoke test"));
    drop(span);
    use tower::ServiceExt;
    let app = axum::Router::new()
        .route(
            "/smoke",
            axum::routing::get(|| async { axum::http::StatusCode::NO_CONTENT }),
        )
        .layer(axum::middleware::from_fn(
            lennette_server::telemetry::context,
        ))
        .layer(
            tower_http::trace::TraceLayer::new_for_http().make_span_with(
                |_: &axum::http::Request<axum::body::Body>| {
                    tracing::info_span!("http_request", otel.kind = "server")
                },
            ),
        );
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/smoke")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
    let meter = opentelemetry::global::meter("lennette-observability-test");
    meter.u64_counter("lennette_smoke_requests").build().add(
        1,
        &[opentelemetry::KeyValue::new("smoke_id", marker.clone())],
    );
    let state = lennette_server::state::AppState::connect(
        lennette_server::config::Config::from_env().unwrap(),
    )
    .await
    .unwrap();
    lennette_server::runtime::infrastructure_metrics::sample(&state).await;
    tokio::task::spawn_blocking(move || telemetry.shutdown())
        .await
        .unwrap()
        .unwrap();
    let http = reqwest::Client::new();
    let user = std::env::var("OPENOBSERVE_USER_EMAIL").unwrap_or("dev@lennette.local".into());
    let password =
        std::env::var("OPENOBSERVE_USER_PASSWORD").unwrap_or("LocalOnly-Observe123!".into());
    // Query persisted ingestion streams, not just HTTP acceptance.
    for (kind, stream) in [
        ("logs", "lennette_api"),
        ("traces", "lennette_api"),
        ("metrics", "lennette_smoke_requests"),
        ("metrics", "postgresql_backends"),
        ("metrics", "redis_clients_connected"),
        ("metrics", "lennette_storage_cleanup_pending"),
    ] {
        let mut found = false;
        for _ in 0..15 {
            let now = chrono::Utc::now().timestamp_micros();
            let sql = if stream == "lennette_api" || stream == "lennette_smoke_requests" {
                format!("SELECT * FROM \"{stream}\" WHERE smoke_id = '{marker}'")
            } else {
                format!("SELECT * FROM \"{stream}\"")
            };
            let response=http.post(format!("http://127.0.0.1:5080/api/default/_search?type={kind}"))
                .basic_auth(&user,Some(&password)).json(&json!({"query":{"sql":sql,"start_time":started,"end_time":now,"from":0,"size":10}})).send().await.unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            if status.is_success() && body["hits"].as_array().is_some_and(|rows| !rows.is_empty()) {
                found = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        assert!(
            found,
            "{kind} smoke record was not queryable in OpenObserve"
        );
    }
}
