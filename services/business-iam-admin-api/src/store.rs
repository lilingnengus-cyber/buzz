use crate::{
    model::{Actor, CatalogView, ChangeRequestView, CreateChangeRequest, Operation},
    Error,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{AssertSqlSafe, PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

impl Store {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("../business-auth-gateway/migrations")
            .run(pool)
            .await
    }

    pub async fn grant_runtime(pool: &PgPool, role: &str) -> Result<(), sqlx::Error> {
        if role.is_empty()
            || role.len() > 63
            || !role
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(sqlx::Error::Protocol(
                "BUSINESS_IAM_ADMIN_RUNTIME_DATABASE_ROLE is invalid".into(),
            ));
        }
        let role = format!("\"{role}\"");
        let sql = format!(
            "REVOKE ALL ON SCHEMA public,business_iam FROM {role};
             REVOKE ALL ON ALL TABLES IN SCHEMA public,business_iam FROM {role};
             GRANT USAGE ON SCHEMA public,business_iam TO {role};
             GRANT SELECT ON enterprise_users TO {role};
             GRANT SELECT,UPDATE ON agent_read_delegations TO {role};
             GRANT SELECT,INSERT ON security_audit_events TO {role};
             GRANT SELECT ON
               business_iam.permissions,business_iam.authorization_decisions
               TO {role};
             GRANT SELECT,INSERT,UPDATE,DELETE ON
               business_iam.principals,business_iam.roles,
               business_iam.role_permissions,business_iam.principal_roles,
               business_iam.principal_permissions TO {role};
             GRANT SELECT,INSERT,UPDATE ON business_iam.change_requests TO {role};
             GRANT SELECT,INSERT ON
               business_iam.change_approvals,business_iam.admin_audit_events TO {role};"
        );
        // The identifier is restricted above to an ASCII PostgreSQL identifier;
        // no untrusted SQL fragments can reach this migration-only statement.
        sqlx::raw_sql(AssertSqlSafe(sql)).execute(pool).await?;
        Ok(())
    }

    pub async fn ready(&self) -> Result<(), Error> {
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map(|_| ())
            .map_err(|_| Error::Unavailable("database_unavailable"))
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn catalog(&self, actor: &Actor) -> Result<CatalogView, Error> {
        self.require(actor, "business_iam:read").await?;
        let principals = sqlx::query_scalar::<_, Value>(
            "SELECT jsonb_build_object(
               'id',id,'kind',kind,'externalId',external_id,'displayName',display_name,
               'status',status,'version',version,'updatedAt',updated_at,
               'roles',COALESCE((
                 SELECT jsonb_agg(jsonb_build_object('code',role.code,'name',role.name)
                                  ORDER BY role.code)
                 FROM business_iam.principal_roles assignment
                 JOIN business_iam.roles role ON role.id=assignment.role_id
                 WHERE assignment.principal_id=principal.id
                   AND assignment.valid_from<=now()
                   AND (assignment.valid_until IS NULL OR assignment.valid_until>now())
               ),'[]'::jsonb),
               'permissions',COALESCE((
                 SELECT jsonb_agg(jsonb_build_object(
                   'capability',permission.capability,'dataScope',assignment.data_scope,
                   'obligations',assignment.obligations) ORDER BY permission.capability)
                 FROM business_iam.principal_permissions assignment
                 JOIN business_iam.permissions permission ON permission.id=assignment.permission_id
                 WHERE assignment.principal_id=principal.id
                   AND assignment.valid_from<=now()
                   AND (assignment.valid_until IS NULL OR assignment.valid_until>now())
               ),'[]'::jsonb))
             FROM business_iam.principals principal ORDER BY kind,external_id LIMIT 1000",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let roles = sqlx::query_scalar::<_, Value>(
            "SELECT jsonb_build_object(
               'id',id,'code',code,'name',name,'status',status,'version',version,
               'updatedAt',updated_at,'permissions',COALESCE((
                 SELECT jsonb_agg(jsonb_build_object(
                   'capability',permission.capability,'dataScope',assignment.data_scope,
                   'obligations',assignment.obligations) ORDER BY permission.capability)
                 FROM business_iam.role_permissions assignment
                 JOIN business_iam.permissions permission ON permission.id=assignment.permission_id
                 WHERE assignment.role_id=role.id
               ),'[]'::jsonb))
             FROM business_iam.roles role ORDER BY code LIMIT 1000",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let permissions = sqlx::query_scalar::<_, Value>(
            "SELECT jsonb_build_object(
               'id',id,'capability',capability,'resourceType',resource_type,'action',action,
               'riskLevel',risk_level,'status',status,'obligations',obligations,
               'defaultDataScope',default_data_scope,'version',version)
             FROM business_iam.permissions ORDER BY capability LIMIT 1000",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(CatalogView {
            principals,
            roles,
            permissions,
        })
    }

    pub async fn list_changes(
        &self,
        actor: &Actor,
        status: Option<&str>,
    ) -> Result<Vec<ChangeRequestView>, Error> {
        self.require(actor, "business_iam:read").await?;
        if status.is_some_and(|value| {
            !matches!(
                value,
                "pending" | "approved" | "rejected" | "applied" | "failed" | "cancelled"
            )
        }) {
            return Err(Error::Invalid("invalid_status"));
        }
        sqlx::query_as::<_, ChangeRequestView>(
            "SELECT request.id,request.operation,request.payload,request.risk_level,
                    request.required_approvals,
                    count(approval.id) FILTER (WHERE approval.decision='approve') AS approval_count,
                    request.status,request.requested_by,requester.display_name AS requester_display_name,
                    COALESCE(jsonb_agg(jsonb_build_object(
                      'approverId',approval.approver_id,'approverDisplayName',approver.display_name,
                      'decision',approval.decision,'comment',approval.comment,
                      'decidedAt',approval.decided_at)
                      ORDER BY approval.decided_at) FILTER (WHERE approval.id IS NOT NULL),'[]'::jsonb)
                      AS approvals,
                    request.reason,request.trace_id,
                    request.requested_at,request.expires_at,request.decided_at,request.applied_at,
                    request.failure_code,request.version
             FROM business_iam.change_requests request
             JOIN business_iam.principals requester ON requester.id=request.requested_by
             LEFT JOIN business_iam.change_approvals approval
               ON approval.change_request_id=request.id
             LEFT JOIN business_iam.principals approver ON approver.id=approval.approver_id
             WHERE ($1::text IS NULL OR request.status=$1)
             GROUP BY request.id,requester.display_name
             ORDER BY request.requested_at DESC LIMIT 500",
        )
        .bind(status)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)
    }

    pub async fn create_change(
        &self,
        actor: &Actor,
        request: CreateChangeRequest,
        trace_id: Uuid,
    ) -> Result<ChangeRequestView, Error> {
        self.require(actor, "business_iam:request").await?;
        validate_text(&request.reason, 3, 500, "invalid_reason")?;
        validate_text(&request.idempotency_key, 8, 128, "invalid_idempotency_key")?;
        validate_payload(request.operation, &request.payload)?;
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        let risk = risk_level(&mut tx, request.operation, &request.payload).await?;
        let required_approvals = if risk == "critical" { 2_i16 } else { 1_i16 };
        let payload_hash = payload_hash(request.operation, &request.payload)?;
        let id = Uuid::new_v4();
        let inserted = sqlx::query(
            "INSERT INTO business_iam.change_requests(
               id,operation,payload,payload_hash,risk_level,required_approvals,
               requested_by,requester_issuer,requester_subject,reason,idempotency_key,trace_id)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
             ON CONFLICT(requested_by,idempotency_key) DO NOTHING",
        )
        .bind(id)
        .bind(request.operation.as_str())
        .bind(&request.payload)
        .bind(&payload_hash)
        .bind(risk)
        .bind(required_approvals)
        .bind(actor.principal_id)
        .bind(&actor.issuer)
        .bind(&actor.subject)
        .bind(&request.reason)
        .bind(&request.idempotency_key)
        .bind(trace_id)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?
        .rows_affected();
        let request_id = if inserted == 1 {
            id
        } else {
            let row = sqlx::query(
                "SELECT id,payload_hash,operation,reason FROM business_iam.change_requests
                 WHERE requested_by=$1 AND idempotency_key=$2",
            )
            .bind(actor.principal_id)
            .bind(&request.idempotency_key)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
            if row.get::<Vec<u8>, _>("payload_hash") != payload_hash
                || row.get::<String, _>("operation") != request.operation.as_str()
                || row.get::<String, _>("reason") != request.reason
            {
                return Err(Error::Conflict("idempotency_key_reused"));
            }
            row.get("id")
        };
        audit(
            &mut tx,
            actor,
            "IAM_CHANGE_REQUESTED",
            "success",
            None,
            Some(request_id),
            trace_id,
            json!({"operation":request.operation.as_str(),"idempotentReplay":inserted==0}),
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        self.change_by_id(request_id).await
    }

    pub async fn approve(
        &self,
        actor: &Actor,
        request_id: Uuid,
        comment: Option<&str>,
        trace_id: Uuid,
    ) -> Result<ChangeRequestView, Error> {
        self.decide(actor, request_id, "approve", comment, trace_id)
            .await
    }

    pub async fn reject(
        &self,
        actor: &Actor,
        request_id: Uuid,
        comment: Option<&str>,
        trace_id: Uuid,
    ) -> Result<ChangeRequestView, Error> {
        self.decide(actor, request_id, "reject", comment, trace_id)
            .await
    }

    async fn decide(
        &self,
        actor: &Actor,
        request_id: Uuid,
        decision: &'static str,
        comment: Option<&str>,
        trace_id: Uuid,
    ) -> Result<ChangeRequestView, Error> {
        self.require(actor, "business_iam:approve").await?;
        if let Some(value) = comment {
            validate_text(value, 3, 500, "invalid_comment")?;
        }
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        let row = sqlx::query(
            "SELECT requested_by,status,required_approvals,operation,payload,expires_at
             FROM business_iam.change_requests WHERE id=$1 FOR UPDATE",
        )
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;
        if row.get::<String, _>("status") != "pending" {
            return Err(Error::Conflict("change_request_not_pending"));
        }
        if row.get::<chrono::DateTime<chrono::Utc>, _>("expires_at") <= chrono::Utc::now() {
            sqlx::query(
                "UPDATE business_iam.change_requests
                 SET status='cancelled',version=version+1 WHERE id=$1",
            )
            .bind(request_id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
            audit(
                &mut tx,
                actor,
                "IAM_CHANGE_EXPIRED",
                "denied",
                Some("change_request_expired"),
                Some(request_id),
                trace_id,
                json!({}),
            )
            .await?;
            tx.commit().await.map_err(db_error)?;
            return Err(Error::Conflict("change_request_expired"));
        }
        if row.get::<Uuid, _>("requested_by") == actor.principal_id {
            return Err(Error::Forbidden("requester_cannot_approve"));
        }
        sqlx::query(
            "INSERT INTO business_iam.change_approvals(
               id,change_request_id,approver_id,decision,comment,step_up_at,evidence_hash,trace_id)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(Uuid::new_v4())
        .bind(request_id)
        .bind(actor.principal_id)
        .bind(decision)
        .bind(comment)
        .bind(actor.auth_time)
        .bind(&actor.evidence_hash)
        .bind(trace_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            if is_unique_violation(&error) {
                Error::Conflict("approver_already_decided")
            } else {
                db_error(error)
            }
        })?;
        if decision == "reject" {
            sqlx::query(
                "UPDATE business_iam.change_requests
                 SET status='rejected',decided_at=now(),version=version+1 WHERE id=$1",
            )
            .bind(request_id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        } else {
            let approvals: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM business_iam.change_approvals
                 WHERE change_request_id=$1 AND decision='approve'",
            )
            .bind(request_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
            let required = i64::from(row.get::<i16, _>("required_approvals"));
            if approvals >= required {
                let operation = parse_operation(&row.get::<String, _>("operation"))?;
                let payload = row.get::<Value, _>("payload");
                sqlx::query("SAVEPOINT business_iam_apply")
                    .execute(&mut *tx)
                    .await
                    .map_err(db_error)?;
                if let Err(error) = apply_change(&mut tx, actor, operation, &payload).await {
                    let code = error.code();
                    sqlx::query("ROLLBACK TO SAVEPOINT business_iam_apply")
                        .execute(&mut *tx)
                        .await
                        .map_err(db_error)?;
                    sqlx::query(
                        "UPDATE business_iam.change_requests SET
                           status='failed',decided_at=now(),failure_code=$2,version=version+1
                         WHERE id=$1",
                    )
                    .bind(request_id)
                    .bind(code)
                    .execute(&mut *tx)
                    .await
                    .map_err(db_error)?;
                    audit(
                        &mut tx,
                        actor,
                        "IAM_CHANGE_APPLY_FAILED",
                        "failed",
                        Some(code),
                        Some(request_id),
                        trace_id,
                        json!({"operation":operation.as_str()}),
                    )
                    .await?;
                    tx.commit().await.map_err(db_error)?;
                    return self.change_by_id(request_id).await;
                }
                sqlx::query("RELEASE SAVEPOINT business_iam_apply")
                    .execute(&mut *tx)
                    .await
                    .map_err(db_error)?;
                sqlx::query(
                    "UPDATE business_iam.change_requests SET
                       status='applied',decided_at=now(),applied_at=now(),version=version+1
                     WHERE id=$1",
                )
                .bind(request_id)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
            }
        }
        audit(
            &mut tx,
            actor,
            if decision == "approve" {
                "IAM_CHANGE_APPROVED"
            } else {
                "IAM_CHANGE_REJECTED"
            },
            "success",
            None,
            Some(request_id),
            trace_id,
            json!({"decision":decision}),
        )
        .await?;
        tx.commit().await.map_err(db_error)?;
        self.change_by_id(request_id).await
    }

    async fn require(&self, actor: &Actor, capability: &'static str) -> Result<(), Error> {
        let allowed: bool = sqlx::query_scalar(
            "SELECT EXISTS(
               SELECT 1 FROM (
                 SELECT permission_id FROM business_iam.principal_permissions
                 WHERE principal_id=$1 AND valid_from<=now()
                   AND (valid_until IS NULL OR valid_until>now())
                 UNION
                 SELECT role_permission.permission_id
                 FROM business_iam.principal_roles principal_role
                 JOIN business_iam.roles role
                   ON role.id=principal_role.role_id AND role.status='active'
                 JOIN business_iam.role_permissions role_permission
                   ON role_permission.role_id=role.id
                 WHERE principal_role.principal_id=$1 AND principal_role.valid_from<=now()
                   AND (principal_role.valid_until IS NULL OR principal_role.valid_until>now())
               ) grant_row
               JOIN business_iam.permissions permission ON permission.id=grant_row.permission_id
               WHERE permission.capability=$2 AND permission.status='active')",
        )
        .bind(actor.principal_id)
        .bind(capability)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;
        allowed
            .then_some(())
            .ok_or(Error::Forbidden("business_iam_permission_denied"))
    }

    async fn change_by_id(&self, id: Uuid) -> Result<ChangeRequestView, Error> {
        sqlx::query_as::<_, ChangeRequestView>(
            "SELECT request.id,request.operation,request.payload,request.risk_level,
                    request.required_approvals,
                    count(approval.id) FILTER (WHERE approval.decision='approve') AS approval_count,
                    request.status,request.requested_by,requester.display_name AS requester_display_name,
                    COALESCE(jsonb_agg(jsonb_build_object(
                      'approverId',approval.approver_id,'approverDisplayName',approver.display_name,
                      'decision',approval.decision,'comment',approval.comment,
                      'decidedAt',approval.decided_at)
                      ORDER BY approval.decided_at) FILTER (WHERE approval.id IS NOT NULL),'[]'::jsonb)
                      AS approvals,
                    request.reason,request.trace_id,
                    request.requested_at,request.expires_at,request.decided_at,request.applied_at,
                    request.failure_code,request.version
             FROM business_iam.change_requests request
             JOIN business_iam.principals requester ON requester.id=request.requested_by
             LEFT JOIN business_iam.change_approvals approval
               ON approval.change_request_id=request.id
             LEFT JOIN business_iam.principals approver ON approver.id=approval.approver_id
             WHERE request.id=$1 GROUP BY request.id,requester.display_name",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)
    }
}

