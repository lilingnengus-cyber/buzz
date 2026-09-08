//! Trusted, published delete previews resolved from a signed textual confirmation.

use crate::{
    write_confirmation::{ExactWriteConfirmation, WriteConfirmationError},
    Store,
};
use chrono::{DateTime, Utc};
use nostr::Event;
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

/// Preview facts recorded by the trusted harness after relay acceptance.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingDeleteRequest {
    /// Delegation that produced this preview.
    pub delegation_id: Uuid,
    /// Host-derived community identity.
    pub community_id: String,
    /// Accepted, agent-signed preview response.
    pub preview_event: Event,
    /// Immutable command created by LifeOS.
    pub command_id: Uuid,
    /// Version displayed in the preview.
    pub expected_version: i64,
    /// LifeOS preview digest.
    pub preview_hash: String,
    /// LifeOS command expiration.
    pub expires_at: DateTime<Utc>,
}

/// Signed short confirmation with trusted routing context.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShortDeleteRequest {
    /// Original user-signed message; never rewritten into another command.
    pub signed_event: Event,
    /// Host-derived community identity.
    pub community_id: String,
    /// Verified agent receiving this direct message.
    pub agent_id: String,
    /// Distributed trace identifier.
    pub trace_id: Uuid,
}

impl Store {
    /// Records only a signed reply associated with a consumed preview delegation.
    pub async fn record_pending_delete(
        &self,
        request: PendingDeleteRequest,
    ) -> Result<(), WriteConfirmationError> {
        let event = &request.preview_event;
        if !event.verify_id()
            || !event.verify_signature()
            || event.kind.as_u16() != 9
            || request.community_id.is_empty()
            || request.community_id.len() > 512
            || request.expected_version < 1
            || request.preview_hash.len() != 64
            || !request
                .preview_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || request.expires_at <= Utc::now()
            || request.expires_at > Utc::now() + chrono::Duration::minutes(10)
            || event.created_at.as_secs() > Utc::now().timestamp() as u64 + 30
        {
            return Err(WriteConfirmationError::Invalid);
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| WriteConfirmationError::Database)?;
        let row = sqlx::query(
            "SELECT d.source_pubkey,d.source_channel_id,d.source_event_id
            FROM life_agent_delegations d JOIN life_workbench_users u ON u.id=d.workbench_user_id
            WHERE d.id=$1 AND d.agent_id=$2 AND u.status='active'
              AND d.status IN ('active','exhausted') AND d.expires_at>now()
              AND EXISTS(SELECT 1 FROM life_delegation_calls c WHERE c.delegation_id=d.id
                  AND c.capability='write_command:preview' AND c.expected_version=$3)
            FOR UPDATE OF u",
        )
        .bind(request.delegation_id)
        .bind(event.pubkey.to_hex())
        .bind(request.expected_version)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| WriteConfirmationError::Database)?
        .ok_or(WriteConfirmationError::Unauthorized)?;
        let channel: Option<String> = row.get("source_channel_id");
        let channel = channel.ok_or(WriteConfirmationError::Invalid)?;
        let source: String = row.get("source_event_id");
        if channel_tag(event)? != channel
            || !event.tags.iter().any(|tag| {
                let parts = tag.as_slice();
                parts.first().map(String::as_str) == Some("e") && parts.get(1) == Some(&source)
            })
        {
            return Err(WriteConfirmationError::Invalid);
        }
        let root = thread_root(event)?.unwrap_or(source);
        let user_pubkey: String = row.get("source_pubkey");
        // New previews supersede older prompts in this topic, never another topic.
        // Refuse delayed delivery that would replace a newer signed prompt.
        let newest: Option<i64> = sqlx::query_scalar("SELECT max(shown_event_seconds) FROM life_pending_deletes
            WHERE community_id=$1 AND channel_id=$2 AND agent_id=$3 AND user_pubkey=$4 AND thread_root_id=$5")
            .bind(&request.community_id).bind(&channel).bind(event.pubkey.to_hex()).bind(&user_pubkey).bind(&root)
            .fetch_one(&mut *tx).await.map_err(|_| WriteConfirmationError::Database)?;
        if newest.is_some_and(|seconds| seconds >= event.created_at.as_secs() as i64) {
            return Err(WriteConfirmationError::Conflict);
        }
        sqlx::query("UPDATE life_pending_deletes SET expires_at=LEAST(expires_at,now())
            WHERE community_id=$1 AND channel_id=$2 AND agent_id=$3 AND user_pubkey=$4 AND thread_root_id=$5")
            .bind(&request.community_id).bind(&channel).bind(event.pubkey.to_hex()).bind(&user_pubkey).bind(&root)
            .execute(&mut *tx).await.map_err(|_| WriteConfirmationError::Database)?;
        sqlx::query("INSERT INTO life_pending_deletes
            (command_id,delegation_id,community_id,channel_id,agent_id,user_pubkey,preview_event_id,expected_version,preview_hash,shown_event_seconds,expires_at,thread_root_id)
            VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
            .bind(request.command_id).bind(request.delegation_id).bind(request.community_id)
            .bind(channel).bind(event.pubkey.to_hex()).bind(user_pubkey)
            .bind(event.id.to_hex()).bind(request.expected_version).bind(request.preview_hash)
            .bind(event.created_at.as_secs() as i64).bind(request.expires_at)
            .bind(root)
            .execute(&mut *tx).await.map_err(|_| WriteConfirmationError::Conflict)?;
        tx.commit()
            .await
            .map_err(|_| WriteConfirmationError::Database)
    }

    /// Resolves the sole unexpired preview; multiple pending commands require a fresh explicit preview.
    pub(crate) async fn resolve_pending_delete(
        &self,
        request: &ShortDeleteRequest,
    ) -> Result<ExactWriteConfirmation, WriteConfirmationError> {
        let mut connection = self
            .pool
            .acquire()
            .await
            .map_err(|_| WriteConfirmationError::Database)?;
        resolve(&mut connection, request).await
    }
}

pub(crate) async fn resolve(
    connection: &mut sqlx::PgConnection,
    request: &ShortDeleteRequest,
) -> Result<ExactWriteConfirmation, WriteConfirmationError> {
    if request.signed_event.content != "确认删除" {
        return Err(WriteConfirmationError::Invalid);
    }
    let channel = channel_tag(&request.signed_event)?;
    let rows = sqlx::query("SELECT p.command_id,p.expected_version,p.preview_hash,p.shown_event_seconds
        FROM life_pending_deletes p WHERE community_id=$1 AND channel_id=$2 AND agent_id=$3 AND user_pubkey=$4
        AND expires_at>now()
        AND ($5::text IS NULL OR thread_root_id=$5 OR preview_event_id=$5)
        ORDER BY created_at DESC LIMIT 2")
        .bind(&request.community_id).bind(channel).bind(&request.agent_id).bind(request.signed_event.pubkey.to_hex())
        .bind(thread_root(&request.signed_event)?)
        .fetch_all(connection).await.map_err(|_| WriteConfirmationError::Database)?;
    if rows.len() != 1 {
        return Err(WriteConfirmationError::Conflict);
    }
    let row = &rows[0];
    if row.get::<i64, _>("shown_event_seconds") >= request.signed_event.created_at.as_secs() as i64
    {
        return Err(WriteConfirmationError::Invalid);
    }
    Ok(ExactWriteConfirmation {
        command_id: row.get("command_id"),
        expected_version: row.get("expected_version"),
        preview_hash: row.get("preview_hash"),
    })
}

fn thread_root(event: &Event) -> Result<Option<String>, WriteConfirmationError> {
    let roots: Vec<_> = event
        .tags
        .iter()
        .filter(|tag| {
            let parts = tag.as_slice();
            parts.first().map(String::as_str) == Some("e")
                && (parts.len() == 2 || parts.get(3).map(String::as_str) == Some("root"))
        })
        .collect();
    if roots.len() > 1 {
        return Err(WriteConfirmationError::Invalid);
    }
    Ok(roots.first().and_then(|tag| tag.as_slice().get(1)).cloned())
}

fn channel_tag(event: &Event) -> Result<String, WriteConfirmationError> {
    let channels: Vec<_> = event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("h"))
        .collect();
    if channels.len() != 1 {
        return Err(WriteConfirmationError::Invalid);
    }
    channels[0]
        .as_slice()
        .get(1)
        .cloned()
        .ok_or(WriteConfirmationError::Invalid)
}
