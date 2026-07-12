use axum::routing::{get, post};
use axum::Router;
use haiker_platform::request_id::request_id_middleware;
use haiker_platform::telemetry::{self, TelemetryConfig};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

mod auth;
mod auth_handlers;
mod error;
mod health;

/// OpenAPI documentation specification.
#[derive(OpenApi)]
#[openapi(
    paths(health::health, health::ready, auth::me,),
    info(
        title = "Haiker API",
        version = "0.1.0",
        description = "Hiking route management API"
    )
)]
struct ApiDoc;

#[tokio::main]
async fn main() {
    let config = TelemetryConfig::from_env();
    telemetry::init_telemetry(&config);

    tracing::info!("Starting Haiker API server");

    let app = Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .route("/me", get(auth::me))
        .route("/auth/login", post(auth_handlers::post_login))
        .route("/auth/callback", get(auth_handlers::get_callback))
        .route("/auth/logout", post(auth_handlers::post_logout))
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(axum::middleware::from_fn(request_id_middleware))
        .layer(TraceLayer::new_for_http());

    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::info!("Listening on {}", listener.local_addr().unwrap());

    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_check_returns_healthy() {
        let app = Router::new()
            .route("/health", get(health::health))
            .route("/ready", get(health::ready));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn openapi_spec_is_accessible() {
        let app = Router::new()
            .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api-docs/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn me_without_auth_header_returns_401() {
        let app = Router::new().route("/me", get(auth::me));

        let response = app
            .oneshot(Request::builder().uri("/me").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn me_with_valid_bearer_uuid_returns_200_with_user_id() {
        let app = Router::new().route("/me", get(auth::me));
        let user_id = uuid::Uuid::new_v4();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .header("Authorization", format!("Bearer {user_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
