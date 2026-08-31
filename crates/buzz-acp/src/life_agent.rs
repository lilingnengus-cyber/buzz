//! Phase-one LifeOS extension boundary.
//!
//! The generic turn host is ready for a future LifeOS extension, but the
//! authorization gateway and delegated MCP are delivered in later phases. This
//! module therefore validates every opt-in switch and then refuses startup
//! instead of exposing a partial or untrusted fallback.

use std::collections::HashMap;

use url::Url;

use crate::config::parse_optional_feature_switch;

const FEATURE_SWITCHES: [&str; 6] = [
    "LIFE_EXTENSION_ENABLED",
    "LIFE_AGENT_READ_ENABLED",
    "LIFE_AGENT_WRITE_ENABLED",
    "LIFE_CHAT_HIGH_RISK_WRITE_ENABLED",
    "LIFE_DOCK_ENABLED",
    "LIFE_NOTIFIER_ENABLED",
];

#[derive(Debug, Clone, Copy, Default)]
struct LifeFeatureSwitches {
    extension: bool,
    agent_read: bool,
    agent_write: bool,
    chat_high_risk_write: bool,
    dock: bool,
    notifier: bool,
}

pub(crate) fn validate_from_env() -> Result<(), String> {
    validate_with(|name| std::env::var(name).ok())
}

fn validate_with(read: impl Fn(&str) -> Option<String>) -> Result<(), String> {
    let values = FEATURE_SWITCHES
        .into_iter()
        .map(|name| (name, read(name)))
        .collect::<HashMap<_, _>>();
    let enabled =
        |name| parse_optional_feature_switch(name, values.get(name).and_then(Option::as_deref));
    let switches = LifeFeatureSwitches {
        extension: enabled("LIFE_EXTENSION_ENABLED")?,
        agent_read: enabled("LIFE_AGENT_READ_ENABLED")?,
        agent_write: enabled("LIFE_AGENT_WRITE_ENABLED")?,
        chat_high_risk_write: enabled("LIFE_CHAT_HIGH_RISK_WRITE_ENABLED")?,
        dock: enabled("LIFE_DOCK_ENABLED")?,
        notifier: enabled("LIFE_NOTIFIER_ENABLED")?,
    };
    validate_switch_hierarchy(switches)?;
    if !switches.extension {
        return Ok(());
    }

    require_exact_http_origin("LIFE_AUTH_GATEWAY_URL", read("LIFE_AUTH_GATEWAY_URL"))?;
    require_exact_http_origin("LIFE_API_URL", read("LIFE_API_URL"))?;
    require_non_empty(
        "LIFE_WORKBENCH_MCP_COMMAND",
        read("LIFE_WORKBENCH_MCP_COMMAND"),
    )?;

    debug_assert!(!include_str!("life_agent_prompt.md").trim().is_empty());
    Err(
        "LIFE_EXTENSION_ENABLED cannot be enabled in phase 1: Life authorization and the delegated MCP are not installed"
            .into(),
    )
}

fn validate_switch_hierarchy(switches: LifeFeatureSwitches) -> Result<(), String> {
    for (child, child_enabled, parent, parent_enabled) in [
        (
            "LIFE_AGENT_READ_ENABLED",
            switches.agent_read,
            "LIFE_EXTENSION_ENABLED",
            switches.extension,
        ),
        (
            "LIFE_AGENT_WRITE_ENABLED",
            switches.agent_write,
            "LIFE_AGENT_READ_ENABLED",
            switches.agent_read,
        ),
        (
            "LIFE_CHAT_HIGH_RISK_WRITE_ENABLED",
            switches.chat_high_risk_write,
            "LIFE_AGENT_WRITE_ENABLED",
            switches.agent_write,
        ),
        (
            "LIFE_DOCK_ENABLED",
            switches.dock,
            "LIFE_EXTENSION_ENABLED",
            switches.extension,
        ),
        (
            "LIFE_NOTIFIER_ENABLED",
            switches.notifier,
            "LIFE_EXTENSION_ENABLED",
            switches.extension,
        ),
    ] {
        if child_enabled && !parent_enabled {
            return Err(format!("{child} requires {parent}=true"));
        }
    }
    Ok(())
}

fn require_non_empty(name: &str, value: Option<String>) -> Result<String, String> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} is required when LIFE_EXTENSION_ENABLED=true"))
}

