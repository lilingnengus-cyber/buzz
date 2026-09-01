//! Per-turn LifeOS delegation and MCP injection.

use crate::{
    acp::{EnvVar, McpServer},
    config::parse_optional_feature_switch,
    turn_observer::{
        TurnApplicability, TurnExtension, TurnExtensionAccess, TurnExtensionFinishContext,
        TurnExtensionFuture, TurnMcpMode, TurnPolicy, VerifiedTurnContext,
    },
};
use nostr::Event;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use url::Url;
use uuid::Uuid;

const FEATURE_SWITCHES: [&str; 6] = [
    "LIFE_EXTENSION_ENABLED",
    "LIFE_AGENT_READ_ENABLED",
    "LIFE_AGENT_WRITE_ENABLED",
    "LIFE_CHAT_HIGH_RISK_WRITE_ENABLED",
    "LIFE_DOCK_ENABLED",
    "LIFE_NOTIFIER_ENABLED",
];

const READ_CAPABILITIES: [&str; 8] = [
    "workspace:read",
    "project:read",
    "action:read",
    "focus:read",
    "journal:read",
    "review:read",
    "knowledge:read",
    "ai_execution:read",
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

#[derive(Clone)]
pub(crate) struct LifeAgentHostConfig {
    gateway_base_url: Url,
    life_api_base_url: Url,
    pacioli_service_token: String,
    mcp_service_token: String,
    mcp_command: String,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueResponse {
    delegation_id: Uuid,
    token: String,
    audience: String,
    effective_capabilities: Vec<String>,
    max_calls: i32,
    trace_id: Uuid,
}

pub(crate) struct LifeTurnAccess {
    mcp_server: McpServer,
    policy: TurnPolicy,
    revocation: LifeRevocationGuard,
}

impl LifeAgentHostConfig {
    pub(crate) fn from_env() -> Result<Option<Self>, String> {
        Self::from_reader(|name| std::env::var(name).ok())
    }

    fn from_reader(read: impl Fn(&str) -> Option<String>) -> Result<Option<Self>, String> {
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
        if !switches.agent_read {
            return Ok(None);
        }

        let gateway_base_url =
            require_exact_http_origin("LIFE_AUTH_GATEWAY_URL", read("LIFE_AUTH_GATEWAY_URL"))?;
        let life_api_base_url = require_exact_http_origin("LIFE_API_URL", read("LIFE_API_URL"))?;
        let pacioli_service_token = require_secret(
            "LIFE_AUTH_PACIOLI_SERVICE_TOKEN",
            read("LIFE_AUTH_PACIOLI_SERVICE_TOKEN"),
        )?;
        let mcp_service_token = require_secret(
            "LIFE_WORKBENCH_MCP_SERVICE_TOKEN",
            read("LIFE_WORKBENCH_MCP_SERVICE_TOKEN"),
        )?;
        let mcp_command = require_non_empty(
            "LIFE_WORKBENCH_MCP_COMMAND",
            read("LIFE_WORKBENCH_MCP_COMMAND"),
        )?;
        if mcp_command.len() > 1_024 || mcp_command.chars().any(char::is_control) {
            return Err("LIFE_WORKBENCH_MCP_COMMAND is invalid".into());
        }
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| "failed to build Life Agent HTTP client")?;
        Ok(Some(Self {
            gateway_base_url,
            life_api_base_url,
            pacioli_service_token,
            mcp_service_token,
            mcp_command,
            client,
        }))
    }

    fn policy(&self) -> TurnPolicy {
        TurnPolicy {
            mcp_mode: TurnMcpMode::ReplaceStandard,
            max_turn_duration: Some(Duration::from_secs(120)),
            base_prompt: Some(include_str!("life_agent_prompt.md")),
            disable_memory: true,
            requires_fresh_session: true,
        }
    }

    async fn authorize_turn(
        &self,
        source_event: &Event,
        source_channel_id: Uuid,
        agent_id: &str,
        agent_turn_id: &str,
        trace_id: &str,
    ) -> Result<LifeTurnAccess, String> {
        let trace_id = Uuid::parse_str(trace_id).map_err(|_| "Life Agent trace ID is invalid")?;
        let url = self
            .gateway_base_url
            .join("v1/life-agent/delegations")
            .map_err(|_| "Life Agent gateway URL is invalid")?;
        let source_pubkey = source_event.pubkey.to_hex();
        let response = self
            .client
            .post(url)
            .header(
                "authorization",
                format!("Service {}", self.pacioli_service_token),
            )
            .header("x-trace-id", trace_id.to_string())
            .json(&serde_json::json!({
                "sourceEvent": source_event,
                "sourceChannelId": source_channel_id,
                "conversation": {
                    "type": "channel",
                    "participantPubkeys": [source_pubkey, agent_id]
                },
                "agentId": agent_id,
                "agentTurnId": agent_turn_id,
                "requestedCapabilities": READ_CAPABILITIES,
                "requestedDataScope": {
                    "workspace": [], "domain": [], "project": [], "resource": [],
                    "sensitivity": [], "operationCount": []
                },
                "resourceContext": null,
                "writeCommandId": null,
                "traceId": trace_id
            }))
            .send()
            .await
            .map_err(|_| "Life authorization gateway is unavailable")?;
        if !response.status().is_success() {
            return Err(match response.status().as_u16() {
                409 => "This signed message has already started a Life Agent turn".into(),
                429 => "Life Agent call budget is unavailable".into(),
                _ => "Life Agent turn was not authorized for this identity".into(),
            });
        }
        if response
            .content_length()
            .is_some_and(|length| length > 64 * 1_024)
        {
            return Err("Life Agent authorization response was too large".into());
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| "Life Agent authorization response could not be read")?;
        if bytes.len() > 64 * 1_024 {
            return Err("Life Agent authorization response was too large".into());
        }
        let issued: IssueResponse = serde_json::from_slice(&bytes)
            .map_err(|_| "Life Agent authorization response was invalid")?;
        self.access_from_issue(issued, agent_id, agent_turn_id, trace_id)
    }

    fn access_from_issue(
        &self,
        issued: IssueResponse,
        agent_id: &str,
        agent_turn_id: &str,
        trace_id: Uuid,
    ) -> Result<LifeTurnAccess, String> {
        let effective = issued
            .effective_capabilities
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        if issued.trace_id != trace_id
            || issued.audience != "life-workbench-mcp"
            || issued.max_calls <= 0
            || issued.max_calls > 100
            || issued.token.len() != 43
            || !issued
                .token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || effective.len() != issued.effective_capabilities.len()
            || effective.is_empty()
            || effective
                .iter()
                .any(|capability| !READ_CAPABILITIES.contains(capability))
        {
            return Err("Life Agent delegation context mismatch".into());
        }
        let mcp_server = McpServer {
            name: "life-workbench-mcp".into(),
            command: self.mcp_command.clone(),
            args: Vec::new(),
            env: vec![
                env("LIFE_DELEGATION_TOKEN", issued.token),
                env("LIFE_AUTH_GATEWAY_URL", self.gateway_base_url.as_str()),
                env("LIFE_API_URL", self.life_api_base_url.as_str()),
                env("LIFE_WORKBENCH_MCP_SERVICE_TOKEN", &self.mcp_service_token),
                env("LIFE_AGENT_ID", agent_id),
                env("LIFE_AGENT_TURN_ID", agent_turn_id),
                env("LIFE_TRACE_ID", trace_id.to_string()),
            ],
        };
        Ok(LifeTurnAccess {
            mcp_server,
            policy: self.policy(),
            revocation: LifeRevocationGuard {
                config: Arc::new(self.clone()),
                delegation_id: issued.delegation_id,
                trace_id,
                revoked: AtomicBool::new(false),
            },
        })
    }

    #[cfg(test)]
    pub(super) fn test_mock() -> Self {
        Self {
            gateway_base_url: Url::parse("http://127.0.0.1:1/").expect("test URL"),
            life_api_base_url: Url::parse("http://127.0.0.1:2/").expect("test URL"),
            pacioli_service_token: "p".repeat(32),
            mcp_service_token: "m".repeat(32),
            mcp_command: "life-workbench-mcp".into(),
            client: reqwest::Client::new(),
        }
    }
}

impl TurnExtension for LifeAgentHostConfig {
    fn id(&self) -> &'static str {
        "life"
    }

    fn classify_turn(
        &self,
        context: &VerifiedTurnContext<'_>,
    ) -> Result<TurnApplicability, String> {
        let Some(event) = context.source_event else {
            return Ok(TurnApplicability::NotApplicable);
        };
        let content = &event.content;
        let life_uri = contains_valid_uri(content, "life");
        let has_life = life_uri || explicit_life_domain(content) || content.contains("life://");
        let has_business = contains_valid_uri(content, "biz") || explicit_business_domain(content);
        if has_life && has_business {
            return Ok(TurnApplicability::Ambiguous {
                reason: "signed turn references both Business and Life domains",
            });
        }
        if has_business {
            return Ok(TurnApplicability::NotApplicable);
        }
        let is_dm = matches!(
            &context.conversation,
            crate::turn_observer::VerifiedConversation::Channel {
                channel_type: Some(channel_type),
                ..
            } if channel_type == "dm"
        );
        if !is_dm {
            return Ok(if has_life {
                TurnApplicability::Ambiguous {
                    reason: "LifeOS disclosure policy is required outside a direct message",
                }
            } else {
                TurnApplicability::NotApplicable
            });
        }
        Ok(if life_uri {
            TurnApplicability::Applicable {
                priority: 300,
                reason: "valid life resource reference",
            }
        } else if has_life {
            TurnApplicability::Applicable {
                priority: 200,
                reason: "explicit LifeOS domain request",
            }
        } else {
            TurnApplicability::Applicable {
                priority: 10,
                reason: "configured Life Agent direct-message turn",
            }
        })
    }

    fn begin_turn<'a>(
        &'a self,
        context: VerifiedTurnContext<'a>,
    ) -> TurnExtensionFuture<'a, Result<Option<Box<dyn TurnExtensionAccess>>, String>> {
        Box::pin(async move {
            let (Some(source_event), Some(channel_id)) =
                (context.source_event, context.channel_id())
            else {
                return Ok(None);
            };
            let access = self
                .authorize_turn(
                    source_event,
                    channel_id,
                    context.agent_id,
                    context.agent_turn_id,
                    context.trace_id,
                )
                .await?;
            Ok(Some(Box::new(access) as Box<dyn TurnExtensionAccess>))
        })
    }
}

