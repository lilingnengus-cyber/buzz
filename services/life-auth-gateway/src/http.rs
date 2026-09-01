use crate::{
    agent::{AgentError, ConsumeDelegationRequest, DelegationPolicy, IssueDelegationRequest},
    auth::{bearer, OidcVerifier},
    call_grant::CallGrantSigner,
    config::Config,
    embed::{ConsumeEmbedRequest, EmbedError, EmbedPolicy, IssueEmbedRequest},
    identity::{IdentityError, LifeOsIdentityClient},
    membership::{MembershipError, MembershipEvent},
    model::{AgentDelegationId, EmbedSessionId, IdentityBindingChallengeId, IdentityBindingId},
    security::{ServiceToken, SigningKeyMaterial},
    write_confirmation::{ValidateWriteConfirmationRequest, WriteConfirmationError},
    Store,
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Clone)]
struct IdentityRuntime {
    verifier: OidcVerifier,
    resolver: LifeOsIdentityClient,
    deployment_id: String,
    challenge_ttl: Duration,
    lifeos_service_token: ServiceToken,
    pacioli_service_token: ServiceToken,
    delegation_policy: DelegationPolicy,
    call_grant_signer: CallGrantSigner,
    embed_policy: EmbedPolicy,
    lifeos_base_url: url::Url,
}

/// Runtime state shared by health and fixed Life identity endpoints.
#[derive(Clone)]
pub struct AppState {
    pub(crate) store: Store,
    pub(crate) signing_key: SigningKeyMaterial,
    identity: Option<IdentityRuntime>,
}

impl AppState {
    /// Creates health-only state, useful before identity dependencies are configured.
    pub fn new(pool: sqlx::PgPool, signing_key: SigningKeyMaterial) -> Self {
        Self {
            store: Store::new(pool),
            signing_key,
            identity: None,
        }
    }

    pub(crate) fn configured(pool: sqlx::PgPool, config: &Config) -> Result<Self, IdentityError> {
        Ok(Self {
            store: Store::new(pool),
            signing_key: config.signing_key().clone(),
            identity: Some(IdentityRuntime {
                verifier: OidcVerifier::new(
                    config.workbench_oidc_issuer(),
                    config.workbench_oidc_audience(),
                )
                .map_err(|_| IdentityError::Unavailable)?,
                resolver: LifeOsIdentityClient::new(
                    config.lifeos_base_url(),
                    config.lifeos_outbound_credential(),
                )?,
                deployment_id: config.deployment_id().to_owned(),
                challenge_ttl: config.identity_challenge_ttl(),
                lifeos_service_token: config.lifeos_service_token().clone(),
                pacioli_service_token: config.pacioli_service_token().clone(),
                delegation_policy: DelegationPolicy::new(
                    config.delegation_audience(),
                    config.deployment_id(),
                    config.delegation_ttl(),
                )
                .map_err(|_| IdentityError::Unavailable)?,
                call_grant_signer: CallGrantSigner::new(
                    config.call_grant_issuer(),
                    config.call_grant_audience(),
                    config.call_grant_ttl(),
                    config.signing_key().clone(),
                )
                .map_err(|_| IdentityError::Unavailable)?,
                embed_policy: EmbedPolicy::standard(),
                lifeos_base_url: config.lifeos_base_url().clone(),
            }),
        })
    }
}

#[derive(Debug)]
enum ApiError {
    Identity(IdentityError),
    Membership(MembershipError),
    Agent(AgentError),
    Embed(EmbedError),
    WriteConfirmation(WriteConfirmationError),
}

impl From<IdentityError> for ApiError {
    fn from(value: IdentityError) -> Self {
        Self::Identity(value)
    }
}

impl From<MembershipError> for ApiError {
    fn from(value: MembershipError) -> Self {
        Self::Membership(value)
    }
}

impl From<AgentError> for ApiError {
    fn from(value: AgentError) -> Self {
        Self::Agent(value)
    }
}

impl From<EmbedError> for ApiError {
    fn from(value: EmbedError) -> Self {
        Self::Embed(value)
    }
}

