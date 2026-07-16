//! In-memory token bucket rate limiter middleware.
//!
//! Provides per-actor (authenticated user) or per-IP (unauthenticated) rate
//! limiting using a simple token bucket algorithm. Each route category has an
//! independent bucket with configurable limits.
//!
//! # Usage
//!
//! 1. Create a [`RateLimiter`] from [`RateLimitConfig`].
//! 2. Store it in request extensions via a preceding layer.
//! 3. Apply [`rate_limit_middleware`] using `axum::middleware::from_fn`.
//! 4. Insert a [`RouteCategoryExtension`] into request extensions for each route group.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use crate::metrics::record_rate_limit_decision;
use crate::request_id::RequestId;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Rate limit configuration loaded from environment variables.
///
/// Each field represents the maximum number of requests per minute allowed
/// for the corresponding route category.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Requests per minute for auth routes (login, register, etc.).
    pub auth_rpm: u32,
    /// Requests per minute for import routes.
    pub imports_rpm: u32,
    /// Requests per minute for mutation routes (create, update, delete).
    pub mutations_rpm: u32,
    /// Requests per minute for read routes.
    pub reads_rpm: u32,
    /// Requests per minute for export routes.
    pub exports_rpm: u32,
}

impl RateLimitConfig {
    /// Load rate limit configuration from environment variables with defaults.
    ///
    /// Environment variables:
    /// - `RATE_LIMIT_AUTH_RPM` (default: 5)
    /// - `RATE_LIMIT_IMPORTS_RPM` (default: 10)
    /// - `RATE_LIMIT_MUTATIONS_RPM` (default: 30)
    /// - `RATE_LIMIT_READS_RPM` (default: 120)
    /// - `RATE_LIMIT_EXPORTS_RPM` (default: 10)
    pub fn from_env() -> Self {
        Self {
            auth_rpm: parse_env("RATE_LIMIT_AUTH_RPM", 5),
            imports_rpm: parse_env("RATE_LIMIT_IMPORTS_RPM", 10),
            mutations_rpm: parse_env("RATE_LIMIT_MUTATIONS_RPM", 30),
            reads_rpm: parse_env("RATE_LIMIT_READS_RPM", 120),
            exports_rpm: parse_env("RATE_LIMIT_EXPORTS_RPM", 10),
        }
    }

    /// Get the requests-per-minute limit for a given route category.
    pub fn rpm_for_category(&self, category: &RouteCategory) -> u32 {
        match category {
            RouteCategory::Auth => self.auth_rpm,
            RouteCategory::Import => self.imports_rpm,
            RouteCategory::Mutation => self.mutations_rpm,
            RouteCategory::Read => self.reads_rpm,
            RouteCategory::Export => self.exports_rpm,
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            auth_rpm: 5,
            imports_rpm: 10,
            mutations_rpm: 30,
            reads_rpm: 120,
            exports_rpm: 10,
        }
    }
}

fn parse_env(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

// ---------------------------------------------------------------------------
// Route category
// ---------------------------------------------------------------------------

/// Classification of routes for rate limiting purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteCategory {
    /// Authentication routes (login, register, password reset).
    Auth,
    /// Import routes (file uploads, data ingestion).
    Import,
    /// Mutation routes (create, update, delete operations).
    Mutation,
    /// Read routes (list, get, search operations).
    Read,
    /// Export routes (data export, download).
    Export,
}

impl RouteCategory {
    /// Return a lowercase string representation for metrics labeling.
    pub fn as_str(&self) -> &'static str {
        match self {
            RouteCategory::Auth => "auth",
            RouteCategory::Import => "import",
            RouteCategory::Mutation => "mutation",
            RouteCategory::Read => "read",
            RouteCategory::Export => "export",
        }
    }
}

// ---------------------------------------------------------------------------
// Route category extension
// ---------------------------------------------------------------------------

/// Extension type to be inserted into request extensions by route groups.
///
/// The rate limit middleware reads this to determine which category bucket
/// to check for the current request.
#[derive(Debug, Clone)]
pub struct RouteCategoryExtension(pub RouteCategory);

// ---------------------------------------------------------------------------
// Rate limit key
// ---------------------------------------------------------------------------

/// Identifies who is making the request for rate limiting purposes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RateLimitKey {
    /// An authenticated user, identified by their user ID.
    UserId(Uuid),
    /// An unauthenticated client, identified by IP address.
    IpAddr(String),
}

// ---------------------------------------------------------------------------
// Token bucket
// ---------------------------------------------------------------------------

