use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;
use tracing_subscriber::{
    Layer,
    filter::{FilterExt, filter_fn},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

// OTLP intentionally returns a generic error to processors. Preserve only its
// transport diagnostic in the console even with the normal info filter. Broad OTLP DEBUG
// logging can include collector response bodies, so do not enable it globally.
fn transport_diagnostic(metadata: &tracing::Metadata<'_>) -> bool {
    metadata.target() == "opentelemetry-otlp" && metadata.name() == "HttpClient.NetworkError"
}

pub struct Telemetry {
    traces: opentelemetry_sdk::trace::SdkTracerProvider,
    metrics: opentelemetry_sdk::metrics::SdkMeterProvider,
    logs: opentelemetry_sdk::logs::SdkLoggerProvider,
}
impl Telemetry {
    pub fn shutdown(self) -> anyhow::Result<()> {
        let budget = std::time::Duration::from_secs(5);
        let errors = [
            self.traces.shutdown_with_timeout(budget),
            self.metrics.shutdown_with_timeout(budget),
            self.logs.shutdown_with_timeout(budget),
        ]
        .into_iter()
        .filter_map(|result| result.err())
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
        anyhow::ensure!(
            errors.is_empty(),
            "telemetry shutdown failed: {}",
            errors.join("; ")
        );
        Ok(())
    }
}

pub fn init() -> anyhow::Result<Option<Telemetry>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    opentelemetry::global::set_text_map_propagator(
        opentelemetry_sdk::propagation::TraceContextPropagator::new(),
    );
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,sqlx=warn".into());
    let (provider, startup_note) = if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some() {
        // Keep the local 30s ingestion budget even when the deployment does not
        // set OTEL_EXPORTER_OTLP_TIMEOUT (the SDK defaults to 10s, which is
        // what produced the Dokploy `Export.Failed.DeadlineExceeded` spam when
        // the Cloudflare-fronted collector was slow to answer). Per-signal
        // OTEL_EXPORTER_OTLP_{TRACES,METRICS,LOGS}_TIMEOUT still wins when set.
        fn otlp_timeout(signal_var: &str) -> std::time::Duration {
            const DEFAULT_MS: u64 = 30_000;
            fn parse_ms(var: &str) -> Option<u64> {
                std::env::var(var).ok()?.parse::<u64>().ok()
            }
            parse_ms(signal_var)
                .or_else(|| parse_ms("OTEL_EXPORTER_OTLP_TIMEOUT"))
                .map(std::time::Duration::from_millis)
                .unwrap_or_else(|| std::time::Duration::from_millis(DEFAULT_MS))
        }
        // Log only the collector host so a misconfigured Dokploy endpoint is
        // obvious without leaking the org/token path into the log stream.
        // (Emitted after the subscriber is installed; see below.)
        let otel_collector_host: Option<String> = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .and_then(|endpoint| endpoint.parse::<url::Url>().ok())
            .and_then(|url| url.host_str().map(str::to_owned));
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_timeout(otlp_timeout("OTEL_EXPORTER_OTLP_TRACES_TIMEOUT"))
            .build()?;
        let resource = opentelemetry_sdk::Resource::builder()
            .with_service_name("lennette-api")
            .build();
        let traces = opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .with_resource(resource.clone())
            .with_batch_exporter(exporter)
            .build();
        let metrics = opentelemetry_sdk::metrics::SdkMeterProvider::builder()
            .with_resource(resource.clone())
            .with_periodic_exporter(
                opentelemetry_otlp::MetricExporter::builder()
                    .with_http()
                    .with_timeout(otlp_timeout("OTEL_EXPORTER_OTLP_METRICS_TIMEOUT"))
                    .build()?,
            )
            .build();
        let logs = opentelemetry_sdk::logs::SdkLoggerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(
                opentelemetry_otlp::LogExporter::builder()
                    .with_http()
                    .with_timeout(otlp_timeout("OTEL_EXPORTER_OTLP_LOGS_TIMEOUT"))
                    .build()?,
            )
            .build();
        opentelemetry::global::set_meter_provider(metrics.clone());
        let startup_note = otel_collector_host.map(|host| {
            format!("OTLP export enabled to {host} (30s default timeout unless overridden)")
        });
        (
            Some(Telemetry {
                traces,
                metrics,
                logs,
            }),
            startup_note,
        )
    } else {
        (None, None)
    };
    let otel = provider.as_ref().map(|p| {
        tracing_opentelemetry::layer()
            .with_tracer(p.traces.tracer("lennette-api"))
            .with_filter(filter.clone())
    });
    let logs = provider.as_ref().map(|p| {
        opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&p.logs)
            .with_filter(filter.clone())
    });
    let console_filter = filter.or(filter_fn(transport_diagnostic));
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_filter(console_filter),
        )
        .with(otel)
        .with(logs)
        .try_init()?;
    if let Some(message) = startup_note {
        tracing::info!(message = message.as_str());
    }
    Ok(provider)
}