impl From<WriteConfirmationError> for ApiError {
    fn from(value: WriteConfirmationError) -> Self {
        Self::WriteConfirmation(value)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Embed(EmbedError::Invalid) => (StatusCode::BAD_REQUEST, "invalid_request"),
            Self::Embed(EmbedError::Unauthorized) => (StatusCode::UNAUTHORIZED, "embed_rejected"),
            Self::Embed(EmbedError::NotFound) => (StatusCode::NOT_FOUND, "not_found"),
            Self::Embed(EmbedError::Database) => {
                (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable")
            }
            Self::WriteConfirmation(WriteConfirmationError::Invalid) => {
                (StatusCode::BAD_REQUEST, "confirmation_invalid")
            }
            Self::WriteConfirmation(WriteConfirmationError::Unauthorized) => {
                (StatusCode::UNAUTHORIZED, "confirmation_rejected")
            }
            Self::WriteConfirmation(WriteConfirmationError::Conflict) => {
                (StatusCode::CONFLICT, "confirmation_conflict")
            }
            Self::WriteConfirmation(WriteConfirmationError::Database) => {
                (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable")
            }
            Self::Agent(AgentError::Invalid) => (StatusCode::BAD_REQUEST, "validation_failed"),
            Self::Agent(AgentError::Unauthorized) => {
                (StatusCode::UNAUTHORIZED, "delegation_rejected")
            }
            Self::Agent(AgentError::Denied) => (StatusCode::FORBIDDEN, "scope_denied"),
            Self::Agent(AgentError::Conflict) => (StatusCode::CONFLICT, "delegation_conflict"),
            Self::Agent(AgentError::Database | AgentError::Signing) => {
                (StatusCode::SERVICE_UNAVAILABLE, "gateway_unavailable")
            }
            Self::Membership(MembershipError::Invalid) => {
                (StatusCode::BAD_REQUEST, "invalid_request")
            }
            Self::Membership(MembershipError::NotFound) => (StatusCode::NOT_FOUND, "not_found"),
            Self::Membership(MembershipError::Database) => {
                (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable")
            }
            Self::Identity(error) => match error {
                IdentityError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
                IdentityError::Invalid => (StatusCode::BAD_REQUEST, "invalid_request"),
                IdentityError::NotMapped => (StatusCode::FORBIDDEN, "identity_not_mapped"),
                IdentityError::Inactive => (StatusCode::FORBIDDEN, "identity_inactive"),
                IdentityError::Conflict => (StatusCode::CONFLICT, "identity_conflict"),
                IdentityError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
                IdentityError::Unavailable => {
                    (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable")
                }
            },
        };
        (status, Json(serde_json::json!({"error": code}))).into_response()
    }
}

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/v1/workbench/sessions", post(create_session))
        .route("/v1/workbench/membership-events", post(membership_event))
        .route("/v1/embed-sessions", post(issue_embed_session))
        .route("/v1/embed-sessions/consume", post(consume_embed_session))
        .route(
            "/v1/embed-sessions/{embed_session_id}/revoke",
            post(revoke_embed_session),
        )
        .route(
            "/v1/write-confirmations/validate",
            post(validate_write_confirmation),
        )
        .route("/v1/life-agent/delegations", post(issue_delegation))
        .route(
            "/v1/life-agent/delegations/consume",
            post(consume_delegation),
        )
        .route(
            "/v1/life-agent/delegations/{delegation_id}/revoke",
            post(revoke_delegation),
        )
        .route("/v1/identity-bindings/challenges", post(create_challenge))
        .route("/v1/identity-bindings", post(verify_binding))
        .route("/v1/identity-bindings/{binding_id}", delete(revoke_binding))
        .route("/v1/me", get(me))
        .with_state(Arc::new(state))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IssueEmbedResponse {
    embed_session_id: EmbedSessionId,
    embed_url: String,
    expires_at: chrono::DateTime<chrono::Utc>,
    trace_id: Uuid,
}

async fn issue_embed_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<IssueEmbedRequest>,
) -> Result<Json<IssueEmbedResponse>, ApiError> {
    let runtime = runtime(&state)?;
    let principal = session(&state, &headers, runtime).await?;
    let issued = state
        .store
        .issue_embed_code(
            &principal,
            request,
            &runtime.embed_policy,
            trace_id(&headers),
        )
        .await?;
    let mut embed_url = runtime
        .lifeos_base_url
        .join("embed/bootstrap")
        .map_err(|_| IdentityError::Unavailable)?;
    embed_url
        .query_pairs_mut()
        .append_pair("code", &issued.code);
    Ok(Json(IssueEmbedResponse {
        embed_session_id: issued.embed_session_id,
        embed_url: embed_url.into(),
        expires_at: issued.expires_at,
        trace_id: issued.trace_id,
    }))
}

async fn consume_embed_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<ConsumeEmbedRequest>,
) -> Result<Json<crate::embed::ConsumedEmbedSession>, ApiError> {
    let runtime = runtime(&state)?;
    require_service(&headers, &runtime.lifeos_service_token)?;
    Ok(Json(
        state
            .store
            .consume_embed_code(
                &request.code,
                &runtime.deployment_id,
                &runtime.embed_policy,
                trace_id(&headers),
            )
            .await?,
    ))
}

async fn revoke_embed_session(
    State(state): State<Arc<AppState>>,
    Path(embed_session_id): Path<EmbedSessionId>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let runtime = runtime(&state)?;
    let principal = session(&state, &headers, runtime).await?;
    state
        .store
        .revoke_embed_session(&principal, embed_session_id, trace_id(&headers))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn validate_write_confirmation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<ValidateWriteConfirmationRequest>,
) -> Result<Json<crate::write_confirmation::ValidatedWriteConfirmation>, ApiError> {
    let runtime = runtime(&state)?;
    require_service(&headers, &runtime.pacioli_service_token)?;
    Ok(Json(
        state
            .store
            .validate_write_confirmation(request, &runtime.deployment_id, Duration::from_secs(600))
            .await?,
    ))
}

async fn issue_delegation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<IssueDelegationRequest>,
) -> Result<Json<crate::agent::IssueDelegationResponse>, ApiError> {
    let runtime = runtime(&state)?;
    require_service(&headers, &runtime.pacioli_service_token)?;
    let (user_id, issuer, subject, _) = state
        .store
        .delegation_identity(&request.source_event.pubkey.to_hex())
        .await?;
    let resolved = match runtime.resolver.resolve(&issuer, &subject).await {
        Ok(resolved) => resolved,
        Err(error) => {
            state
                .store
                .mark_membership_sync_failed(user_id, request.trace_id)
                .await?;
            return Err(error.into());
        }
    };
    Ok(Json(
        state
            .store
            .issue_agent_delegation(request, &runtime.delegation_policy, &resolved)
            .await?,
    ))
}

