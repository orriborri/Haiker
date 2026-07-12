//! Health and readiness endpoints.
//!
//! Provides liveness (`/health`) and readiness (`/ready`) checks for
//! orchestration and load balancer integration.

use axum::Json;
use serde_json::{json, Value};

/// Liveness health check.
///
/// Returns a 200 response indicating the server process is running.
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is healthy")
    )
)]
pub async fn health() -> Json<Value> {
    Json(json!({ "status": "healthy" }))
}

/// Readiness check.
///
/// Returns a 200 response indicating the server is ready to accept traffic.
/// In the future, this will verify database and storage connectivity.
#[utoipa::path(
    get,
    path = "/ready",
    responses(
        (status = 200, description = "Service is ready to accept traffic")
    )
)]
pub async fn ready() -> Json<Value> {
    Json(json!({ "status": "ready" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_returns_healthy() {
        let response = health().await;
        assert_eq!(response.0["status"], "healthy");
    }

    #[tokio::test]
    async fn ready_returns_ready() {
        let response = ready().await;
        assert_eq!(response.0["status"], "ready");
    }
}