impl TurnExtensionAccess for LifeTurnAccess {
    fn policy(&self) -> &TurnPolicy {
        &self.policy
    }

    fn mcp_server(&self) -> Option<&McpServer> {
        Some(&self.mcp_server)
    }

    fn start_observation(&mut self, acp: &mut crate::acp::AcpClient) {
        super::life_response::start_capture(acp);
    }

    fn finish<'a>(
        &'a mut self,
        context: TurnExtensionFinishContext<'a>,
    ) -> TurnExtensionFuture<'a, ()> {
        Box::pin(async move {
            let captured = super::life_response::finish_capture(context.acp);
            if context.completed {
                if let (Some(captured), Some(source_event), Some(channel_id)) =
                    (captured, context.source_event, context.channel_id)
                {
                    super::life_response::publish(
                        context.rest_client,
                        channel_id,
                        source_event,
                        captured,
                    )
                    .await;
                }
            }
            self.revocation.revoke().await;
        })
    }
}

struct LifeRevocationGuard {
    config: Arc<LifeAgentHostConfig>,
    delegation_id: Uuid,
    trace_id: Uuid,
    revoked: AtomicBool,
}

impl LifeRevocationGuard {
    async fn revoke(&self) {
        if self.revoked.load(Ordering::Acquire) {
            return;
        }
        let Ok(url) = self.config.gateway_base_url.join(&format!(
            "v1/life-agent/delegations/{}/revoke",
            self.delegation_id
        )) else {
            return;
        };
        let result = self
            .config
            .client
            .post(url)
            .header(
                "authorization",
                format!("Service {}", self.config.pacioli_service_token),
            )
            .header("x-trace-id", self.trace_id.to_string())
            .send()
            .await;
        if result.as_ref().is_ok_and(|response| {
            response.status().is_success() || response.status().as_u16() == 404
        }) {
            self.revoked.store(true, Ordering::Release);
        } else {
            tracing::warn!(
                delegation_id = %self.delegation_id,
                trace_id = %self.trace_id,
                "failed to revoke Life Agent delegation"
            );
        }
    }
}

