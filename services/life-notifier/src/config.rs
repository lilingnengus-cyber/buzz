use nostr::Keys;
use std::{env, net::SocketAddr, time::Duration};
use url::Url;

#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) enabled: bool,
    pub(crate) lifeos_base_url: Url,
    pub(crate) service_token: String,
    pub(crate) relay_url: Url,
    pub(crate) community_id: String,
    pub(crate) metrics_bind_addr: SocketAddr,
    pub(crate) keys: Keys,
    pub(crate) poll_interval: Duration,
    pub(crate) lease_seconds: u64,
}

impl Config {
    pub(crate) fn from_env() -> anyhow::Result<Self> {
        let enabled = enabled_from_env()?;
        let lifeos_base_url = endpoint("LIFE_NOTIFIER_LIFEOS_URL")?;
        let relay_url = relay_endpoint("LIFE_NOTIFIER_RELAY_URL")?;
        let community_id = required("LIFE_NOTIFIER_COMMUNITY_ID")?;
        if !safe_opaque(&community_id, 256) {
            anyhow::bail!("LIFE_NOTIFIER_COMMUNITY_ID is invalid");
        }
        let service_token = required("LIFE_NOTIFIER_SERVICE_TOKEN")?;
        if service_token.len() < 32 || service_token.chars().any(char::is_whitespace) {
            anyhow::bail!(
                "LIFE_NOTIFIER_SERVICE_TOKEN must be at least 32 non-whitespace characters"
            );
        }
        let keys = Keys::parse(required("LIFE_NOTIFIER_PRIVATE_KEY")?.trim())?;
        let metrics_bind_addr = env::var("LIFE_NOTIFIER_METRICS_BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:9104".to_owned())
            .parse()
            .map_err(|_| anyhow::anyhow!("LIFE_NOTIFIER_METRICS_BIND_ADDR is invalid"))?;
        let poll_interval =
            Duration::from_millis(parse_u64("LIFE_NOTIFIER_POLL_MS", 2_000, 100, 60_000)?);
        let lease_seconds = parse_u64("LIFE_NOTIFIER_LEASE_SECONDS", 60, 10, 300)?;
        Ok(Self {
            enabled,
            lifeos_base_url,
            service_token,
            relay_url,
            community_id,
            metrics_bind_addr,
            keys,
            poll_interval,
            lease_seconds,
        })
    }
}

fn safe_opaque(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._~:-".contains(&byte))
}

pub(crate) fn enabled_from_env() -> anyhow::Result<bool> {
    parse_bool("LIFE_NOTIFIER_ENABLED", false)
}

fn required(name: &str) -> anyhow::Result<String> {
    env::var(name).map_err(|_| anyhow::anyhow!("{name} is required"))
}

fn parse_bool(name: &str, default: bool) -> anyhow::Result<bool> {
    match env::var(name) {
        Ok(value) if value == "true" => Ok(true),
        Ok(value) if value == "false" => Ok(false),
        Ok(_) => anyhow::bail!("{name} must be true or false"),
        Err(_) => Ok(default),
    }
}

fn parse_u64(name: &str, default: u64, min: u64, max: u64) -> anyhow::Result<u64> {
    let value = env::var(name)
        .ok()
        .map(|raw| raw.parse())
        .transpose()?
        .unwrap_or(default);
    if !(min..=max).contains(&value) {
        anyhow::bail!("{name} is outside the supported range");
    }
    Ok(value)
}

fn endpoint(name: &str) -> anyhow::Result<Url> {
    let url = Url::parse(&required(name)?)?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if (url.scheme() != "https" && !(loopback && url.scheme() == "http"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        anyhow::bail!("{name} must be HTTPS or loopback HTTP without credentials or query");
    }
    Ok(url)
}

fn relay_endpoint(name: &str) -> anyhow::Result<Url> {
    let url = Url::parse(&required(name)?)?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if (url.scheme() != "wss" && !(loopback && url.scheme() == "ws"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        anyhow::bail!("{name} must be WSS or loopback WS without credentials or query");
    }
    Ok(url)
}
