//! Auth0 OIDC provider implementation.
//!
//! Uses the `openidconnect` crate for discovery and authorization URL generation,
//! but performs the token exchange manually to work around Auth0's non-standard
//! `updated_at` timestamp format that breaks `openidconnect`'s strict parsing.
//!
//! JWT signature verification is performed using the `jsonwebtoken` crate against
//! the provider's JWKS (fetched from the discovery document's `jwks_uri`).

use async_trait::async_trait;
use openidconnect::core::{CoreClient, CoreProviderMetadata, CoreResponseType};
use openidconnect::reqwest;
use openidconnect::{
    ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet, EndpointSet, IssuerUrl,
    Nonce, RedirectUrl, Scope,
};
use haiker_app::identity::{AuthenticationError, OidcClaims, OidcConfig, OidcProvider};

/// The concrete CoreClient type after construction from provider metadata.
type ConfiguredClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

/// Auth0 OIDC provider backed by the `openidconnect` crate.
pub struct Auth0OidcProvider {
    client: ConfiguredClient,
    http_client: reqwest::Client,
    /// Token endpoint URL (cached from discovery).
    token_endpoint: String,
    /// JWKS URI for fetching signing keys.
    jwks_uri: String,
    /// Issuer URL for validating ID token claims.
    issuer: String,
    /// Redirect URI for token exchange.
    redirect_uri: String,
    /// Client ID for token exchange.
    client_id: String,
    /// Client secret for token exchange.
    client_secret: String,
}

impl std::fmt::Debug for Auth0OidcProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Auth0OidcProvider")
            .field("token_endpoint", &self.token_endpoint)
            .finish_non_exhaustive()
    }
}

impl Auth0OidcProvider {
    /// Create a new provider by discovering the OIDC metadata from the issuer URL.
    ///
    /// This performs a network call to fetch the provider's discovery document.
    pub async fn from_config(config: &OidcConfig) -> Result<Self, AuthenticationError> {
        // Auth0 (and some other providers) include a trailing slash in their
        // discovery document's issuer claim. The openidconnect crate performs a
        // strict string comparison, so we normalize the configured URL to include
        // a trailing slash to avoid mismatches.
        let normalized_issuer = if config.issuer_url.ends_with('/') {
            config.issuer_url.clone()
        } else {
            format!("{}/", config.issuer_url)
        };

        let issuer_url = IssuerUrl::new(normalized_issuer.clone())
            .map_err(|e| AuthenticationError::ProviderError(format!("invalid issuer URL: {e}")))?;

        let http_client = reqwest::ClientBuilder::new()
            .build()
            .map_err(|e| {
                AuthenticationError::ProviderError(format!("failed to build HTTP client: {e}"))
            })?;

        let metadata = CoreProviderMetadata::discover_async(issuer_url, &http_client)
            .await
            .map_err(|e| {
                AuthenticationError::ProviderError(format!("OIDC discovery failed: {e}"))
            })?;

        // Cache the token endpoint URL from the discovered metadata.
        let token_endpoint = metadata
            .token_endpoint()
            .map(|u| u.to_string())
            .ok_or_else(|| {
                AuthenticationError::ProviderError(
                    "discovery document missing token endpoint".to_string(),
                )
            })?;

        // Cache the JWKS URI for signature verification.
        let jwks_uri = metadata.jwks_uri().to_string();

        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(config.client_id.clone()),
            Some(ClientSecret::new(config.client_secret.clone())),
        )
        .set_redirect_uri(RedirectUrl::new(config.redirect_uri.clone()).map_err(|e| {
            AuthenticationError::ProviderError(format!("invalid redirect URI: {e}"))
        })?);

        Ok(Self {
            client,
            http_client,
            token_endpoint,
            jwks_uri,
            issuer: normalized_issuer,
            redirect_uri: config.redirect_uri.clone(),
            client_id: config.client_id.clone(),
            client_secret: config.client_secret.clone(),
        })
    }

    /// Manually exchange the authorization code using a direct HTTP POST
    /// to the token endpoint, bypassing the `openidconnect` crate's strict
    /// response parsing that fails on Auth0's non-standard `updated_at` field.
    ///
    /// Returns the raw `id_token` JWT string.
    async fn exchange_code_manual(&self, code: &str) -> Result<String, AuthenticationError> {
        let response = self
            .http_client
            .post(&self.token_endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.redirect_uri.as_str()),
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
            ])
            .send()
            .await
            .map_err(|e| {
                AuthenticationError::CodeExchangeFailed(format!(
                    "token endpoint request failed: {e}"
                ))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::error!(status = %status, body = %body, "Token endpoint returned error");
            return Err(AuthenticationError::CodeExchangeFailed(format!(
                "token endpoint returned {status}"
            )));
        }

        let body_text = response.text().await.map_err(|e| {
            AuthenticationError::CodeExchangeFailed(format!(
                "failed to read token response body: {e}"
            ))
        })?;

        let body: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| {
            AuthenticationError::CodeExchangeFailed(format!(
                "failed to parse token response: {e}"
            ))
        })?;

        body.get("id_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                AuthenticationError::CodeExchangeFailed(
                    "no id_token in token response".to_string(),
                )
            })
    }
}

#[async_trait]
impl OidcProvider for Auth0OidcProvider {
    async fn authorization_url(&self) -> Result<(String, String, String), AuthenticationError> {
        let (auth_url, csrf_state, nonce) = self
            .client
            .authorize_url(
                openidconnect::AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new("profile".to_string()))
            .add_scope(Scope::new("email".to_string()))
            .url();

        Ok((
            auth_url.to_string(),
            csrf_state.secret().clone(),
            nonce.secret().clone(),
        ))
    }

