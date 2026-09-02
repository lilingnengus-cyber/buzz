use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;
use url::Url;

/// Verified OIDC identity facts accepted from the Workbench issuer.
#[derive(Clone, Debug)]
pub struct OidcIdentity {
    /// Exact configured issuer.
    pub issuer: String,
    /// Issuer-local opaque subject; never inferred from email.
    pub subject: String,
    /// Upper bound for the resulting Workbench Session.
    pub expires_at: DateTime<Utc>,
}

/// Stable OIDC verification failure classes that do not expose provider details.
#[derive(Debug, thiserror::Error)]
pub enum OidcError {
    /// The token or one of its required claims failed verification.
    #[error("OIDC token rejected")]
    Rejected,
    /// Discovery, JWKS, or another provider dependency was unavailable.
    #[error("OIDC provider unavailable")]
    Unavailable,
}

#[derive(Clone, Debug, Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    exp: i64,
    nonce: String,
}

struct CachedKeys {
    keys: JwkSet,
    fetched_at: Instant,
}

/// RS256 verifier backed by the configured issuer's discovery and JWKS documents.
#[derive(Clone)]
pub struct OidcVerifier {
    client: reqwest::Client,
    issuer: Url,
    issuer_claim: String,
    audience: String,
    cache: Arc<RwLock<Option<CachedKeys>>>,
}

impl OidcVerifier {
    /// Creates a verifier fixed to one issuer and one Life Workbench audience.
    pub fn new(issuer: &str, audience: &str) -> Result<Self, OidcError> {
        let issuer_claim = issuer.to_owned();
        let issuer = Url::parse(issuer).map_err(|_| OidcError::Rejected)?;
        if audience.is_empty() || audience.len() > 256 {
            return Err(OidcError::Rejected);
        }
        Ok(Self {
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| OidcError::Unavailable)?,
            issuer,
            issuer_claim,
            audience: audience.to_owned(),
            cache: Arc::new(RwLock::new(None)),
        })
    }

    /// Verifies signature, issuer, audience, expiry, subject, and the login nonce.
    pub async fn verify(
        &self,
        token: &str,
        expected_nonce: &str,
    ) -> Result<OidcIdentity, OidcError> {
        if token.is_empty()
            || token.len() > 16 * 1024
            || expected_nonce.is_empty()
            || expected_nonce.len() > 512
        {
            return Err(OidcError::Rejected);
        }
        let header = decode_header(token).map_err(|_| OidcError::Rejected)?;
        if header.alg != Algorithm::RS256 {
            return Err(OidcError::Rejected);
        }
        let kid = header.kid.ok_or(OidcError::Rejected)?;
        if kid.is_empty() || kid.len() > 512 {
            return Err(OidcError::Rejected);
        }

        if self.keys_stale().await {
            self.refresh_keys().await?;
        }
        let mut jwk = self.find_key(&kid).await;
        if jwk.is_none() {
            self.refresh_keys().await?;
            jwk = self.find_key(&kid).await;
        }
        let jwk = jwk.ok_or(OidcError::Rejected)?;
        let key = DecodingKey::from_jwk(&jwk).map_err(|_| OidcError::Rejected)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[self.issuer_claim.as_str()]);
        validation.set_audience(&[self.audience.as_str()]);
        validation.validate_exp = true;
        validation.leeway = 30;
        let claims = decode::<Claims>(token, &key, &validation)
            .map_err(|_| OidcError::Rejected)?
            .claims;
        if claims.iss != self.issuer_claim
            || claims.sub.is_empty()
            || claims.sub.len() > 512
            || claims.sub.contains(['\r', '\n', '\0'])
            || claims.nonce != expected_nonce
        {
            return Err(OidcError::Rejected);
        }
        let expires_at = DateTime::from_timestamp(claims.exp, 0).ok_or(OidcError::Rejected)?;
        if expires_at <= Utc::now() {
            return Err(OidcError::Rejected);
        }
        Ok(OidcIdentity {
            issuer: claims.iss,
            subject: claims.sub,
            expires_at,
        })
    }

    async fn keys_stale(&self) -> bool {
        self.cache
            .read()
            .await
            .as_ref()
            .is_none_or(|cached| cached.fetched_at.elapsed() > Duration::from_secs(300))
    }

    async fn find_key(&self, kid: &str) -> Option<jsonwebtoken::jwk::Jwk> {
        self.cache
            .read()
            .await
            .as_ref()
            .and_then(|cached| cached.keys.find(kid))
            .cloned()
    }

    async fn refresh_keys(&self) -> Result<(), OidcError> {
        #[derive(Deserialize)]
        struct Discovery {
            issuer: String,
            jwks_uri: String,
        }

        let discovery_url = self
            .issuer
            .join(".well-known/openid-configuration")
            .map_err(|_| OidcError::Rejected)?;
        let discovery = self
            .client
            .get(discovery_url)
            .send()
            .await
            .map_err(|_| OidcError::Unavailable)?
            .error_for_status()
            .map_err(|_| OidcError::Unavailable)?
            .json::<Discovery>()
            .await
            .map_err(|_| OidcError::Unavailable)?;
        if discovery.issuer != self.issuer_claim {
            return Err(OidcError::Rejected);
        }
        let jwks_uri = Url::parse(&discovery.jwks_uri).map_err(|_| OidcError::Rejected)?;
        if jwks_uri.origin() != self.issuer.origin()
            || !jwks_uri.username().is_empty()
            || jwks_uri.password().is_some()
            || jwks_uri.fragment().is_some()
        {
            return Err(OidcError::Rejected);
        }
        let keys = self
            .client
            .get(jwks_uri)
            .send()
            .await
            .map_err(|_| OidcError::Unavailable)?
            .error_for_status()
            .map_err(|_| OidcError::Unavailable)?
            .json::<JwkSet>()
            .await
            .map_err(|_| OidcError::Unavailable)?;
        *self.cache.write().await = Some(CachedKeys {
            keys,
            fetched_at: Instant::now(),
        });
        Ok(())
    }
}

/// Extracts one case-sensitive Bearer credential without accepting extra whitespace.
pub fn bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|value| !value.is_empty() && !value.bytes().any(|byte| byte.is_ascii_whitespace()))
}
