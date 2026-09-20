use super::business_response::BusinessResponseObservation;
use crate::acp::{EnvVar, McpServer};
use crate::turn_observer::{
    TurnApplicability, TurnExtension, TurnExtensionAccess, TurnExtensionFinishContext,
    TurnExtensionFuture, TurnMcpMode, TurnPolicy, VerifiedTurnContext,
};
use nostr::Event;
use serde::Deserialize;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use url::Url;
use uuid::Uuid;

const AGENT_SCOPES: [&str; 65] = [
    "operating_report_snapshot_intent:create",
    "management_report_snapshot_intent:create",
    "sales_order_hold_intent:create",
    "sales_order_release_hold_intent:create",
    "core_master_status_intent:create",
    "product_master_status_intent:create",
    "core_master_creation_intent:create",
    "core_master_update_intent:create",
    "product_master_creation_intent:create",
    "product_master_update_intent:create",
    "crm_creation_intent:create",
    "crm_update_intent:create",
    "crm_followup_intent:create",
    "crm:read",
    "inventory_count_creation_intent:create",
    "inventory_count_submission_intent:create",
    "inventory_count_posting_intent:create",
    "inventory_count_cancellation_intent:create",
    "sales_return_reversal_intent:create",
    "sales_return_cancellation_intent:create",
    "purchase_return_reversal_intent:create",
    "purchase_return_cancellation_intent:create",
    "sales_return:update_draft",
    "purchase_return:update_draft",
    "sales_return:create",
    "purchase_return:create",
    "sales_return_inspection_intent:create",
    "purchase_return_dispatch_intent:create",
    "purchase_return_acknowledgment_intent:create",
    "sales_return:read",
    "purchase_return:read",
    "shipment_reversal_intent:create",
    "goods_receipt_reversal_intent:create",
    "inventory_opening_reversal_intent:create",
    "sales_order_cancellation_intent:create",
    "purchase_order_cancellation_intent:create",
    "customer_receipt_reversal_intent:create",
    "supplier_payment_reversal_intent:create",
    "receivable_allocation_reversal_intent:create",
    "payable_allocation_reversal_intent:create",
    "business_master_data:read",
    "business_product_master:read",
    "sales_order:read",
    "purchase_order:read",
    "inventory:read",
    "customer_receipt:read",
    "supplier_payment:read",
    "shipment:read",
    "goods_receipt:read",
    "receivable:read",
    "payable:read",
    "order_profit:read",
    "business_anomaly:read",
    "business_action:read",
    "sales_order:update_draft",
    "purchase_order:update_draft",
    "inventory_opening:create",
    "receivable_allocation_intent:create",
    "payable_allocation_intent:create",
    "sales_order:create",
    "shipment:create",
    "purchase_order:create",
    "goods_receipt:create",
    "customer_receipt:create",
    "supplier_payment:create",
];

