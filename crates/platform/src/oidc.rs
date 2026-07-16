//! Auth0 OIDC provider implementation.
//!
//! Uses the `openidconnect` crate to perform the standard Authorization Code flow
//! against an Auth0 tenant (or any compliant OIDC provider).

use async_trait::async_trait;
use openidconnect::core::{CoreClient, CoreProviderMetadata, CoreResponseType};
use openidconnect::reqwest;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce, RedirectUrl, Scope, TokenResponse,
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
}

impl std::fmt::Debug for Auth0OidcProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Auth0OidcProvider")
            .field("http_client", &self.http_client)
            .finish_non_exhaustive()
    }
}

impl Auth0OidcProvider {
    /// Create a new provider by discovering the OIDC metadata from the issuer URL.
    ///
    /// This performs a network call to fetch the provider's discovery document.
    pub async fn from_config(config: &OidcConfig) -> Result<Self, AuthenticationError> {
        let issuer_url = IssuerUrl::new(config.issuer_url.clone())
            .map_err(|e| AuthenticationError::ProviderError(format!("invalid issuer URL: {e}")))?;

        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| {
                AuthenticationError::ProviderError(format!("failed to build HTTP client: {e}"))
            })?;

        let metadata = CoreProviderMetadata::discover_async(issuer_url, &http_client)
            .await
            .map_err(|e| {
                AuthenticationError::ProviderError(format!("OIDC discovery failed: {e}"))
            })?;

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
            .add_scope(Scope::new("openid".to_string()))
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
        let token_response = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .map_err(|e| {
                AuthenticationError::CodeExchangeFailed(format!(
                    "token endpoint not configured: {e}"
                ))
            })?
            .request_async(&self.http_client)
            .await
            .map_err(|e| {
                AuthenticationError::CodeExchangeFailed(format!("token exchange failed: {e}"))
            })?;

        // Extract the ID token from the response.
        let id_token = token_response.id_token().ok_or_else(|| {
            AuthenticationError::CodeExchangeFailed("no ID token in token response".to_string())
        })?;

        // Verify the ID token (signature, issuer, audience, expiry, nonce).
        let id_token_verifier = self.client.id_token_verifier();
        let claims = id_token
            .claims(&id_token_verifier, &Nonce::new(nonce.to_string()))
            .map_err(|e| {
                AuthenticationError::CodeExchangeFailed(format!(
                    "ID token verification failed: {e}"
                ))
            })?;

        let subject = claims.subject().to_string();
        let email: Option<String> = claims.email().map(|e| e.as_str().to_string());
        // Auth0 provides name in the standard name claim (localized map).
        let name: Option<String> = claims
            .name()
            .and_then(|localized_map| localized_map.get(None))
            .map(|n| n.as_str().to_string());

        Ok(OidcClaims {
            subject,
            email,
            name,
        })
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
