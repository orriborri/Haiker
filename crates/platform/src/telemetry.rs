//! Telemetry setup - tracing, metrics, and logging.

use tower_http::classify::{ServerErrorsAsFailures, SharedClassifier};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Log output format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogFormat {
    /// Human-readable output for local development.
    Pretty,
    /// Structured JSON output for production environments.
    Json,
}

impl LogFormat {
    /// Parse a string into a `LogFormat`, defaulting to `Pretty`.
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => Self::Json,
            _ => Self::Pretty,
        }
    }
}

/// Configuration for telemetry initialization.
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// The output format for log lines.
    pub log_format: LogFormat,
    /// The default log level / filter directives (e.g. "info" or "info,haiker_api=debug").
    pub log_level: String,
    /// The name of the service emitting telemetry.
    pub service_name: String,
}

impl TelemetryConfig {
    /// Build a `TelemetryConfig` from environment variables:
    ///
    /// - `RUST_LOG`: filter directives (default: "info")
    /// - `LOG_FORMAT`: "pretty" or "json" (default: "pretty")
    /// - `SERVICE_NAME`: the service name (default: "haiker")
    pub fn from_env() -> Self {
        let log_level =
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
        let log_format =
            LogFormat::from_str(&std::env::var("LOG_FORMAT").unwrap_or_else(|_| "pretty".to_string()));
        let service_name =
            std::env::var("SERVICE_NAME").unwrap_or_else(|_| "haiker".to_string());

        Self {
            log_format,
            log_level,
            service_name,
        }
    }
}

/// Initialize the global tracing subscriber based on the provided configuration.
///
/// This should be called once at application startup. It sets up an `EnvFilter`
/// and either a pretty or JSON formatter layer.
pub fn init_telemetry(config: &TelemetryConfig) {
    let env_filter = EnvFilter::try_new(&config.log_level)
        .unwrap_or_else(|_| EnvFilter::new("info"));

    match config.log_format {
        LogFormat::Pretty => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    fmt::layer()
                        .with_target(true)
                        .with_thread_ids(false)
                        .with_file(false),
                )
                .init();
        }
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    fmt::layer()
                        .json()
                        .with_target(true)
                        .with_span_events(fmt::format::FmtSpan::NEW | fmt::format::FmtSpan::CLOSE)
                        .with_current_span(true),
                )
                .init();
        }
    }

    tracing::info!(service = %config.service_name, "Telemetry initialized");
}

/// Create a `tower_http` trace layer for Axum routers.
///
/// This layer traces HTTP requests with method, URI, status code, and latency.
/// Each request is assigned a unique `request_id` (UUID v4) in its span.
pub fn http_trace_layer<B>() -> TraceLayer<SharedClassifier<ServerErrorsAsFailures>, impl Fn(&http::Request<B>) -> tracing::Span + Clone>
{
    TraceLayer::new_for_http().make_span_with(|request: &http::Request<B>| {
        let request_id = uuid::Uuid::new_v4().to_string();
        tracing::info_span!(
            "http_request",
            method = %request.method(),
            uri = %request.uri(),
            request_id = %request_id,
        )
    })
}
