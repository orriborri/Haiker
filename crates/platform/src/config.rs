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
#[derive(Debug, Clone, Deserialize)]
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

/// OIDC identity provider configuration.
#[derive(Debug, Clone, Deserialize)]
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
}