pub async fn context(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use tracing::Instrument;
    use tracing_opentelemetry::OpenTelemetrySpanExt;
    let route = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map_or("unmatched", |p| p.as_str())
        .to_owned();
    let method = match request.method().as_str() {
        method @ ("GET" | "HEAD" | "POST" | "PUT" | "DELETE" | "CONNECT" | "OPTIONS" | "TRACE"
        | "PATCH") => method,
        _ => "_OTHER",
    }
    .to_owned();
    let span = tracing::info_span!(
        "http_request",
        otel.kind = "server",
        http.route = route.as_str(),
        http.request.method = method.as_str(),
        http.response.status_code = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    );
    let parent = opentelemetry::global::get_text_map_propagator(|p| {
        p.extract(&opentelemetry_http::HeaderExtractor(request.headers()))
    });
    let _ = span.set_parent(parent);
    async move {
        let start = std::time::Instant::now();
        let response = next.run(request).await;
        let elapsed = start.elapsed();
        let status = response.status().as_u16();
        let span = tracing::Span::current();
        span.record("http.response.status_code", status);
        if response.status().is_server_error() {
            span.record("otel.status_code", "ERROR");
            let principal = response.extensions().get::<crate::auth::Principal>();
            let reason = response
                .extensions()
                .get::<crate::error::FailureCode>()
                .map_or("server_error", |code| code.0);
            let latency_ms = elapsed.as_millis() as u64;
            let account_id = principal.map(|p| tracing::field::display(p.account_id));
            let device_id = principal.map(|p| tracing::field::display(p.device_id));
            let session_id = principal.map(|p| tracing::field::display(p.session_id));
            if status == 503 {
                tracing::warn!(
                    event = "http.request.dependency_unavailable",
                    method,
                    path = route,
                    status,
                    reason,
                    latency_ms,
                    account_id,
                    device_id,
                    session_id,
                    "HTTP dependency unavailable"
                );
            } else {
                tracing::error!(
                    event = "http.request.failed",
                    method,
                    path = route,
                    status,
                    reason,
                    latency_ms,
                    account_id,
                    device_id,
                    session_id,
                    "HTTP request failed"
                );
            }
        }
        let attributes = [
            opentelemetry::KeyValue::new("http.route", route),
            opentelemetry::KeyValue::new("http.request.method", method),
            opentelemetry::KeyValue::new("http.response.status_code", i64::from(status)),
        ];
        let metrics = http_metrics();
        metrics.0.add(1, &attributes);
        metrics.1.record(elapsed.as_secs_f64(), &attributes);
        response
    }
    .instrument(span)
    .await
}

/// Poll a response stream under its originating span after the handler returns.
pub(crate) fn instrument_stream<S: futures_util::Stream>(
    stream: S,
    span: tracing::Span,
) -> impl futures_util::Stream<Item = S::Item> {
    let mut stream = Box::pin(stream);
    futures_util::stream::poll_fn(move |cx| span.in_scope(|| stream.as_mut().poll_next(cx)))
}