impl Drop for LifeRevocationGuard {
    fn drop(&mut self) {
        if self.revoked.load(Ordering::Acquire) {
            return;
        }
        let config = Arc::clone(&self.config);
        let delegation_id = self.delegation_id;
        let trace_id = self.trace_id;
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let Ok(url) = config
                    .gateway_base_url
                    .join(&format!("v1/life-agent/delegations/{delegation_id}/revoke"))
                else {
                    return;
                };
                let result = config
                    .client
                    .post(url)
                    .header(
                        "authorization",
                        format!("Service {}", config.pacioli_service_token),
                    )
                    .header("x-trace-id", trace_id.to_string())
                    .send()
                    .await;
                if !result.as_ref().is_ok_and(|response| {
                    response.status().is_success() || response.status().as_u16() == 404
                }) {
                    tracing::warn!(
                        delegation_id = %delegation_id,
                        trace_id = %trace_id,
                        "failed to revoke dropped Life Agent delegation"
                    );
                }
            });
        }
    }
}

fn env(name: impl Into<String>, value: impl Into<String>) -> EnvVar {
    EnvVar {
        name: name.into(),
        value: value.into(),
    }
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
        .ok_or_else(|| format!("{name} is required when LIFE_AGENT_READ_ENABLED=true"))
}

