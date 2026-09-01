use crate::security::{OutboundServiceCredential, ServiceToken, SigningKeyMaterial};
use std::{fmt, net::SocketAddr, time::Duration};
use url::Url;

const CALL_GRANT_AUDIENCE: &str = "lifeos-workbench-api";
const DELEGATION_AUDIENCE: &str = "life-workbench-mcp";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Environment {
    Production,
    Development,
    Test,
}

impl Environment {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "production" => Ok(Self::Production),
            "development" => Ok(Self::Development),
            "test" => Ok(Self::Test),
            _ => Err("LIFE_AUTH_ENVIRONMENT must be production, development, or test".into()),
        }
    }
}

/// Strongly validated configuration for the isolated Life security domain.
#[derive(Clone)]
pub struct Config {
    database_url: String,
    bind_addr: SocketAddr,
    deployment_id: String,
    pacioli_service_token: ServiceToken,
    mcp_service_token: ServiceToken,
    lifeos_service_token: ServiceToken,
    lifeos_outbound_credential: OutboundServiceCredential,
    lifeos_base_url: Url,
    call_grant_issuer: String,
    call_grant_audience: String,
    delegation_audience: String,
    signing_key: SigningKeyMaterial,
    workbench_oidc_issuer: String,
    workbench_oidc_audience: String,
    identity_challenge_ttl: Duration,
    delegation_ttl: Duration,
    call_grant_ttl: Duration,
    environment: Environment,
}

impl Config {
    /// Loads configuration from process environment variables.
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    fn from_lookup(read: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let environment = Environment::parse(
            read("LIFE_AUTH_ENVIRONMENT")
                .as_deref()
                .unwrap_or("production"),
        )?;
        let database_url = required(&read, "LIFE_AUTH_DATABASE_URL")?;
        validate_database_url(&database_url)?;
        let deployment_id = safe_identifier(
            "LIFE_AUTH_DEPLOYMENT_ID",
            required(&read, "LIFE_AUTH_DEPLOYMENT_ID")?,
        )?;
        let pacioli_service_token = ServiceToken::parse(
            "LIFE_AUTH_PACIOLI_SERVICE_TOKEN",
            required(&read, "LIFE_AUTH_PACIOLI_SERVICE_TOKEN")?,
        )?;
        let mcp_service_token = ServiceToken::parse(
            "LIFE_AUTH_MCP_SERVICE_TOKEN",
            required(&read, "LIFE_AUTH_MCP_SERVICE_TOKEN")?,
        )?;
        let lifeos_service_value = required(&read, "LIFE_AUTH_LIFEOS_SERVICE_TOKEN")?;
        let lifeos_service_token = ServiceToken::parse(
            "LIFE_AUTH_LIFEOS_SERVICE_TOKEN",
            lifeos_service_value.clone(),
        )?;
        let lifeos_outbound_credential = OutboundServiceCredential::parse(
            "LIFE_AUTH_LIFEOS_SERVICE_TOKEN",
            lifeos_service_value,
        )?;
        if pacioli_service_token.same_as(&mcp_service_token)
            || pacioli_service_token.same_as(&lifeos_service_token)
            || mcp_service_token.same_as(&lifeos_service_token)
        {
            return Err("Life service tokens must be distinct".into());
        }

        let call_grant_issuer = safe_identifier(
            "LIFE_AUTH_CALL_GRANT_ISSUER",
            required(&read, "LIFE_AUTH_CALL_GRANT_ISSUER")?,
        )?;
        let call_grant_audience =
            fixed_value(&read, "LIFE_AUTH_CALL_GRANT_AUDIENCE", CALL_GRANT_AUDIENCE)?;
        let delegation_audience =
            fixed_value(&read, "LIFE_AUTH_DELEGATION_AUDIENCE", DELEGATION_AUDIENCE)?;
        let signing_key =
            SigningKeyMaterial::parse(&required(&read, "LIFE_AUTH_ED25519_PRIVATE_KEY")?)?;
        let lifeos_base_url = validate_service_base_url(
            "LIFE_AUTH_LIFEOS_BASE_URL",
            &required(&read, "LIFE_AUTH_LIFEOS_BASE_URL")?,
            environment,
        )?;
        let workbench_oidc_issuer = required(&read, "LIFE_AUTH_WORKBENCH_OIDC_ISSUER")?;
        validate_oidc_issuer(&workbench_oidc_issuer, environment)?;
        let workbench_oidc_audience = safe_identifier(
            "LIFE_AUTH_WORKBENCH_OIDC_AUDIENCE",
            required(&read, "LIFE_AUTH_WORKBENCH_OIDC_AUDIENCE")?,
        )?;
        if workbench_oidc_audience == "business-workbench" {
            return Err(
                "LIFE_AUTH_WORKBENCH_OIDC_AUDIENCE must not use a Business audience".into(),
            );
        }

        Ok(Self {
            database_url,
            bind_addr: required(&read, "LIFE_AUTH_BIND_ADDR")?
                .parse()
                .map_err(|_| "LIFE_AUTH_BIND_ADDR is invalid")?,
            deployment_id,
            pacioli_service_token,
            mcp_service_token,
            lifeos_service_token,
            lifeos_outbound_credential,
            lifeos_base_url,
            call_grant_issuer,
            call_grant_audience,
            delegation_audience,
            signing_key,
            workbench_oidc_issuer,
            workbench_oidc_audience,
            identity_challenge_ttl: seconds(
                &read,
                "LIFE_AUTH_IDENTITY_CHALLENGE_TTL_SECONDS",
                90,
                30,
                300,
            )?,
            delegation_ttl: seconds(&read, "LIFE_AUTH_DELEGATION_TTL_SECONDS", 300, 30, 900)?,
            call_grant_ttl: seconds(&read, "LIFE_AUTH_CALL_GRANT_TTL_SECONDS", 30, 1, 60)?,
            environment,
        })
    }

