use std::{collections::HashSet, net::SocketAddr, time::Duration};
use url::Url;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    pub authentik_issuer: String,
    pub authentik_backchannel_issuer: String,
    pub client_id: String,
    pub allowed_origins: HashSet<String>,
    pub step_up_max_age: Duration,
    pub required_mfa_amr: HashSet<String>,
}

fn required(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

fn https_url(value: &str, name: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| format!("{name} must be a URL"))?;
    if url.scheme() != "https" && !(cfg!(debug_assertions) && url.scheme() == "http") {
        return Err(format!("{name} must use HTTPS"));
    }
    Ok(url)
}

fn allowed_origin(value: &str) -> Result<String, String> {
    // Tauri uses these two fixed local origins for its production webviews.
    // Keep the exception exact: arbitrary custom schemes and HTTP hosts remain
    // forbidden for the management plane.
    if matches!(value, "tauri://localhost" | "http://tauri.localhost") {
        return Ok(value.to_owned());
    }
    let url = Url::parse(value)
        .map_err(|_| "BUSINESS_IAM_ADMIN_ALLOWED_ORIGINS must be a URL".to_owned())?;
    if url.scheme() != "https" {
        return Err("BUSINESS_IAM_ADMIN_ALLOWED_ORIGINS must use HTTPS".into());
    }
    if url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("BUSINESS_IAM_ADMIN_ALLOWED_ORIGINS must contain origins".into());
    }
    Ok(url.origin().ascii_serialization())
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let issuer = https_url(&required("AUTHENTIK_ISSUER")?, "AUTHENTIK_ISSUER")?;
        let backchannel_issuer = std::env::var("AUTHENTIK_BACKCHANNEL_ISSUER")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|value| https_url(&value, "AUTHENTIK_BACKCHANNEL_ISSUER"))
            .transpose()?
            .unwrap_or_else(|| issuer.clone());
        let allowed_origins = required("BUSINESS_IAM_ADMIN_ALLOWED_ORIGINS")?
            .split(',')
            .map(str::trim)
            .map(allowed_origin)
            .collect::<Result<HashSet<_>, String>>()?;
        let step_up_seconds = std::env::var("BUSINESS_IAM_STEP_UP_MAX_AGE_SECONDS")
            .unwrap_or_else(|_| "300".into())
            .parse::<u64>()
            .map_err(|_| "BUSINESS_IAM_STEP_UP_MAX_AGE_SECONDS must be an integer")?;
        if !(60..=900).contains(&step_up_seconds) {
            return Err("BUSINESS_IAM_STEP_UP_MAX_AGE_SECONDS must be between 60 and 900".into());
        }
        let required_mfa_amr = std::env::var("BUSINESS_IAM_REQUIRED_MFA_AMR")
            .unwrap_or_else(|_| "mfa".into())
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        if required_mfa_amr.is_empty() {
            return Err("BUSINESS_IAM_REQUIRED_MFA_AMR cannot be empty".into());
        }
        Ok(Self {
            database_url: required("BUSINESS_IAM_ADMIN_DATABASE_URL")?,
            bind_addr: std::env::var("BUSINESS_IAM_ADMIN_BIND_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:3110".into())
                .parse()
                .map_err(|_| "BUSINESS_IAM_ADMIN_BIND_ADDR is invalid")?,
            authentik_issuer: issuer.as_str().to_owned(),
            authentik_backchannel_issuer: backchannel_issuer.as_str().to_owned(),
            client_id: required("BUSINESS_IAM_ADMIN_CLIENT_ID")?,
            allowed_origins,
            step_up_max_age: Duration::from_secs(step_up_seconds),
            required_mfa_amr,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::allowed_origin;

    #[test]
    fn management_origins_allow_https_and_fixed_tauri_origins() {
        assert_eq!(
            allowed_origin("https://workbench.example.com/").as_deref(),
            Ok("https://workbench.example.com")
        );
        assert_eq!(
            allowed_origin("tauri://localhost").as_deref(),
            Ok("tauri://localhost")
        );
        assert_eq!(
            allowed_origin("http://tauri.localhost").as_deref(),
            Ok("http://tauri.localhost")
        );
    }

    #[test]
    fn management_origins_reject_paths_and_untrusted_http() {
        assert!(allowed_origin("https://workbench.example.com/admin").is_err());
        assert!(allowed_origin("http://workbench.example.com").is_err());
        assert!(allowed_origin("buzz://localhost").is_err());
    }
}