fn require_secret(name: &str, value: Option<String>) -> Result<String, String> {
    let value = require_non_empty(name, value)?;
    if !(32..=512).contains(&value.len()) || value.chars().any(char::is_whitespace) {
        return Err(format!(
            "{name} must be between 32 and 512 non-whitespace bytes"
        ));
    }
    Ok(value)
}

fn require_exact_http_origin(name: &str, value: Option<String>) -> Result<Url, String> {
    let value = require_non_empty(name, value)?;
    let url = Url::parse(&value).map_err(|_| format!("{name} must be an exact HTTP(S) origin"))?;
    let loopback_http = cfg!(debug_assertions)
        && url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]"));
    if (url.scheme() != "https" && !loopback_http)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(format!("{name} must be an exact HTTP(S) origin"));
    }
    Ok(url)
}

fn contains_valid_uri(content: &str, scheme: &str) -> bool {
    content.split_whitespace().any(|token| {
        let token = token.trim_matches(|character: char| {
            matches!(
                character,
                ',' | '.' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '\''
            )
        });
        let Some(path) = token.strip_prefix(&format!("{scheme}://")) else {
            return false;
        };
        let mut parts = path.split('/');
        let Some(kind) = parts.next() else {
            return false;
        };
        let id = parts.next();
        let no_more = parts.next().is_none();
        let kind_valid = match scheme {
            "life" => matches!(
                kind,
                "dashboard"
                    | "domain"
                    | "goal"
                    | "project"
                    | "action"
                    | "calendar"
                    | "journal"
                    | "knowledge"
                    | "review"
                    | "ai-execution"
                    | "draft"
            ),
            "biz" => !kind.is_empty(),
            _ => false,
        };
        let id_valid = if scheme == "life" && kind == "dashboard" {
            id.is_none()
        } else {
            id.is_some_and(safe_uri_id)
        };
        kind_valid && id_valid && no_more
    })
}

fn safe_uri_id(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b':')
        })
}

fn explicit_life_domain(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("lifeos")
        || lower.contains("life os")
        || content.contains("个人工作台")
        || content.contains("个人系统")
}

