#![forbid(unsafe_code)]

mod allocation_history;
mod config;
mod crm;
mod crm_writes;
mod financial_documents;
mod inventory_count_previews;
mod inventory_count_writes;
mod inventory_counts;
mod master_data;
mod master_writes;
mod order_hold_writes;
mod return_documents;
mod stock_documents;
mod tool_catalog;
use tool_catalog::*;
mod core_reads;
use core_reads::*;
mod analytics_results;
use analytics_results::*;
mod writes;
use writes::*;

pub use config::Config;

use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
#[cfg(test)]
use business_analytics::acceptance_scope;
use business_analytics::{
    AnalysisDomain, AuthorizationScope, BusinessAnalyticsService, BusinessDataset, RuleConfig,
};
use business_anomaly_contracts::{
    AnomalyFilterInput, BusinessAnomaly, CrossDomainRiskInput, GetAnomalyInput, InventoryRiskInput,
    ProfitChangeInput, ProfitRiskInput, PurchaseRiskInput, ReceivableRiskInput,
    ValidateAnomalyInput,
};
use business_iam::{DataScope, EffectiveGrant};
use business_query_contracts::{
    BusinessToolResult, BusinessToolStatus, Evidence, Pagination, ResourceRef, ScopeSummary,
};
use chrono::DateTime;
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, str::FromStr, time::Duration};
use subtle::ConstantTimeEq;
use url::Url;
use uuid::Uuid;

#[derive(Clone)]
struct ApiState {
    credential_hash: [u8; 32],
    service_audience: String,
    analytics: BusinessAnalyticsService,
    verifier: DelegationVerifier,
    max_findings: usize,
    max_payload_bytes: usize,
    core: Option<CoreClient>,
    draft_write_enabled: bool,
    chat_approval_enabled: bool,
}

#[derive(Clone)]
struct CoreClient {
    client: reqwest::Client,
    base_url: Url,
    credential: String,
}

struct RouterRuntime {
    rule_config: RuleConfig,
    max_findings: usize,
    max_payload_bytes: usize,
    core: Option<CoreClient>,
    draft_write_enabled: bool,
    chat_approval_enabled: bool,
}

#[derive(Clone)]
enum DelegationVerifier {
    Gateway {
        client: reqwest::Client,
        url: Url,
        credential: String,
    },
    #[cfg(test)]
    AcceptanceTest,
}

#[derive(Debug, Clone)]
struct RequestContext {
    enterprise_user_id: Uuid,
    identity_binding_id: Uuid,
    delegation_id: Uuid,
    agent_id: String,
    agent_turn_id: String,
    trace_id: Uuid,
    used_calls: i32,
    required_scope: String,
    source_buzz_event_id: String,
    source_channel_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VerifyRequest<'a> {
    delegation_id: Uuid,
    enterprise_user_id: Uuid,
    identity_binding_id: Uuid,
    agent_id: &'a str,
    agent_turn_id: &'a str,
    trace_id: Uuid,
    used_calls: i32,
    required_scope: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    approval: Option<&'a Value>,
}

enum VerifiedAuthority {
    Iam(EffectiveGrant),
    #[cfg(test)]
    AcceptanceTest,
}

impl DelegationVerifier {
    async fn verify(&self, context: &RequestContext) -> Option<VerifiedAuthority> {
        self.verify_write(context, None).await
    }

    async fn verify_write(
        &self,
        context: &RequestContext,
        approval: Option<&Value>,
    ) -> Option<VerifiedAuthority> {
        match self {
            #[cfg(test)]
            Self::AcceptanceTest => Some(VerifiedAuthority::AcceptanceTest),
            Self::Gateway {
                client,
                url,
                credential,
            } => {
                let Ok(endpoint) = url.join("internal/agent-delegations/verify") else {
                    return None;
                };
                client
                    .post(endpoint)
                    .header("x-business-service-credential", credential)
                    .header("x-trace-id", context.trace_id.to_string())
                    .json(&VerifyRequest {
                        delegation_id: context.delegation_id,
                        enterprise_user_id: context.enterprise_user_id,
                        identity_binding_id: context.identity_binding_id,
                        agent_id: &context.agent_id,
                        agent_turn_id: &context.agent_turn_id,
                        trace_id: context.trace_id,
                        used_calls: context.used_calls,
                        required_scope: &context.required_scope,
                        approval,
                    })
                    .send()
                    .await
                    .ok()
                    .filter(|response| response.status().is_success())?
                    .json::<EffectiveGrant>()
                    .await
                    .ok()
                    .map(VerifiedAuthority::Iam)
            }
        }
    }
}

