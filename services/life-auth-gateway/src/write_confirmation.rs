//! Exact signed confirmation parsing, persistence, and one-time command binding.

use crate::{
    model::{LifeWorkbenchUserId, WorkbenchSessionId, WriteCommandConfirmationId},
    Store,
};
use chrono::{TimeZone as _, Utc};
use nostr::Event;
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Row, Transaction};
use std::time::Duration;
use uuid::Uuid;

const MAX_CONFIRMATION_SECONDS: u64 = 600;
const FUTURE_SKEW_SECONDS: u64 = 30;

/// Canonical fields extracted from an exact `/confirm life-write ...` message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactWriteConfirmation {
    /// Immutable LifeOS WriteCommand identifier.
    pub command_id: Uuid,
    /// Exact optimistic resource version shown in the preview.
    pub expected_version: i64,
    /// Exact 64-character lower-case hexadecimal preview digest.
    pub preview_hash: String,
}

/// Trusted Pacioli request containing both the signed message and expected command fields.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidateWriteConfirmationRequest {
    /// Complete newly signed Nostr message containing only the canonical command.
    pub signed_event: Event,
    /// LifeOS-persisted immutable command identifier.
    pub command_id: Uuid,
    /// LifeOS-persisted optimistic version.
    pub expected_version: i64,
    /// LifeOS-persisted preview digest.
    pub preview_hash: String,
    /// Distributed trace identifier.
    pub trace_id: Uuid,
}

/// Active one-time confirmation grant bound to user, session, event, and command.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedWriteConfirmation {
    /// Gateway confirmation record identifier.
    pub confirmation_id: WriteCommandConfirmationId,
    /// Immutable LifeOS command identifier.
    pub command_id: Uuid,
    /// Bound Life Workbench user.
    pub user_id: LifeWorkbenchUserId,
    /// Bound Workbench Session.
    pub workbench_session_id: WorkbenchSessionId,
    /// Confirmation expiry, at most ten minutes after the signed message.
    pub expires_at: chrono::DateTime<Utc>,
    /// Distributed trace identifier.
    pub trace_id: Uuid,
}

/// Exact command evidence returned when the confirmation is atomically consumed.
#[derive(Clone, Debug)]
pub struct ConsumedWriteConfirmation {
    /// Gateway confirmation record identifier.
    pub confirmation_id: WriteCommandConfirmationId,
    /// Canonical lower-case preview digest.
    pub preview_hash: String,
}

/// Stable fail-closed write-confirmation failures.
#[derive(Debug, thiserror::Error)]
pub enum WriteConfirmationError {
    /// Command syntax, signature, kind, time, hash, or TTL was invalid.
    #[error("Life write confirmation is invalid")]
    Invalid,
    /// The signing identity or bound session is not currently authorized.
    #[error("Life write confirmation is unauthorized")]
    Unauthorized,
    /// The source command was already confirmed or consumed.
    #[error("Life write confirmation conflicts with existing state")]
    Conflict,
    /// PostgreSQL could not complete the atomic transition.
    #[error("Life write confirmation store unavailable")]
    Database,
}

impl From<WriteConfirmationError> for crate::agent::AgentError {
    fn from(value: WriteConfirmationError) -> Self {
        match value {
            WriteConfirmationError::Invalid => Self::Invalid,
            WriteConfirmationError::Unauthorized => Self::Unauthorized,
            WriteConfirmationError::Conflict => Self::Conflict,
            WriteConfirmationError::Database => Self::Database,
        }
    }
}

