use serde::{Deserialize, Deserializer};
use std::{collections::BTreeMap, fmt, time::Duration};
use url::Url;
use uuid::Uuid;
use zeroize::Zeroize;

pub(crate) const REQUIRED_ENV: [&str; 7] = [
    "LIFE_DELEGATION_TOKEN",
    "LIFE_AUTH_GATEWAY_URL",
    "LIFE_API_URL",
    "LIFE_WORKBENCH_MCP_SERVICE_TOKEN",
    "LIFE_AGENT_ID",
    "LIFE_AGENT_TURN_ID",
    "LIFE_TRACE_ID",
];

pub(crate) const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const MAX_RESPONSE_BYTES: usize = 256 * 1_024;

pub(crate) struct SecretString(String);

impl SecretString {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self)
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

pub struct Config {
    pub(crate) gateway_base_url: Url,
    pub(crate) life_api_base_url: Url,
    pub(crate) delegation_token: SecretString,
    pub(crate) service_token: SecretString,
    pub(crate) agent_id: String,
    pub(crate) agent_turn_id: String,
    pub(crate) trace_id: Uuid,
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("gateway_base_url", &self.gateway_base_url)
            .field("life_api_base_url", &self.life_api_base_url)
            .field("delegation_token", &self.delegation_token)
            .field("service_token", &self.service_token)
            .field("agent_id", &self.agent_id)
            .field("agent_turn_id", &self.agent_turn_id)
            .field("trace_id", &self.trace_id)
            .finish()
    }
}

impl Config {
    pub(crate) fn from_env() -> Result<Self, ConfigError> {
        let values = REQUIRED_ENV
            .into_iter()
            .filter_map(|name| {
                std::env::var(name)
                    .ok()
                    .map(|value| (name.to_owned(), value))
            })
            .collect();
        Self::from_values(&values)
    }

    pub fn from_values(values: &BTreeMap<String, String>) -> Result<Self, ConfigError> {
        let required = |name: &'static str| {
            values
                .get(name)
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .ok_or(ConfigError::Missing(name))
        };
        let delegation_token = required("LIFE_DELEGATION_TOKEN")?;
        if delegation_token.len() != 43
            || !delegation_token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(ConfigError::Invalid("LIFE_DELEGATION_TOKEN"));
        }
        let service_token = required("LIFE_WORKBENCH_MCP_SERVICE_TOKEN")?;
        if service_token.len() < 32 || service_token.len() > 512 {
            return Err(ConfigError::Invalid("LIFE_WORKBENCH_MCP_SERVICE_TOKEN"));
        }
        Ok(Self {
            gateway_base_url: exact_origin(
                "LIFE_AUTH_GATEWAY_URL",
                &required("LIFE_AUTH_GATEWAY_URL")?,
            )?,
            life_api_base_url: exact_origin("LIFE_API_URL", &required("LIFE_API_URL")?)?,
            delegation_token: SecretString::new(delegation_token),
            service_token: SecretString::new(service_token),
            agent_id: runtime_id("LIFE_AGENT_ID", required("LIFE_AGENT_ID")?)?,
            agent_turn_id: runtime_id("LIFE_AGENT_TURN_ID", required("LIFE_AGENT_TURN_ID")?)?,
            trace_id: required("LIFE_TRACE_ID")?
                .parse()
                .map_err(|_| ConfigError::Invalid("LIFE_TRACE_ID"))?,
        })
    }
}

fn exact_origin(name: &'static str, value: &str) -> Result<Url, ConfigError> {
    let url = Url::parse(value).map_err(|_| ConfigError::Invalid(name))?;
    let loopback_http = url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|host| matches!(host, "127.0.0.1" | "[::1]" | "::1" | "localhost"));
    if (url.scheme() != "https" && !(cfg!(debug_assertions) && loopback_http))
        || url.host_str().is_none()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ConfigError::Invalid(name));
    }
    Ok(url)
}

fn runtime_id(name: &'static str, value: String) -> Result<String, ConfigError> {
    if !(1..=128).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(ConfigError::Invalid(name));
    }
    Ok(value)
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("required Life Workbench configuration is missing: {0}")]
    Missing(&'static str),
    #[error("Life Workbench configuration is invalid: {0}")]
    Invalid(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("LIFE_DELEGATION_TOKEN".into(), "d".repeat(43)),
            (
                "LIFE_AUTH_GATEWAY_URL".into(),
                "https://gateway.test".into(),
            ),
            ("LIFE_API_URL".into(), "https://life.test".into()),
            ("LIFE_WORKBENCH_MCP_SERVICE_TOKEN".into(), "s".repeat(32)),
            ("LIFE_AGENT_ID".into(), "life-agent".into()),
            ("LIFE_AGENT_TURN_ID".into(), "turn-1".into()),
            ("LIFE_TRACE_ID".into(), Uuid::nil().to_string()),
        ])
    }

    #[test]
    fn all_seven_values_are_required_and_urls_are_exact_origins() {
        let complete = values();
        Config::from_values(&complete).expect("valid config");
        for name in REQUIRED_ENV {
            let mut missing = values();
            missing.remove(name);
            assert!(matches!(
                Config::from_values(&missing),
                Err(ConfigError::Missing(missing_name)) if missing_name == name
            ));
        }
        for invalid in [
            "https://life.test/path",
            "https://life.test?host=evil",
            "https://user@life.test",
            "http://life.test",
        ] {
            let mut input = values();
            input.insert("LIFE_API_URL".into(), invalid.into());
            assert!(Config::from_values(&input).is_err());
        }
    }

    #[test]
    fn debug_output_redacts_both_tokens() {
        let config = Config::from_values(&values()).expect("valid config");
        let debug = format!("{config:?}");
        assert!(!debug.contains(&"d".repeat(43)));
        assert!(!debug.contains(&"s".repeat(32)));
        assert_eq!(format!("{:?}", config.delegation_token), "[REDACTED]");
    }
}
