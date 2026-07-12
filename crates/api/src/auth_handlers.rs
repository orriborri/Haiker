//! Authentication endpoint handlers.
//!
//! Provides handlers for the OIDC login flow:
//! - POST /auth/login: Redirect to OIDC authorization URL
//! - GET /auth/callback: Exchange code, create session, set cookie
//! - POST /auth/logout: Revoke session, clear cookie

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// POST /auth/login
///
/// Initiates the OIDC login flow by redirecting the user to the identity provider's
/// authorization endpoint.
///
/// # Stub Implementation
/// This handler currently returns a 501 Not Implemented response.
/// Once an OidcProvider is wired into application state, it will:
/// 1. Call `authorization_url()` on the provider
/// 2. Store the state/nonce for verification
/// 3. Return a redirect to the authorization URL
pub async fn post_login() -> Response {
    // TODO: Wire up OidcProvider from app state to generate authorization URL.
    let body = json!({
        "error": {
            "code": "NOT_IMPLEMENTED",
            "message": "OIDC provider not yet configured"
        }
    });
    (StatusCode::NOT_IMPLEMENTED, Json(body)).into_response()
}

/// GET /auth/callback
///
/// Handles the OIDC callback after the user authenticates with the identity provider.
///
/// # Stub Implementation
/// This handler currently returns a 501 Not Implemented response.
/// Once fully wired, it will:
/// 1. Validate the state parameter
/// 2. Exchange the authorization code for tokens
/// 3. Extract user claims from the ID token
/// 4. Find or create the user in the database
/// 5. Create a session and set the session cookie
/// 6. Redirect to the application frontend
pub async fn get_callback() -> Response {
    // TODO: Wire up OidcProvider and SessionStore to complete the callback flow.
    let body = json!({
        "error": {
            "code": "NOT_IMPLEMENTED",
            "message": "OIDC callback not yet configured"
        }
    });
    (StatusCode::NOT_IMPLEMENTED, Json(body)).into_response()
}

/// POST /auth/logout
///
/// Revokes the current session and clears the session cookie.
///
/// # Stub Implementation
/// This handler currently returns 204 No Content.
/// Once fully wired, it will:
/// 1. Read the session cookie
/// 2. Validate CSRF token
/// 3. Revoke the session in the database
/// 4. Clear the session cookie
/// 5. Return 204 No Content
pub async fn post_logout() -> Response {
    // TODO: Wire up SessionStore to revoke the session and clear cookie.
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn post_login_returns_not_implemented() {
        let response = post_login().await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn get_callback_returns_not_implemented() {
        let response = get_callback().await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn post_logout_returns_no_content() {
        let response = post_logout().await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}