impl Error {
    fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized(code)
            | Self::Forbidden(code)
            | Self::Invalid(code)
            | Self::Conflict(code)
            | Self::Unavailable(code) => code,
            Self::NotFound => "not_found",
        }
    }
}

fn db_error(_: sqlx::Error) -> Error {
    Error::Unavailable("database_unavailable")
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|value| value.code())
        .is_some_and(|code| code == "23505")
}

fn validate_text(value: &str, min: usize, max: usize, code: &'static str) -> Result<(), Error> {
    let length = value.chars().count();
    if !(min..=max).contains(&length) || value.chars().any(char::is_control) {
        return Err(Error::Invalid(code));
    }
    Ok(())
}

fn string<'a>(payload: &'a Value, key: &str, code: &'static str) -> Result<&'a str, Error> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .ok_or(Error::Invalid(code))
}

fn expected_version(payload: &Value) -> Result<Option<i64>, Error> {
    payload
        .get("expectedVersion")
        .map(|value| {
            value
                .as_i64()
                .filter(|version| *version > 0)
                .ok_or(Error::Invalid("invalid_expected_version"))
        })
        .transpose()
}

fn validate_payload(operation: Operation, payload: &Value) -> Result<(), Error> {
    let object = payload
        .as_object()
        .ok_or(Error::Invalid("payload_must_be_object"))?;
    if serde_json::to_vec(payload)
        .map_err(|_| Error::Invalid("invalid_payload"))?
        .len()
        > 16 * 1024
    {
        return Err(Error::Invalid("payload_too_large"));
    }
    let allowed: &[&str] = match operation {
        Operation::PrincipalUpsert => &["kind", "externalId", "displayName", "expectedVersion"],
        Operation::PrincipalDisable => &["externalId", "expectedVersion"],
        Operation::RoleUpsert => &["code", "name", "expectedVersion"],
        Operation::RoleDisable => &["code", "expectedVersion"],
        Operation::PermissionGrant => &[
            "externalId",
            "capability",
            "dataScope",
            "obligations",
            "expectedVersion",
        ],
        Operation::PermissionRevoke => &["externalId", "capability", "expectedVersion"],
        Operation::RolePermissionGrant => &[
            "role",
            "capability",
            "dataScope",
            "obligations",
            "expectedVersion",
        ],
        Operation::RolePermissionRevoke => &["role", "capability", "expectedVersion"],
        Operation::RoleAssign | Operation::RoleUnassign => {
            &["externalId", "role", "expectedVersion"]
        }
    };
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(Error::Invalid("payload_field_not_allowed"));
    }
    match operation {
        Operation::PrincipalUpsert => {
            let kind = string(payload, "kind", "invalid_principal_kind")?;
            if !matches!(kind, "human" | "independent_agent" | "proxy_agent") {
                return Err(Error::Invalid("invalid_principal_kind"));
            }
            validate_text(
                string(payload, "externalId", "invalid_external_id")?,
                1,
                200,
                "invalid_external_id",
            )?;
            validate_text(
                string(payload, "displayName", "invalid_display_name")?,
                1,
                200,
                "invalid_display_name",
            )?;
        }
        Operation::PrincipalDisable => {
            validate_text(
                string(payload, "externalId", "invalid_external_id")?,
                1,
                200,
                "invalid_external_id",
            )?;
            if expected_version(payload)?.is_none() {
                return Err(Error::Invalid("expected_version_required"));
            }
        }
        Operation::RoleUpsert => {
            validate_code(string(payload, "code", "invalid_role_code")?)?;
            validate_text(
                string(payload, "name", "invalid_role_name")?,
                1,
                200,
                "invalid_role_name",
            )?;
        }
        Operation::RoleDisable => {
            validate_code(string(payload, "code", "invalid_role_code")?)?;
            if expected_version(payload)?.is_none() {
                return Err(Error::Invalid("expected_version_required"));
            }
        }
        Operation::PermissionGrant | Operation::RolePermissionGrant => {
            validate_assignment_target(operation, payload)?;
            validate_capability(payload)?;
            let scope = payload
                .get("dataScope")
                .cloned()
                .unwrap_or_else(|| json!({"mode":"unrestricted"}));
            validate_scope(&scope)?;
            validate_obligations(payload.get("obligations"))?;
            if expected_version(payload)?.is_none() {
                return Err(Error::Invalid("expected_version_required"));
            }
        }
        Operation::PermissionRevoke | Operation::RolePermissionRevoke => {
            validate_assignment_target(operation, payload)?;
            validate_capability(payload)?;
            if expected_version(payload)?.is_none() {
                return Err(Error::Invalid("expected_version_required"));
            }
        }
        Operation::RoleAssign | Operation::RoleUnassign => {
            validate_text(
                string(payload, "externalId", "invalid_external_id")?,
                1,
                200,
                "invalid_external_id",
            )?;
            validate_code(string(payload, "role", "invalid_role_code")?)?;
            if expected_version(payload)?.is_none() {
                return Err(Error::Invalid("expected_version_required"));
            }
        }
    }
    Ok(())
}

