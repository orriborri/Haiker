//! Telemetry and structured logging setup.
//!
//! Initializes the `tracing` subscriber with environment-controlled filtering
//! and JSON or pretty-printed output.

use tracing_subscriber::{fmt, EnvFilter};

/// Initialize the global tracing subscriber.
///
/// Configures structured logging with an environment filter controlled by the
/// `RUST_LOG` environment variable. Defaults to `info` level.
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    fmt().with_env_filter(filter).with_target(true).init();
}