fn sha256(value: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(value.as_bytes()).into()
}

#[cfg(test)]
fn router_with_verifier(
    credential: String,
    verifier: DelegationVerifier,
) -> Result<Router, String> {
    router_with_runtime(
        credential,
        verifier,
        RouterRuntime {
            rule_config: RuleConfig::bundled().map_err(|e| e.to_string())?,
            max_findings: 100,
            max_payload_bytes: 128 * 1024,
            core: None,
            draft_write_enabled: false,
            chat_approval_enabled: false,
        },
    )
}

fn router_with_runtime(
    credential: String,
    verifier: DelegationVerifier,
    runtime: RouterRuntime,
) -> Result<Router, String> {
    let analytics = BusinessAnalyticsService::new(
        BusinessDataset::desensitized_acceptance().map_err(|e| e.to_string())?,
        runtime.rule_config,
    )
    .map_err(|e| e.to_string())?;
    let state = ApiState {
        credential_hash: sha256(&credential),
        service_audience: "business-read-api".into(),
        analytics,
        verifier,
        max_findings: runtime.max_findings,
        max_payload_bytes: runtime.max_payload_bytes,
        core: runtime.core,
        draft_write_enabled: runtime.draft_write_enabled,
        chat_approval_enabled: runtime.chat_approval_enabled,
    };
    Ok(Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/read/{tool}", post(read_tool))
        .route("/v1/write/{tool}", post(write_tool))
        .with_state(state))
}

async fn read_tool(
    State(state): State<ApiState>,
    Path(tool): Path<String>,
    request: Request<Body>,
) -> Response {
    if !READ_TOOLS.contains(&tool.as_str()) && !ANOMALY_TOOLS.contains(&tool.as_str()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let headers = request.headers();
    if !authorized_service(headers, &state.credential_hash, &state.service_audience) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(context) = parse_context(headers) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Some(verified_authority) = state.verifier.verify(&context).await else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let bytes = match axum::body::to_bytes(request.into_body(), state.max_payload_bytes).await {
        Ok(value) => value,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let input: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid_filter").into_response(),
    };
    if ANOMALY_TOOLS.contains(&tool.as_str()) && !valid_anomaly_input(&tool, &input) {
        return (StatusCode::BAD_REQUEST, "invalid_filter").into_response();
    }
    let requested = requested_scope(&input);
    let authorized_scope = match verified_authority {
        VerifiedAuthority::Iam(grant) => {
            let Some(required) = required_capability(&tool) else {
                return StatusCode::FORBIDDEN.into_response();
            };
            if context.required_scope != required {
                return StatusCode::FORBIDDEN.into_response();
            }
            let resolved = if matches!(
                tool.as_str(),
                "get_business_master_record" | "get_business_product_master_record"
            ) {
                master_writes::authorization_scope(&grant, required)
            } else {
                iam_authorization_scope(&grant, required)
            };
            let Some(scope) = resolved else {
                return StatusCode::FORBIDDEN.into_response();
            };
            scope
        }
        #[cfg(test)]
        VerifiedAuthority::AcceptanceTest => {
            let Some(scope) = acceptance_scope(context.enterprise_user_id) else {
                return StatusCode::FORBIDDEN.into_response();
            };
            scope
        }
    };
    let effective_scope = authorized_scope.intersect(&requested);
    let response = if READ_TOOLS.contains(&tool.as_str()) {
        if let Some(core) = &state.core {
            core_read_result(core, &tool, &input, &effective_scope, &context).await
        } else if tool == "get_inventory_count_approval_preview"
            || inventory_counts::handles(&tool)
            || crm::handles(&tool)
            || tool == "search_business_master_data"
            || matches!(
                tool.as_str(),
                "get_business_master_record" | "get_business_product_master_record"
            )
            || matches!(
                tool.as_str(),
                "get_customer_receipt_allocations"
                    | "get_supplier_payment_allocations"
                    | "search_customer_receipts"
                    | "search_supplier_payments"
                    | "search_receivables"
                    | "search_payables"
            )
        {
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        } else {
            legacy_read_result(
                &state.analytics,
                &tool,
                &input,
                &effective_scope,
                context.trace_id,
            )
        }
    } else if matches!(
        tool.as_str(),
        "analyze_order_profit_risks" | "analyze_cross_domain_risks"
    ) && state.core.is_some()
    {
        let Some(core) = state.core.as_ref() else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        core_profit_risk_result(core, &tool, &input, &effective_scope, &context).await
    } else if tool == "explain_profit_change" {
        if let Some(core) = state.core.as_ref() {
            core_profit_change_result(core, &input, &effective_scope, &context).await
        } else {
            anomaly_result(
                &state.analytics,
                &tool,
                &input,
                &effective_scope,
                context.trace_id,
                state.max_findings,
            )
        }
    } else {
        anomaly_result(
            &state.analytics,
            &tool,
            &input,
            &effective_scope,
            context.trace_id,
            state.max_findings,
        )
    };
    if state.verifier.verify(&context).await.is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    response
}

fn authorized_service(headers: &HeaderMap, expected: &[u8; 32], audience: &str) -> bool {
    let supplied = headers
        .get("x-business-service-credential")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let supplied_audience = headers
        .get("x-business-service-audience")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    bool::from(sha256(supplied).ct_eq(expected)) && supplied_audience == audience
}

fn parse_context(headers: &HeaderMap) -> Option<RequestContext> {
    let text = |name: &'static str| headers.get(name)?.to_str().ok().map(str::to_owned);
    let optional_text = |name: &'static str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .unwrap_or_default()
    };
    Some(RequestContext {
        enterprise_user_id: text("x-enterprise-user-id")?.parse().ok()?,
        identity_binding_id: text("x-identity-binding-id")?.parse().ok()?,
        delegation_id: text("x-agent-delegation-id")?.parse().ok()?,
        agent_id: text("x-agent-id")?,
        agent_turn_id: text("x-agent-turn-id")?,
        trace_id: text("x-trace-id")?.parse().ok()?,
        used_calls: text("x-agent-used-calls")?.parse().ok()?,
        required_scope: text("x-agent-required-scope")?,
        source_buzz_event_id: optional_text("x-source-buzz-event-id"),
        source_channel_id: optional_text("x-source-channel-id"),
    })
}