    async fn exchange_code(
        &self,
        code: &str,
        nonce: &str,
    ) -> Result<OidcClaims, AuthenticationError> {
        // Use manual exchange to avoid openidconnect's strict response parsing
        // that fails on Auth0's non-standard `updated_at` timestamp format.
        let id_token_str = self.exchange_code_manual(code).await?;

        // Verify the JWT signature against the provider's JWKS.
        // This ensures the token was actually issued by the identity provider
        // and has not been tampered with.
        let claims_json = self.verify_and_decode_jwt(&id_token_str).await?;

        // Verify nonce to prevent replay attacks
        let token_nonce = claims_json
            .get("nonce")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AuthenticationError::CodeExchangeFailed(
                    "ID token missing nonce claim".to_string(),
                )
            })?;

        if token_nonce != nonce {
            return Err(AuthenticationError::CodeExchangeFailed(
                "ID token nonce does not match expected value".to_string(),
            ));
        }

        // Verify issuer
        let token_issuer = claims_json
            .get("iss")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let expected_issuer = &self.issuer;
        if token_issuer != expected_issuer {
            return Err(AuthenticationError::CodeExchangeFailed(format!(
                "ID token issuer mismatch: got '{}', expected '{}'",
                token_issuer, expected_issuer
            )));
        }

        // Verify audience contains our client_id
        let valid_audience = match claims_json.get("aud") {
            Some(serde_json::Value::String(aud)) => aud == &self.client_id,
            Some(serde_json::Value::Array(auds)) => auds
                .iter()
                .any(|a| a.as_str() == Some(&self.client_id)),
            _ => false,
        };
        if !valid_audience {
            return Err(AuthenticationError::CodeExchangeFailed(
                "ID token audience does not contain our client ID".to_string(),
            ));
        }

        // Verify expiry
        if let Some(exp) = claims_json.get("exp").and_then(|v| v.as_i64()) {
            let now = chrono::Utc::now().timestamp();
            if now > exp {
                return Err(AuthenticationError::CodeExchangeFailed(
                    "ID token has expired".to_string(),
                ));
            }
        }

        // Extract the claims we need
        let subject = claims_json
            .get("sub")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AuthenticationError::CodeExchangeFailed(
                    "ID token missing sub claim".to_string(),
                )
            })?
            .to_string();

        let email = claims_json
            .get("email")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let name = claims_json
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(OidcClaims {
            subject,
            email,
            name,
        })
    }
}

impl Auth0OidcProvider {
    /// Verify the JWT signature against the JWKS and decode the payload.
    ///
    /// Fetches the JWKS from the provider's `jwks_uri`, finds the key matching
    /// the token's `kid` header, and verifies the RS256 signature.
    /// Returns the decoded claims as a JSON value.
    async fn verify_and_decode_jwt(
        &self,
        token: &str,
    ) -> Result<serde_json::Value, AuthenticationError> {
        // Fetch JWKS from the provider
        let jwks_response = self
            .http_client
            .get(&self.jwks_uri)
            .send()
            .await
            .map_err(|e| {
                AuthenticationError::CodeExchangeFailed(format!(
                    "failed to fetch JWKS: {e}"
                ))
            })?;

        let jwks_text = jwks_response.text().await.map_err(|e| {
            AuthenticationError::CodeExchangeFailed(format!(
                "failed to read JWKS response: {e}"
            ))
        })?;

        let jwks: jsonwebtoken::jwk::JwkSet =
            serde_json::from_str(&jwks_text).map_err(|e| {
                AuthenticationError::CodeExchangeFailed(format!(
                    "failed to parse JWKS: {e}"
                ))
            })?;

        // Decode the JWT header to get the key ID (kid)
        let header = jsonwebtoken::decode_header(token).map_err(|e| {
            AuthenticationError::CodeExchangeFailed(format!(
                "failed to decode JWT header: {e}"
            ))
        })?;

        let kid = header.kid.ok_or_else(|| {
            AuthenticationError::CodeExchangeFailed(
                "JWT header missing kid claim".to_string(),
            )
        })?;

        // Find the matching key in the JWKS
        let jwk = jwks
            .find(&kid)
            .ok_or_else(|| {
                AuthenticationError::CodeExchangeFailed(format!(
                    "no matching key found in JWKS for kid '{kid}'"
                ))
            })?;

        // Build the decoding key from the JWK
        let decoding_key =
            jsonwebtoken::DecodingKey::from_jwk(jwk).map_err(|e| {
                AuthenticationError::CodeExchangeFailed(format!(
                    "failed to build decoding key from JWK: {e}"
                ))
            })?;

        // Decode and verify the JWT signature.
        // We disable all claim validation here because we do it manually
        // (the standard validation doesn't handle Auth0's non-standard fields).
        let mut validation = jsonwebtoken::Validation::new(header.alg);
        validation.validate_exp = false;
        validation.validate_aud = false;
        // Set required claims to empty to avoid default validation
        validation.set_required_spec_claims::<&str>(&[]);
        // Set issuer to our expected value
        validation.set_issuer(&[&self.issuer]);

        let token_data = jsonwebtoken::decode::<serde_json::Value>(
            token,
            &decoding_key,
            &validation,
        )
        .map_err(|e| {
            AuthenticationError::CodeExchangeFailed(format!(
                "JWT signature verification failed: {e}"
            ))
        })?;

        Ok(token_data.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oidc_config_rejects_invalid_issuer_url() {
        let config = OidcConfig {
            issuer_url: "not a url".to_string(),
            client_id: "test-client".to_string(),
            client_secret: "test-secret".to_string(),
            redirect_uri: "http://localhost:3000/auth/callback".to_string(),
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let result = rt.block_on(Auth0OidcProvider::from_config(&config));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AuthenticationError::ProviderError(_)));
    }
}