fn explicit_business_domain(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("business workspace")
        || content.contains("企业工作台")
        || content.contains("企业系统")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn_observer::{VerifiedConversation, VerifiedTurnContext};
    use axum::{
        extract::{Path, State},
        http::{HeaderMap, StatusCode},
        routing::post,
        Json, Router,
    };
    use nostr::{EventBuilder, Keys, Kind, Tag};
    use serde_json::Value;
    use std::sync::Mutex;
    use tokio::net::TcpListener;

    fn values() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            ("LIFE_EXTENSION_ENABLED", "true"),
            ("LIFE_AGENT_READ_ENABLED", "true"),
            ("LIFE_AUTH_GATEWAY_URL", "https://life-auth.example.com"),
            ("LIFE_API_URL", "https://life.example.com"),
            (
                "LIFE_AUTH_PACIOLI_SERVICE_TOKEN",
                "pppppppppppppppppppppppppppppppp",
            ),
            (
                "LIFE_WORKBENCH_MCP_SERVICE_TOKEN",
                "mmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm",
            ),
            ("LIFE_WORKBENCH_MCP_COMMAND", "life-workbench-mcp"),
        ])
    }

    fn config(input: &HashMap<&str, &str>) -> Result<Option<LifeAgentHostConfig>, String> {
        LifeAgentHostConfig::from_reader(|name| input.get(name).map(|value| (*value).to_owned()))
    }

    fn event(content: &str, channel_id: Uuid) -> Event {
        EventBuilder::new(Kind::Custom(40002), content)
            .tags([Tag::parse(["h", &channel_id.to_string()]).expect("tag")])
            .sign_with_keys(&Keys::generate())
            .expect("event")
    }

    #[test]
    fn read_extension_is_disabled_by_default_and_requires_all_service_boundaries() {
        assert!(config(&HashMap::new()).expect("disabled").is_none());
        let mut enabled = values();
        enabled.remove("LIFE_WORKBENCH_MCP_SERVICE_TOKEN");
        let error = match config(&enabled) {
            Ok(_) => panic!("service token must be required"),
            Err(error) => error,
        };
        assert!(error.contains("LIFE_WORKBENCH_MCP_SERVICE_TOKEN"));
        assert!(config(&values()).expect("enabled").is_some());
    }

    #[test]
    fn routing_prioritizes_valid_life_uri_then_explicit_domain_and_rejects_ambiguity() {
        let config = LifeAgentHostConfig::test_mock();
        let channel_id = Uuid::new_v4();
        for (content, expected_priority) in [
            ("打开 life://action/action-1", 300),
            ("查看我的 LifeOS 今日行动", 200),
            ("今天有什么安排", 10),
        ] {
            let event = event(content, channel_id);
            let context = VerifiedTurnContext {
                source_event: Some(&event),
                source_event_id: Some(event.id),
                source_pubkey: Some(event.pubkey),
                community_id: "community",
                conversation: VerifiedConversation::Channel {
                    channel_id,
                    channel_type: Some("dm".into()),
                },
                agent_id: "a",
                agent_turn_id: "turn",
                trace_id: "trace",
            };
            assert!(matches!(
                config.classify_turn(&context).expect("classification"),
                TurnApplicability::Applicable { priority, .. } if priority == expected_priority
            ));
        }
        let ambiguous = event(
            "比较 life://action/action-1 和 biz://sales-order/order-1",
            channel_id,
        );
        let context = VerifiedTurnContext {
            source_event: Some(&ambiguous),
            source_event_id: Some(ambiguous.id),
            source_pubkey: Some(ambiguous.pubkey),
            community_id: "community",
            conversation: VerifiedConversation::Channel {
                channel_id,
                channel_type: Some("dm".into()),
            },
            agent_id: "a",
            agent_turn_id: "turn",
            trace_id: "trace",
        };
        assert!(matches!(
            config.classify_turn(&context).expect("classification"),
            TurnApplicability::Ambiguous { .. }
        ));
    }

    #[test]
    fn multi_party_channels_never_receive_life_data_tools_without_disclosure_policy() {
        let config = LifeAgentHostConfig::test_mock();
        let channel_id = Uuid::new_v4();
        let event = event("打开 life://action/action-1", channel_id);
        let context = VerifiedTurnContext {
            source_event: Some(&event),
            source_event_id: Some(event.id),
            source_pubkey: Some(event.pubkey),
            community_id: "community",
            conversation: VerifiedConversation::Channel {
                channel_id,
                channel_type: Some("stream".into()),
            },
            agent_id: "a",
            agent_turn_id: "turn",
            trace_id: "trace",
        };
        assert!(matches!(
            config.classify_turn(&context).expect("classification"),
            TurnApplicability::Ambiguous { .. }
        ));
    }

    #[test]
    fn issued_delegation_injects_exactly_seven_env_values_and_never_the_host_secret() {
        let config = LifeAgentHostConfig::test_mock();
        let trace_id = Uuid::new_v4();
        let access = config
            .access_from_issue(
                IssueResponse {
                    delegation_id: Uuid::new_v4(),
                    token: "d".repeat(43),
                    audience: "life-workbench-mcp".into(),
                    effective_capabilities: vec!["action:read".into()],
                    max_calls: 10,
                    trace_id,
                },
                &"a".repeat(64),
                "turn-1",
                trace_id,
            )
            .expect("access");
        let names = access
            .mcp_server
            .env
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(names.len(), 7);
        assert!(names.contains("LIFE_DELEGATION_TOKEN"));
        assert!(names.contains("LIFE_WORKBENCH_MCP_SERVICE_TOKEN"));
        assert!(!names.contains("LIFE_AUTH_PACIOLI_SERVICE_TOKEN"));
        let prompt = access.policy.base_prompt.expect("prompt");
        assert!(!prompt.contains(&"d".repeat(43)));
        assert!(!prompt.contains(&config.pacioli_service_token));
        let debug = format!("{:?}", access.mcp_server);
        assert!(!debug.contains(&"d".repeat(43)));
        assert!(!debug.contains(&config.mcp_service_token));
        std::mem::forget(access);
    }

    #[tokio::test]
    async fn authorization_posts_verified_turn_and_finish_revokes_the_delegation() {
        #[derive(Default)]
        struct StateData {
            issues: Mutex<Vec<(HeaderMap, Value)>>,
            revokes: Mutex<Vec<(HeaderMap, Uuid)>>,
        }
        async fn issue(
            State((state, trace_id, delegation_id)): State<(Arc<StateData>, Uuid, Uuid)>,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            state
                .issues
                .lock()
                .expect("issue lock")
                .push((headers, body));
            Json(serde_json::json!({
                "delegationId":delegation_id,
                "token":"d".repeat(43),
                "audience":"life-workbench-mcp",
                "effectiveCapabilities":READ_CAPABILITIES,
                "maxCalls":100,
                "traceId":trace_id
            }))
        }
        async fn revoke(
            State((state, _, _)): State<(Arc<StateData>, Uuid, Uuid)>,
            Path(id): Path<Uuid>,
            headers: HeaderMap,
        ) -> StatusCode {
            state
                .revokes
                .lock()
                .expect("revoke lock")
                .push((headers, id));
            StatusCode::NO_CONTENT
        }

        let trace_id = Uuid::new_v4();
        let delegation_id = Uuid::new_v4();
        let state = Arc::new(StateData::default());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let origin = format!("http://{}", listener.local_addr().expect("address"));
        let app = Router::new()
            .route("/v1/life-agent/delegations", post(issue))
            .route("/v1/life-agent/delegations/{id}/revoke", post(revoke))
            .with_state((state.clone(), trace_id, delegation_id));
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("mock gateway");
        });
        let config = LifeAgentHostConfig {
            gateway_base_url: Url::parse(&origin).expect("origin"),
            life_api_base_url: Url::parse("https://life.example.com").expect("API URL"),
            pacioli_service_token: "p".repeat(32),
            mcp_service_token: "m".repeat(32),
            mcp_command: "life-workbench-mcp".into(),
            client: reqwest::Client::new(),
        };
        let channel_id = Uuid::new_v4();
        let source_keys = Keys::generate();
        let source = EventBuilder::new(Kind::Custom(40002), "查看 LifeOS")
            .tags([Tag::parse(["h", &channel_id.to_string()]).expect("tag")])
            .sign_with_keys(&source_keys)
            .expect("source");
        let agent_id = Keys::generate().public_key().to_hex();
        let access = config
            .authorize_turn(
                &source,
                channel_id,
                &agent_id,
                "turn-1",
                &trace_id.to_string(),
            )
            .await
            .expect("authorized access");
        {
            let issues = state.issues.lock().expect("issue lock");
            assert_eq!(issues.len(), 1);
            assert_eq!(
                issues[0]
                    .0
                    .get("authorization")
                    .and_then(|value| value.to_str().ok()),
                Some(format!("Service {}", "p".repeat(32)).as_str())
            );
            assert_eq!(issues[0].1["sourceEvent"]["id"], source.id.to_hex());
            assert_eq!(issues[0].1["sourceChannelId"], channel_id.to_string());
            assert_eq!(issues[0].1["conversation"]["type"], "channel");
            assert_eq!(
                issues[0].1["requestedCapabilities"]
                    .as_array()
                    .expect("capabilities")
                    .len(),
                READ_CAPABILITIES.len()
            );
        }
        access.revocation.revoke().await;
        let revokes = state.revokes.lock().expect("revoke lock");
        assert_eq!(revokes.len(), 1);
        assert_eq!(revokes[0].1, delegation_id);
        assert_eq!(
            revokes[0]
                .0
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some(format!("Service {}", "p".repeat(32)).as_str())
        );
        drop(revokes);
        drop(access);
        server.abort();
    }
}