/// Parses only the canonical four-token confirmation command.
pub fn parse_exact_confirmation(
    value: &str,
) -> Result<ExactWriteConfirmation, WriteConfirmationError> {
    let mut fields = value.split(' ');
    if fields.next() != Some("/confirm") || fields.next() != Some("life-write") {
        return Err(WriteConfirmationError::Invalid);
    }
    let raw_id = fields.next().ok_or(WriteConfirmationError::Invalid)?;
    let raw_version = fields.next().ok_or(WriteConfirmationError::Invalid)?;
    let preview_hash = fields.next().ok_or(WriteConfirmationError::Invalid)?;
    if fields.next().is_some() {
        return Err(WriteConfirmationError::Invalid);
    }
    let command_id = Uuid::parse_str(raw_id).map_err(|_| WriteConfirmationError::Invalid)?;
    let version = raw_version
        .strip_prefix('v')
        .ok_or(WriteConfirmationError::Invalid)?;
    let expected_version = version
        .parse::<i64>()
        .map_err(|_| WriteConfirmationError::Invalid)?;
    if command_id.to_string() != raw_id
        || expected_version < 1
        || expected_version.to_string() != version
        || !valid_hash(preview_hash)
    {
        return Err(WriteConfirmationError::Invalid);
    }
    Ok(ExactWriteConfirmation {
        command_id,
        expected_version,
        preview_hash: preview_hash.to_owned(),
    })
}

