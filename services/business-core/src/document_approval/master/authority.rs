//! Hold a concrete current permission witness through approval commit.
use super::*;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};

pub(super) async fn permission(
    tx: &mut Transaction<'_, Postgres>,
    actor: Uuid,
    capability: &str,
) -> Result<Option<DateTime<Utc>>, StoreError> {
    let core = sqlx::query("SELECT rp.permission_key FROM business_role_permissions rp JOIN business_user_roles ur ON ur.role_id=rp.role_id JOIN business_roles r ON r.id=rp.role_id WHERE ur.enterprise_user_id=$1 AND r.status='active' AND rp.permission_key=$2 FOR SHARE OF rp,ur,r")
        .bind(actor).bind(capability).fetch_all(&mut **tx).await?;
    if !core.is_empty() {
        return Ok(None);
    }
    // Core's permission snapshot only imports unrestricted, obligation-free IAM
    // permissions. Lock precisely the same witnesses, including role assignments.
    let mut deadlines: Vec<Option<DateTime<Utc>>> = sqlx::query_scalar("SELECT g.valid_until FROM business_iam.principals p JOIN business_iam.principal_permissions g ON g.principal_id=p.id JOIN business_iam.permissions permission ON permission.id=g.permission_id WHERE p.kind='human' AND p.status='active' AND p.external_id=$1 AND permission.capability=$2 AND permission.status='active' AND g.valid_from<=clock_timestamp() AND (g.valid_until IS NULL OR g.valid_until>clock_timestamp()) AND g.data_scope->>'mode'='unrestricted' AND g.obligations='[]'::jsonb AND permission.obligations='[]'::jsonb FOR SHARE OF p,g,permission")
        .bind(actor.to_string()).bind(capability).fetch_all(&mut **tx).await?;
    let indirect: Vec<Option<DateTime<Utc>>> = sqlx::query_scalar("SELECT assignment.valid_until FROM business_iam.principals p JOIN business_iam.principal_roles assignment ON assignment.principal_id=p.id JOIN business_iam.roles role ON role.id=assignment.role_id JOIN business_iam.role_permissions g ON g.role_id=role.id JOIN business_iam.permissions permission ON permission.id=g.permission_id WHERE p.kind='human' AND p.status='active' AND p.external_id=$1 AND role.status='active' AND permission.capability=$2 AND permission.status='active' AND assignment.valid_from<=clock_timestamp() AND (assignment.valid_until IS NULL OR assignment.valid_until>clock_timestamp()) AND g.data_scope->>'mode'='unrestricted' AND g.obligations='[]'::jsonb AND permission.obligations='[]'::jsonb FOR SHARE OF p,assignment,role,g,permission")
        .bind(actor.to_string()).bind(capability).fetch_all(&mut **tx).await?;
    deadlines.extend(indirect);
    if deadlines.is_empty() {
        return Err(StoreError::NotFoundOrForbidden);
    }
    if deadlines.iter().any(Option::is_none) {
        return Ok(None);
    }
    Ok(deadlines.into_iter().flatten().max())
}