async fn consume_delegation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<ConsumeDelegationRequest>,
) -> Result<Json<crate::call_grant::SignedLifeCallGrant>, ApiError> {
    let runtime = runtime(&state)?;
    let token = bearer(&headers).ok_or(AgentError::Unauthorized)?;
    Ok(Json(
        state
            .store
            .consume_agent_delegation(token, request, &runtime.call_grant_signer)
            .await?,
    ))
}

async fn revoke_delegation(
    State(state): State<Arc<AppState>>,
    Path(delegation_id): Path<AgentDelegationId>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let runtime = runtime(&state)?;
    require_service(&headers, &runtime.pacioli_service_token)?;
    let revoked = state
        .store
        .revoke_agent_delegation(delegation_id)
        .await
        .map_err(|_| AgentError::Database)?;
    if !revoked {
        return Err(IdentityError::NotFound.into());
    }
    Ok(StatusCode::NO_CONTENT)
}

fn require_service(headers: &HeaderMap, expected: &ServiceToken) -> Result<(), ApiError> {
    let presented = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Service "))
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .ok_or(IdentityError::Unauthorized)?;
    if !expected.matches(presented) {
        return Err(IdentityError::Unauthorized.into());
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MembershipEventResponse {
    applied: bool,
    membership_version: i64,
}

async fn membership_event(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(event): Json<MembershipEvent>,
) -> Result<Json<MembershipEventResponse>, ApiError> {
    let runtime = runtime(&state)?;
    let presented = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Service "))
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .ok_or(IdentityError::Unauthorized)?;
    if !runtime.lifeos_service_token.matches(presented) {
        return Err(IdentityError::Unauthorized.into());
    }
    let applied = state.store.apply_membership_event(&event).await?;
    Ok(Json(MembershipEventResponse {
        applied,
        membership_version: event.membership_version,
    }))
}

async fn live() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn ready(State(state): State<Arc<AppState>>) -> StatusCode {
    if !state.signing_key.ready() {
        return StatusCode::SERVICE_UNAVAILABLE;
    }
    match state.store.ready().await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateSessionRequest {
    nonce: String,
}

async fn create_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<crate::identity::IssuedSession>, ApiError> {
    let runtime = runtime(&state)?;
    let oidc_token = bearer(&headers).ok_or(IdentityError::Unauthorized)?;
    let oidc = runtime
        .verifier
        .verify(oidc_token, &request.nonce)
        .await
        .map_err(|_| IdentityError::Unauthorized)?;
    let resolved = runtime
        .resolver
        .resolve(&oidc.issuer, &oidc.subject)
        .await?;
    Ok(Json(
        state
            .store
            .create_workbench_session(&oidc, &resolved, &runtime.deployment_id, trace_id(&headers))
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChallengeRequest {
    pubkey: String,
}

async fn create_challenge(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<ChallengeRequest>,
) -> Result<Json<crate::identity::BindingChallenge>, ApiError> {
    let runtime = runtime(&state)?;
    let principal = session(&state, &headers, runtime).await?;
    Ok(Json(
        state
            .store
            .create_identity_binding_challenge(
                &principal,
                &request.pubkey,
                chrono::Duration::from_std(runtime.challenge_ttl)
                    .map_err(|_| IdentityError::Unavailable)?,
                trace_id(&headers),
            )
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerifyBindingRequest {
    challenge_id: IdentityBindingChallengeId,
    signed_event: nostr::Event,
}

async fn verify_binding(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<VerifyBindingRequest>,
) -> Result<Json<crate::identity::IdentityBinding>, ApiError> {
    let runtime = runtime(&state)?;
    let principal = session(&state, &headers, runtime).await?;
    Ok(Json(
        state
            .store
            .verify_identity_binding(
                &principal,
                request.challenge_id,
                request.signed_event,
                trace_id(&headers),
            )
            .await?,
    ))
}

async fn revoke_binding(
    State(state): State<Arc<AppState>>,
    Path(binding_id): Path<IdentityBindingId>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let runtime = runtime(&state)?;
    let principal = session(&state, &headers, runtime).await?;
    state
        .store
        .revoke_identity_binding(&principal, binding_id, trace_id(&headers))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<crate::identity::MeResponse>, ApiError> {
    let runtime = runtime(&state)?;
    let principal = session(&state, &headers, runtime).await?;
    Ok(Json(state.store.me(&principal).await?))
}

fn runtime(state: &AppState) -> Result<&IdentityRuntime, ApiError> {
    state
        .identity
        .as_ref()
        .ok_or_else(|| IdentityError::Unavailable.into())
}

async fn session(
    state: &AppState,
    headers: &HeaderMap,
    runtime: &IdentityRuntime,
) -> Result<crate::identity::SessionPrincipal, ApiError> {
    let token = bearer(headers).ok_or(IdentityError::Unauthorized)?;
    state
        .store
        .authenticate_workbench_session(token, &runtime.deployment_id)
        .await
        .map_err(Into::into)
}

fn trace_id(headers: &HeaderMap) -> Uuid {
    headers
        .get("x-trace-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Uuid::parse_str(value).ok())
        .unwrap_or_else(Uuid::new_v4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn pacioli_service_authorization_is_exact() {
        let raw = "p".repeat(32);
        let expected = ServiceToken::parse("TEST_TOKEN", raw.clone()).expect("service token");
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!("Service {raw}")).expect("header"),
        );
        assert!(require_service(&headers, &expected).is_ok());

        for invalid in [format!("Bearer {raw}"), format!("Service  {raw}"), raw] {
            headers.insert(
                "authorization",
                HeaderValue::from_str(&invalid).expect("invalid test header"),
            );
            assert!(require_service(&headers, &expected).is_err());
        }
    }
}
