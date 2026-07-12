use axum::routing::{get, post};
use axum::Router;
use haiker_platform::activity_persistence::PgActivityRepository;
use haiker_platform::config::AppConfig;
use haiker_platform::database;
use haiker_platform::import_persistence::PgImportRepository;
use haiker_platform::object_storage::ObjectStorageClient;
use haiker_platform::request_id::request_id_middleware;
use haiker_platform::telemetry::{self, TelemetryConfig};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

mod activities;
mod activities_dto;
mod auth;
mod auth_handlers;
mod error;
mod health;
mod imports;
mod imports_dto;

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

    let app_config = AppConfig::from_env();

    // Connect to PostgreSQL
    let pool = database::connect(&app_config.database)
        .await
        .expect("failed to connect to database");

    // Initialize object storage client for presigned URL generation
    let object_storage = ObjectStorageClient::new(&app_config.storage)
        .await
        .expect("failed to initialize object storage");

    // Import subsystem state with real PostgreSQL repository
    let import_state = imports::ImportAppState {
        repo: Arc::new(PgImportRepository::new(pool.clone())),
        url_generator: Arc::new(PresignedUrlGenerator {
            client: object_storage,
        }),
        job_queue: None,
    };

    let import_routes = Router::new()
        .route("/v1/imports", post(imports::post_start_import))
        .route(
            "/v1/imports/{import_id}/completion",
            post(imports::post_complete_upload),
        )
        .route("/v1/imports/{import_id}", get(imports::get_import_status))
        .with_state(import_state);

    // Activity subsystem state with real PostgreSQL repository
    let activity_state = activities::ActivityAppState {
        repo: Arc::new(PgActivityRepository::new(pool.clone())),
    };

    let activity_routes = Router::new()
        .route("/v1/activities", get(activities::get_activities))
        .with_state(activity_state);

    let app = Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .route("/me", get(auth::me))
        .route("/auth/login", post(auth_handlers::post_login))
        .route("/auth/callback", get(auth_handlers::get_callback))
        .route("/auth/logout", post(auth_handlers::post_logout))
        .merge(import_routes)
        .merge(activity_routes)
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(axum::middleware::from_fn(request_id_middleware))
        .layer(TraceLayer::new_for_http());

    let bind_addr = format!("{}:{}", app_config.server.host, app_config.server.port);
    let listener = TcpListener::bind(&bind_addr).await.unwrap();
    tracing::info!("Listening on {}", listener.local_addr().unwrap());

    axum::serve(listener, app).await.unwrap();
}

/// Presigned URL generator backed by real object storage.
struct PresignedUrlGenerator {
    client: ObjectStorageClient,
}

#[async_trait::async_trait]
impl haiker_app::imports::commands::UploadUrlGenerator for PresignedUrlGenerator {
    async fn generate_upload_url(
        &self,
        key: &str,
    ) -> Result<String, haiker_app::imports::ImportError> {
        self.client
            .presigned_upload_url(key, Duration::from_secs(3600))
            .await
            .map_err(|e| haiker_app::imports::ImportError::StorageError {
                message: format!("failed to generate presigned URL: {e}"),
            })
    }
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
