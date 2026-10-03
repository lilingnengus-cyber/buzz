use super::*;
use serde::{Deserialize, Serialize};

/// Delete only the version reviewed by the caller.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteOpportunity {
    /// Current opportunity version.
    pub expected_version: i64,
}

impl CrmService {
    /// Hide an opportunity while preserving follow-ups and downstream records.
    pub async fn delete(
        &self,
        actor: Uuid,
        trace: Uuid,
        id: Uuid,
        key: &str,
        input: &DeleteOpportunity,
    ) -> Result<Value, DomainError> {
        let scope = self.scope(actor, "crm:manage").await?;
        // Authorize against the retained row so an uncertain result can be retried.
        let allowed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM crm_opportunities WHERE id=$1 AND legal_entity_id=ANY($2) AND business_unit_id=ANY($3) AND (customer_id IS NULL OR customer_id=ANY($4)))")
            .bind(id).bind(scope.scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>())
            .bind(scope.scopes.business_unit_ids.iter().copied().collect::<Vec<_>>())
            .bind(scope.scopes.customer_ids.iter().copied().collect::<Vec<_>>())
            .fetch_one(self.store.pool()).await?;
        if !allowed {
            return Err(DomainError::NotFoundOrForbidden);
        }
        let mut tx = self.store.pool().begin().await?;
        let hash = request_hash(&(id, input))?;
        if let Some(result) =
            begin_idempotent::<Value>(&mut tx, actor, "crm:delete", key, &hash).await?
        {
            tx.commit().await?;
            return Ok(result);
        }
        let version: Option<i64> = sqlx::query_scalar("UPDATE crm_opportunities SET deleted_at=now(),updated_at=now(),version=version+1 WHERE id=$1 AND version=$2 AND deleted_at IS NULL RETURNING version")
            .bind(id).bind(input.expected_version).fetch_optional(&mut *tx).await?;
        let version = version.ok_or(DomainError::VersionConflict)?;
        let result = json!({"id":id,"version":version,"deleted":true,"traceId":trace});
        record(
            &mut tx,
            trace,
            actor,
            "crm.opportunity.deleted",
            "crm.opportunity.deleted",
            "crm_opportunity",
            id,
            json!({"version":version}),
        )
        .await?;
        finish_idempotent(&mut tx, actor, "crm:delete", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
}
