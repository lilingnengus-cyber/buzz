//! Ed25519-signed, short-lived authorization for exactly one LifeOS API call.

use crate::{
    agent::{RequestedDataScope, ResourceContext},
    security::SigningKeyMaterial,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use life_iam::DataScope;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;

/// Fixed claims bound to one consumed delegation call.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifeCallGrantClaims {
    /// Configured Gateway issuer.
    pub iss: String,
    /// Fixed LifeOS Workbench API audience.
    pub aud: String,
    /// Issued-at Unix timestamp.
    pub iat: i64,
    /// Expiration Unix timestamp, no more than 60 seconds after issue.
    pub exp: i64,
    /// Consumed delegation identifier.
    pub delegation_id: Uuid,
    /// Canonical opaque LifeOS user identifier resolved by the Gateway.
    pub life_os_user_id: String,
    /// Unique call identifier persisted before signing.
    pub call_id: Uuid,
    /// Exact authorized capability.
    pub capability: String,
    /// Effective data scope carried to LifeOS for final enforcement.
    pub data_scope: RequestedDataScope,
    /// Exact resource type.
    pub resource_type: String,
    /// Exact opaque resource identifier.
    pub resource_id: String,
    /// Optimistic version required for mutation capabilities.
    pub expected_version: Option<i64>,
    /// Exact confirmed preview digest for a high-risk WriteCommand.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_hash: Option<String>,
    /// SHA-256 digest of normalized tool input.
    pub normalized_input_hash: String,
    /// Caller idempotency key bound to this payload.
    pub idempotency_key: String,
    /// Distributed trace identifier.
    pub trace_id: Uuid,
}

/// Compact JWS and the exact claims used to create it.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedLifeCallGrant {
    /// EdDSA compact JWS; it must never be logged.
    pub token: String,
    /// Non-secret claims supplied for explicit downstream handling.
    pub claims: LifeCallGrantClaims,
}

/// Validated Ed25519 LifeCallGrant issuer.
#[derive(Clone)]
pub struct CallGrantSigner {
    issuer: String,
    audience: String,
    ttl: Duration,
    key: SigningKeyMaterial,
    key_id: String,
}

pub(crate) struct CallGrantInput<'a> {
    pub delegation_id: Uuid,
    pub life_os_user_id: &'a str,
    pub call_id: Uuid,
    pub capability: &'a str,
    pub data_scope: DataScope,
    pub resource: &'a ResourceContext,
    pub normalized_input_hash: &'a str,
    pub idempotency_key: &'a str,
    pub trace_id: Uuid,
}

/// Stable call-grant construction failure.
#[derive(Debug, thiserror::Error)]
pub enum CallGrantError {
    /// Issuer, audience, TTL, or claims were invalid.
    #[error("Life call grant is invalid")]
    Invalid,
    /// Claims could not be encoded.
    #[error("Life call grant could not be encoded")]
    Encoding,
}

impl From<CallGrantError> for crate::agent::AgentError {
    fn from(_: CallGrantError) -> Self {
        Self::Signing
    }
}

impl CallGrantSigner {
    /// Builds a signer with a fixed issuer/audience and a TTL from 1 through 60 seconds.
    pub fn new(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        ttl: Duration,
        key: SigningKeyMaterial,
    ) -> Result<Self, CallGrantError> {
        let issuer = issuer.into();
        let audience = audience.into();
        if !safe_identifier(&issuer)
            || audience != "lifeos-workbench-api"
            || !(1..=60).contains(&ttl.as_secs())
        {
            return Err(CallGrantError::Invalid);
        }
        let key_id = hex::encode(&Sha256::digest(key.verifying_key_bytes())[..8]);
        Ok(Self {
            issuer,
            audience,
            ttl,
            key,
            key_id,
        })
    }

    pub(crate) fn issue(
        &self,
        input: CallGrantInput<'_>,
    ) -> Result<SignedLifeCallGrant, CallGrantError> {
        if !safe_identifier(input.life_os_user_id) {
            return Err(CallGrantError::Invalid);
        }
        let issued_at = Utc::now().timestamp();
        let ttl = i64::try_from(self.ttl.as_secs()).map_err(|_| CallGrantError::Invalid)?;
        let claims = LifeCallGrantClaims {
            iss: self.issuer.clone(),
            aud: self.audience.clone(),
            iat: issued_at,
            exp: issued_at + ttl,
            delegation_id: input.delegation_id,
            life_os_user_id: input.life_os_user_id.to_owned(),
            call_id: input.call_id,
            capability: input.capability.to_owned(),
            data_scope: RequestedDataScope::from_data_scope(&input.data_scope),
            resource_type: input.resource.resource_type.clone(),
            resource_id: input.resource.id.clone(),
            expected_version: input.resource.expected_version,
            preview_hash: input.resource.preview_hash.clone(),
            normalized_input_hash: input.normalized_input_hash.to_owned(),
            idempotency_key: input.idempotency_key.to_owned(),
            trace_id: input.trace_id,
        };
        let header = serde_json::json!({"alg":"EdDSA","typ":"JWT","kid":self.key_id});
        let header = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&header).map_err(|_| CallGrantError::Encoding)?);
        let body = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&claims).map_err(|_| CallGrantError::Encoding)?);
        let signing_input = format!("{header}.{body}");
        let signature = URL_SAFE_NO_PAD.encode(self.key.sign(signing_input.as_bytes()));
        Ok(SignedLifeCallGrant {
            token: format!("{signing_input}.{signature}"),
            claims,
        })
    }

    /// Returns the verification-key bytes used by downstream contract tests and key publication.
    pub fn verifying_key_bytes(&self) -> [u8; 32] {
        self.key.verifying_key_bytes()
    }
}

fn safe_identifier(value: &str) -> bool {
    (1..=256).contains(&value.len())
        && value.trim() == value
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}