fn chat_approval_scope(content: &str) -> Option<&'static str> {
    let mut parts = content.split_whitespace();
    if !matches!(parts.next()?, "/approve" | "/reject" | "确认" | "拒绝") {
        return None;
    }
    let scope = match parts.next()? {
        "operating-report-snapshot-intent" => "operating_report_snapshot_intent:approve",
        "management-report-snapshot-intent" => "management_report_snapshot_intent:approve",
        "sales-order-hold-intent" => "sales_order_hold_intent:approve",
        "sales-order-release-hold-intent" => "sales_order_release_hold_intent:approve",
        "core-master-status-intent" => "core_master_status_intent:approve",
        "product-master-status-intent" => "product_master_status_intent:approve",
        "core-master-creation-intent" => "core_master_creation_intent:approve",
        "core-master-update-intent" => "core_master_update_intent:approve",
        "product-master-creation-intent" => "product_master_creation_intent:approve",
        "product-master-update-intent" => "product_master_update_intent:approve",
        "crm-creation-intent" => "crm_creation_intent:approve",
        "crm-update-intent" => "crm_update_intent:approve",
        "crm-followup-intent" => "crm_followup_intent:approve",
        "inventory-count-creation-intent" => "inventory_count_creation_intent:approve",
        "inventory-count-submission-intent" => "inventory_count_submission_intent:approve",
        "inventory-count-posting-intent" => "inventory_count_posting_intent:approve",
        "inventory-count-cancellation-intent" => "inventory_count_cancellation_intent:approve",

        "sales-return-reversal-intent" => "sales_return_reversal_intent:approve",
        "sales-return-cancellation-intent" => "sales_return_cancellation_intent:approve",
        "purchase-return-reversal-intent" => "purchase_return_reversal_intent:approve",
        "purchase-return-cancellation-intent" => "purchase_return_cancellation_intent:approve",
        "sales-return" => "sales_return:approve",
        "purchase-return" => "purchase_return:approve",
        "sales-return-inspection-intent" => "sales_return_inspection_intent:approve",
        "purchase-return-dispatch-intent" => "purchase_return_dispatch_intent:approve",
        "purchase-return-acknowledgment-intent" => "purchase_return_acknowledgment_intent:approve",
        "customer-receipt-reversal-intent" => "customer_receipt_reversal_intent:approve",
        "supplier-payment-reversal-intent" => "supplier_payment_reversal_intent:approve",
        "receivable-allocation-reversal-intent" => "receivable_allocation_reversal_intent:approve",
        "payable-allocation-reversal-intent" => "payable_allocation_reversal_intent:approve",
        "shipment-reversal-intent" => "shipment_reversal_intent:approve",
        "goods-receipt-reversal-intent" => "goods_receipt_reversal_intent:approve",
        "inventory-opening-reversal-intent" => "inventory_opening_reversal_intent:approve",
        "sales-order-cancellation-intent" => "sales_order_cancellation_intent:approve",
        "purchase-order-cancellation-intent" => "purchase_order_cancellation_intent:approve",
        "sales-order" => "sales_order:approve",
        "purchase-order" => "purchase_order:approve",
        "shipment" => "shipment:approve",
        "goods-receipt" => "goods_receipt:approve",
        "receivable-allocation-intent" => "receivable_allocation_intent:approve",
        "payable-allocation-intent" => "payable_allocation_intent:approve",
        "inventory-opening" => "inventory_opening:approve",
        "customer-receipt" => "customer_receipt:approve",
        "supplier-payment" => "supplier_payment:approve",

        _ => return None,
    };
    let _: Uuid = parts.next()?.parse().ok()?;
    let version = parts.next()?.strip_prefix('v')?.parse::<i64>().ok()?;
    let hash = parts.next()?;
    if parts.next().is_some()
        || version <= 0
        || hash.len() != 64
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(scope)
}

#[derive(Clone)]
pub(crate) struct BusinessAgentHostConfig {
    gateway_base_url: Url,
    business_api_base_url: Option<Url>,
    business_action_api_base_url: Option<Url>,
    service_credential: String,
    mcp_command: String,
    adapter: String,
    tool_timeout_seconds: u64,
    turn_timeout_seconds: u64,
    max_payload_bytes: usize,
    default_limit: u64,
    max_limit: u64,
    draft_write_enabled: bool,
    chat_approval_enabled: bool,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueResponse {
    id: Uuid,
    token: String,
    audience: String,
    scopes: Vec<String>,
    trace_id: Uuid,
}

pub(crate) struct BusinessTurnAccess {
    mcp_server: McpServer,
    policy: TurnPolicy,
    _revocation: RevocationGuard,
}

struct BusinessPolicyOnlyAccess {
    policy: TurnPolicy,
}

impl BusinessAgentHostConfig {
    fn turn_policy(&self) -> TurnPolicy {
        TurnPolicy {
            mcp_mode: TurnMcpMode::ReplaceStandard,
            max_turn_duration: Some(Duration::from_secs(self.turn_timeout_seconds)),
            base_prompt: Some(include_str!("business_agent_prompt.md")),
            disable_memory: true,
            requires_fresh_session: true,
            harness_publishes_response: false,
        }
    }