fn validate_assignment_target(operation: Operation, payload: &Value) -> Result<(), Error> {
    if matches!(
        operation,
        Operation::PermissionGrant | Operation::PermissionRevoke
    ) {
        validate_text(
            string(payload, "externalId", "invalid_external_id")?,
            1,
            200,
            "invalid_external_id",
        )
    } else {
        validate_code(string(payload, "role", "invalid_role_code")?)
    }
}

fn validate_code(value: &str) -> Result<(), Error> {
    let valid = (3..=128).contains(&value.len())
        && value.starts_with(|character: char| character.is_ascii_lowercase())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.:-".contains(&byte)
        });
    valid
        .then_some(())
        .ok_or(Error::Invalid("invalid_role_code"))
}

fn validate_capability(payload: &Value) -> Result<(), Error> {
    business_iam::Capability::parse(string(payload, "capability", "invalid_capability")?.to_owned())
        .map(|_| ())
        .map_err(|_| Error::Invalid("invalid_capability"))
}

fn validate_scope(value: &Value) -> Result<(), Error> {
    let scope = serde_json::from_value::<business_iam::DataScope>(value.clone())
        .map_err(|_| Error::Invalid("invalid_data_scope"))?;
    if let business_iam::DataScope::Restricted(dimensions) = scope {
        if dimensions.is_empty()
            || dimensions.iter().any(|(name, values)| {
                name.is_empty()
                    || name.len() > 128
                    || values.is_empty()
                    || values
                        .iter()
                        .any(|value| value.is_empty() || value.len() > 200)
            })
        {
            return Err(Error::Invalid("invalid_data_scope"));
        }
    }
    Ok(())
}