fn require_exact_http_origin(name: &str, value: Option<String>) -> Result<Url, String> {
    let value = require_non_empty(name, value)?;
    let url = Url::parse(&value).map_err(|_| format!("{name} must be an exact HTTP(S) origin"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || url.origin().ascii_serialization() != value
    {
        return Err(format!("{name} must be an exact HTTP(S) origin"));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(entries: &[(&str, &str)]) -> Result<(), String> {
        let values = entries.iter().copied().collect::<HashMap<_, _>>();
        validate_with(|name| values.get(name).map(|value| (*value).to_string()))
    }

    fn configured_entries() -> Vec<(&'static str, &'static str)> {
        vec![
            ("LIFE_EXTENSION_ENABLED", "true"),
            ("LIFE_AUTH_GATEWAY_URL", "https://life-auth.example.com"),
            ("LIFE_API_URL", "https://life.example.com"),
            ("LIFE_WORKBENCH_MCP_COMMAND", "life-workbench-mcp"),
        ]
    }

    #[test]
    fn life_agent_defaults_to_disabled_without_environment() {
        assert_eq!(validate(&[]), Ok(()));
    }

    #[test]
    fn life_agent_switches_are_strict_booleans() {
        let error = validate(&[("LIFE_EXTENSION_ENABLED", "yes")]).expect_err("invalid switch");
        assert_eq!(error, "LIFE_EXTENSION_ENABLED must be true or false");
    }

    #[test]
    fn life_agent_rejects_child_switches_without_their_parent() {
        for (entries, expected) in [
            (
                vec![("LIFE_AGENT_READ_ENABLED", "true")],
                "LIFE_AGENT_READ_ENABLED requires LIFE_EXTENSION_ENABLED=true",
            ),
            (
                vec![
                    ("LIFE_EXTENSION_ENABLED", "true"),
                    ("LIFE_AGENT_WRITE_ENABLED", "true"),
                ],
                "LIFE_AGENT_WRITE_ENABLED requires LIFE_AGENT_READ_ENABLED=true",
            ),
            (
                vec![("LIFE_DOCK_ENABLED", "true")],
                "LIFE_DOCK_ENABLED requires LIFE_EXTENSION_ENABLED=true",
            ),
            (
                vec![("LIFE_NOTIFIER_ENABLED", "true")],
                "LIFE_NOTIFIER_ENABLED requires LIFE_EXTENSION_ENABLED=true",
            ),
        ] {
            assert_eq!(validate(&entries).expect_err("invalid hierarchy"), expected);
        }
    }

    #[test]
    fn enabled_life_agent_requires_gateway_api_and_mcp_command() {
        assert!(validate(&[("LIFE_EXTENSION_ENABLED", "true")])
            .expect_err("gateway required")
            .contains("LIFE_AUTH_GATEWAY_URL is required"));
        assert!(validate(&[
            ("LIFE_EXTENSION_ENABLED", "true"),
            ("LIFE_AUTH_GATEWAY_URL", "https://life-auth.example.com"),
        ])
        .expect_err("API required")
        .contains("LIFE_API_URL is required"));
        assert!(validate(&[
            ("LIFE_EXTENSION_ENABLED", "true"),
            ("LIFE_AUTH_GATEWAY_URL", "https://life-auth.example.com"),
            ("LIFE_API_URL", "https://life.example.com"),
        ])
        .expect_err("MCP required")
        .contains("LIFE_WORKBENCH_MCP_COMMAND is required"));
    }

    #[test]
    fn life_agent_requires_exact_http_origins() {
        for invalid in [
            "file:///tmp/life",
            "https://user@life.example.com",
            "https://life.example.com/path",
            "https://life.example.com?query=1",
            "https://life.example.com#fragment",
            "https://life.example.com/",
        ] {
            let mut entries = configured_entries();
            entries[1].1 = invalid;
            assert!(validate(&entries)
                .expect_err("invalid gateway origin")
                .contains("LIFE_AUTH_GATEWAY_URL must be an exact HTTP(S) origin"));
        }

        let mut entries = configured_entries();
        entries[2].1 = "https://life.example.com/embed/";
        assert!(validate(&entries)
            .expect_err("invalid API origin")
            .contains("LIFE_API_URL must be an exact HTTP(S) origin"));
    }

    #[test]
    fn configured_life_agent_is_a_rejecting_phase_one_placeholder() {
        let error = validate(&configured_entries()).expect_err("phase one rejects activation");
        assert!(error.contains("cannot be enabled in phase 1"));
        assert!(error.contains("delegated MCP"));
    }
}