    #[cfg(test)]
    fn from_values(values: &std::collections::BTreeMap<String, String>) -> Result<Self, String> {
        Self::from_lookup(|name| values.get(name).cloned())
    }

    pub(crate) fn database_url(&self) -> &str {
        &self.database_url
    }

    pub(crate) fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    pub(crate) fn signing_key(&self) -> &SigningKeyMaterial {
        &self.signing_key
    }

    pub(crate) fn deployment_id(&self) -> &str {
        &self.deployment_id
    }

    pub(crate) fn lifeos_base_url(&self) -> &Url {
        &self.lifeos_base_url
    }

    pub(crate) fn lifeos_outbound_credential(&self) -> &OutboundServiceCredential {
        &self.lifeos_outbound_credential
    }

    pub(crate) fn workbench_oidc_issuer(&self) -> &str {
        &self.workbench_oidc_issuer
    }

    pub(crate) fn workbench_oidc_audience(&self) -> &str {
        &self.workbench_oidc_audience
    }

    pub(crate) fn identity_challenge_ttl(&self) -> Duration {
        self.identity_challenge_ttl
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("database_url", &"<redacted>")
            .field("bind_addr", &self.bind_addr)
            .field("deployment_id", &self.deployment_id)
            .field("pacioli_service_token", &self.pacioli_service_token)
            .field("mcp_service_token", &self.mcp_service_token)
            .field("lifeos_service_token", &self.lifeos_service_token)
            .field(
                "lifeos_outbound_credential",
                &self.lifeos_outbound_credential,
            )
            .field("lifeos_base_url", &self.lifeos_base_url)
            .field("call_grant_issuer", &self.call_grant_issuer)
            .field("call_grant_audience", &self.call_grant_audience)
            .field("delegation_audience", &self.delegation_audience)
            .field("signing_key", &self.signing_key)
            .field("workbench_oidc_issuer", &self.workbench_oidc_issuer)
            .field("workbench_oidc_audience", &self.workbench_oidc_audience)
            .field("identity_challenge_ttl", &self.identity_challenge_ttl)
            .field("delegation_ttl", &self.delegation_ttl)
            .field("call_grant_ttl", &self.call_grant_ttl)
            .field("environment", &self.environment)
            .finish()
    }
}

impl fmt::Display for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Life auth gateway deployment={} bind={} environment={:?}",
            self.deployment_id, self.bind_addr, self.environment
        )
    }
}

fn required(read: &impl Fn(&str) -> Option<String>, name: &str) -> Result<String, String> {
    read(name)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

fn seconds(
    read: &impl Fn(&str) -> Option<String>,
    name: &str,
    default: u64,
    min: u64,
    max: u64,
) -> Result<Duration, String> {
    let value = read(name)
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| format!("{name} must be an integer"))
        })
        .transpose()?
        .unwrap_or(default);
    if !(min..=max).contains(&value) {
        return Err(format!("{name} must be between {min} and {max}"));
    }
    Ok(Duration::from_secs(value))
}

fn fixed_value(
    read: &impl Fn(&str) -> Option<String>,
    name: &str,
    expected: &str,
) -> Result<String, String> {
    let value = required(read, name)?;
    if value != expected {
        return Err(format!("{name} must be {expected}"));
    }
    Ok(value)
}

fn safe_identifier(name: &str, value: String) -> Result<String, String> {
    if !(1..=256).contains(&value.len())
        || value.trim() != value
        || value.chars().any(char::is_control)
        || value.chars().any(char::is_whitespace)
    {
        return Err(format!("{name} is invalid"));
    }
    Ok(value)
}