fn http_metrics() -> &'static (
    opentelemetry::metrics::Counter<u64>,
    opentelemetry::metrics::Histogram<f64>,
) {
    static METRICS: std::sync::OnceLock<(
        opentelemetry::metrics::Counter<u64>,
        opentelemetry::metrics::Histogram<f64>,
    )> = std::sync::OnceLock::new();
    METRICS.get_or_init(|| {
        let meter = opentelemetry::global::meter("lennette-api");
        (
            meter.u64_counter("http.server.requests").build(),
            meter
                .f64_histogram("http.server.request.duration")
                .with_unit("s")
                .build(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[derive(Debug, Clone, Default)]
    struct LogCapture(Arc<Mutex<Vec<opentelemetry_sdk::logs::SdkLogRecord>>>);

    impl opentelemetry_sdk::logs::LogProcessor for LogCapture {
        fn emit(
            &self,
            record: &mut opentelemetry_sdk::logs::SdkLogRecord,
            _: &opentelemetry::InstrumentationScope,
        ) {
            self.0.lock().unwrap().push(record.clone());
        }

        fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult {
            Ok(())
        }
    }

    #[tokio::test]
    async fn request_and_stream_failures_keep_safe_fields_and_trace_context() {
        use axum::{
            Router,
            body::Body,
            http::Request,
            response::sse::{Event, Sse},
            routing::get,
        };
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        use tracing::instrument::WithSubscriber;

        let output = Arc::new(Mutex::new(Vec::new()));
        let writer = output.clone();
        let capture = LogCapture::default();
        let logs = opentelemetry_sdk::logs::SdkLoggerProvider::builder()
            .with_log_processor(capture.clone())
            .build();
        let traces = opentelemetry_sdk::trace::SdkTracerProvider::builder().build();
        opentelemetry::global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );
        let subscriber = tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_writer(move || Capture(writer.clone())),
            )
            .with(tracing_opentelemetry::layer().with_tracer(traces.tracer("test")))
            .with(opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logs));
        let dispatch = tracing::Dispatch::new(subscriber);
        let app = Router::new()
            .route(
                "/good",
                get(|| async { axum::http::StatusCode::NO_CONTENT }),
            )
            .route(
                "/example/{id}",
                get(|| async {
                    crate::error::ApiError::Internal(anyhow::anyhow!("private database details"))
                }),
            )
            .route(
                "/unavailable",
                get(|| async { crate::error::ApiError::StorageUnavailable }),
            )
            .route("/events", get(|| async {
                let stream = async_stream::stream! {
                    yield Ok::<_, std::convert::Infallible>(Event::default().data("connected"));
                    tracing::warn!(event = "test.stream.failed", "Stream failed after the first event");
                };
                Sse::new(instrument_stream(stream, tracing::Span::current()))
            }))
            .layer(axum::middleware::from_fn(context));
        for (path, status) in [
            ("/good", 204),
            ("/example/private-id?token=private-token", 500),
            ("/unavailable", 503),
            ("/events", 200),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header(
                            "traceparent",
                            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .with_subscriber(dispatch.clone())
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), status);
            if path == "/events" {
                assert!(
                    !String::from_utf8_lossy(&output.lock().unwrap())
                        .contains("test.stream.failed")
                );
                let body = response
                    .into_body()
                    .collect()
                    .with_subscriber(dispatch.clone())
                    .await
                    .unwrap();
                assert_eq!(body.to_bytes(), "data: connected\n\n");
            }
        }
        let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        let logs: Vec<serde_json::Value> = output
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .filter(|log| log["fields"]["event"].is_string())
            .collect();
        assert_eq!(logs.len(), 3, "{output}");
        assert_eq!(logs[0]["fields"]["event"], "http.request.failed");
        assert_eq!(logs[0]["fields"]["path"], "/example/{id}");
        assert_eq!(logs[0]["fields"]["reason"], "internal_error");
        assert_eq!(
            logs[1]["fields"]["event"],
            "http.request.dependency_unavailable"
        );
        assert_eq!(logs[1]["fields"]["reason"], "storage_unavailable");
        assert_eq!(logs[2]["fields"]["event"], "test.stream.failed");
        assert!(!output.contains("private"));
        let exported = capture.0.lock().unwrap();
        let request_logs = exported
            .iter()
            .filter(|record| {
                record
                    .attributes_iter()
                    .any(|(key, _)| key.as_str() == "event")
            })
            .collect::<Vec<_>>();
        assert_eq!(request_logs.len(), 3);
        for record in request_logs {
            let context = record.trace_context().unwrap();
            assert_eq!(
                context.trace_id.to_string(),
                "4bf92f3577b34da6a3ce929d0e0e4736"
            );
            assert_ne!(context.span_id.to_string(), "00f067aa0ba902b7");
        }
    }

    #[test]
    fn console_captures_transport_failures_without_enabling_other_debug_events() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let writer = output.clone();
        let filter = tracing_subscriber::EnvFilter::new("info").or(filter_fn(transport_diagnostic));
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(move || Capture(writer.clone()))
                .with_filter(filter),
        );
        tracing::subscriber::with_default(subscriber, || {
            tracing::event!(name: "HttpClient.NetworkError", target: "opentelemetry-otlp",
                tracing::Level::DEBUG, error = "request timed out");
            tracing::event!(name: "HttpClient.Response", target: "opentelemetry-otlp",
                tracing::Level::DEBUG, body = "private collector response");
            tracing::debug!("unrelated debug event");
            tracing::info!("API listening");
        });
        let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        assert!(output.contains("request timed out"));
        assert!(output.contains("API listening"));
        assert!(!output.contains("private collector response"));
        assert!(!output.contains("unrelated debug event"));
    }
}
