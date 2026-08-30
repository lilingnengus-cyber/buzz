use crate::{config::Config, model::Actor, Error};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use jsonwebtoken::{
    decode, decode_header,
    errors::{Error as JwtError, ErrorKind},
    jwk::JwkSet,
    Algorithm, DecodingKey, Validation,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

#[derive(Clone, Debug, Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    #[serde(rename = "exp")]
    _exp: i64,
    aud: Option<Audience>,
    azp: Option<String>,
    client_id: Option<String>,
    auth_time: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    fn has(&self, expected: &str) -> bool {
        match self {
            Self::One(value) => value == expected,
            Self::Many(values) => values.iter().any(|value| value == expected),
        }
    }
}

struct Cache {
    set: JwkSet,
    fetched_at: Instant,
}

#[derive(Clone)]
pub struct Authenticator {
    client: reqwest::Client,
    issuer: String,
    backchannel_issuer: String,
    client_id: String,
    cache: Arc<RwLock<Option<Cache>>>,
}

impl Authenticator {
    pub fn new(config: &Config) -> Self {
        Self {
            client: reqwest::Client::new(),
            issuer: config.authentik_issuer.clone(),
            backchannel_issuer: config.authentik_backchannel_issuer.clone(),
            client_id: config.client_id.clone(),
            cache: Arc::new(RwLock::new(None)),
        }
    }

    async fn refresh(&self) -> Result<(), Error> {
        #[derive(Deserialize)]
        struct Discovery {
            jwks_uri: String,
        }
        let discovery = self
            .client
            .get(format!(
                "{}/.well-known/openid-configuration",
                self.backchannel_issuer.trim_end_matches('/')
            ))
            .send()
            .await
            .map_err(|_| Error::Unavailable("oidc_discovery_failed"))?
            .error_for_status()
            .map_err(|_| Error::Unavailable("oidc_discovery_rejected"))?
            .json::<Discovery>()
            .await
            .map_err(|_| Error::Unavailable("oidc_discovery_invalid"))?;
        let set = self
            .client
            .get(discovery.jwks_uri)
            .send()
            .await
            .map_err(|_| Error::Unavailable("jwks_fetch_failed"))?
            .error_for_status()
            .map_err(|_| Error::Unavailable("jwks_fetch_rejected"))?
            .json::<JwkSet>()
            .await
            .map_err(|_| Error::Unavailable("jwks_invalid"))?;
        *self.cache.write().await = Some(Cache {
            set,
            fetched_at: Instant::now(),
        });
        Ok(())
    }

    async fn verify(&self, token: &str) -> Result<Claims, Error> {
        let header = decode_header(token).map_err(|_| Error::Unauthorized("jwt_header_invalid"))?;
        if header.alg != Algorithm::RS256 {
            return Err(Error::Unauthorized("jwt_algorithm_rejected"));
        }
        let kid = header.kid.ok_or(Error::Unauthorized("jwt_kid_missing"))?;
        let stale = self
            .cache
            .read()
            .await
            .as_ref()
            .is_none_or(|cache| cache.fetched_at.elapsed() > Duration::from_secs(300));
        if stale {
            self.refresh().await?;
        }
        let find =
            |cache: &Option<Cache>| cache.as_ref().and_then(|item| item.set.find(&kid)).cloned();
        let mut jwk = {
            let guard = self.cache.read().await;
            find(&guard)
        };
        if jwk.is_none() {
            self.refresh().await?;
            jwk = {
                let guard = self.cache.read().await;
                find(&guard)
            };
        }
        let key = DecodingKey::from_jwk(&jwk.ok_or(Error::Unauthorized("jwt_key_missing"))?)
            .map_err(|_| Error::Unauthorized("jwt_key_invalid"))?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&self.issuer]);
        validation.validate_aud = false;
        validation.validate_exp = true;
        validation.leeway = 30;
        let claims = decode::<Claims>(token, &key, &validation)
            .map_err(|error| Error::Unauthorized(jwt_error_code(&error)))?
            .claims;
        let audience_matches = claims
            .aud
            .as_ref()
            .is_some_and(|audience| audience.has(&self.client_id));
        let presented_client = claims.azp.as_deref().or(claims.client_id.as_deref());
        if presented_client.is_some_and(|client| client != self.client_id)
            || (!audience_matches && presented_client != Some(self.client_id.as_str()))
        {
            return Err(Error::Unauthorized("jwt_client_mismatch"));
        }
        Ok(claims)
    }

    pub async fn actor(&self, pool: &PgPool, token: &str) -> Result<Actor, Error> {
        let claims = self.verify(token).await?;
        let auth_time = authentication_time(&claims)?;
        let row = sqlx::query(
            "SELECT principal.id
             FROM enterprise_users user_row
             JOIN business_iam.principals principal
               ON principal.kind='human' AND principal.external_id=user_row.id::text
             WHERE user_row.oidc_issuer=$1 AND user_row.oidc_subject=$2
               AND user_row.status='active' AND principal.status='active'",
        )
        .bind(&claims.iss)
        .bind(&claims.sub)
        .fetch_optional(pool)
        .await
        .map_err(|_| Error::Unavailable("database_unavailable"))?
        .ok_or(Error::Forbidden("iam_human_principal_missing"))?;
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        Ok(Actor {
            principal_id: row.get("id"),
            issuer: claims.iss,
            subject: claims.sub,
            auth_time,
            evidence_hash: hasher.finalize().to_vec(),
        })
    }
}

fn authentication_time(claims: &Claims) -> Result<DateTime<Utc>, Error> {
    claims
        .auth_time
        .and_then(|value| DateTime::from_timestamp(value, 0))
        .ok_or(Error::Unauthorized("jwt_auth_time_missing"))
}

fn jwt_error_code(error: &JwtError) -> &'static str {
    match error.kind() {
        ErrorKind::InvalidSignature => "jwt_signature_invalid",
        ErrorKind::ExpiredSignature => "jwt_expired",
        ErrorKind::InvalidIssuer => "jwt_issuer_mismatch",
        ErrorKind::InvalidAudience => "jwt_audience_mismatch",
        ErrorKind::ImmatureSignature => "jwt_not_yet_valid",
        ErrorKind::MissingRequiredClaim(_) => "jwt_required_claim_missing",
        ErrorKind::InvalidAlgorithm | ErrorKind::InvalidAlgorithmName => "jwt_algorithm_rejected",
        _ => "jwt_verification_failed",
    }
}

pub fn bearer(headers: &axum::http::HeaderMap) -> Result<&str, Error> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
        .ok_or(Error::Unauthorized("bearer_token_required"))
}

#[allow(dead_code)]
fn unverified_issuer(token: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct IssuerOnly {
        iss: String,
    }
    let encoded = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    serde_json::from_slice::<IssuerOnly>(&decoded)
        .ok()
        .map(|claims| claims.iss)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(auth_time: Option<i64>) -> Claims {
        Claims {
            iss: "https://auth.test/".into(),
            sub: "user".into(),
            _exp: Utc::now().timestamp() + 600,
            aud: Some(Audience::One("iam-admin".into())),
            azp: Some("iam-admin".into()),
            client_id: None,
            auth_time,
        }
    }

    #[test]
    fn accepts_the_original_authentication_time_without_step_up() {
        let now = Utc::now();
        assert!(matches!(
            authentication_time(&claims(Some(now.timestamp() - 86_400))),
            Ok(value) if value.timestamp() == now.timestamp() - 86_400
        ));
        assert!(matches!(
            authentication_time(&claims(None)),
            Err(Error::Unauthorized("jwt_auth_time_missing"))
        ));
    }
}