fn validate_obligations(value: Option<&Value>) -> Result<(), Error> {
    if let Some(value) = value {
        serde_json::from_value::<std::collections::BTreeSet<business_iam::Obligation>>(
            value.clone(),
        )
        .map_err(|_| Error::Invalid("invalid_obligations"))?;
    }
    Ok(())
}

fn payload_hash(operation: Operation, payload: &Value) -> Result<Vec<u8>, Error> {
    let encoded = serde_json::to_vec(&json!({
        "operation": operation.as_str(),
        "payload": payload,
    }))
    .map_err(|_| Error::Invalid("invalid_payload"))?;
    Ok(Sha256::digest(encoded).to_vec())
}

async fn risk_level(
    tx: &mut Transaction<'_, Postgres>,
    operation: Operation,
    payload: &Value,
) -> Result<&'static str, Error> {
    if matches!(
        operation,
        Operation::PrincipalDisable
            | Operation::RoleDisable
            | Operation::PermissionRevoke
            | Operation::RolePermissionRevoke
            | Operation::RoleUnassign
    ) {
        return Ok("critical");
    }
    if matches!(
        operation,
        Operation::PermissionGrant | Operation::RolePermissionGrant
    ) {
        let risk = sqlx::query_scalar::<_, String>(
            "SELECT risk_level FROM business_iam.permissions
             WHERE capability=$1 AND status='active'",
        )
        .bind(string(payload, "capability", "invalid_capability")?)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)?
        .ok_or(Error::Invalid("permission_not_found"))?;
        return Ok(if matches!(risk.as_str(), "high" | "critical") {
            "critical"
        } else {
            "high"
        });
    }
    if matches!(operation, Operation::RoleAssign) {
        let role_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM business_iam.roles WHERE code=$1 AND status='active')",
        )
        .bind(string(payload, "role", "invalid_role_code")?)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
        if !role_exists {
            return Err(Error::Invalid("role_not_found"));
        }
        let has_sensitive: bool = sqlx::query_scalar(
            "SELECT EXISTS(
               SELECT 1 FROM business_iam.roles role
               JOIN business_iam.role_permissions role_permission ON role_permission.role_id=role.id
               JOIN business_iam.permissions permission ON permission.id=role_permission.permission_id
               WHERE role.code=$1 AND permission.risk_level IN ('high','critical'))",
        )
        .bind(string(payload, "role", "invalid_role_code")?)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
        if has_sensitive {
            return Ok("critical");
        }
    }
    Ok("high")
}