/// A single token bucket for rate limiting.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    /// Current number of available tokens.
    pub tokens: f64,
    /// Maximum number of tokens (bucket capacity).
    pub max_tokens: f64,
    /// Tokens added per second.
    pub refill_rate: f64,
    /// When the bucket was last refilled.
    pub last_refill: Instant,
}

impl TokenBucket {
    /// Create a new full token bucket.
    pub fn new(max_tokens: u32, refill_rate: f64) -> Self {
        Self {
            tokens: f64::from(max_tokens),
            max_tokens: f64::from(max_tokens),
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    /// Create a new token bucket with a specific start time (for testing).
    #[cfg(test)]
    fn new_at(max_tokens: u32, refill_rate: f64, now: Instant) -> Self {
        Self {
            tokens: f64::from(max_tokens),
            max_tokens: f64::from(max_tokens),
            refill_rate,
            last_refill: now,
        }
    }

    /// Attempt to consume one token. Returns true if successful, false if exhausted.
    ///
    /// Refills tokens based on elapsed time before checking.
    pub fn try_consume(&mut self, now: Instant) -> bool {
        self.refill(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Calculate seconds until the next token is available.
    pub fn seconds_until_refill(&self) -> u64 {
        if self.tokens >= 1.0 {
            return 0;
        }
        let deficit = 1.0 - self.tokens;
        let seconds = deficit / self.refill_rate;
        seconds.ceil() as u64
    }

    /// Refill tokens based on elapsed time.
    fn refill(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
            self.last_refill = now;
        }
    }
}

// ---------------------------------------------------------------------------
// Rate limiter
// ---------------------------------------------------------------------------

/// In-memory rate limiter using token buckets.
///
/// Thread-safe via `Arc<Mutex<...>>`. Each (actor, route category) pair
/// gets its own independent bucket.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    /// Token buckets keyed by (actor, route category).
    buckets: Arc<Mutex<HashMap<(RateLimitKey, RouteCategory), TokenBucket>>>,
    /// Configuration for bucket sizes.
    config: RateLimitConfig,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            buckets: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Maximum number of entries before triggering eviction of stale buckets.
    const EVICTION_THRESHOLD: usize = 10_000;

    /// Buckets whose `last_refill` is older than this duration are considered
    /// stale and eligible for eviction (they would be fully replenished anyway).
    const EVICTION_AGE: std::time::Duration = std::time::Duration::from_secs(120);

    /// Check if a request is allowed. Returns Ok(()) if allowed, or Err(seconds)
    /// with the retry-after duration if rate limited.
    pub fn check(&self, key: RateLimitKey, category: RouteCategory) -> Result<(), u64> {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());

        // Evict stale entries when the map grows beyond the threshold.
        if buckets.len() > Self::EVICTION_THRESHOLD {
            buckets.retain(|_, bucket| now.duration_since(bucket.last_refill) < Self::EVICTION_AGE);
        }

        let bucket = buckets.entry((key, category)).or_insert_with(|| {
            let rpm = self.config.rpm_for_category(&category);
            let refill_rate = f64::from(rpm) / 60.0;
            TokenBucket::new(rpm, refill_rate)
        });

        if bucket.try_consume(now) {
            Ok(())
        } else {
            Err(bucket.seconds_until_refill())
        }
    }
}

// ---------------------------------------------------------------------------
// Middleware
// ---------------------------------------------------------------------------

/// Axum middleware function that enforces rate limits.
///
/// Use with `axum::middleware::from_fn(rate_limit_middleware)`.
///
/// Prerequisites in request extensions:
/// - `Arc<RateLimiter>`: the shared rate limiter (set by a preceding layer)
/// - `RouteCategoryExtension`: the route category (set by route group layers)
/// - Optionally, an `Actor` from the auth middleware for user-based limiting
///
/// If no `RouteCategoryExtension` is set, the request passes through without
/// rate limiting. If no `Arc<RateLimiter>` is in extensions, the request also
/// passes through.
pub async fn rate_limit_middleware(request: Request, next: Next) -> Response {
    // Extract rate limiter from extensions.
    let limiter = request.extensions().get::<Arc<RateLimiter>>().cloned();

    let limiter = match limiter {
        Some(l) => l,
        None => return next.run(request).await,
    };

    // Extract route category from extensions.
    let category = request
        .extensions()
        .get::<RouteCategoryExtension>()
        .map(|ext| ext.0);

    let category = match category {
        Some(c) => c,
        None => return next.run(request).await,
    };

    // Determine the rate limit key: prefer authenticated user, fall back to IP.
    let key = extract_rate_limit_key(&request);

    // Extract request ID for the error response.
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .map(|r| r.0.clone())
        .unwrap_or_default();

    // Check the rate limit.
    match limiter.check(key, category) {
        Ok(()) => {
            record_rate_limit_decision(category.as_str(), "allowed");
            next.run(request).await
        }
        Err(retry_after_secs) => {
            record_rate_limit_decision(category.as_str(), "rejected");
            build_429_response(retry_after_secs, &request_id)
        }
    }
}

/// Extract the rate limit key from the request.
///
/// Checks for an authenticated actor first (via `haiker_app::identity::Actor`
/// in extensions), then falls back to client IP from `X-Forwarded-For` header
/// or uses "unknown" as a last resort.
fn extract_rate_limit_key(request: &Request) -> RateLimitKey {
    // Check for authenticated actor in extensions.
    if let Some(actor) = request.extensions().get::<haiker_app::identity::Actor>() {
        return RateLimitKey::UserId(actor.user_id.0);
    }

    // Fall back to IP from X-Forwarded-For header.
    let ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    RateLimitKey::IpAddr(ip)
}

/// Build a 429 Too Many Requests response with Retry-After header and
/// RFC 9457 Problem Details body.
fn build_429_response(retry_after_secs: u64, request_id: &str) -> Response {
    let body = RateLimitProblemDetail {
        problem_type: "/problems/rate-limited".to_string(),
        title: "Too Many Requests".to_string(),
        status: 429,
        code: "RATE_LIMITED".to_string(),
        detail: "You have exceeded the rate limit. Please wait before retrying.".to_string(),
        request_id: request_id.to_string(),
    };

    let mut response = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();

    if let Ok(val) = HeaderValue::from_str(&retry_after_secs.to_string()) {
        response.headers_mut().insert("retry-after", val);
    }

    response
}

/// RFC 9457 Problem Details body for rate limit responses.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitProblemDetail {
    /// A URI path that identifies the problem type.
    #[serde(rename = "type")]
    pub problem_type: String,
    /// A short, human-readable summary of the problem type.
    pub title: String,
    /// The HTTP status code for this occurrence.
    pub status: u16,
    /// An application-specific error code in UPPER_SNAKE_CASE.
    pub code: String,
    /// A human-readable explanation specific to this occurrence.
    pub detail: String,
    /// A unique request identifier for correlation.
    pub request_id: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    async fn dummy_handler() -> &'static str {
        "ok"
    }

    fn test_limiter(config: RateLimitConfig) -> Arc<RateLimiter> {
        Arc::new(RateLimiter::new(config))
    }

    fn build_app(limiter: Arc<RateLimiter>, category: RouteCategory) -> Router {
        Router::new()
            .route("/test", get(dummy_handler))
            .layer(axum::middleware::from_fn(rate_limit_middleware))
            .layer(axum::Extension(RouteCategoryExtension(category)))
            .layer(axum::Extension(limiter))
    }

    fn build_request() -> http::Request<Body> {
        http::Request::builder()
            .uri("/test")
            .header("x-forwarded-for", "192.168.1.1")
            .body(Body::empty())
            .unwrap()
    }

    fn build_request_with_ip(ip: &str) -> http::Request<Body> {
        http::Request::builder()
            .uri("/test")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn token_bucket_refill_over_time() {
        let start = Instant::now();
        let mut bucket = TokenBucket::new_at(2, 2.0, start);

        // Drain the bucket.
        assert!(bucket.try_consume(start));
        assert!(bucket.try_consume(start));
        assert!(!bucket.try_consume(start));

        // After 1 second at refill rate 2.0/s, we should have 2 tokens.
        let later = start + std::time::Duration::from_secs(1);
        assert!(bucket.try_consume(later));
        assert!(bucket.try_consume(later));
        assert!(!bucket.try_consume(later));
    }

    #[tokio::test]
    async fn token_bucket_does_not_exceed_max() {
        let start = Instant::now();
        let mut bucket = TokenBucket::new_at(3, 10.0, start);

        // Wait a long time - should still cap at max_tokens.
        let later = start + std::time::Duration::from_secs(100);
        bucket.refill(later);
        assert_eq!(bucket.tokens, 3.0);
    }

    #[tokio::test]
    async fn exhausted_bucket_returns_429() {
        let config = RateLimitConfig {
            auth_rpm: 1,
            ..Default::default()
        };
        let limiter = test_limiter(config);
        let app = build_app(limiter, RouteCategory::Auth);

        // First request should pass.
        let response = app.clone().oneshot(build_request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Second request should be rate limited.
        let response = app.oneshot(build_request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn retry_after_header_present_in_429() {
        let config = RateLimitConfig {
            auth_rpm: 1,
            ..Default::default()
        };
        let limiter = test_limiter(config);
        let app = build_app(limiter, RouteCategory::Auth);

        // Exhaust the bucket.
        let _ = app.clone().oneshot(build_request()).await.unwrap();
        let response = app.oneshot(build_request()).await.unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(response.headers().contains_key("retry-after"));
        let retry_after = response
            .headers()
            .get("retry-after")
            .unwrap()
            .to_str()
            .unwrap();
        let seconds: u64 = retry_after.parse().unwrap();
        assert!(seconds > 0);
    }

    #[tokio::test]
    async fn problem_details_body_shape() {
        let config = RateLimitConfig {
            auth_rpm: 1,
            ..Default::default()
        };
        let limiter = test_limiter(config);

        // Add a request ID extension to the request.
        let app = Router::new()
            .route("/test", get(dummy_handler))
            .layer(axum::middleware::from_fn(rate_limit_middleware))
            .layer(axum::Extension(RouteCategoryExtension(RouteCategory::Auth)))
            .layer(axum::Extension(limiter))
            .layer(axum::Extension(RequestId("test-request-id".to_string())));

        // Exhaust the bucket.
        let request = http::Request::builder()
            .uri("/test")
            .header("x-forwarded-for", "10.0.0.1")
            .body(Body::empty())
            .unwrap();
        let _ = app.clone().oneshot(request).await.unwrap();

        let request = http::Request::builder()
            .uri("/test")
            .header("x-forwarded-for", "10.0.0.1")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(json["type"], "/problems/rate-limited");
        assert_eq!(json["title"], "Too Many Requests");
        assert_eq!(json["status"], 429);
        assert_eq!(json["code"], "RATE_LIMITED");
        assert!(json["detail"].is_string());
        assert_eq!(json["requestId"], "test-request-id");
    }

    #[tokio::test]
    async fn requests_below_limit_pass_through() {
        let config = RateLimitConfig {
            reads_rpm: 5,
            ..Default::default()
        };
        let limiter = test_limiter(config);
        let app = build_app(limiter, RouteCategory::Read);

        for _ in 0..5 {
            let response = app.clone().oneshot(build_request()).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn different_categories_have_independent_limits() {
        let config = RateLimitConfig {
            auth_rpm: 1,
            reads_rpm: 1,
            ..Default::default()
        };
        let limiter = test_limiter(config);

        // Build separate apps for auth and read.
        let auth_app = build_app(limiter.clone(), RouteCategory::Auth);
        let read_app = build_app(limiter, RouteCategory::Read);

        // Exhaust auth bucket.
        let response = auth_app.clone().oneshot(build_request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = auth_app.oneshot(build_request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        // Read bucket should still be available.
        let response = read_app.oneshot(build_request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn actor_based_key_vs_ip_based_fallback() {
        let config = RateLimitConfig {
            auth_rpm: 1,
            ..Default::default()
        };
        let limiter = test_limiter(config);

        // Two different IPs should have independent buckets.
        let app = build_app(limiter, RouteCategory::Auth);

        let response = app
            .clone()
            .oneshot(build_request_with_ip("10.0.0.1"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Same IP should be limited.
        let response = app
            .clone()
            .oneshot(build_request_with_ip("10.0.0.1"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        // Different IP should pass.
        let response = app
            .oneshot(build_request_with_ip("10.0.0.2"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn authenticated_user_key_is_independent_of_ip() {
        let config = RateLimitConfig {
            auth_rpm: 1,
            ..Default::default()
        };
        let limiter = test_limiter(config);

        let user_id = Uuid::new_v4();
        let actor = haiker_app::identity::Actor {
            user_id: haiker_app::identity::UserId(user_id),
        };

        // Build app with actor in extensions.
        let app = Router::new()
            .route("/test", get(dummy_handler))
            .layer(axum::middleware::from_fn(rate_limit_middleware))
            .layer(axum::Extension(RouteCategoryExtension(RouteCategory::Auth)))
            .layer(axum::Extension(limiter.clone()))
            .layer(axum::Extension(actor));

        // First request by authenticated user passes.
        let response = app.clone().oneshot(build_request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Second request by same user is limited.
        let response = app.oneshot(build_request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        // An unauthenticated request from the same IP should still pass
        // because it uses IP key, not user key.
        let unauth_app = build_app(limiter, RouteCategory::Auth);
        let response = unauth_app.oneshot(build_request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn no_category_extension_passes_through() {
        let config = RateLimitConfig::default();
        let limiter = test_limiter(config);

        // App without RouteCategoryExtension.
        let app = Router::new()
            .route("/test", get(dummy_handler))
            .layer(axum::middleware::from_fn(rate_limit_middleware))
            .layer(axum::Extension(limiter));

        let response = app.oneshot(build_request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn no_limiter_extension_passes_through() {
        // App without RateLimiter extension.
        let app = Router::new()
            .route("/test", get(dummy_handler))
            .layer(axum::middleware::from_fn(rate_limit_middleware));

        let response = app.oneshot(build_request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn seconds_until_refill_when_empty() {
        let start = Instant::now();
        let mut bucket = TokenBucket::new_at(1, 1.0, start);
        // Consume the only token.
        assert!(bucket.try_consume(start));
        // Should need 1 second for next token.
        let secs = bucket.seconds_until_refill();
        assert_eq!(secs, 1);
    }

    #[test]
    fn config_from_env_uses_defaults() {
        let config = RateLimitConfig::default();
        assert_eq!(config.auth_rpm, 5);
        assert_eq!(config.imports_rpm, 10);
        assert_eq!(config.mutations_rpm, 30);
        assert_eq!(config.reads_rpm, 120);
        assert_eq!(config.exports_rpm, 10);
    }

    #[test]
    fn eviction_removes_stale_entries_when_threshold_exceeded() {
        let config = RateLimitConfig {
            reads_rpm: 10,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);

        // Manually insert entries that exceed the threshold with old timestamps.
        {
            let mut buckets = limiter.buckets.lock().unwrap();
            let old_time = Instant::now() - std::time::Duration::from_secs(300);
            for i in 0..10_001 {
                let key = RateLimitKey::IpAddr(format!("10.0.0.{}", i));
                let mut bucket = TokenBucket::new(10, 10.0 / 60.0);
                bucket.last_refill = old_time;
                buckets.insert((key, RouteCategory::Read), bucket);
            }
        }

        // Next check should trigger eviction of stale entries.
        let fresh_key = RateLimitKey::IpAddr("fresh.ip".to_string());
        assert!(limiter.check(fresh_key, RouteCategory::Read).is_ok());

        // All old entries should have been evicted (they are > 120s old).
        let buckets = limiter.buckets.lock().unwrap();
        // Only the fresh entry should remain.
        assert_eq!(buckets.len(), 1);
    }

    #[test]
    fn rate_limit_key_equality() {
        let key1 = RateLimitKey::UserId(Uuid::nil());
        let key2 = RateLimitKey::UserId(Uuid::nil());
        assert_eq!(key1, key2);

        let key3 = RateLimitKey::IpAddr("192.168.1.1".to_string());
        let key4 = RateLimitKey::IpAddr("192.168.1.1".to_string());
        assert_eq!(key3, key4);

        assert_ne!(
            RateLimitKey::IpAddr("1.2.3.4".to_string()),
            RateLimitKey::IpAddr("5.6.7.8".to_string())
        );
    }

    #[test]
    fn route_category_as_str() {
        assert_eq!(RouteCategory::Auth.as_str(), "auth");
        assert_eq!(RouteCategory::Import.as_str(), "import");
        assert_eq!(RouteCategory::Mutation.as_str(), "mutation");
        assert_eq!(RouteCategory::Read.as_str(), "read");
        assert_eq!(RouteCategory::Export.as_str(), "export");
    }

    #[tokio::test]
    async fn load_test_burst_requests_are_bounded() {
        // Demonstrates bounded API resource use under burst:
        // a small bucket (5 per minute) correctly rejects excess traffic.
        let config = RateLimitConfig {
            auth_rpm: 5,
            ..Default::default()
        };
        let limiter = test_limiter(config);
        let app = build_app(limiter, RouteCategory::Auth);

        let mut passed = 0u32;
        let mut rejected = 0u32;

        for _ in 0..50 {
            let response = app.clone().oneshot(build_request()).await.unwrap();
            match response.status() {
                StatusCode::OK => passed += 1,
                StatusCode::TOO_MANY_REQUESTS => rejected += 1,
                other => panic!("Unexpected status: {}", other),
            }
        }

        // Exactly 5 requests should pass (the bucket capacity)
        assert_eq!(
            passed, 5,
            "Expected exactly 5 requests to pass, got {}",
            passed
        );
        // The remaining 45 should be rejected
        assert_eq!(
            rejected, 45,
            "Expected exactly 45 rejections, got {}",
            rejected
        );
    }
}
