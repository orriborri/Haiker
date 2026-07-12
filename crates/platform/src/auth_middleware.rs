//! Authentication middleware and extractors.
//!
//! Provides an Axum extractor (`AuthSession`) that validates session cookies
//! and CSRF tokens, producing an authenticated `Actor` for downstream handlers.

use std::sync::OnceLock;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use uuid::Uuid;

use haiker_app::identity::{Actor, UserId};

use crate::session::SessionStore;

/// Cached value of the `DEV_AUTH_ENABLED` environment variable, read once at
/// first access so we avoid re-reading the env on every request.
static DEV_AUTH_ENABLED: OnceLock<bool> = OnceLock::new();

/// The name of the session cookie.
pub const SESSION_COOKIE_NAME: &str = "haiker_sid";

/// The header name for CSRF token validation.
pub const CSRF_HEADER_NAME: &str = "x-csrf-token";

/// Axum extractor that authenticates a request via session cookie.
///
/// For state-changing methods (POST, PUT, PATCH, DELETE), it also validates
/// the CSRF token from the `X-CSRF-Token` header against the session's stored value.
///
/// In development mode (when `DEV_AUTH_ENABLED=true` env var is set), it falls back
/// to accepting a Bearer token as a UUID user ID (for testing without OIDC).
pub struct AuthSession(pub Actor);

/// Application state trait that provides access to the session store.
pub trait HasSessionStore {
    fn session_store(&self) -> &SessionStore;
}

/// Errors produced by the `AuthSession` extractor.
#[derive(Debug)]
pub enum AuthSessionError {
    /// No session cookie or auth header was provided.
    MissingCredentials,
    /// The session token is invalid or expired.
    InvalidSession,
    /// The CSRF token is missing or does not match.
    CsrfMismatch,
}

impl IntoResponse for AuthSessionError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            AuthSessionError::MissingCredentials => (
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "missing session credentials",
            ),
            AuthSessionError::InvalidSession => (
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "invalid or expired session",
            ),
            AuthSessionError::CsrfMismatch => (
                StatusCode::FORBIDDEN,
                "CSRF_MISMATCH",
                "CSRF token missing or invalid",
            ),
        };

        let body = json!({
            "error": {
                "code": code,
                "message": message
            }
        });

        (status, Json(body)).into_response()
    }
}

impl<S> FromRequestParts<S> for AuthSession
where
    S: HasSessionStore + Send + Sync,
{
    type Rejection = AuthSessionError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Dev mode fallback: accept Bearer token as UUID user ID.
        if is_dev_auth_enabled() {
            if let Some(actor) = try_dev_bearer_auth(parts) {
                return Ok(AuthSession(actor));
            }
        }

        // Extract session cookie from Cookie header.
        let raw_token = extract_cookie_value(parts, SESSION_COOKIE_NAME)
            .ok_or(AuthSessionError::MissingCredentials)?;

        // Validate session against the store.
        let session_store = state.session_store();
        let session_info = session_store
            .validate_session(&raw_token)
            .await
            .map_err(|_| AuthSessionError::InvalidSession)?
            .ok_or(AuthSessionError::InvalidSession)?;

        // For state-changing methods, validate CSRF token.
        if is_state_changing_method(&parts.method) {
            let csrf_header = parts
                .headers
                .get(CSRF_HEADER_NAME)
                .and_then(|v| v.to_str().ok())
                .ok_or(AuthSessionError::CsrfMismatch)?;

            if csrf_header != session_info.csrf_token {
                return Err(AuthSessionError::CsrfMismatch);
            }
        }

        Ok(AuthSession(Actor {
            user_id: UserId(session_info.user_id),
        }))
    }
}

/// Check whether the request method is state-changing (POST, PUT, PATCH, DELETE).
fn is_state_changing_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

/// Check if dev auth mode is enabled via environment variable.
///
/// The value is cached at first access using `OnceLock`, so the env var is
/// read only once rather than on every incoming request.
fn is_dev_auth_enabled() -> bool {
    *DEV_AUTH_ENABLED.get_or_init(|| {
        std::env::var("DEV_AUTH_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
    })
}

/// Try to extract a Bearer token from the Authorization header and parse it as a UUID.
fn try_dev_bearer_auth(parts: &Parts) -> Option<Actor> {
    let auth_header = parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())?;

    let token = auth_header.strip_prefix("Bearer ")?;
    let user_uuid = Uuid::parse_str(token).ok()?;

    Some(Actor {
        user_id: UserId(user_uuid),
    })
}

/// Extract a cookie value by name from the Cookie header.
fn extract_cookie_value(parts: &Parts, name: &str) -> Option<String> {
    let cookie_header = parts.headers.get("cookie").and_then(|v| v.to_str().ok())?;

    for cookie_str in cookie_header.split(';') {
        let cookie_str = cookie_str.trim();
        if let Some(value) = cookie_str.strip_prefix(name) {
            if let Some(value) = value.strip_prefix('=') {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_changing_methods_detected() {
        assert!(is_state_changing_method(&Method::POST));
        assert!(is_state_changing_method(&Method::PUT));
        assert!(is_state_changing_method(&Method::PATCH));
        assert!(is_state_changing_method(&Method::DELETE));
        assert!(!is_state_changing_method(&Method::GET));
        assert!(!is_state_changing_method(&Method::HEAD));
        assert!(!is_state_changing_method(&Method::OPTIONS));
    }

    #[test]
    fn extract_cookie_value_finds_named_cookie() {
        use axum::http::Request;

        let request = Request::builder()
            .header("cookie", "haiker_sid=abc123; other=xyz")
            .body(())
            .unwrap();
        let (parts, _) = request.into_parts();

        let value = extract_cookie_value(&parts, "haiker_sid");
        assert_eq!(value, Some("abc123".to_string()));
    }

    #[test]
    fn extract_cookie_value_returns_none_when_missing() {
        use axum::http::Request;

        let request = Request::builder()
            .header("cookie", "other=xyz")
            .body(())
            .unwrap();
        let (parts, _) = request.into_parts();

        let value = extract_cookie_value(&parts, "haiker_sid");
        assert_eq!(value, None);
    }

    #[test]
    fn extract_cookie_value_returns_none_without_cookie_header() {
        use axum::http::Request;

        let request = Request::builder().body(()).unwrap();
        let (parts, _) = request.into_parts();

        let value = extract_cookie_value(&parts, "haiker_sid");
        assert_eq!(value, None);
    }

    #[test]
    fn auth_session_error_into_response() {
        let err = AuthSessionError::MissingCredentials;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let err = AuthSessionError::InvalidSession;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let err = AuthSessionError::CsrfMismatch;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