fn parse_operation(value: &str) -> Result<Operation, Error> {
    match value {
        "principal_upsert" => Ok(Operation::PrincipalUpsert),
        "principal_disable" => Ok(Operation::PrincipalDisable),
        "role_upsert" => Ok(Operation::RoleUpsert),
        "role_disable" => Ok(Operation::RoleDisable),
        "permission_grant" => Ok(Operation::PermissionGrant),
        "permission_revoke" => Ok(Operation::PermissionRevoke),
        "role_permission_grant" => Ok(Operation::RolePermissionGrant),
        "role_permission_revoke" => Ok(Operation::RolePermissionRevoke),
        "role_assign" => Ok(Operation::RoleAssign),
        "role_unassign" => Ok(Operation::RoleUnassign),
        _ => Err(Error::Unavailable("invalid_persisted_operation")),
    }
}

async fn apply_change(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Actor,
    operation: Operation,
    payload: &Value,
) -> Result<(), Error> {
    match operation {
        Operation::PrincipalUpsert => apply_principal_upsert(tx, payload).await,
        Operation::PrincipalDisable => apply_principal_disable(tx, payload).await,
        Operation::RoleUpsert => apply_role_upsert(tx, payload).await,
        Operation::RoleDisable => apply_role_disable(tx, payload).await,
        Operation::PermissionGrant => apply_principal_permission(tx, actor, payload, true).await,
        Operation::PermissionRevoke => apply_principal_permission(tx, actor, payload, false).await,
        Operation::RolePermissionGrant => apply_role_permission(tx, payload, true).await,
        Operation::RolePermissionRevoke => apply_role_permission(tx, payload, false).await,
        Operation::RoleAssign => apply_role_assignment(tx, actor, payload, true).await,
        Operation::RoleUnassign => apply_role_assignment(tx, actor, payload, false).await,
    }
}

