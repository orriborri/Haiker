//! In-memory store for OIDC state/nonce pairs.
//!
//! State values are generated during `POST /auth/login` and consumed during
//! `GET /auth/callback`. Each entry has a TTL of 10 minutes to prevent stale
//! state accumulation from abandoned login flows.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Default TTL for OIDC state entries (10 minutes).
const DEFAULT_TTL: Duration = Duration::from_secs(600);

/// Entry in the state store, pairing a nonce with its expiration time.
#[derive(Debug, Clone)]
struct StateEntry {
    nonce: String,
    expires_at: Instant,
}

/// In-memory store for OIDC state/nonce pairs with automatic expiry.
///
/// Thread-safe via `Mutex`. Suitable for single-instance deployments.
/// For multi-instance deployments, replace with a Redis or database-backed store.
#[derive(Debug)]
pub struct OidcStateStore {
    entries: Mutex<HashMap<String, StateEntry>>,
    ttl: Duration,
}

impl OidcStateStore {
    /// Create a new state store with the default 10-minute TTL.
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl: DEFAULT_TTL,
        }
    }

    /// Create a new state store with a custom TTL (useful for testing).
    #[cfg(test)]
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// Store a state/nonce pair. The entry will expire after the configured TTL.
    pub fn store_state(&self, state: String, nonce: String) {
        let mut entries = self.entries.lock().expect("state store lock poisoned");
        // Opportunistic cleanup of expired entries
        let now = Instant::now();
        entries.retain(|_, entry| entry.expires_at > now);
        entries.insert(
            state,
            StateEntry {
                nonce,
                expires_at: now + self.ttl,
            },
        );
    }

    /// Consume a state entry, returning the associated nonce if found and not expired.
    ///
    /// The entry is removed from the store after consumption to prevent replay.
    pub fn consume_state(&self, state: &str) -> Option<String> {
        let mut entries = self.entries.lock().expect("state store lock poisoned");
        let entry = entries.remove(state)?;
        if entry.expires_at > Instant::now() {
            Some(entry.nonce)
        } else {
            None
        }
    }
}

impl Default for OidcStateStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_consume_state() {
        let store = OidcStateStore::new();
        store.store_state("state123".to_string(), "nonce456".to_string());

        let nonce = store.consume_state("state123");
        assert_eq!(nonce, Some("nonce456".to_string()));
    }

    #[test]
    fn consume_removes_entry() {
        let store = OidcStateStore::new();
        store.store_state("state123".to_string(), "nonce456".to_string());

        let _ = store.consume_state("state123");
        let second = store.consume_state("state123");
        assert_eq!(second, None);
    }

    #[test]
    fn consume_unknown_state_returns_none() {
        let store = OidcStateStore::new();
        let result = store.consume_state("unknown");
        assert_eq!(result, None);
    }

    #[test]
    fn expired_entry_returns_none() {
        let store = OidcStateStore::with_ttl(Duration::from_millis(1));
        store.store_state("state123".to_string(), "nonce456".to_string());

        // Wait for expiry
        std::thread::sleep(Duration::from_millis(10));

        let result = store.consume_state("state123");
        assert_eq!(result, None);
    }

    #[test]
    fn store_cleans_up_expired_entries() {
        let store = OidcStateStore::with_ttl(Duration::from_millis(1));
        store.store_state("old_state".to_string(), "old_nonce".to_string());

        std::thread::sleep(Duration::from_millis(10));

        // Storing a new entry should clean up the expired one
        store.store_state("new_state".to_string(), "new_nonce".to_string());

        let entries = store.entries.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key("new_state"));
    }
}
