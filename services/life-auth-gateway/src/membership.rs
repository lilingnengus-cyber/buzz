//! Monotonic LifeOS membership snapshots and immediate authority revocation.

use crate::{model::LifeWorkbenchUserId, Store};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::collections::BTreeSet;
use uuid::Uuid;

/// One workspace role in a complete LifeOS authority snapshot.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MembershipSnapshot {
    /// Opaque LifeOS workspace identifier.
    pub workspace_id: String,
    /// Current role: `OWNER`, `ADMIN`, `MEMBER`, or `VIEWER`.
    pub role: String,
}

/// Complete, monotonically versioned LifeOS authority snapshot for one user.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MembershipEvent {
    /// Canonical opaque LifeOS user identifier.
    pub life_os_user_id: String,
    /// Whether the LifeOS source currently permits access.
    pub user_active: bool,
    /// Positive global authority revision for this user.
    pub membership_version: i64,
    /// Complete current set of workspace roles.
    pub memberships: Vec<MembershipSnapshot>,
    /// Low-sensitivity distributed trace identifier.
    pub trace_id: Uuid,
}

/// Stable membership synchronization failure classes.
#[derive(Debug, thiserror::Error)]
pub enum MembershipError {
    /// Event fields or snapshot contents were invalid.
    #[error("membership event is invalid")]
    Invalid,
    /// No mapped Gateway user exists for this LifeOS identifier.
    #[error("membership user is not mapped")]
    NotFound,
    /// PostgreSQL rejected the atomic authority transition.
    #[error("membership store unavailable")]
    Database,
}

impl Store {
    /// Applies a newer complete snapshot and revokes existing delegated authority atomically.
    pub async fn apply_membership_event(
        &self,
        event: &MembershipEvent,
    ) -> Result<bool, MembershipError> {
        validate(event)?;
        let mut transaction = self.pool.begin().await.map_err(database)?;
        let row = sqlx::query(
            "SELECT id,authority_version FROM life_workbench_users
             WHERE life_os_user_id=$1 FOR UPDATE",
        )
        .bind(&event.life_os_user_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?
        .ok_or(MembershipError::NotFound)?;
        let user_id = row.get::<Uuid, _>("id");
        if event.membership_version <= row.get::<i64, _>("authority_version") {
            transaction.commit().await.map_err(database)?;
            return Ok(false);
        }

        sqlx::query(
            "UPDATE life_workbench_users
             SET status=$2,disabled_at=CASE WHEN $3 THEN NULL ELSE now() END,
                 authority_version=$4,authority_sync_status='current',
                 authority_synced_at=now(),updated_at=now()
             WHERE id=$1",
        )
        .bind(user_id)
        .bind(if event.user_active {
            "active"
        } else {
            "disabled"
        })
        .bind(event.user_active)
        .bind(event.membership_version)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        sqlx::query(
            "UPDATE life_workspace_memberships
             SET status='revoked',revoked_at=now(),updated_at=now()
             WHERE workbench_user_id=$1 AND status='active'",
        )
        .bind(user_id)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        if event.user_active {
            for membership in &event.memberships {
                sqlx::query(
                    "INSERT INTO life_workspace_memberships
                     (id,workbench_user_id,workspace_id,role_code,status,membership_version)
                     VALUES($1,$2,$3,$4,'active',$5)",
                )
                .bind(Uuid::new_v4())
                .bind(user_id)
                .bind(&membership.workspace_id)
                .bind(&membership.role)
                .bind(event.membership_version)
                .execute(&mut *transaction)
                .await
                .map_err(database)?;
            }
        }
        sqlx::query(
            "UPDATE life_agent_delegations SET status='revoked',revoked_at=now()
             WHERE workbench_user_id=$1 AND status IN ('active','exhausted')",
        )
        .bind(user_id)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        sqlx::query(
            "UPDATE life_embed_sessions SET status='revoked',revoked_at=now()
             WHERE workbench_user_id=$1 AND status='active'",
        )
        .bind(user_id)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        sqlx::query(
            "INSERT INTO life_security_audit
             (event_type,outcome,subject_kind,subject_id,workbench_user_id,trace_id)
             VALUES('MEMBERSHIP_SNAPSHOT_APPLIED','success','human',$1,$2,$3)",
        )
        .bind(&event.life_os_user_id)
        .bind(user_id)
        .bind(event.trace_id)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        transaction.commit().await.map_err(database)?;
        Ok(true)
    }

    /// Marks the authority mirror stale so all new authorization fails closed.
    pub async fn mark_membership_sync_failed(
        &self,
        user_id: LifeWorkbenchUserId,
        trace_id: Uuid,
    ) -> Result<(), MembershipError> {
        let mut transaction = self.pool.begin().await.map_err(database)?;
        let changed = sqlx::query(
            "UPDATE life_workbench_users SET authority_sync_status='stale',updated_at=now()
             WHERE id=$1",
        )
        .bind(user_id.as_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        if changed.rows_affected() != 1 {
            return Err(MembershipError::NotFound);
        }
        sqlx::query(
            "INSERT INTO life_security_audit
             (event_type,outcome,reason_code,subject_kind,workbench_user_id,trace_id)
             VALUES('MEMBERSHIP_SYNC_FAILED','failure','authority_snapshot_stale','human',$1,$2)",
        )
        .bind(user_id.as_uuid())
        .bind(trace_id)
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
        transaction.commit().await.map_err(database)?;
        tracing::warn!(trace_id = %trace_id, "LifeOS membership sync failed; new authorization disabled");
        Ok(())
    }
}

fn validate(event: &MembershipEvent) -> Result<(), MembershipError> {
    let safe = |value: &str| {
        !value.is_empty()
            && value.len() <= 512
            && value.trim() == value
            && !value.chars().any(char::is_control)
    };
    let mut workspaces = BTreeSet::new();
    if !safe(&event.life_os_user_id)
        || event.membership_version <= 0
        || event.memberships.len() > 10_000
        || event.memberships.iter().any(|membership| {
            !safe(&membership.workspace_id)
                || !matches!(
                    membership.role.as_str(),
                    "OWNER" | "ADMIN" | "MEMBER" | "VIEWER"
                )
                || !workspaces.insert(membership.workspace_id.as_str())
        })
    {
        return Err(MembershipError::Invalid);
    }
    Ok(())
}

fn database(_: sqlx::Error) -> MembershipError {
    MembershipError::Database
}
