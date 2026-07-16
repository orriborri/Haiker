//! Upload quota enforcement.
//!
//! Limits the number of import operations a user may initiate within a
//! rolling 24-hour window. When the quota is exceeded, a [`QuotaExceeded`]
//! error is returned with the number of seconds until the window resets.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use uuid::Uuid;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for per-user daily upload quotas.
#[derive(Debug, Clone)]
pub struct UploadQuotaConfig {
    /// Maximum number of imports allowed per user within a 24-hour window.
    pub max_imports_per_day: u64,
}

impl UploadQuotaConfig {
    /// Load upload quota configuration from environment variables.
    ///
    /// Environment variables:
    /// - `UPLOAD_QUOTA_MAX_PER_DAY` (default: 100)
    pub fn from_env() -> Self {
        let max_imports_per_day = std::env::var("UPLOAD_QUOTA_MAX_PER_DAY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100);

        Self {
            max_imports_per_day,
        }
    }
}

impl Default for UploadQuotaConfig {
    fn default() -> Self {
        Self {
            max_imports_per_day: 100,
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Returned when a user has exceeded their daily import quota.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaExceeded {
    /// Number of seconds until the quota window resets.
    pub seconds_until_reset: u64,
}

// ---------------------------------------------------------------------------
// Upload quota tracker
// ---------------------------------------------------------------------------

/// In-memory per-user upload quota tracker.
///
/// Uses a rolling 24-hour window per user. Thread-safe via `Arc<Mutex<...>>`.
#[derive(Debug, Clone)]
pub struct UploadQuota {
    /// Per-user import count and window start.
    state: Arc<Mutex<HashMap<Uuid, (u64, Instant)>>>,
    /// Maximum imports allowed per window.
    max_imports: u64,
    /// Window duration (24 hours by default).
    window_duration: Duration,
}

impl UploadQuota {
    /// Create a new upload quota tracker with the given configuration.
    pub fn new(config: &UploadQuotaConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(HashMap::new())),
            max_imports: config.max_imports_per_day,
            window_duration: Duration::from_secs(24 * 60 * 60),
        }
    }

    /// Create an upload quota tracker with a custom window duration (for testing).
    #[cfg(test)]
    fn new_with_window(max_imports: u64, window_duration: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(HashMap::new())),
            max_imports,
            window_duration,
        }
    }

    /// Check if the user is within quota and increment their count.
    ///
    /// Returns `Ok(())` if the user is within quota, or `Err(QuotaExceeded)`
    /// with the number of seconds until the quota resets.
    pub fn check_and_increment(&self, user_id: Uuid) -> Result<(), QuotaExceeded> {
        self.check_and_increment_at(user_id, Instant::now())
    }

    /// Internal implementation with injectable "now" for testing.
    fn check_and_increment_at(&self, user_id: Uuid, now: Instant) -> Result<(), QuotaExceeded> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());

        let entry = state.entry(user_id).or_insert((0, now));

        // Check if the window has expired and reset if so.
        let elapsed = now.duration_since(entry.1);
        if elapsed >= self.window_duration {
            *entry = (0, now);
        }

        // Check if the user is at the limit.
        if entry.0 >= self.max_imports {
            let seconds_until_reset = self
                .window_duration
                .saturating_sub(elapsed)
                .as_secs()
                .max(1);
            return Err(QuotaExceeded {
                seconds_until_reset,
            });
        }

        // Increment the counter.
        entry.0 += 1;
        Ok(())
    }

    /// Return the maximum imports per window (useful for error messages).
    pub fn max_imports_per_day(&self) -> u64 {
        self.max_imports
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_requests_within_quota() {
        let quota = UploadQuota::new_with_window(3, Duration::from_secs(60));
        let user_id = Uuid::new_v4();

        assert!(quota.check_and_increment(user_id).is_ok());
        assert!(quota.check_and_increment(user_id).is_ok());
        assert!(quota.check_and_increment(user_id).is_ok());
    }

    #[test]
    fn rejects_requests_over_quota() {
        let quota = UploadQuota::new_with_window(2, Duration::from_secs(60));
        let user_id = Uuid::new_v4();

        assert!(quota.check_and_increment(user_id).is_ok());
        assert!(quota.check_and_increment(user_id).is_ok());

        let result = quota.check_and_increment(user_id);
        assert!(result.is_err());
        let exceeded = result.unwrap_err();
        assert!(exceeded.seconds_until_reset > 0);
    }

    #[test]
    fn window_reset_allows_new_requests() {
        let window = Duration::from_secs(60);
        let quota = UploadQuota::new_with_window(1, window);
        let user_id = Uuid::new_v4();

        let start = Instant::now();

        // First request within window.
        assert!(quota.check_and_increment_at(user_id, start).is_ok());

        // Second request still within window - rejected.
        let within_window = start + Duration::from_secs(30);
        assert!(quota
            .check_and_increment_at(user_id, within_window)
            .is_err());

        // After window expires - allowed again.
        let after_window = start + Duration::from_secs(61);
        assert!(quota.check_and_increment_at(user_id, after_window).is_ok());
    }

    #[test]
    fn different_users_have_independent_quotas() {
        let quota = UploadQuota::new_with_window(1, Duration::from_secs(60));
        let user1 = Uuid::new_v4();
        let user2 = Uuid::new_v4();

        assert!(quota.check_and_increment(user1).is_ok());
        assert!(quota.check_and_increment(user1).is_err());

        // user2 should still be allowed.
        assert!(quota.check_and_increment(user2).is_ok());
    }

    #[test]
    fn seconds_until_reset_is_correct() {
        let window = Duration::from_secs(100);
        let quota = UploadQuota::new_with_window(1, window);
        let user_id = Uuid::new_v4();

        let start = Instant::now();
        assert!(quota.check_and_increment_at(user_id, start).is_ok());

        // 40 seconds into the window, 60 seconds remain.
        let later = start + Duration::from_secs(40);
        let result = quota.check_and_increment_at(user_id, later);
        assert!(result.is_err());
        let exceeded = result.unwrap_err();
        assert_eq!(exceeded.seconds_until_reset, 60);
    }

    #[test]
    fn config_from_env_uses_defaults() {
        let config = UploadQuotaConfig::default();
        assert_eq!(config.max_imports_per_day, 100);
    }

    #[test]
    fn max_imports_per_day_accessor() {
        let config = UploadQuotaConfig {
            max_imports_per_day: 50,
        };
        let quota = UploadQuota::new(&config);
        assert_eq!(quota.max_imports_per_day(), 50);
    }
}
