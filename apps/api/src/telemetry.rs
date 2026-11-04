use opentelemetry::trace::TracerProvider;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init() -> anyhow::Result<Option<opentelemetry_sdk::trace::SdkTracerProvider>> {
    opentelemetry::global::set_text_map_propagator(
        opentelemetry_sdk::propagation::TraceContextPropagator::new(),
    );
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,sqlx=warn".into());
    let provider = if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some() {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .build()?;
        Some(
            opentelemetry_sdk::trace::SdkTracerProvider::builder()
                .with_resource(
                    opentelemetry_sdk::Resource::builder()
                        .with_service_name("lennette-api")
                        .build(),
                )
                .with_batch_exporter(exporter)
                .build(),
        )
    } else {
        None
    };
    let otel = provider
        .as_ref()
        .map(|p| tracing_opentelemetry::layer().with_tracer(p.tracer("lennette-api")));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().json())
        .with(otel)
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
    next.run(request).await
}
