use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    PrincipalUpsert,
    PrincipalDisable,
    RoleUpsert,
    RoleDisable,
    PermissionGrant,
    PermissionRevoke,
    RolePermissionGrant,
    RolePermissionRevoke,
    RoleAssign,
    RoleUnassign,
}

impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrincipalUpsert => "principal_upsert",
            Self::PrincipalDisable => "principal_disable",
            Self::RoleUpsert => "role_upsert",
            Self::RoleDisable => "role_disable",
            Self::PermissionGrant => "permission_grant",
            Self::PermissionRevoke => "permission_revoke",
            Self::RolePermissionGrant => "role_permission_grant",
            Self::RolePermissionRevoke => "role_permission_revoke",
            Self::RoleAssign => "role_assign",
            Self::RoleUnassign => "role_unassign",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateChangeRequest {
    pub operation: Operation,
    pub payload: Value,
    pub reason: String,
    pub idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionRequest {
    pub comment: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Actor {
    pub principal_id: Uuid,
    pub issuer: String,
    pub subject: String,
    pub auth_time: DateTime<Utc>,
    pub evidence_hash: Vec<u8>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRequestView {
    pub id: Uuid,
    pub operation: String,
    pub payload: Value,
    pub risk_level: String,
    pub required_approvals: i16,
    pub approval_count: i64,
    pub status: String,
    pub requested_by: Uuid,
    pub reason: String,
    pub trace_id: Uuid,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub applied_at: Option<DateTime<Utc>>,
    pub failure_code: Option<String>,
    pub version: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    pub principals: Vec<Value>,
    pub roles: Vec<Value>,
    pub permissions: Vec<Value>,
}