impl Store {
    /// Verifies a new signed exact command and persists a one-time confirmation grant.
    pub async fn validate_write_confirmation(
        &self,
        request: ValidateWriteConfirmationRequest,
        deployment_id: &str,
        ttl: Duration,
    ) -> Result<ValidatedWriteConfirmation, WriteConfirmationError> {
        if !(60..=MAX_CONFIRMATION_SECONDS).contains(&ttl.as_secs())
            || !safe_identifier(deployment_id, 256)
        {
            return Err(WriteConfirmationError::Invalid);
        }
        let parsed = parse_exact_confirmation(&request.signed_event.content)?;
        let event = &request.signed_event;
        let now =
            u64::try_from(Utc::now().timestamp()).map_err(|_| WriteConfirmationError::Invalid)?;
        let created = event.created_at.as_secs();
        let valid_kind = matches!(event.kind.as_u16(), 1 | 9 | 40002 | 45001 | 45003);
        if parsed.command_id != request.command_id
            || parsed.expected_version != request.expected_version
            || parsed.preview_hash != request.preview_hash
            || !valid_hash(&request.preview_hash)
            || !valid_kind
            || created.saturating_add(MAX_CONFIRMATION_SECONDS) < now
            || created > now.saturating_add(FUTURE_SKEW_SECONDS)
            || !event.verify_id()
            || !event.verify_signature()
        {
            return Err(WriteConfirmationError::Invalid);
        }
        let mut transaction = self.pool.begin().await.map_err(database)?;
        let binding = sqlx::query(
            "SELECT b.workbench_user_id FROM life_identity_bindings b
             JOIN life_workbench_users u ON u.id=b.workbench_user_id
             WHERE b.buzz_pubkey=$1 AND b.status='active' AND u.status='active'
             FOR UPDATE OF b,u",
        )
        .bind(event.pubkey.to_hex())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?
        .ok_or(WriteConfirmationError::Unauthorized)?;
        let user_id: Uuid = binding.get("workbench_user_id");
        let session = sqlx::query(
            "SELECT id FROM life_workbench_sessions
             WHERE workbench_user_id=$1 AND deployment_id=$2
               AND status='active' AND expires_at>now()
             ORDER BY created_at DESC,id DESC LIMIT 1 FOR UPDATE",
        )
        .bind(user_id)
        .bind(deployment_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?
        .ok_or(WriteConfirmationError::Unauthorized)?;
        let workbench_session_id: Uuid = session.get("id");
        let created_at = i64::try_from(created).map_err(|_| WriteConfirmationError::Invalid)?;
        let ttl_seconds =
            i64::try_from(ttl.as_secs()).map_err(|_| WriteConfirmationError::Invalid)?;
        let expires_at = Utc
            .timestamp_opt(created_at + ttl_seconds, 0)
            .single()
            .ok_or(WriteConfirmationError::Invalid)?;
        if expires_at <= Utc::now() {
            return Err(WriteConfirmationError::Invalid);
        }
        let confirmation_id = Uuid::new_v4();
        let inserted = sqlx::query(
            "INSERT INTO life_write_command_confirmations
             (id,command_id,workbench_user_id,workbench_session_id,source_event_id,
              expected_version,preview_hash,status,expires_at,trace_id)
             VALUES($1,$2,$3,$4,$5,$6,$7,'active',$8,$9)",
        )
        .bind(confirmation_id)
        .bind(request.command_id)
        .bind(user_id)
        .bind(workbench_session_id)
        .bind(event.id.to_hex())
        .bind(request.expected_version)
        .bind(hex::decode(&request.preview_hash).map_err(|_| WriteConfirmationError::Invalid)?)
        .bind(expires_at)
        .bind(request.trace_id)
        .execute(&mut *transaction)
        .await;
        if let Err(error) = inserted {
            if error
                .as_database_error()
                .is_some_and(|error| error.is_unique_violation())
            {
                return Err(WriteConfirmationError::Conflict);
            }
            return Err(WriteConfirmationError::Database);
        }
        audit(
            &mut transaction,
            "WRITE_PREVIEW_CONFIRMED",
            user_id,
            workbench_session_id,
            request.command_id,
            &event.id.to_hex(),
            request.trace_id,
        )
        .await?;
        transaction.commit().await.map_err(database)?;
        Ok(ValidatedWriteConfirmation {
            confirmation_id: WriteCommandConfirmationId::new(confirmation_id),
            command_id: request.command_id,
            user_id: LifeWorkbenchUserId::new(user_id),
            workbench_session_id: WorkbenchSessionId::new(workbench_session_id),
            expires_at,
            trace_id: request.trace_id,
        })
    }

    /// Atomically consumes one exact confirmation for a matching source event and session.
    pub async fn consume_write_confirmation(
        &self,
        command_id: Uuid,
        user_id: LifeWorkbenchUserId,
        session_id: WorkbenchSessionId,
        source_event_id: &str,
        expected_version: i64,
    ) -> Result<ConsumedWriteConfirmation, WriteConfirmationError> {
        let mut transaction = self.pool.begin().await.map_err(database)?;
        let consumed = consume_write_confirmation_in_transaction(
            &mut transaction,
            command_id,
            user_id.as_uuid(),
            session_id.as_uuid(),
            source_event_id,
            expected_version,
        )
        .await?;
        transaction.commit().await.map_err(database)?;
        Ok(consumed)
    }
}

pub(crate) async fn consume_write_confirmation_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    command_id: Uuid,
    user_id: Uuid,
    session_id: Uuid,
    source_event_id: &str,
    expected_version: i64,
) -> Result<ConsumedWriteConfirmation, WriteConfirmationError> {
    let row = sqlx::query(
        "UPDATE life_write_command_confirmations
         SET status='consumed',consumed_at=now()
         WHERE command_id=$1 AND workbench_user_id=$2 AND workbench_session_id=$3
           AND source_event_id=$4 AND expected_version=$5
           AND status='active' AND expires_at>now()
         RETURNING id,encode(preview_hash,'hex') AS preview_hash,trace_id",
    )
    .bind(command_id)
    .bind(user_id)
    .bind(session_id)
    .bind(source_event_id)
    .bind(expected_version)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database)?
    .ok_or(WriteConfirmationError::Unauthorized)?;
    audit(
        transaction,
        "WRITE_CONFIRMATION_CONSUMED",
        user_id,
        session_id,
        command_id,
        source_event_id,
        row.get("trace_id"),
    )
    .await?;
    Ok(ConsumedWriteConfirmation {
        confirmation_id: WriteCommandConfirmationId::new(row.get("id")),
        preview_hash: row.get("preview_hash"),
    })
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn safe_identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

async fn audit(
    transaction: &mut Transaction<'_, Postgres>,
    event_type: &str,
    user_id: Uuid,
    workbench_session_id: Uuid,
    command_id: Uuid,
    source_event_id: &str,
    trace_id: Uuid,
) -> Result<(), WriteConfirmationError> {
    sqlx::query(
        "INSERT INTO life_security_audit
         (event_type,outcome,subject_kind,subject_id,workbench_user_id,
          workbench_session_id,source_event_id,trace_id)
         VALUES($1,'success','write_command',$2,$3,$4,$5,$6)",
    )
    .bind(event_type)
    .bind(command_id.to_string())
    .bind(user_id)
    .bind(workbench_session_id)
    .bind(source_event_id)
    .bind(trace_id)
    .execute(&mut **transaction)
    .await
    .map_err(database)?;
    Ok(())
}

fn database(_: sqlx::Error) -> WriteConfirmationError {
    WriteConfirmationError::Database
}