async fn apply_principal_upsert(
    tx: &mut Transaction<'_, Postgres>,
    payload: &Value,
) -> Result<(), Error> {
    let expected = expected_version(payload)?;
    let changed = sqlx::query(
        "INSERT INTO business_iam.principals(id,kind,external_id,display_name)
         VALUES($1,$2,$3,$4)
         ON CONFLICT(kind,external_id) DO UPDATE SET
           display_name=EXCLUDED.display_name,status='active',disabled_at=NULL,
           updated_at=now(),version=business_iam.principals.version+1
         WHERE $5::bigint IS NOT NULL AND business_iam.principals.version=$5",
    )
    .bind(Uuid::new_v4())
    .bind(string(payload, "kind", "invalid_principal_kind")?)
    .bind(string(payload, "externalId", "invalid_external_id")?)
    .bind(string(payload, "displayName", "invalid_display_name")?)
    .bind(expected)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?
    .rows_affected();
    (changed == 1)
        .then_some(())
        .ok_or(Error::Conflict("stale_principal_version"))
}

async fn apply_principal_disable(
    tx: &mut Transaction<'_, Postgres>,
    payload: &Value,
) -> Result<(), Error> {
    let changed = sqlx::query(
        "UPDATE business_iam.principals SET status='disabled',disabled_at=now(),
           updated_at=now(),version=version+1
         WHERE external_id=$1 AND status='active' AND version=$2",
    )
    .bind(string(payload, "externalId", "invalid_external_id")?)
    .bind(expected_version(payload)?.ok_or(Error::Invalid("expected_version_required"))?)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?
    .rows_affected();
    (changed == 1)
        .then_some(())
        .ok_or(Error::Conflict("stale_principal_version"))
}