    fn heartbeat_policy(&self) -> TurnPolicy {
        TurnPolicy {
            requires_fresh_session: false,
            ..self.turn_policy()
        }
    }
}

impl TurnExtension for BusinessAgentHostConfig {
    fn id(&self) -> &'static str {
        "business"
    }

    fn begin_error_message(&self, error: &str) -> Option<&'static str> {
        Some(business_begin_error_message(error))
    }

    fn classify_turn(
        &self,
        context: &VerifiedTurnContext<'_>,
    ) -> Result<TurnApplicability, String> {
        Ok(
            match (
                context.source_event.is_some(),
                context.channel_id().is_some(),
            ) {
                (true, true) => TurnApplicability::Applicable {
                    priority: 10,
                    reason: "configured Business Agent channel turn",
                },
                (false, false) => TurnApplicability::Applicable {
                    priority: 10,
                    reason: "configured Business Agent heartbeat policy",
                },
                _ => TurnApplicability::Ambiguous {
                    reason: "Business Agent event and channel facts disagree",
                },
            },
        )
    }

    fn begin_turn<'a>(
        &'a self,
        context: VerifiedTurnContext<'a>,
    ) -> TurnExtensionFuture<'a, Result<Option<Box<dyn TurnExtensionAccess>>, String>> {
        Box::pin(async move {
            let (Some(source_event), Some(channel_id)) =
                (context.source_event, context.channel_id())
            else {
                return Ok(Some(Box::new(BusinessPolicyOnlyAccess {
                    policy: self.heartbeat_policy(),
                }) as Box<dyn TurnExtensionAccess>));
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

fn business_begin_error_message(error: &str) -> &'static str {
    match error {
        "Business Agent turn was not authorized for this user or device" =>
            "本次企业工作台授权未通过，尚未执行查询或写入。下一步：打开企业工作台检查聊天身份绑定与账号权限；已登录不代表已完成绑定。",
        "Business Agent query rate limit exceeded" =>
            "企业助手请求过于频繁，本次尚未执行查询或写入。下一步：稍后重试。",
        "This Buzz event has already started a Business Agent turn" =>
            "这条消息已经处理过，本次没有重复执行。下一步：查看原请求的结果。",
        _ => "企业助手暂时无法完成授权，本次尚未执行查询或写入。下一步：稍后重试，若持续失败请联系管理员。",
    }
}

impl TurnExtensionAccess for BusinessTurnAccess {
    fn policy(&self) -> &TurnPolicy {
        &self.policy
    }

    fn mcp_server(&self) -> Option<&McpServer> {
        Some(&self.mcp_server)
    }

    fn start_observation(&mut self, acp: &mut crate::acp::AcpClient) {
        super::business_response::start_capture(acp);
    }

    fn finish<'a>(
        &'a mut self,
        context: TurnExtensionFinishContext<'a>,
    ) -> TurnExtensionFuture<'a, ()> {
        Box::pin(async move {
            let captured = super::business_response::finish_capture(context.acp);
            let mut observation = captured
                .as_ref()
                .map(|captured| captured.observation.clone())
                .unwrap_or_default();
            if context.completed {
                if let (Some(source_event), Some(channel_id), Some(content)) = (
                    context.source_event,
                    context.channel_id,
                    captured.and_then(|captured| captured.text),
                ) {
                    observation = super::business_response::publish(
                        context.rest_client,
                        channel_id,
                        source_event,
                        &content,
                        observation,
                    )
                    .await;
                }
            }
            self.audit_response(observation).await;
            self._revocation.revoke().await;
        })
    }
}

impl TurnExtensionAccess for BusinessPolicyOnlyAccess {
    fn policy(&self) -> &TurnPolicy {
        &self.policy
    }

    fn mcp_server(&self) -> Option<&McpServer> {
        None
    }

    fn start_observation(&mut self, _acp: &mut crate::acp::AcpClient) {}

    fn finish<'a>(
        &'a mut self,
        _context: TurnExtensionFinishContext<'a>,
    ) -> TurnExtensionFuture<'a, ()> {
        Box::pin(async {})
    }
}

