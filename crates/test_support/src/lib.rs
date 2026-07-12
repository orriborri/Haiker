//! Haiker test utilities and fixtures.
//!
//! Provides shared test helpers, fixture loading, and common test setup
//! used across integration and unit tests.

/// Initialize tracing for tests with a test-friendly subscriber.
///
/// Call this at the beginning of integration tests to enable log output
/// during test execution. Uses `RUST_LOG` environment variable for filtering.
pub fn init_test_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));

    // Ignore errors from re-initialization in parallel tests
    let _ = fmt().with_env_filter(filter).with_test_writer().try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracing_init_does_not_panic() {
        init_test_tracing();
    }
}
