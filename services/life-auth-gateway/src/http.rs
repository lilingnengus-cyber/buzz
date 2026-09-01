use crate::{
    auth::{bearer, OidcVerifier},
    config::Config,
    identity::{IdentityError, LifeOsIdentityClient},
    model::{IdentityBindingChallengeId, IdentityBindingId},
    security::SigningKeyMaterial,
    Store,
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Clone)]
struct IdentityRuntime {
    verifier: OidcVerifier,
    resolver: LifeOsIdentityClient,
    deployment_id: String,
    challenge_ttl: Duration,
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
            }),
        })
    }
}

#[derive(Debug)]
struct ApiError(IdentityError);

impl From<IdentityError> for ApiError {
    fn from(value: IdentityError) -> Self {
        Self(value)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match self.0 {
            IdentityError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            IdentityError::Invalid => (StatusCode::BAD_REQUEST, "invalid_request"),
            IdentityError::NotMapped => (StatusCode::FORBIDDEN, "identity_not_mapped"),
            IdentityError::Inactive => (StatusCode::FORBIDDEN, "identity_inactive"),
            IdentityError::Conflict => (StatusCode::CONFLICT, "identity_conflict"),
            IdentityError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            IdentityError::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable"),
        };
        (status, Json(serde_json::json!({"error": code}))).into_response()
    }
}

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/v1/workbench/sessions", post(create_session))
        .route("/v1/identity-bindings/challenges", post(create_challenge))
        .route("/v1/identity-bindings", post(verify_binding))
        .route("/v1/identity-bindings/{binding_id}", delete(revoke_binding))
        .route("/v1/me", get(me))
        .with_state(Arc::new(state))
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
