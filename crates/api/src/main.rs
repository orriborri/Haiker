use axum::routing::{get, patch, post};
use axum::Router;
use haiker_app::activity_catalog::commands::AuditSink;
use haiker_platform::activity_persistence::PgActivityRepository;
use haiker_platform::audit::AuditLog;
use haiker_platform::config::AppConfig;
use haiker_platform::database;
use haiker_platform::import_persistence::PgImportRepository;
use haiker_platform::object_storage::ObjectStorageClient;
use haiker_platform::oidc::Auth0OidcProvider;
use haiker_platform::oidc_state_store::OidcStateStore;
use haiker_platform::publication_commit::PgPublicationCommitter;
use haiker_platform::rate_limit::{
    rate_limit_middleware, RateLimitConfig, RateLimiter, RouteCategory, RouteCategoryExtension,
};
use haiker_platform::recorded_route_persistence::PgRecordedRouteRepository;
use haiker_platform::request_id::request_id_middleware;
use haiker_platform::route_editing_gateways::{PgActivityGateway, PgRouteVersionGateway};
use haiker_platform::route_editing_persistence::PgRouteDraftRepository;
use haiker_platform::telemetry::{self, TelemetryConfig};
use haiker_platform::upload_quota::{UploadQuota, UploadQuotaConfig};
use haiker_platform::user_persistence::PgUserRepository;
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
mod exports;
mod exports_dto;
mod health;
mod imports;
mod imports_dto;
mod recorded_route;
mod recorded_route_dto;
mod route_editing;
mod route_editing_dto;

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

    // Initialize rate limiter
    let rate_limit_config = RateLimitConfig::from_env();
    let rate_limiter = Arc::new(RateLimiter::new(rate_limit_config));

    // Initialize upload quota
    let upload_quota_config = UploadQuotaConfig::from_env();
    let upload_quota = Arc::new(UploadQuota::new(&upload_quota_config));

    // Import subsystem state with real PostgreSQL repository
    let import_state = imports::ImportAppState {
        repo: Arc::new(PgImportRepository::new(pool.clone())),
        url_generator: Arc::new(PresignedUrlGenerator {
            client: object_storage.clone(),
        }),
        upload_verifier: Arc::new(object_storage),
        job_queue: None,
        upload_quota: Some(upload_quota),
    };

    let import_routes = Router::new()
        .route("/v1/imports", post(imports::post_start_import))
        .route(
            "/v1/imports/{import_id}/completion",
            post(imports::post_complete_upload),
        )
        .route("/v1/imports/{import_id}", get(imports::get_import_status))
        .layer(axum::Extension(RouteCategoryExtension(
            RouteCategory::Import,
        )))
        .with_state(import_state);

    // Activity subsystem state with real PostgreSQL repository
    let audit_log = AuditLog::new(pool.clone());
    let activity_state = activities::ActivityAppState {
        repo: Arc::new(PgActivityRepository::new(pool.clone())),
        audit: Arc::new(AuditSinkAdapter {
            audit_log: audit_log.clone(),
        }),
    };

    let activity_routes = Router::new()
        .route("/v1/activities", get(activities::get_activities))
        .route(
            "/v1/activities/{activityId}",
            get(activities::get_activity_detail).delete(activities::delete_activity_handler),
        )
        .route(
            "/v1/activities/{activityId}/title",
            patch(activities::patch_activity_title),
        )
        .layer(axum::Extension(RouteCategoryExtension(RouteCategory::Read)))
        .with_state(activity_state);

    // Recorded route subsystem state
    let recorded_route_state = recorded_route::RecordedRouteAppState {
        activity_repo: Arc::new(PgActivityRepository::new(pool.clone())),
        route_repo: Arc::new(PgRecordedRouteRepository::new(pool.clone())),
    };

    let recorded_route_routes = Router::new()
        .route(
            "/v1/activities/{activityId}/recorded-route",
            get(recorded_route::get_recorded_route_handler),
        )
        .layer(axum::Extension(RouteCategoryExtension(RouteCategory::Read)))
        .with_state(recorded_route_state);

    // Route editing subsystem state with PostgreSQL persistence
    let route_editing_state = route_editing::RouteEditingAppState {
        repo: Arc::new(PgRouteDraftRepository::new(pool.clone())),
        activity_gateway: Arc::new(PgActivityGateway::new()),
        route_version_gateway: Arc::new(PgRouteVersionGateway::new()),
        publication_committer: Some(Arc::new(PgPublicationCommitter::new(pool.clone()))),
    };

    let route_editing_routes = Router::new()
        .route(
            "/v1/activities/{activityId}/route-drafts",
            post(route_editing::post_create_draft),
        )
        .route(
            "/v1/route-drafts/{draftId}",
            get(route_editing::get_draft).delete(route_editing::delete_draft),
        )
        .route(
            "/v1/route-drafts/{draftId}/operations",
            post(route_editing::post_apply_operation),
        )
        .route(
            "/v1/route-drafts/{draftId}/undo",
            post(route_editing::post_undo),
        )
        .route(
            "/v1/route-drafts/{draftId}/redo",
            post(route_editing::post_redo),
        )
        .route(
            "/v1/route-drafts/{draftId}/reset",
            post(route_editing::post_reset),
        )
        .route(
            "/v1/route-drafts/{draftId}/validation",
            post(route_editing::post_validate_draft),
        )
        .route(
            "/v1/route-drafts/{draftId}/publication",
            post(route_editing::post_publish_draft),
        )
        .layer(axum::Extension(RouteCategoryExtension(
            RouteCategory::Mutation,
        )))
        .with_state(route_editing_state);

    // Export subsystem state (placeholder - no real persistence impl yet)
    // TODO: Wire up real persistence and gateway implementations when available
    let export_state = exports::ExportAppState {
        repo: Arc::new(InMemoryExportRepoPlaceholder),
        route_version_gateway: Arc::new(StubRouteVersionGatewayPlaceholder),
        job_queue: None,
        download_url_generator: None,
        audit_sink: None,
    };

    let export_routes = Router::new()
        .route(
            "/v1/activities/{activityId}/exports",
            post(exports::post_request_export),
        )
        .route("/v1/exports/{exportId}", get(exports::get_export_status))
        .route(
            "/v1/exports/{exportId}/download",
            get(exports::get_export_download),
        )
        .layer(axum::Extension(RouteCategoryExtension(
            RouteCategory::Export,
        )))
        .with_state(export_state);

    let auth_routes = Router::new()
        .route("/me", get(auth::me))
        .layer(axum::Extension(RouteCategoryExtension(RouteCategory::Auth)));

    // Construct OIDC provider if configured
    let oidc_provider: Option<Arc<dyn haiker_app::identity::OidcProvider>> = if let Some(
        ref oidc_config,
    ) = app_config.oidc
    {
        let domain_config = haiker_app::identity::OidcConfig {
            issuer_url: oidc_config.issuer_url.clone(),
            client_id: oidc_config.client_id.clone(),
            client_secret: oidc_config.client_secret.clone(),
            redirect_uri: oidc_config.redirect_uri.clone(),
        };
        match Auth0OidcProvider::from_config(&domain_config).await {
            Ok(provider) => {
                tracing::info!("OIDC provider configured successfully");
                Some(Arc::new(provider))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to initialize OIDC provider; auth routes will return 503");
                None
            }
        }
    } else {
        tracing::info!("No OIDC configuration found; auth login/callback/logout will return 503");
        None
    };

    let auth_app_state = auth_handlers::AuthAppState {
        oidc_provider,
        state_store: Arc::new(OidcStateStore::new()),
        user_repo: Arc::new(PgUserRepository::new(pool.clone())),
        session_store: haiker_platform::session::SessionStore::new(pool.clone()),
        cookie_secure: app_config
            .oidc
            .as_ref()
            .map(|c| c.redirect_uri.starts_with("https://"))
            .unwrap_or(true),
    };

    // Warn loudly if both OIDC and dev auth are enabled simultaneously
    if app_config.oidc.is_some() {
        let dev_auth_enabled = std::env::var("DEV_AUTH_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        if dev_auth_enabled {
            tracing::error!(
                "SECURITY RISK: Both OIDC provider and DEV_AUTH_ENABLED=true are configured. \
                 The dev auth bypass allows any caller to fabricate identity via a Bearer UUID. \
                 This MUST NOT be used in production. Disable DEV_AUTH_ENABLED for production deployments."
            );
        }
    }

    let auth_flow_routes = Router::new()
        .route("/auth/login", post(auth_handlers::post_login))
        .route("/auth/callback", get(auth_handlers::get_callback))
        .route("/auth/logout", post(auth_handlers::post_logout))
        .layer(axum::Extension(RouteCategoryExtension(RouteCategory::Auth)))
        .with_state(auth_app_state);

    let app = Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .merge(auth_routes)
        .merge(auth_flow_routes)
        .merge(import_routes)
        .merge(activity_routes)
        .merge(recorded_route_routes)
        .merge(route_editing_routes)
        .merge(export_routes)
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(axum::middleware::from_fn(rate_limit_middleware))
        .layer(axum::Extension(rate_limiter))
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