impl BusinessTurnAccess {
    pub(crate) async fn audit_response(&self, observation: BusinessResponseObservation) {
        let Ok(url) = self
            ._revocation
            .config
            .gateway_base_url
            .join("internal/agent-audit")
        else {
            return;
        };
        let succeeded =
            observation.publish_succeeded && observation.response_buzz_event_id.is_some();
        let response = self
            ._revocation
            .config
            .client
            .post(url.clone())
            .header(
                "x-business-service-credential",
                &self._revocation.config.service_credential,
            )
            .header("x-trace-id", self._revocation.trace_id.to_string())
            .json(&serde_json::json!({
                "delegationId": self._revocation.delegation_id,
                "toolName": "buzz_response",
                "eventType": if succeeded { "AGENT_BUSINESS_RESPONSE_EMITTED" } else { "AGENT_BUSINESS_RESPONSE_FAILED" },
                "result": if succeeded { "success" } else { "failure" },
                "resultCount": observation.finding_count,
                "findingCount": observation.finding_count,
                "resourceRefCount": observation.resource_ref_count,
                "ruleSetVersion": null,
                "anomalyRunId": null,
                "responseBuzzEventId": observation.response_buzz_event_id,
                "durationMs": observation.duration_ms.clamp(0, 120_000),
                "reasonCode": null,
                "traceId": self._revocation.trace_id,
            }))
            .send()
            .await;
        if !response.is_ok_and(|value| value.status().is_success()) {
            tracing::warn!(
                delegation_id = %self._revocation.delegation_id,
                trace_id = %self._revocation.trace_id,
                publish_attempted = observation.publish_attempted,
                "failed to audit Business Agent Buzz response"
            );
        }
        if succeeded && observation.anomaly_tool_used {
            let anomaly_response = self
                ._revocation
                .config
                .client
                .post(url)
                .header(
                    "x-business-service-credential",
                    &self._revocation.config.service_credential,
                )
                .header("x-trace-id", self._revocation.trace_id.to_string())
                .json(&serde_json::json!({
                    "delegationId": self._revocation.delegation_id,
                    "toolName": "buzz_response",
                    "eventType": "BUSINESS_ANOMALY_RESPONSE_EMITTED",
                    "result": "success",
                    "resultCount": observation.finding_count,
                    "findingCount": observation.finding_count,
                    "resourceRefCount": observation.resource_ref_count,
                    "ruleSetVersion": null,
                    "anomalyRunId": null,
                    "responseBuzzEventId": observation.response_buzz_event_id,
                    "durationMs": observation.duration_ms.clamp(0, 120_000),
                    "reasonCode": null,
                    "traceId": self._revocation.trace_id,
                }))
                .send()
                .await;
            if !anomaly_response.is_ok_and(|value| value.status().is_success()) {
                tracing::warn!(
                    delegation_id = %self._revocation.delegation_id,
                    trace_id = %self._revocation.trace_id,
                    "failed to audit emitted Business Anomaly response"
                );
            }
        }
    }
}

struct RevocationGuard {
    config: Arc<BusinessAgentHostConfig>,
    delegation_id: Uuid,
    trace_id: Uuid,
    revoked: AtomicBool,
}

impl RevocationGuard {
    async fn revoke(&self) {
        if self.revoked.load(Ordering::Acquire) {
            return;
        }
        let Ok(url) = self.config.gateway_base_url.join(&format!(
            "internal/agent-delegations/{}/revoke",
            self.delegation_id
        )) else {
            return;
        };
        let result = self
            .config
            .client
            .post(url)
            .header(
                "x-business-service-credential",
                &self.config.service_credential,
            )
            .header("x-trace-id", self.trace_id.to_string())
            .send()
            .await;
        let succeeded = result.as_ref().is_ok_and(|response| {
            response.status().is_success() || response.status().as_u16() == 404
        });
        if succeeded {
            self.revoked.store(true, Ordering::Release);
        } else {
            tracing::warn!(
                delegation_id = %self.delegation_id,
                trace_id = %self.trace_id,
                "failed to revoke Business Agent delegation"
            );
        }
    }
}