fn validate_database_url(value: &str) -> Result<(), String> {
    let url = Url::parse(value).map_err(|_| "LIFE_AUTH_DATABASE_URL must be a PostgreSQL URL")?;
    if !matches!(url.scheme(), "postgres" | "postgresql")
        || url.host_str().is_none()
        || url.path() == "/"
        || url.fragment().is_some()
    {
        return Err("LIFE_AUTH_DATABASE_URL must be a PostgreSQL URL".into());
    }
    Ok(())
}

fn validate_oidc_issuer(value: &str, environment: Environment) -> Result<(), String> {
    let url = Url::parse(value)
        .map_err(|_| "LIFE_AUTH_WORKBENCH_OIDC_ISSUER must be a URL".to_string())?;
    let development_http = environment != Environment::Production
        && url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    if url.scheme() != "https" && !development_http
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("LIFE_AUTH_WORKBENCH_OIDC_ISSUER must use HTTPS in production".into());
    }
    Ok(())
}

fn validate_service_base_url(
    name: &str,
    value: &str,
    environment: Environment,
) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| format!("{name} must be a URL"))?;
    let development_http = environment != Environment::Production
        && url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    if (url.scheme() != "https" && !development_http)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(format!(
            "{name} must be an HTTPS origin (loopback HTTP is allowed outside production)"
        ));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn valid_values() -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                "LIFE_AUTH_DATABASE_URL".into(),
                "postgres://life:secret@db.example/life_auth".into(),
            ),
            ("LIFE_AUTH_BIND_ADDR".into(), "127.0.0.1:3200".into()),
            ("LIFE_AUTH_DEPLOYMENT_ID".into(), "life-prod-cn".into()),
            ("LIFE_AUTH_PACIOLI_SERVICE_TOKEN".into(), "p".repeat(32)),
            ("LIFE_AUTH_MCP_SERVICE_TOKEN".into(), "m".repeat(32)),
            ("LIFE_AUTH_LIFEOS_SERVICE_TOKEN".into(), "l".repeat(32)),
            (
                "LIFE_AUTH_CALL_GRANT_ISSUER".into(),
                "life-auth-gateway".into(),
            ),
            (
                "LIFE_AUTH_CALL_GRANT_AUDIENCE".into(),
                "lifeos-workbench-api".into(),
            ),
            (
                "LIFE_AUTH_DELEGATION_AUDIENCE".into(),
                "life-workbench-mcp".into(),
            ),
            ("LIFE_AUTH_ED25519_PRIVATE_KEY".into(), "11".repeat(32)),
            (
                "LIFE_AUTH_LIFEOS_BASE_URL".into(),
                "https://life.example/".into(),
            ),
            (
                "LIFE_AUTH_WORKBENCH_OIDC_ISSUER".into(),
                "https://identity.example/application/o/life/".into(),
            ),
            (
                "LIFE_AUTH_WORKBENCH_OIDC_AUDIENCE".into(),
                "life-workbench".into(),
            ),
            ("LIFE_AUTH_ENVIRONMENT".into(), "production".into()),
        ])
    }

    #[test]
    fn all_security_boundary_values_are_required() {
        for name in [
            "LIFE_AUTH_DATABASE_URL",
            "LIFE_AUTH_BIND_ADDR",
            "LIFE_AUTH_DEPLOYMENT_ID",
            "LIFE_AUTH_PACIOLI_SERVICE_TOKEN",
            "LIFE_AUTH_MCP_SERVICE_TOKEN",
            "LIFE_AUTH_LIFEOS_SERVICE_TOKEN",
            "LIFE_AUTH_CALL_GRANT_ISSUER",
            "LIFE_AUTH_CALL_GRANT_AUDIENCE",
            "LIFE_AUTH_DELEGATION_AUDIENCE",
            "LIFE_AUTH_ED25519_PRIVATE_KEY",
            "LIFE_AUTH_LIFEOS_BASE_URL",
            "LIFE_AUTH_WORKBENCH_OIDC_ISSUER",
            "LIFE_AUTH_WORKBENCH_OIDC_AUDIENCE",
        ] {
            let mut values = valid_values();
            values.remove(name);
            assert!(
                Config::from_values(&values)
                    .expect_err("missing value must fail")
                    .contains(name),
                "missing {name} must be named"
            );
        }
    }

    #[test]
    fn signing_key_must_be_exactly_32_bytes_of_lower_hex() {
        for invalid in ["11".repeat(31), "11".repeat(33), "GG".repeat(32)] {
            let mut values = valid_values();
            values.insert("LIFE_AUTH_ED25519_PRIVATE_KEY".into(), invalid);
            assert!(Config::from_values(&values)
                .expect_err("invalid key")
                .contains("LIFE_AUTH_ED25519_PRIVATE_KEY"));
        }
    }

    #[test]
    fn life_audiences_are_fixed_and_reject_business_values() {
        for (name, value) in [
            ("LIFE_AUTH_DELEGATION_AUDIENCE", "business-read-mcp"),
            ("LIFE_AUTH_CALL_GRANT_AUDIENCE", "business-workbench-api"),
            ("LIFE_AUTH_DELEGATION_AUDIENCE", "custom-life-mcp"),
        ] {
            let mut values = valid_values();
            values.insert(name.into(), value.into());
            assert!(Config::from_values(&values)
                .expect_err("audience mismatch")
                .contains(name));
        }
    }

    #[test]
    fn service_tokens_must_be_long_and_pairwise_distinct() {
        let mut too_short = valid_values();
        too_short.insert("LIFE_AUTH_MCP_SERVICE_TOKEN".into(), "short".into());
        assert!(Config::from_values(&too_short)
            .expect_err("short token")
            .contains("LIFE_AUTH_MCP_SERVICE_TOKEN"));

        let mut duplicate = valid_values();
        duplicate.insert("LIFE_AUTH_MCP_SERVICE_TOKEN".into(), "p".repeat(32));
        assert!(Config::from_values(&duplicate)
            .expect_err("shared token")
            .contains("must be distinct"));
    }

    #[test]
    fn delegation_and_call_grant_ttls_are_bounded() {
        for (name, value) in [
            ("LIFE_AUTH_DELEGATION_TTL_SECONDS", "29"),
            ("LIFE_AUTH_DELEGATION_TTL_SECONDS", "901"),
            ("LIFE_AUTH_CALL_GRANT_TTL_SECONDS", "0"),
            ("LIFE_AUTH_CALL_GRANT_TTL_SECONDS", "61"),
        ] {
            let mut values = valid_values();
            values.insert(name.into(), value.into());
            assert!(Config::from_values(&values)
                .expect_err("TTL out of bounds")
                .contains(name));
        }
    }

    #[test]
    fn production_rejects_insecure_oidc_issuer() {
        let mut values = valid_values();
        values.insert(
            "LIFE_AUTH_WORKBENCH_OIDC_ISSUER".into(),
            "http://identity.example/application/o/life/".into(),
        );
        assert!(Config::from_values(&values)
            .expect_err("production HTTP")
            .contains("HTTPS"));
    }

    #[test]
    fn development_http_is_limited_to_loopback() {
        let mut loopback = valid_values();
        loopback.insert("LIFE_AUTH_ENVIRONMENT".into(), "development".into());
        loopback.insert(
            "LIFE_AUTH_WORKBENCH_OIDC_ISSUER".into(),
            "http://127.0.0.1:9000/application/o/life/".into(),
        );
        assert!(Config::from_values(&loopback).is_ok());

        loopback.insert(
            "LIFE_AUTH_WORKBENCH_OIDC_ISSUER".into(),
            "http://identity.example/application/o/life/".into(),
        );
        assert!(Config::from_values(&loopback)
            .expect_err("non-loopback HTTP")
            .contains("HTTPS"));
    }

    #[test]
    fn database_url_and_bind_address_are_strictly_validated() {
        let mut values = valid_values();
        values.insert(
            "LIFE_AUTH_DATABASE_URL".into(),
            "postgres://life:secret@db.example/life_auth?sslmode=require".into(),
        );
        assert!(Config::from_values(&values).is_ok());

        values.insert(
            "LIFE_AUTH_DATABASE_URL".into(),
            "https://db.example/life_auth".into(),
        );
        assert!(Config::from_values(&values)
            .expect_err("non-PostgreSQL URL")
            .contains("LIFE_AUTH_DATABASE_URL"));

        let mut values = valid_values();
        values.insert("LIFE_AUTH_BIND_ADDR".into(), "localhost:3200".into());
        assert!(Config::from_values(&values)
            .expect_err("invalid socket address")
            .contains("LIFE_AUTH_BIND_ADDR"));
    }

    #[test]
    fn config_debug_and_display_redact_every_secret() {
        let values = valid_values();
        let config = Config::from_values(&values).expect("valid config");
        for rendered in [format!("{config:?}"), format!("{config}")] {
            for secret in [
                values["LIFE_AUTH_DATABASE_URL"].as_str(),
                values["LIFE_AUTH_PACIOLI_SERVICE_TOKEN"].as_str(),
                values["LIFE_AUTH_MCP_SERVICE_TOKEN"].as_str(),
                values["LIFE_AUTH_LIFEOS_SERVICE_TOKEN"].as_str(),
                values["LIFE_AUTH_ED25519_PRIVATE_KEY"].as_str(),
            ] {
                assert!(!rendered.contains(secret));
            }
        }
    }
}