async fn apply_role_upsert(
    tx: &mut Transaction<'_, Postgres>,
    payload: &Value,
) -> Result<(), Error> {
    let expected = expected_version(payload)?;
    let changed = sqlx::query(
        "INSERT INTO business_iam.roles(id,code,name) VALUES($1,$2,$3)
         ON CONFLICT(code) DO UPDATE SET name=EXCLUDED.name,status='active',
           updated_at=now(),version=business_iam.roles.version+1
         WHERE $4::bigint IS NOT NULL AND business_iam.roles.version=$4",
    )
    .bind(Uuid::new_v4())
    .bind(string(payload, "code", "invalid_role_code")?)
    .bind(string(payload, "name", "invalid_role_name")?)
    .bind(expected)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?
    .rows_affected();
    (changed == 1)
        .then_some(())
        .ok_or(Error::Conflict("stale_role_version"))
}

async fn apply_role_disable(
    tx: &mut Transaction<'_, Postgres>,
    payload: &Value,
) -> Result<(), Error> {
    let changed = sqlx::query(
        "UPDATE business_iam.roles SET status='disabled',updated_at=now(),version=version+1
         WHERE code=$1 AND status='active' AND version=$2",
    )
    .bind(string(payload, "code", "invalid_role_code")?)
    .bind(expected_version(payload)?.ok_or(Error::Invalid("expected_version_required"))?)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?
    .rows_affected();
    (changed == 1)
        .then_some(())
        .ok_or(Error::Conflict("stale_role_version"))
}

async fn lock_principal(
    tx: &mut Transaction<'_, Postgres>,
    payload: &Value,
) -> Result<Uuid, Error> {
    sqlx::query_scalar(
        "SELECT id FROM business_iam.principals
         WHERE external_id=$1 AND status='active' AND version=$2 FOR UPDATE",
    )
    .bind(string(payload, "externalId", "invalid_external_id")?)
    .bind(expected_version(payload)?.ok_or(Error::Invalid("expected_version_required"))?)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?
    .ok_or(Error::Conflict("stale_principal_version"))
}

async fn lock_role(tx: &mut Transaction<'_, Postgres>, payload: &Value) -> Result<Uuid, Error> {
    sqlx::query_scalar(
        "SELECT id FROM business_iam.roles
         WHERE code=$1 AND status='active' AND version=$2 FOR UPDATE",
    )
    .bind(string(payload, "role", "invalid_role_code")?)
    .bind(expected_version(payload)?.ok_or(Error::Invalid("expected_version_required"))?)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?
    .ok_or(Error::Conflict("stale_role_version"))
}

async fn permission_id(tx: &mut Transaction<'_, Postgres>, payload: &Value) -> Result<Uuid, Error> {
    sqlx::query_scalar(
        "SELECT id FROM business_iam.permissions WHERE capability=$1 AND status='active'",
    )
    .bind(string(payload, "capability", "invalid_capability")?)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?
    .ok_or(Error::Invalid("permission_not_found"))
}

async fn apply_principal_permission(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Actor,
    payload: &Value,
    grant: bool,
) -> Result<(), Error> {
    let principal_id = lock_principal(tx, payload).await?;
    let permission_id = permission_id(tx, payload).await?;
    let changed = if grant {
        sqlx::query(
            "INSERT INTO business_iam.principal_permissions(
               principal_id,permission_id,data_scope,obligations,granted_by,reason)
             VALUES($1,$2,$3,$4,$5,'approved_change_request')
             ON CONFLICT(principal_id,permission_id) DO UPDATE SET
               data_scope=EXCLUDED.data_scope,obligations=EXCLUDED.obligations,
               valid_from=now(),valid_until=NULL,granted_by=EXCLUDED.granted_by,
               reason=EXCLUDED.reason",
        )
        .bind(principal_id)
        .bind(permission_id)
        .bind(
            payload
                .get("dataScope")
                .cloned()
                .unwrap_or_else(|| json!({"mode":"unrestricted"})),
        )
        .bind(
            payload
                .get("obligations")
                .cloned()
                .unwrap_or_else(|| json!([])),
        )
        .bind(actor.principal_id)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?
        .rows_affected()
    } else {
        sqlx::query(
            "DELETE FROM business_iam.principal_permissions
             WHERE principal_id=$1 AND permission_id=$2",
        )
        .bind(principal_id)
        .bind(permission_id)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?
        .rows_affected()
    };
    if changed != 1 {
        return Err(Error::Conflict("permission_assignment_not_changed"));
    }
    sqlx::query(
        "UPDATE business_iam.principals SET version=version+1,updated_at=now() WHERE id=$1",
    )
    .bind(principal_id)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    Ok(())
}