impl Drop for RevocationGuard {
    fn drop(&mut self) {
        if self.revoked.load(Ordering::Acquire) {
            return;
        }
        let config = Arc::clone(&self.config);
        let id = self.delegation_id;
        let trace_id = self.trace_id;
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let Ok(url) = config
                    .gateway_base_url
                    .join(&format!("internal/agent-delegations/{id}/revoke"))
                else {
                    return;
                };
                let result = config
                    .client
                    .post(url)
                    .header("x-business-service-credential", &config.service_credential)
                    .header("x-trace-id", trace_id.to_string())
                    .send()
                    .await;
                if !result.as_ref().is_ok_and(|response| {
                    response.status().is_success() || response.status().as_u16() == 404
                }) {
                    tracing::warn!(
                        delegation_id = %id,
                        trace_id = %trace_id,
                        "failed to revoke dropped Business Agent delegation"
                    );
                }
            });
        }
    }
}

impl BusinessAgentHostConfig {
    #[cfg(test)]
    pub(super) fn test_mock() -> Self {
        Self {
            gateway_base_url: Url::parse("http://127.0.0.1:1/").expect("test URL"),
            business_api_base_url: None,
            business_action_api_base_url: None,
            service_credential: "test-service-credential-at-least-32-bytes".into(),
            mcp_command: "business-read-mcp".into(),
            adapter: "mock".into(),
            tool_timeout_seconds: 10,
            turn_timeout_seconds: 120,
            max_payload_bytes: 128 * 1024,
            default_limit: 20,
            max_limit: 100,
            draft_write_enabled: false,
            chat_approval_enabled: false,
            client: reqwest::Client::new(),
        }
    }

