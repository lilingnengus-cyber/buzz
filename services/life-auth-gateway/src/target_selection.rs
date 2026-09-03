//! Short-lived, single-use Pacioli target-selection tickets.

use crate::Store;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row as _;
use uuid::Uuid;

/// The target type selected in Pacioli's trusted UI.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetSelectionKind {
    /// The user's bound Pacioli identity and default DM target.
    Identity,
    /// One relay-verified community channel.
    Channel,
}

impl TargetSelectionKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Channel => "channel",
        }
    }
}

/// Trusted selection facts submitted by Pacioli after its own UI selection.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IssueTargetSelectionRequest {
    /// Selection class.
    pub kind: TargetSelectionKind,
    /// LifeOS user mapped to the selected Pacioli identity.
    pub life_os_user_id: String,
    /// Host-derived community identifier.
    pub community_id: String,
    /// Relay-verified bound user public key.
    pub user_pubkey: String,
    /// Relay-verified channel identifier for channel selections.
    pub channel_id: Option<String>,
}

/// Minimal consume request sent by LifeOS.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConsumeTargetSelectionRequest {
    /// Expected target class.
    pub kind: TargetSelectionKind,
    /// Authenticated LifeOS user consuming the ticket.
    pub life_os_user_id: String,
}

/// Opaque selection result. LifeOS persists these trusted identifiers only.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetSelection {
    /// Selection class.
    pub kind: TargetSelectionKind,
    /// Single-use opaque ticket identifier.
    pub selection_id: Uuid,
    /// LifeOS user bound to the selected identity.
    pub life_os_user_id: String,
    /// Host-derived community identifier.
    pub community_id: String,
    /// Bound Pacioli identity public key.
    pub user_pubkey: String,
    /// Host-derived channel identifier, when applicable.
    pub channel_id: Option<String>,
    /// Ticket expiry.
    pub expires_at: DateTime<Utc>,
}

/// Stable target-selection failures.
#[derive(Debug, thiserror::Error)]
pub enum TargetSelectionError {
    /// Input or identity mapping is invalid.
    #[error("target selection is invalid")]
    Invalid,
    /// The ticket is absent, expired, consumed, or belongs to another user.
    #[error("target selection was rejected")]
    Rejected,
    /// Persistence is unavailable.
    #[error("target selection store unavailable")]
    Database,
}

impl Store {
    /// Issues a five-minute selection only for an active LifeOS/Pacioli identity mapping.
    pub async fn issue_target_selection(
        &self,
        request: IssueTargetSelectionRequest,
        trace_id: Uuid,
    ) -> Result<TargetSelection, TargetSelectionError> {
        validate_request(&request)?;
        let user_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT u.id FROM life_workbench_users u
             JOIN life_identity_bindings b ON b.workbench_user_id=u.id
             WHERE u.life_os_user_id=$1 AND u.status='active'
               AND b.buzz_pubkey=$2 AND b.status='active'",
        )
        .bind(&request.life_os_user_id)
        .bind(&request.user_pubkey)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TargetSelectionError::Database)?
        .ok_or(TargetSelectionError::Rejected)?;
        let selection_id = Uuid::new_v4();
        let expires_at = Utc::now() + Duration::minutes(5);
        sqlx::query(
            "INSERT INTO life_pacioli_target_selections
             (id,kind,workbench_user_id,life_os_user_id,community_id,user_pubkey,
              channel_id,expires_at,trace_id)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(selection_id)
        .bind(request.kind.as_str())
        .bind(user_id)
        .bind(&request.life_os_user_id)
        .bind(&request.community_id)
        .bind(&request.user_pubkey)
        .bind(&request.channel_id)
        .bind(expires_at)
        .bind(trace_id)
        .execute(&self.pool)
        .await
        .map_err(|_| TargetSelectionError::Database)?;
        Ok(TargetSelection {
            kind: request.kind,
            selection_id,
            life_os_user_id: request.life_os_user_id,
            community_id: request.community_id,
            user_pubkey: request.user_pubkey,
            channel_id: request.channel_id,
            expires_at,
        })
    }

    /// Atomically consumes an exact selection ticket once.
    pub async fn consume_target_selection(
        &self,
        selection_id: Uuid,
        request: ConsumeTargetSelectionRequest,
    ) -> Result<TargetSelection, TargetSelectionError> {
        let row = sqlx::query(
            "UPDATE life_pacioli_target_selections
             SET consumed_at=now()
             WHERE id=$1 AND kind=$2 AND life_os_user_id=$3
               AND consumed_at IS NULL AND expires_at>now()
               AND EXISTS (
                 SELECT 1 FROM life_workbench_users u
                 JOIN life_identity_bindings b ON b.workbench_user_id=u.id
                 WHERE u.id=life_pacioli_target_selections.workbench_user_id
                   AND u.status='active' AND b.status='active'
                   AND b.buzz_pubkey=life_pacioli_target_selections.user_pubkey
               )
             RETURNING kind,life_os_user_id,community_id,user_pubkey,channel_id,expires_at",
        )
        .bind(selection_id)
        .bind(request.kind.as_str())
        .bind(&request.life_os_user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TargetSelectionError::Database)?
        .ok_or(TargetSelectionError::Rejected)?;
        Ok(TargetSelection {
            kind: request.kind,
            selection_id,
            life_os_user_id: row.get("life_os_user_id"),
            community_id: row.get("community_id"),
            user_pubkey: row.get("user_pubkey"),
            channel_id: row.get("channel_id"),
            expires_at: row.get("expires_at"),
        })
    }
}

fn validate_request(request: &IssueTargetSelectionRequest) -> Result<(), TargetSelectionError> {
    let opaque = |value: &str, max: usize| {
        !value.is_empty()
            && value.len() <= max
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._~:-".contains(&byte))
    };
    if !opaque(&request.life_os_user_id, 512)
        || !opaque(&request.community_id, 256)
        || request.user_pubkey.len() != 64
        || !request
            .user_pubkey
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || match (request.kind, request.channel_id.as_deref()) {
            (TargetSelectionKind::Identity, None) => false,
            (TargetSelectionKind::Channel, Some(channel)) => !opaque(channel, 256),
            _ => true,
        }
    {
        return Err(TargetSelectionError::Invalid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_channel_shapes_are_strict() {
        let identity = IssueTargetSelectionRequest {
            kind: TargetSelectionKind::Identity,
            life_os_user_id: "life-user".into(),
            community_id: "community".into(),
            user_pubkey: "a".repeat(64),
            channel_id: None,
        };
        assert!(validate_request(&identity).is_ok());
        let invalid_channel = IssueTargetSelectionRequest {
            kind: TargetSelectionKind::Channel,
            ..identity
        };
        assert!(validate_request(&invalid_channel).is_err());
    }
}
