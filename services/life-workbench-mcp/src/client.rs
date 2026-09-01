use crate::{
    config::{Config, MAX_RESPONSE_BYTES},
    tools::{parse_invocation, Invocation, ResourceContext},
};
use life_workbench_contracts::result::WorkbenchResult;
use reqwest::StatusCode;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;

const SERVICE_HEADER: &str = "x-life-workbench-service-token";
const CONSUME_PATH: &str = "/v1/life-agent/delegations/consume";

pub struct LifeClient {
    config: Config,
    http: reqwest::Client,
}

impl LifeClient {
    pub fn new(config: Config) -> Result<Self, ClientError> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(crate::config::HTTP_TIMEOUT)
            .build()
            .map_err(|_| ClientError::Internal)?;
        Ok(Self { config, http })
    }

    pub async fn invoke(&self, tool: &str, arguments: Value) -> Result<String, ClientError> {
        let invocation = parse_invocation(tool, arguments).map_err(|_| ClientError::Validation)?;
        self.invoke_prepared(&invocation).await
    }

    pub async fn invoke_safe(&self, tool: &str, arguments: Value) -> String {
        match self.invoke(tool, arguments).await {
            Ok(result) => result,
            Err(error) => error.safe_result(self.config.trace_id),
        }
    }

    async fn invoke_prepared(&self, invocation: &Invocation) -> Result<String, ClientError> {
        let idempotency_key = deterministic_idempotency_key(&self.config.agent_turn_id, invocation);
        let consume = ConsumeRequest {
            agent_id: &self.config.agent_id,
            agent_turn_id: &self.config.agent_turn_id,
            tool: invocation.tool,
            capability: invocation.capability,
            resource: &invocation.resource,
            normalized_input_hash: &invocation.normalized_input_hash,
            idempotency_key,
            trace_id: self.config.trace_id,
        };
        let gateway_url = self
            .config
            .gateway_base_url
            .join(CONSUME_PATH.trim_start_matches('/'))
            .map_err(|_| ClientError::Internal)?;
        let gateway_response = self
            .http
            .post(gateway_url)
            .bearer_auth(self.config.delegation_token.expose())
            .json(&consume)
            .send()
            .await
            .map_err(|_| ClientError::GatewayUnavailable)?;
        if !gateway_response.status().is_success() {
            return Err(map_gateway_status(gateway_response.status()));
        }
        let grant: SignedGrant = bounded_json(gateway_response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|_| ClientError::GatewayUnavailable)?;
        grant.validate(&consume)?;

        let envelope = WorkbenchEnvelope {
            input: &invocation.api_input,
            resource: &invocation.resource,
            idempotency_key,
            trace_id: self.config.trace_id,
        };
        let api_url = self
            .config
            .life_api_base_url
            .join(invocation.route.trim_start_matches('/'))
            .map_err(|_| ClientError::Internal)?;
        let attempts = if invocation.is_write { 1 } else { 2 };
        let mut last_error = if invocation.is_write {
            ClientError::WriteOutcomeUnknown
        } else {
            ClientError::LifeApiUnavailable
        };
        for attempt in 0..attempts {
            let response = self
                .http
                .post(api_url.clone())
                .bearer_auth(grant.token.expose())
                .header(SERVICE_HEADER, self.config.service_token.expose())
                .json(&envelope)
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(_) if !invocation.is_write && attempt == 0 => continue,
                Err(_) if invocation.is_write => return Err(ClientError::WriteOutcomeUnknown),
                Err(_) => return Err(ClientError::LifeApiUnavailable),
            };
            if is_temporary(response.status()) && invocation.is_write {
                return Err(ClientError::WriteOutcomeUnknown);
            }
            if is_temporary(response.status()) && attempt == 0 {
                last_error = ClientError::LifeApiUnavailable;
                continue;
            }
            let result: WorkbenchResult<Value> = bounded_json(response, MAX_RESPONSE_BYTES)
                .await
                .map_err(|_| ClientError::InvalidResponse)?;
            let trace_id = match &result {
                WorkbenchResult::Success(success) => success.trace_id,
                WorkbenchResult::Failure(failure) => failure.trace_id,
            };
            if trace_id != self.config.trace_id {
                return Err(ClientError::InvalidResponse);
            }
            let serialized = serde_json::to_string(&result).map_err(|_| ClientError::Internal)?;
            if serialized.len() > MAX_RESPONSE_BYTES {
                return Err(ClientError::InvalidResponse);
            }
            return Ok(serialized);
        }
        Err(last_error)
    }
}