    pub(crate) fn from_env() -> Result<Option<Self>, String> {
        let enabled = std::env::var("BUSINESS_AGENT_READ_ENABLED")
            .ok()
            .map(|value| {
                value
                    .parse::<bool>()
                    .map_err(|_| "BUSINESS_AGENT_READ_ENABLED must be true or false".to_string())
            })
            .transpose()?
            .unwrap_or(false);
        if !enabled {
            return Ok(None);
        }
        let required = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| format!("{name} is required"))
        };
        let parse_url = |name: &str, value: String| {
            let url = Url::parse(&value).map_err(|_| format!("{name} must be a URL"))?;
            if url.scheme() != "https" && !(cfg!(debug_assertions) && url.scheme() == "http") {
                return Err(format!("{name} must use HTTPS"));
            }
            Ok(url)
        };
        let adapter =
            std::env::var("BUSINESS_READ_ADAPTER").unwrap_or_else(|_| "production".into());
        if adapter != "production" && !(cfg!(debug_assertions) && adapter == "mock") {
            return Err(
                "BUSINESS_READ_ADAPTER must be production (or mock in debug builds)".into(),
            );
        }
        let api_url = if adapter == "production" {
            Some(parse_url(
                "BUSINESS_READ_API_BASE_URL",
                required("BUSINESS_READ_API_BASE_URL")?,
            )?)
        } else {
            None
        };
        let action_enabled = std::env::var("BUSINESS_ACTION_ENABLED")
            .ok()
            .map(|value| {
                value
                    .parse::<bool>()
                    .map_err(|_| "BUSINESS_ACTION_ENABLED must be true or false".to_string())
            })
            .transpose()?
            .unwrap_or(false);
        let action_api_url = if adapter == "production" && action_enabled {
            Some(parse_url(
                "BUSINESS_ACTION_API_BASE_URL",
                required("BUSINESS_ACTION_API_BASE_URL")?,
            )?)
        } else {
            None
        };
        let credential = super::business_credential::load(
            std::env::var("BUSINESS_READ_SERVICE_CREDENTIAL").ok(),
            std::env::var("BUSINESS_READ_SERVICE_CREDENTIAL_FILE").ok(),
        )?;
        let tool_timeout_seconds = bounded_number("BUSINESS_TOOL_TIMEOUT_SECONDS", 10, 1, 30)?;
        let turn_timeout_seconds = bounded_number("AGENT_TURN_TIMEOUT_SECONDS", 120, 30, 900)?;
        let max_payload_bytes = bounded_number(
            "BUSINESS_TOOL_MAX_PAYLOAD_BYTES",
            128 * 1024,
            4096,
            1024 * 1024,
        )? as usize;
        let default_limit = bounded_number("BUSINESS_TOOL_DEFAULT_LIMIT", 20, 1, 100)?;
        let max_limit = bounded_number("BUSINESS_TOOL_MAX_LIMIT", 100, default_limit, 100)?;
        let draft_write_enabled = std::env::var("BUSINESS_AGENT_DRAFT_WRITE_ENABLED")
            .ok()
            .map(|value| {
                value.parse::<bool>().map_err(|_| {
                    "BUSINESS_AGENT_DRAFT_WRITE_ENABLED must be true or false".to_string()
                })
            })
            .transpose()?
            .unwrap_or(false);
        let chat_approval_enabled = std::env::var("BUSINESS_CHAT_APPROVAL_ENABLED")
            .ok()
            .map(|value| {
                value
                    .parse::<bool>()
                    .map_err(|_| "BUSINESS_CHAT_APPROVAL_ENABLED must be true or false".to_string())
            })
            .transpose()?
            .unwrap_or(false);
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| "failed to build Business Agent HTTP client")?;
        Ok(Some(Self {
            gateway_base_url: parse_url(
                "BUSINESS_AUTH_GATEWAY_BASE_URL",
                required("BUSINESS_AUTH_GATEWAY_BASE_URL")?,
            )?,
            business_api_base_url: api_url,
            business_action_api_base_url: action_api_url,
            service_credential: credential,
            mcp_command: std::env::var("BUSINESS_READ_MCP_COMMAND")
                .unwrap_or_else(|_| "business-read-mcp".into()),
            adapter,
            tool_timeout_seconds,
            turn_timeout_seconds,
            max_payload_bytes,
            default_limit,
            max_limit,
            draft_write_enabled,
            chat_approval_enabled,
            client,
        }))
    }

    pub(crate) async fn authorize_turn(
        &self,
        source_event: &Event,
        source_channel_id: Uuid,
        agent_id: &str,
        agent_turn_id: &str,
        turn_trace_id: &str,
    ) -> Result<BusinessTurnAccess, String> {
        let trace_id = Uuid::parse_str(turn_trace_id)
            .map_err(|_| "Business Agent turn trace ID is invalid")?;
        let url = self
            .gateway_base_url
            .join("internal/agent-delegations")
            .map_err(|_| "Business Agent gateway URL is invalid")?;
        let mut requested_scopes = AGENT_SCOPES
            .iter()
            .copied()
            .filter(|scope| {
                self.draft_write_enabled
                    || !(scope.ends_with(":create") || scope.ends_with(":update_draft"))
            })
            .collect::<Vec<_>>();
        if self.chat_approval_enabled {
            if let Some(scope) = chat_approval_scope(&source_event.content) {
                requested_scopes.push(scope);
            }
        }
        let response = self
            .client
            .post(url)
            .header("x-business-service-credential", &self.service_credential)
            .header("x-trace-id", trace_id.to_string())
            .json(&serde_json::json!({
                "sourceEvent": source_event,
                "sourceBuzzEventId": source_event.id.to_hex(),
                "sourceBuzzPubkey": source_event.pubkey.to_hex(),
                "sourceChannelId": source_channel_id.to_string(),
                "agentId": agent_id,
                "agentTurnId": agent_turn_id,
                "scopes": requested_scopes,
            }))
            .send()
            .await
            .map_err(|_| "Business Agent authorization gateway is unavailable")?;
        if !response.status().is_success() {
            return Err(match response.status().as_u16() {
                409 => "This Buzz event has already started a Business Agent turn".into(),
                429 => "Business Agent query rate limit exceeded".into(),
                _ => "Business Agent turn was not authorized for this user or device".into(),
            });
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| "Business Agent authorization response could not be read")?;
        if bytes.len() > 64 * 1024 {
            return Err("Business Agent authorization response was too large".into());
        }
        let issued: IssueResponse = serde_json::from_slice(&bytes)
            .map_err(|_| "Business Agent authorization response was invalid")?;
        if issued.trace_id != trace_id
            || issued.audience != "business-read-mcp"
            || issued.token.len() != 43
            || issued.scopes.is_empty()
            || issued.scopes.len() > requested_scopes.len()
            || issued
                .scopes
                .iter()
                .any(|scope| !requested_scopes.contains(&scope.as_str()))
            || issued
                .scopes
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != issued.scopes.len()
        {
            return Err("Business Agent delegation context mismatch".into());
        }
        let mut mcp_env = vec![
            env("BUSINESS_AGENT_DELEGATION_TOKEN", issued.token),
            env(
                "BUSINESS_AGENT_APPROVAL_SCOPE",
                issued
                    .scopes
                    .iter()
                    .find(|scope| scope.ends_with(":approve"))
                    .map(String::as_str)
                    .unwrap_or(""),
            ),
            env("BUSINESS_AGENT_ID", agent_id),
            env("BUSINESS_AGENT_TURN_ID", agent_turn_id),
            env("BUSINESS_AGENT_TRACE_ID", trace_id.to_string()),
            env(
                "BUSINESS_AUTH_GATEWAY_BASE_URL",
                self.gateway_base_url.as_str(),
            ),
            env("BUSINESS_READ_SERVICE_CREDENTIAL", &self.service_credential),
            env("BUSINESS_READ_SERVICE_AUTH_MODE", "shared_secret"),
            env("BUSINESS_READ_SERVICE_AUDIENCE", "business-read-api"),
            env("BUSINESS_ANOMALY_ENABLED", "true"),
            env(
                "BUSINESS_ACTION_ENABLED",
                self.business_action_api_base_url.is_some().to_string(),
            ),
            env(
                "BUSINESS_AGENT_DRAFT_WRITE_ENABLED",
                self.draft_write_enabled.to_string(),
            ),
            env(
                "BUSINESS_CHAT_APPROVAL_ENABLED",
                self.chat_approval_enabled.to_string(),
            ),
            env("BUSINESS_READ_ADAPTER", &self.adapter),
            env(
                "BUSINESS_TOOL_TIMEOUT_SECONDS",
                self.tool_timeout_seconds.to_string(),
            ),
            env(
                "BUSINESS_TOOL_MAX_PAYLOAD_BYTES",
                self.max_payload_bytes.to_string(),
            ),
            env(
                "BUSINESS_TOOL_DEFAULT_LIMIT",
                self.default_limit.to_string(),
            ),
            env("BUSINESS_TOOL_MAX_LIMIT", self.max_limit.to_string()),
        ];
        if let Some(url) = &self.business_api_base_url {
            mcp_env.push(env("BUSINESS_READ_API_BASE_URL", url.as_str()));
        } else {
            mcp_env.push(env(
                "BUSINESS_READ_MOCK_ACKNOWLEDGE",
                "Mock Only - Production Disabled",
            ));
        }
        if let Some(url) = &self.business_action_api_base_url {
            mcp_env.push(env("BUSINESS_ACTION_API_BASE_URL", url.as_str()));
        }
        Ok(BusinessTurnAccess {
            mcp_server: McpServer {
                name: "business-read-mcp".into(),
                command: self.mcp_command.clone(),
                args: Vec::new(),
                env: mcp_env,
            },
            policy: self.turn_policy(),
            _revocation: RevocationGuard {
                config: Arc::new(self.clone()),
                delegation_id: issued.id,
                trace_id,
                revoked: AtomicBool::new(false),
            },
        })
    }
}

fn env(name: impl Into<String>, value: impl Into<String>) -> EnvVar {
    EnvVar {
        name: name.into(),
        value: value.into(),
    }
}

fn bounded_number(name: &str, default: u64, min: u64, max: u64) -> Result<u64, String> {
    let value = std::env::var(name)
        .ok()
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
    Ok(value)
}

#[cfg(test)]
#[path = "business_agent/tests.rs"]
mod tests;
