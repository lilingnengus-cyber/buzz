//! Monotonic LifeOS membership snapshots and immediate authority revocation.

use crate::{identity::ResolvedMembership, model::LifeWorkbenchUserId, Store};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Row, Transaction};
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
    pub(crate) async fn refresh_membership_snapshot(
        &self,
        life_os_user_id: &str,
        user_active: bool,
        memberships: &[ResolvedMembership],
        trace_id: Uuid,
    ) -> Result<bool, MembershipError> {
        let mut transaction = self.pool.begin().await.map_err(database)?;
        let row = sqlx::query(
            "SELECT id,status,authority_version FROM life_workbench_users
             WHERE life_os_user_id=$1 FOR UPDATE",
        )
        .bind(life_os_user_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?
        .ok_or(MembershipError::NotFound)?;
        let user_id: Uuid = row.get("id");
        let current = sqlx::query(
            "SELECT workspace_id,role_code FROM life_workspace_memberships
             WHERE workbench_user_id=$1 AND status='active' ORDER BY workspace_id",
        )
        .bind(user_id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(database)?
        .into_iter()
        .map(|row| {
            (
                row.get::<String, _>("workspace_id"),
                row.get::<String, _>("role_code"),
            )
        })
        .collect::<Vec<_>>();
        let current_version = row.get::<i64, _>("authority_version");
        let source_version = memberships
            .iter()
            .map(|membership| membership.membership_version)
            .max()
            .unwrap_or(0);
        let event = MembershipEvent {
            life_os_user_id: life_os_user_id.to_owned(),
            user_active,
            membership_version: source_version.max(current_version.saturating_add(1)),
            memberships: memberships
                .iter()
                .map(|membership| MembershipSnapshot {
                    workspace_id: membership.workspace_id.clone(),
                    role: membership.role.clone(),
                })
                .collect(),
            trace_id,
        };
        validate(&event)?;
        let mut incoming = event
            .memberships
            .iter()
            .map(|membership| (membership.workspace_id.clone(), membership.role.clone()))
            .collect::<Vec<_>>();
        incoming.sort();
        let unchanged =
            (row.get::<String, _>("status") == "active") == user_active && current == incoming;
        if unchanged {
            sqlx::query(
                "UPDATE life_workbench_users
                 SET authority_version=GREATEST(authority_version,$2),
                     authority_sync_status='current',authority_synced_at=now(),updated_at=now()
                 WHERE id=$1",
            )
            .bind(user_id)
            .bind(source_version)
            .execute(&mut *transaction)
            .await
            .map_err(database)?;
            transaction.commit().await.map_err(database)?;
            return Ok(false);
        }
        apply_snapshot(&mut transaction, user_id, &event).await?;
        transaction.commit().await.map_err(database)?;
        Ok(true)
    }

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

        apply_snapshot(&mut transaction, user_id, event).await?;
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

async fn apply_snapshot(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    event: &MembershipEvent,
) -> Result<(), MembershipError> {
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
    .execute(&mut **transaction)
    .await
    .map_err(database)?;
    sqlx::query(
        "UPDATE life_workspace_memberships
         SET status='revoked',revoked_at=now(),updated_at=now()
         WHERE workbench_user_id=$1 AND status='active'",
    )
    .bind(user_id)
    .execute(&mut **transaction)
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
            .execute(&mut **transaction)
            .await
            .map_err(database)?;
        }
    }
    sqlx::query(
        "UPDATE life_agent_delegations SET status='revoked',revoked_at=now()
         WHERE workbench_user_id=$1 AND status IN ('active','exhausted')",
    )
    .bind(user_id)
    .execute(&mut **transaction)
    .await
    .map_err(database)?;
    sqlx::query(
        "UPDATE life_embed_sessions SET status='revoked',revoked_at=now()
         WHERE workbench_user_id=$1 AND status='active'",
    )
    .bind(user_id)
    .execute(&mut **transaction)
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
    .execute(&mut **transaction)
    .await
    .map_err(database)?;
    Ok(())
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