fn iam_authorization_scope(
    grant: &EffectiveGrant,
    required_capability: &str,
) -> Option<AuthorizationScope> {
    if grant.capability.as_str() != required_capability {
        return None;
    }
    let DataScope::Restricted(dimensions) = &grant.data_scope else {
        return Some(AuthorizationScope::default());
    };
    let mut scope = AuthorizationScope::default();
    for (dimension, values) in dimensions {
        if values.is_empty() {
            return None;
        }
        let target = match dimension.as_str() {
            "legal_entity" | "legal_entity_id" | "legalEntityIds" => &mut scope.legal_entity_ids,
            "warehouse" | "warehouse_id" | "warehouseIds" => &mut scope.warehouse_ids,
            "customer" | "customer_id" | "customerIds" => &mut scope.customer_ids,
            "supplier" | "supplier_id" | "supplierIds" => &mut scope.supplier_ids,
            "brand" | "brand_id" | "brandIds" => &mut scope.brand_ids,
            "business_unit" | "business_unit_id" | "businessUnitIds" => {
                &mut scope.business_unit_ids
            }
            _ => return None,
        };
        target.extend(values.iter().cloned());
    }
    Some(scope)
}

fn requested_scope(input: &Value) -> AuthorizationScope {
    let values = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    AuthorizationScope {
        legal_entity_ids: values("legalEntityIds"),
        warehouse_ids: values("warehouseIds"),
        customer_ids: values("customerIds"),
        supplier_ids: values("supplierIds"),
        brand_ids: values("brandIds"),
        business_unit_ids: values("businessUnitIds"),
    }
}

/// Run the production API. B2 reads use Business Core; the bundled dataset is
/// retained for the pre-existing anomaly acceptance path and tests.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_ansi(false).init();
    let config = Config::from_env().map_err(|e| format!("configuration error: {e}"))?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(5))
        .build()?;
    let router = router_with_runtime(
        config.service_credential.clone(),
        DelegationVerifier::Gateway {
            client,
            url: config.gateway_base_url,
            credential: config.service_credential,
        },
        RouterRuntime {
            rule_config: config.rule_config,
            max_findings: config.max_findings,
            max_payload_bytes: config.max_payload_bytes,
            core: Some(CoreClient {
                client: reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(2))
                    .timeout(Duration::from_secs(5))
                    .build()?,
                base_url: config.core_base_url,
                credential: config.core_credential,
            }),
            draft_write_enabled: config.draft_write_enabled,
            chat_approval_enabled: config.chat_approval_enabled,
        },
    )?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async move {
            let listener = tokio::net::TcpListener::bind(config.bind).await?;
            axum::serve(listener, router).await?;
            Ok::<_, Box<dyn std::error::Error>>(())
        })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod test_fixture;