/// Adapter bridging the platform AuditLog to the domain AuditSink trait.
struct AuditSinkAdapter {
    audit_log: AuditLog,
}

#[async_trait::async_trait]
impl AuditSink for AuditSinkAdapter {
    async fn record(
        &self,
        actor_id: uuid::Uuid,
        action: &str,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<(), haiker_app::activity_catalog::ActivityCatalogError> {
        self.audit_log
            .append(actor_id, action, resource_type, resource_id, None)
            .await
            .map_err(
                |e| haiker_app::activity_catalog::ActivityCatalogError::PersistenceError {
                    message: format!("audit log error: {e}"),
                },
            )?;
        Ok(())
    }
}

/// Placeholder in-memory export repository for the main binary.
/// TODO: Replace with PgExportRepository once persistence layer is implemented.
struct InMemoryExportRepoPlaceholder;

#[async_trait::async_trait]
impl haiker_app::exports::ExportRepository for InMemoryExportRepoPlaceholder {
    async fn save(
        &self,
        _export_job: &haiker_app::exports::ExportJob,
    ) -> Result<(), haiker_app::exports::ExportError> {
        Ok(())
    }

    async fn find_by_id(
        &self,
        _id: haiker_app::exports::ExportJobId,
    ) -> Result<Option<haiker_app::exports::ExportJob>, haiker_app::exports::ExportError> {
        Ok(None)
    }

    async fn find_by_idempotency_key(
        &self,
        _owner_id: haiker_app::identity::UserId,
        _key: &str,
    ) -> Result<Option<haiker_app::exports::ExportJob>, haiker_app::exports::ExportError> {
        Ok(None)
    }

    async fn update(
        &self,
        _export_job: &haiker_app::exports::ExportJob,
    ) -> Result<(), haiker_app::exports::ExportError> {
        Ok(())
    }
}

/// Placeholder route version gateway for the main binary.
/// TODO: Replace with real cross-context gateway once available.
struct StubRouteVersionGatewayPlaceholder;

#[async_trait::async_trait]
impl haiker_app::exports::RouteVersionGateway for StubRouteVersionGatewayPlaceholder {
    async fn verify_owned_route_version(
        &self,
        _activity_id: haiker_app::activity_catalog::ActivityId,
        _route_version_id: haiker_app::route_versioning::RouteVersionId,
        _owner_id: haiker_app::identity::UserId,
    ) -> Result<(), haiker_app::exports::ExportError> {
        Ok(())
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
