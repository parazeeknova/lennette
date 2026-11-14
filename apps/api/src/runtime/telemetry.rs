use opentelemetry::trace::TracerProvider;
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
    let provider = if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some() {
        // Keep SDK timeout resolution: per-signal OTEL_*_TIMEOUT, then the shared
        // OTEL_EXPORTER_OTLP_TIMEOUT (milliseconds), then the SDK default.
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
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
                    .build()?,
            )
            .build();
        let logs = opentelemetry_sdk::logs::SdkLoggerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(
                opentelemetry_otlp::LogExporter::builder()
                    .with_http()
                    .build()?,
            )
            .build();
        opentelemetry::global::set_meter_provider(metrics.clone());
        Some(Telemetry {
            traces,
            metrics,
            logs,
        })
    } else {
        None
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
    Ok(provider)
}

pub async fn context(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use tracing_opentelemetry::OpenTelemetrySpanExt;
    let context = opentelemetry::global::get_text_map_propagator(|p| {
        p.extract(&opentelemetry_http::HeaderExtractor(request.headers()))
    });
    let _ = tracing::Span::current().set_parent(context);
    let route = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map_or("unmatched", |p| p.as_str())
        .to_owned();
    let method = request.method().to_string();
    let start = std::time::Instant::now();
    let response = next.run(request).await;
    let attributes = [
        opentelemetry::KeyValue::new("http.route", route),
        opentelemetry::KeyValue::new("http.request.method", method),
        opentelemetry::KeyValue::new(
            "http.response.status_code",
            i64::from(response.status().as_u16()),
        ),
    ];
    let metrics = http_metrics();
    metrics.0.add(1, &attributes);
    metrics.1.record(start.elapsed().as_secs_f64(), &attributes);
    response
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
