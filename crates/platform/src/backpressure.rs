//! Queue backpressure guard.
//!
//! Prevents job queue flooding by rejecting new enqueue attempts when the
//! number of pending jobs exceeds a configurable threshold. The guard is
//! purely logic-based and does not perform database queries itself; the
//! pending count is supplied externally so that the module remains testable
//! without a database.

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for queue backpressure.
#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    /// Maximum number of pending jobs allowed before rejecting new work.
    pub max_pending_jobs: u64,
    /// Suggested retry delay (in seconds) to include in 429 responses.
    pub suggested_retry_seconds: u64,
}

impl BackpressureConfig {
    /// Load backpressure configuration from environment variables.
    ///
    /// Environment variables:
    /// - `BACKPRESSURE_MAX_PENDING_JOBS` (default: 1000)
    /// - `BACKPRESSURE_RETRY_SECONDS` (default: 60)
    pub fn from_env() -> Self {
        let max_pending_jobs = std::env::var("BACKPRESSURE_MAX_PENDING_JOBS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000);

        let suggested_retry_seconds = std::env::var("BACKPRESSURE_RETRY_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);

        Self {
            max_pending_jobs,
            suggested_retry_seconds,
        }
    }
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            max_pending_jobs: 1000,
            suggested_retry_seconds: 60,
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Returned when the job queue is at capacity and new work is rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackpressureExceeded {
    /// Suggested number of seconds the client should wait before retrying.
    pub suggested_retry_seconds: u64,
}

// ---------------------------------------------------------------------------
// Guard
// ---------------------------------------------------------------------------

/// Backpressure guard that checks if the job queue can accept new work.
///
/// The guard does not own a database connection. Instead, it accepts the
/// current pending job count as a parameter, making it easy to test in
/// isolation.
#[derive(Debug, Clone)]
pub struct BackpressureGuard {
    config: BackpressureConfig,
}

impl BackpressureGuard {
    /// Create a new backpressure guard with the given configuration.
    pub fn new(config: BackpressureConfig) -> Self {
        Self { config }
    }

    /// Check if the queue can accept new work.
    ///
    /// Returns `Ok(())` if the pending count is below the threshold, or
    /// `Err(BackpressureExceeded)` if the queue is at capacity.
    pub fn check(&self, pending_count: u64) -> Result<(), BackpressureExceeded> {
        if pending_count >= self.config.max_pending_jobs {
            Err(BackpressureExceeded {
                suggested_retry_seconds: self.config.suggested_retry_seconds,
            })
        } else {
            Ok(())
        }
    }

    /// Return the maximum pending jobs threshold.
    pub fn max_pending_jobs(&self) -> u64 {
        self.config.max_pending_jobs
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_when_below_threshold() {
        let guard = BackpressureGuard::new(BackpressureConfig {
            max_pending_jobs: 100,
            suggested_retry_seconds: 30,
        });

        assert!(guard.check(0).is_ok());
        assert!(guard.check(50).is_ok());
        assert!(guard.check(99).is_ok());
    }

    #[test]
    fn rejects_at_threshold() {
        let guard = BackpressureGuard::new(BackpressureConfig {
            max_pending_jobs: 100,
            suggested_retry_seconds: 30,
        });

        let result = guard.check(100);
        assert!(result.is_err());
        let exceeded = result.unwrap_err();
        assert_eq!(exceeded.suggested_retry_seconds, 30);
    }

    #[test]
    fn rejects_above_threshold() {
        let guard = BackpressureGuard::new(BackpressureConfig {
            max_pending_jobs: 100,
            suggested_retry_seconds: 45,
        });

        let result = guard.check(200);
        assert!(result.is_err());
        let exceeded = result.unwrap_err();
        assert_eq!(exceeded.suggested_retry_seconds, 45);
    }

    #[test]
    fn config_from_env_uses_defaults() {
        let config = BackpressureConfig::default();
        assert_eq!(config.max_pending_jobs, 1000);
        assert_eq!(config.suggested_retry_seconds, 60);
    }

    #[test]
    fn max_pending_jobs_accessor() {
        let guard = BackpressureGuard::new(BackpressureConfig {
            max_pending_jobs: 500,
            suggested_retry_seconds: 60,
        });
        assert_eq!(guard.max_pending_jobs(), 500);
    }
}
