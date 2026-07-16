//! Application configuration.
//!
//! Loads configuration from environment variables with sensible defaults
//! for local development.

use serde::Deserialize;

/// Top-level application configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    /// Server configuration.
    pub server: ServerConfig,
    /// Database configuration.
    pub database: DatabaseConfig,
    /// Object storage configuration.
    pub storage: StorageConfig,
    /// OIDC configuration (optional; auth routes return 503 if absent).
    pub oidc: Option<OidcConfig>,
}

/// HTTP server configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Host to bind the server to.
    pub host: String,
    /// Port to bind the server to.
    pub port: u16,
}

/// Database connection configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// PostgreSQL connection URL.
    pub url: String,
    /// Maximum number of connections in the pool.
    pub max_connections: u32,
}

/// S3-compatible object storage configuration.
#[derive(Clone, Deserialize)]
pub struct StorageConfig {
    /// Endpoint URL for the storage service.
    pub endpoint: String,
    /// Bucket name.
    pub bucket: String,
    /// Access key ID.
    pub access_key_id: String,
    /// Secret access key.
    pub secret_access_key: String,
}

impl std::fmt::Debug for StorageConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageConfig")
            .field("endpoint", &self.endpoint)
            .field("bucket", &self.bucket)
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"[REDACTED]")
            .finish()
    }
}

/// OIDC identity provider configuration.
#[derive(Clone, Deserialize)]
pub struct OidcConfig {
    /// The OIDC issuer URL (e.g. https://your-tenant.auth0.com/).
    pub issuer_url: String,
    /// OAuth2 client ID.
    pub client_id: String,
    /// OAuth2 client secret.
    pub client_secret: String,
    /// The callback URL registered with the provider.
    pub redirect_uri: String,
}

impl std::fmt::Debug for OidcConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OidcConfig")
            .field("issuer_url", &self.issuer_url)
            .field("client_id", &self.client_id)
            .field("client_secret", &"[REDACTED]")
            .field("redirect_uri", &self.redirect_uri)
            .finish()
    }
}

impl AppConfig {
    /// Load configuration from environment variables.
    ///
    /// Reads a `.env` file if present, then loads values from the environment.
    /// Uses default values suitable for local development when variables are unset.
    pub fn from_env() -> Self {
        // Best-effort .env loading; ignore if file is missing
        let _ = dotenvy::dotenv();

        let oidc = match (
            std::env::var("OIDC_ISSUER_URL").ok(),
            std::env::var("OIDC_CLIENT_ID").ok(),
            std::env::var("OIDC_CLIENT_SECRET").ok(),
            std::env::var("OIDC_REDIRECT_URI").ok(),
        ) {
            (Some(issuer_url), Some(client_id), Some(client_secret), Some(redirect_uri)) => {
                Some(OidcConfig {
                    issuer_url,
                    client_id,
                    client_secret,
                    redirect_uri,
                })
            }
            _ => None,
        };

        Self {
            server: ServerConfig {
                host: std::env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                port: std::env::var("SERVER_PORT")
                    .ok()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(3000),
            },
            database: DatabaseConfig {
                url: std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                    "postgres://haiker:haiker@localhost:5432/haiker".to_string()
                }),
                max_connections: std::env::var("DATABASE_MAX_CONNECTIONS")
                    .ok()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(10),
            },
            storage: StorageConfig {
                endpoint: std::env::var("STORAGE_ENDPOINT")
                    .unwrap_or_else(|_| "http://localhost:9000".to_string()),
                bucket: std::env::var("STORAGE_BUCKET").unwrap_or_else(|_| "haiker".to_string()),
                access_key_id: std::env::var("STORAGE_ACCESS_KEY_ID")
                    .unwrap_or_else(|_| "minioadmin".to_string()),
                secret_access_key: std::env::var("STORAGE_SECRET_ACCESS_KEY")
                    .unwrap_or_else(|_| "minioadmin".to_string()),
            },
            oidc,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_loads() {
        let config = AppConfig::from_env();
        assert_eq!(config.server.port, 3000);
        assert_eq!(config.database.max_connections, 10);
    }

    #[test]
    fn storage_config_debug_redacts_secret_access_key() {
        let config = StorageConfig {
            endpoint: "http://localhost:9000".to_string(),
            bucket: "test-bucket".to_string(),
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
        };
        let debug_output = format!("{:?}", config);
        assert!(
            !debug_output.contains("wJalrXUtnFEMI"),
            "secret_access_key must not appear in Debug output"
        );
        assert!(
            debug_output.contains("[REDACTED]"),
            "Debug output must show [REDACTED] for secret_access_key"
        );
        assert!(
            debug_output.contains("AKIAIOSFODNN7EXAMPLE"),
            "access_key_id should still appear in Debug output"
        );
    }

    #[test]
    fn oidc_config_debug_redacts_client_secret() {
        let config = OidcConfig {
            issuer_url: "https://example.auth0.com/".to_string(),
            client_id: "my-client-id".to_string(),
            client_secret: "super-secret-value-12345".to_string(),
            redirect_uri: "http://localhost:3000/auth/callback".to_string(),
        };
        let debug_output = format!("{:?}", config);
        assert!(
            !debug_output.contains("super-secret-value-12345"),
            "client_secret must not appear in Debug output"
        );
        assert!(
            debug_output.contains("[REDACTED]"),
            "Debug output must show [REDACTED] for client_secret"
        );
        assert!(
            debug_output.contains("my-client-id"),
            "client_id should still appear in Debug output"
        );
    }
}