async fn apply_role_permission(
    tx: &mut Transaction<'_, Postgres>,
    payload: &Value,
    grant: bool,
) -> Result<(), Error> {
    let role_id = lock_role(tx, payload).await?;
    let permission_id = permission_id(tx, payload).await?;
    let changed = if grant {
        sqlx::query(
            "INSERT INTO business_iam.role_permissions(role_id,permission_id,data_scope,obligations)
             VALUES($1,$2,$3,$4)
             ON CONFLICT(role_id,permission_id) DO UPDATE SET
               data_scope=EXCLUDED.data_scope,obligations=EXCLUDED.obligations",
        )
        .bind(role_id)
        .bind(permission_id)
        .bind(
            payload
                .get("dataScope")
                .cloned()
                .unwrap_or_else(|| json!({"mode":"unrestricted"})),
        )
        .bind(
            payload
                .get("obligations")
                .cloned()
                .unwrap_or_else(|| json!([])),
        )
        .execute(&mut **tx)
        .await
        .map_err(db_error)?
        .rows_affected()
    } else {
        sqlx::query(
            "DELETE FROM business_iam.role_permissions WHERE role_id=$1 AND permission_id=$2",
        )
        .bind(role_id)
        .bind(permission_id)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?
        .rows_affected()
    };
    if changed != 1 {
        return Err(Error::Conflict("role_permission_not_changed"));
    }
    sqlx::query("UPDATE business_iam.roles SET version=version+1,updated_at=now() WHERE id=$1")
        .bind(role_id)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    Ok(())
}

async fn apply_role_assignment(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Actor,
    payload: &Value,
    grant: bool,
) -> Result<(), Error> {
    let principal_id = lock_principal(tx, payload).await?;
    let role_id: Uuid =
        sqlx::query_scalar("SELECT id FROM business_iam.roles WHERE code=$1 AND status='active'")
            .bind(string(payload, "role", "invalid_role_code")?)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_error)?
            .ok_or(Error::Invalid("role_not_found"))?;
    let changed = if grant {
        sqlx::query(
            "INSERT INTO business_iam.principal_roles(principal_id,role_id,granted_by,reason)
             VALUES($1,$2,$3,'approved_change_request')
             ON CONFLICT(principal_id,role_id) DO UPDATE SET
               valid_from=now(),valid_until=NULL,granted_by=EXCLUDED.granted_by,
               reason=EXCLUDED.reason",
        )
        .bind(principal_id)
        .bind(role_id)
        .bind(actor.principal_id)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?
        .rows_affected()
    } else {
        sqlx::query("DELETE FROM business_iam.principal_roles WHERE principal_id=$1 AND role_id=$2")
            .bind(principal_id)
            .bind(role_id)
            .execute(&mut **tx)
            .await
            .map_err(db_error)?
            .rows_affected()
    };
    if changed != 1 {
        return Err(Error::Conflict("role_assignment_not_changed"));
    }
    sqlx::query(
        "UPDATE business_iam.principals SET version=version+1,updated_at=now() WHERE id=$1",
    )
    .bind(principal_id)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Actor,
    event_type: &'static str,
    result: &'static str,
    reason_code: Option<&'static str>,
    request_id: Option<Uuid>,
    trace_id: Uuid,
    metadata: Value,
) -> Result<(), Error> {
    sqlx::query(
        "INSERT INTO business_iam.admin_audit_events(
           id,event_type,result,reason_code,actor_principal_id,actor_issuer,
           actor_subject,change_request_id,trace_id,metadata)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(Uuid::new_v4())
    .bind(event_type)
    .bind(result)
    .bind(reason_code)
    .bind(actor.principal_id)
    .bind(&actor.issuer)
    .bind(&actor.subject)
    .bind(request_id)
    .bind(trace_id)
    .bind(metadata)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_fields_and_invalid_scopes() {
        let unknown = json!({
            "externalId":"agent-1",
            "capability":"inventory:read",
            "expectedVersion":1,
            "bypass":true
        });
        assert!(matches!(
            validate_payload(Operation::PermissionGrant, &unknown),
            Err(Error::Invalid("payload_field_not_allowed"))
        ));
        let empty_scope = json!({
            "externalId":"agent-1",
            "capability":"inventory:read",
            "expectedVersion":1,
            "dataScope":{"mode":"restricted","dimensions":{}}
        });
        assert!(matches!(
            validate_payload(Operation::PermissionGrant, &empty_scope),
            Err(Error::Invalid("invalid_data_scope"))
        ));
    }

    #[test]
    fn hashes_operation_with_payload() {
        let payload = json!({"externalId":"agent-1","expectedVersion":1});
        assert_ne!(
            payload_hash(Operation::PrincipalDisable, &payload).expect("hash"),
            payload_hash(Operation::RoleDisable, &payload).expect("hash")
        );
    }
}
