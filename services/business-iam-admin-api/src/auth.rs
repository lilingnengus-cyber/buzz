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
    collections::HashSet,
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
    #[serde(default)]
    amr: Vec<String>,
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
    client_id: String,
    max_age: Duration,
    required_amr: HashSet<String>,
    cache: Arc<RwLock<Option<Cache>>>,
}

impl Authenticator {
    pub fn new(config: &Config) -> Self {
        Self {
            client: reqwest::Client::new(),
            issuer: config.authentik_issuer.clone(),
            client_id: config.client_id.clone(),
            max_age: config.step_up_max_age,
            required_amr: config.required_mfa_amr.clone(),
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
                self.issuer.trim_end_matches('/')
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
        let auth_time = validate_step_up(&claims, &self.required_amr, self.max_age, Utc::now())?;
        let row = sqlx::query(
            "SELECT principal.id
             FROM enterprise_users user_row
             JOIN business_iam.principals principal
               ON principal.kind='human' AND principal.external_id=user_row.id::text
             WHERE user_row.issuer=$1 AND user_row.subject=$2
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

fn validate_step_up(
    claims: &Claims,
    required_amr: &HashSet<String>,
    max_age: Duration,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, Error> {
    let auth_time = claims
        .auth_time
        .and_then(|value| DateTime::from_timestamp(value, 0))
        .ok_or(Error::Forbidden("step_up_auth_time_missing"))?;
    let age = now.signed_duration_since(auth_time);
    if age.num_seconds() < -30 || age.to_std().map_or(true, |duration| duration > max_age) {
        return Err(Error::Forbidden("step_up_expired"));
    }
    if !claims
        .amr
        .iter()
        .any(|method| required_amr.contains(method))
    {
        return Err(Error::Forbidden("step_up_mfa_required"));
    }
    Ok(auth_time)
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

    fn claims(auth_time: Option<i64>, amr: &[&str]) -> Claims {
        Claims {
            iss: "https://auth.test/".into(),
            sub: "user".into(),
            _exp: Utc::now().timestamp() + 600,
            aud: Some(Audience::One("iam-admin".into())),
            azp: Some("iam-admin".into()),
            client_id: None,
            auth_time,
            amr: amr.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    #[test]
    fn requires_recent_mfa_step_up() {
        let now = Utc::now();
        let required = HashSet::from(["mfa".to_owned()]);
        assert!(validate_step_up(
            &claims(Some(now.timestamp() - 120), &["pwd", "mfa"]),
            &required,
            Duration::from_secs(300),
            now,
        )
        .is_ok());
        assert!(matches!(
            validate_step_up(
                &claims(Some(now.timestamp() - 301), &["mfa"]),
                &required,
                Duration::from_secs(300),
                now,
            ),
            Err(Error::Forbidden("step_up_expired"))
        ));
        assert!(matches!(
            validate_step_up(
                &claims(Some(now.timestamp()), &["pwd"]),
                &required,
                Duration::from_secs(300),
                now,
            ),
            Err(Error::Forbidden("step_up_mfa_required"))
        ));
        assert!(matches!(
            validate_step_up(
                &claims(None, &["mfa"]),
                &required,
                Duration::from_secs(300),
                now,
            ),
            Err(Error::Forbidden("step_up_auth_time_missing"))
        ));
    }
}
