//! Request ID and Correlation ID middleware for Axum.
//!
//! This middleware:
//! - Generates a unique `request_id` (UUID v4) for each incoming request.
//! - Reads the `X-Correlation-Id` header from the request, or generates one if absent.
//! - Adds both IDs to the current tracing span as structured fields.
//! - Adds `X-Request-Id` and `X-Correlation-Id` headers to the response.

use axum::{extract::Request, http::HeaderValue, middleware::Next, response::Response};

/// Header name for the request ID (set by the server).
pub const X_REQUEST_ID: &str = "x-request-id";
/// Header name for the correlation ID (may be set by the caller).
pub const X_CORRELATION_ID: &str = "x-correlation-id";

/// Axum middleware function that injects request and correlation IDs.
///
/// Use with `axum::middleware::from_fn(request_id_middleware)`.
pub async fn request_id_middleware(request: Request, next: Next) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();

    let correlation_id = request
        .headers()
        .get(X_CORRELATION_ID)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Record IDs in the current tracing span.
    let current_span = tracing::Span::current();
    current_span.record("request_id", tracing::field::display(&request_id));
    current_span.record("correlation_id", tracing::field::display(&correlation_id));

    // If the span doesn't have the fields pre-declared, emit an event with the IDs.
    tracing::info!(
        request_id = %request_id,
        correlation_id = %correlation_id,
        "request identified"
    );

    let mut response = next.run(request).await;

    // Inject headers into the response.
    if let Ok(val) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert(X_REQUEST_ID, val);
    }
    if let Ok(val) = HeaderValue::from_str(&correlation_id) {
        response.headers_mut().insert(X_CORRELATION_ID, val);
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, routing::get, Router};
    use http::StatusCode;
    use tower::ServiceExt;

    async fn dummy_handler() -> &'static str {
        "ok"
    }

    #[tokio::test]
    async fn adds_request_id_header_to_response() {
        let app = Router::new()
            .route("/test", get(dummy_handler))
            .layer(axum::middleware::from_fn(request_id_middleware));

        let request = http::Request::builder()
            .uri("/test")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key(X_REQUEST_ID));
        assert!(response.headers().contains_key(X_CORRELATION_ID));
    }

    #[tokio::test]
    async fn preserves_incoming_correlation_id() {
        let app = Router::new()
            .route("/test", get(dummy_handler))
            .layer(axum::middleware::from_fn(request_id_middleware));

        let correlation = "my-correlation-123";
        let request = http::Request::builder()
            .uri("/test")
            .header(X_CORRELATION_ID, correlation)
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(X_CORRELATION_ID)
                .unwrap()
                .to_str()
                .unwrap(),
            correlation
        );
    }
}
