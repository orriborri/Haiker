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
    pub fn parse(s: &str) -> Self {
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
    /// Optional OTLP exporter endpoint (e.g. "http://localhost:4317").
    /// When set, the system is configured for OpenTelemetry export.
    ///
    /// NOTE: Full OpenTelemetry SDK integration (opentelemetry, opentelemetry-otlp,
    /// tracing-opentelemetry crates) is deferred until the project upgrades to Rust >= 1.94.
    /// Until then, this field logs that OTLP export is configured but does not actually
    /// export spans via the OTel protocol.
    pub otlp_endpoint: Option<String>,
    /// The version of the service, used as a resource attribute in telemetry.
    pub service_version: String,
    /// Optional directory for log file output.
    /// When set, logs are written to daily-rotating files in this directory.
    /// Logs are still emitted to stdout as well.
    pub log_file_dir: Option<String>,
    /// Optional file name prefix for log files (default: "haiker").
    pub log_file_prefix: String,
}

impl TelemetryConfig {
    /// Build a `TelemetryConfig` from environment variables:
    ///
    /// - `RUST_LOG`: filter directives (default: "info")
    /// - `LOG_FORMAT`: "pretty" or "json" (default: "pretty")
    /// - `SERVICE_NAME`: the service name (default: "haiker")
    /// - `LOG_FILE_DIR`: directory for log file output (optional; if unset, no file logging)
    /// - `LOG_FILE_PREFIX`: file name prefix (default: "haiker")
    pub fn from_env() -> Self {
        let log_level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
        let log_format =
            LogFormat::parse(&std::env::var("LOG_FORMAT").unwrap_or_else(|_| "pretty".to_string()));
        let service_name = std::env::var("SERVICE_NAME").unwrap_or_else(|_| "haiker".to_string());
        let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();
        let service_version =
            std::env::var("SERVICE_VERSION").unwrap_or_else(|_| "0.1.0".to_string());
        let log_file_dir = std::env::var("LOG_FILE_DIR").ok();
        let log_file_prefix =
            std::env::var("LOG_FILE_PREFIX").unwrap_or_else(|_| "haiker".to_string());

        Self {
            log_format,
            log_level,
            service_name,
            otlp_endpoint,
            service_version,
            log_file_dir,
            log_file_prefix,
        }
    }
}

/// Initialize the global tracing subscriber based on the provided configuration.
///
/// This should be called once at application startup. It sets up an `EnvFilter`
/// and either a pretty or JSON formatter layer. When `LOG_FILE_DIR` is configured,
/// an additional file logging layer writes JSON logs to daily-rotating files.
pub fn init_telemetry(config: &TelemetryConfig) {
    let env_filter =
        EnvFilter::try_new(&config.log_level).unwrap_or_else(|_| EnvFilter::new("info"));

    // Optional file appender for daily-rotating log files
    let file_appender = config
        .log_file_dir
        .as_ref()
        .map(|dir| tracing_appender::rolling::daily(dir, &config.log_file_prefix));

    match config.log_format {
        LogFormat::Pretty => {
            let file_layer = file_appender.map(|appender| {
                fmt::layer()
                    .json()
                    .with_target(true)
                    .with_current_span(true)
                    .with_writer(appender)
            });

            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    fmt::layer()
                        .with_target(true)
                        .with_thread_ids(false)
                        .with_file(false),
                )
                .with(file_layer)
                .init();
        }
        LogFormat::Json => {
            let file_layer = file_appender.map(|appender| {
                fmt::layer()
                    .json()
                    .with_target(true)
                    .with_current_span(true)
                    .with_writer(appender)
            });

            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    fmt::layer()
                        .json()
                        .with_target(true)
                        .with_span_events(fmt::format::FmtSpan::NEW | fmt::format::FmtSpan::CLOSE)
                        .with_current_span(true),
                )
                .with(file_layer)
                .init();
        }
    }

    tracing::info!(service = %config.service_name, version = %config.service_version, "Telemetry initialized");

    if let Some(ref dir) = config.log_file_dir {
        tracing::info!(log_dir = %dir, prefix = %config.log_file_prefix, "File logging enabled (daily rotation)");
    }

    if let Some(ref endpoint) = config.otlp_endpoint {
        tracing::info!(
            otlp_endpoint = %endpoint,
            "OTLP export configured (actual export deferred until opentelemetry crates are added with Rust >= 1.94)"
        );
    }
}

/// Create a `tower_http` trace layer for Axum routers.
///
/// This layer traces HTTP requests with method, URI, status code, and latency.
/// Each request is assigned a unique `request_id` (UUID v4) in its span.
pub fn http_trace_layer<B>() -> TraceLayer<
    SharedClassifier<ServerErrorsAsFailures>,
    impl Fn(&http::Request<B>) -> tracing::Span + Clone,
> {
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
