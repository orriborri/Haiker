//! Haiker HTTP API server.
//!
//! Starts an Axum-based HTTP server exposing the REST API for the Haiker
//! hiking-route management application.

use axum::{routing::get, Router};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::info;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

mod auth;
mod error;
mod health;

/// OpenAPI documentation specification.
#[derive(OpenApi)]
#[openapi(
    paths(
        health::health,
        health::ready,
        auth::me,
    ),
    info(
        title = "Haiker API",
        version = "0.1.0",
        description = "Hiking route management API"
    )
)]
struct ApiDoc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    haiker_platform::telemetry::init();

    let config = haiker_platform::config::AppConfig::from_env();

    let app = Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .route("/me", get(auth::me))
        .merge(
            SwaggerUi::new("/docs")
                .url("/api-docs/openapi.json", ApiDoc::openapi()),
        )
        .layer(TraceLayer::new_for_http());

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = TcpListener::bind(&addr).await?;
    info!("API server listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}
