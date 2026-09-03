//! Fail-closed LifeOS channel-disclosure policy lookups.

use crate::security::OutboundServiceCredential;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

/// Fixed low-sensitivity disclosure categories understood by the Gateway.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisclosureCategory {
    /// Minimal action state, never notes or journal content.
    ActionSummary,
    /// Minimal project state, never knowledge content.
    ProjectStatus,
}

impl DisclosureCategory {
    /// Parses the stable wire value stored in delegation rows.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "action_summary" => Some(Self::ActionSummary),
            "project_status" => Some(Self::ProjectStatus),
            _ => None,
        }
    }

    /// Returns the stable wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ActionSummary => "action_summary",
            Self::ProjectStatus => "project_status",
        }
    }

    /// Returns the fixed read-only capability ceiling for this category.
    pub fn capabilities(self) -> &'static [&'static str] {
        match self {
            Self::ActionSummary => &["workspace:read", "action:read", "focus:read"],
            Self::ProjectStatus => &["workspace:read", "project:read", "action:read"],
        }
    }
}

/// Bounded sensitivity asserted by the trusted host and checked by LifeOS.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DisclosureSensitivity {
    /// Public metadata.
    Public,
    /// Ordinary personal-work state eligible for explicit disclosure.
    Normal,
}

impl DisclosureSensitivity {
    /// Parses the stable database value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "PUBLIC" => Some(Self::Public),
            "NORMAL" => Some(Self::Normal),
            _ => None,
        }
    }

    /// Returns the stable database value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "PUBLIC",
            Self::Normal => "NORMAL",
        }
    }
}

/// Current policy grant returned by the LifeOS policy source.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisclosureGrant {
    /// Success marker added by the LifeOS API response envelope.
    #[serde(rename = "ok")]
    pub api_ok: bool,
    /// Whether the exact category and sensitivity are currently allowed.
    pub allowed: bool,
    /// Stable policy identifier when allowed.
    pub policy_id: Option<Uuid>,
    /// Policy expiry when allowed.
    pub expires_at: Option<DateTime<Utc>>,
    /// Fixed obligations asserted by LifeOS.
    #[serde(default)]
    pub obligations: Vec<String>,
    /// Stable denial reason when denied.
    pub reason: Option<String>,
}

impl DisclosureGrant {
    /// Validates that an allow response carries the full read-only contract.
    pub fn validate(&self) -> bool {
        if !self.api_ok {
            return false;
        }
        if !self.allowed {
            return self.policy_id.is_none() && self.expires_at.is_none();
        }
        let obligations = self
            .obligations
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        self.policy_id.is_some()
            && self.expires_at.is_some_and(|expiry| expiry > Utc::now())
            && ["read_only", "redact_sensitive", "summary_only"]
                .into_iter()
                .all(|required| obligations.contains(required))
    }
}

/// Fixed LifeOS disclosure-policy client. It cannot call arbitrary paths.
#[derive(Clone)]
pub struct DisclosureClient {
    client: reqwest::Client,
    endpoint: Url,
    credential: OutboundServiceCredential,
}

impl DisclosureClient {
    /// Creates a client pinned to the one internal evaluation endpoint.
    pub fn new(
        base_url: &Url,
        credential: &OutboundServiceCredential,
    ) -> Result<Self, DisclosureError> {
        let endpoint = base_url
            .join("api/internal/pacioli-disclosure/evaluate")
            .map_err(|_| DisclosureError::Unavailable)?;
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| DisclosureError::Unavailable)?;
        Ok(Self {
            client,
            endpoint,
            credential: credential.clone(),
        })
    }

    /// Reads the current policy. Any transport or response error fails closed.
    pub async fn evaluate(
        &self,
        life_os_user_id: &str,
        community_id: &str,
        channel_id: &str,
        category: DisclosureCategory,
        sensitivity: DisclosureSensitivity,
    ) -> Result<DisclosureGrant, DisclosureError> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Request<'a> {
            life_os_user_id: &'a str,
            community_id: &'a str,
            channel_id: &'a str,
            category: DisclosureCategory,
            sensitivity: DisclosureSensitivity,
        }
        let response = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(self.credential.expose())
            .json(&Request {
                life_os_user_id,
                community_id,
                channel_id,
                category,
                sensitivity,
            })
            .send()
            .await
            .map_err(|_| DisclosureError::Unavailable)?;
        if !response.status().is_success() {
            return Err(DisclosureError::Unavailable);
        }
        let grant = response
            .json::<DisclosureGrant>()
            .await
            .map_err(|_| DisclosureError::Unavailable)?;
        if !grant.validate() {
            return if grant.allowed {
                Err(DisclosureError::Invalid)
            } else {
                Err(DisclosureError::Denied)
            };
        }
        Ok(grant)
    }
}

/// Stable disclosure failure classes safe for authorization mapping.
#[derive(Debug, thiserror::Error)]
pub enum DisclosureError {
    /// No current policy allows this disclosure.
    #[error("LifeOS channel disclosure is denied")]
    Denied,
    /// LifeOS returned an invalid allow envelope.
    #[error("LifeOS channel disclosure response is invalid")]
    Invalid,
    /// The policy source was unavailable; authorization fails closed.
    #[error("LifeOS channel disclosure policy is unavailable")]
    Unavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_have_read_only_fixed_capabilities() {
        for category in [
            DisclosureCategory::ActionSummary,
            DisclosureCategory::ProjectStatus,
        ] {
            assert!(category
                .capabilities()
                .iter()
                .all(|capability| capability.ends_with(":read")));
            assert!(!category.capabilities().is_empty());
        }
    }

    #[test]
    fn allow_requires_every_minimization_obligation() {
        let grant = DisclosureGrant {
            api_ok: true,
            allowed: true,
            policy_id: Some(Uuid::new_v4()),
            expires_at: Some(Utc::now() + chrono::Duration::minutes(1)),
            obligations: vec!["read_only".into(), "redact_sensitive".into()],
            reason: None,
        };
        assert!(!grant.validate());
    }
}