fn deterministic_idempotency_key(agent_turn_id: &str, invocation: &Invocation) -> Uuid {
    let mut hasher = Sha256::new();
    for value in [
        agent_turn_id,
        invocation.tool,
        &invocation.resource.resource_type,
        &invocation.resource.id,
        &invocation.normalized_input_hash,
    ] {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    }
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConsumeRequest<'a> {
    agent_id: &'a str,
    agent_turn_id: &'a str,
    tool: &'a str,
    capability: &'a str,
    resource: &'a ResourceContext,
    normalized_input_hash: &'a str,
    idempotency_key: Uuid,
    trace_id: Uuid,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchEnvelope<'a> {
    input: &'a Value,
    resource: &'a ResourceContext,
    idempotency_key: Uuid,
    trace_id: Uuid,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignedGrant {
    token: crate::config::SecretString,
    claims: GrantClaims,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrantClaims {
    capability: String,
    resource_type: String,
    resource_id: String,
    expected_version: Option<i64>,
    normalized_input_hash: String,
    idempotency_key: String,
    trace_id: Uuid,
}

impl SignedGrant {
    fn validate(&self, request: &ConsumeRequest<'_>) -> Result<(), ClientError> {
        if self.token.expose().is_empty()
            || self.token.expose().len() > 16_384
            || self.claims.capability != request.capability
            || self.claims.resource_type != request.resource.resource_type
            || self.claims.resource_id != request.resource.id
            || self.claims.expected_version != request.resource.expected_version
            || self.claims.normalized_input_hash != request.normalized_input_hash
            || self.claims.idempotency_key != request.idempotency_key.to_string()
            || self.claims.trace_id != request.trace_id
        {
            return Err(ClientError::GatewayUnavailable);
        }
        Ok(())
    }
}

async fn bounded_json<T: DeserializeOwned>(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<T, ()> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(());
    }
    let body = response.bytes().await.map_err(|_| ())?;
    if body.len() > max_bytes {
        return Err(());
    }
    serde_json::from_slice(&body).map_err(|_| ())
}

fn map_gateway_status(status: StatusCode) -> ClientError {
    match status {
        StatusCode::TOO_MANY_REQUESTS => ClientError::RateLimited,
        status if status.is_server_error() => ClientError::GatewayUnavailable,
        _ => ClientError::ScopeDenied,
    }
}

fn is_temporary(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT
    )
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum ClientError {
    #[error("Life tool input is invalid")]
    Validation,
    #[error("Life delegation was denied")]
    ScopeDenied,
    #[error("Life delegation rate limit reached")]
    RateLimited,
    #[error("Life authorization gateway is unavailable")]
    GatewayUnavailable,
    #[error("LifeOS Workbench API is unavailable")]
    LifeApiUnavailable,
    #[error("LifeOS write outcome is unknown")]
    WriteOutcomeUnknown,
    #[error("LifeOS returned an invalid response")]
    InvalidResponse,
    #[error("Life Workbench internal error")]
    Internal,
}

impl ClientError {
    pub fn safe_result(self, trace_id: Uuid) -> String {
        let (code, message, retryable) = match self {
            Self::Validation => ("validation_failed", "Life tool input is invalid", false),
            Self::ScopeDenied => ("scope_denied", "Life access was denied", false),
            Self::RateLimited => ("rate_limited", "Life call budget is exhausted", false),
            Self::GatewayUnavailable => (
                "gateway_unavailable",
                "Life authorization is temporarily unavailable",
                true,
            ),
            Self::LifeApiUnavailable => (
                "life_api_unavailable",
                "LifeOS is temporarily unavailable",
                true,
            ),
            Self::WriteOutcomeUnknown => (
                "write_outcome_unknown",
                "LifeOS write outcome is unknown; do not retry blindly",
                false,
            ),
            Self::InvalidResponse | Self::Internal => (
                "internal_error",
                "Life Workbench could not complete the call",
                false,
            ),
        };
        json!({
            "ok": false,
            "error": {"code": code, "message": message, "retryable": retryable},
            "traceId": trace_id
        })
        .to_string()
    }
}
