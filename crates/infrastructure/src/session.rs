//! Session management for cookie-based authentication.
//!
//! Sessions are stored in the database with a SHA-256 hash of the token.
//! The raw token is sent to the client in an HTTP-only cookie and never stored server-side.

use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

/// Information about a validated session.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// The user who owns this session.
    pub user_id: Uuid,
    /// CSRF token for validating state-changing requests.
    pub csrf_token: String,
    /// The session's database identifier.
    pub session_id: Uuid,
}

/// Manages session lifecycle in the database.
#[derive(Clone)]
pub struct SessionStore {
    pool: PgPool,
}

impl SessionStore {
    /// Create a new session store backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Create a new session for a user.
    ///
    /// Returns the raw token (to be set in the session cookie) and the CSRF token
    /// (to be sent to the client for inclusion in state-changing requests).
    pub async fn create_session(
        &self,
        user_id: Uuid,
        duration: Duration,
    ) -> Result<(String, String), sqlx::Error> {
        let raw_token = Uuid::new_v4().to_string();
        let csrf_token = Uuid::new_v4().to_string();
        let token_hash = hash_token(&raw_token);
        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(duration.as_secs() as i64);

        sqlx::query(
            r#"
            INSERT INTO identity.sessions (user_id, token_hash, csrf_token, expires_at)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(user_id)
        .bind(&token_hash)
        .bind(&csrf_token)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;

        Ok((raw_token, csrf_token))
    }

    /// Validate a session token.
    ///
    /// Hashes the raw token and looks up a matching, non-expired, non-revoked session.
    /// Returns `None` if no valid session is found.
    pub async fn validate_session(
        &self,
        raw_token: &str,
    ) -> Result<Option<SessionInfo>, sqlx::Error> {
        let token_hash = hash_token(raw_token);

        let row = sqlx::query_as::<_, SessionRow>(
            r#"
            SELECT id, user_id, csrf_token
            FROM identity.sessions
            WHERE token_hash = $1
              AND expires_at > now()
              AND revoked_at IS NULL
            "#,
        )
        .bind(&token_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| SessionInfo {
            user_id: r.user_id,
            csrf_token: r.csrf_token,
            session_id: r.id,
        }))
    }

    /// Revoke a specific session.
    pub async fn revoke_session(&self, session_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE identity.sessions
            SET revoked_at = now()
            WHERE id = $1 AND revoked_at IS NULL
            "#,
        )
        .bind(session_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Revoke all sessions for a user.
    pub async fn revoke_all_user_sessions(&self, user_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE identity.sessions
            SET revoked_at = now()
            WHERE user_id = $1 AND revoked_at IS NULL
            "#,
        )
        .bind(user_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Delete expired sessions from the database.
    ///
    /// Returns the number of sessions deleted.
    pub async fn cleanup_expired(&self) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            r#"
            DELETE FROM identity.sessions
            WHERE expires_at <= now()
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }
}

/// Compute SHA-256 hash of a session token for storage.
fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Internal row type for session queries.
#[derive(sqlx::FromRow)]
struct SessionRow {
    id: Uuid,
    user_id: Uuid,
    csrf_token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_token_produces_consistent_output() {
        let token = "test-token-value";
        let hash1 = hash_token(token);
        let hash2 = hash_token(token);
        assert_eq!(hash1, hash2);
        // SHA-256 produces a 64-character hex string
        assert_eq!(hash1.len(), 64);
    }

    #[test]
    fn hash_token_different_inputs_produce_different_hashes() {
        let hash1 = hash_token("token-a");
        let hash2 = hash_token("token-b");
        assert_ne!(hash1, hash2);
    }
}
