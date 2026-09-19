//! Exact master-data commands shared by preview and future bound approvals.
use crate::b2::DomainError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{AssertSqlSafe, Postgres, Transaction};
use uuid::Uuid;

/// Closed operation envelope; resource families validate their own typed fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum MasterCommand<S, T> {
    /// Create a new master record; no real object ID exists at preview time.
    Create { command: S },
    /// Replace editable fields on the exact current version.
    Update {
        #[serde(rename = "documentId")]
        document_id: Uuid,
        command: S,
    },
    /// Enable or disable an exact current master record.
    ChangeStatus {
        #[serde(rename = "resourceType")]
        resource_type: String,
        #[serde(rename = "documentId")]
        document_id: Uuid,
        command: T,
    },
}
impl<S, T> MasterCommand<S, T> {
    /// Intent family suffix; never selected from untrusted free-form strings.
    pub fn intent_suffix(&self) -> &'static str {
        match self {
            Self::Create { .. } => "creation",
            Self::Update { .. } => "update",
            Self::ChangeStatus { .. } => "status",
        }
    }
}
// Table identifiers come only from fixed call sites, never from command input.
pub(crate) async fn parent(
    tx: &mut Transaction<'_, Postgres>,
    table: &'static str,
    id: Uuid,
) -> Result<Value, DomainError> {
    sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT to_jsonb(p) FROM {table} p WHERE id=$1 FOR SHARE"
    )))
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(DomainError::NotFoundOrForbidden)
}
pub(crate) fn unchanged<T: PartialEq>(old: T, new: T) -> Result<(), DomainError> {
    if old != new {
        return Err(DomainError::Invalid(
            "immutable master-data fields cannot be changed".into(),
        ));
    }
    Ok(())
}
