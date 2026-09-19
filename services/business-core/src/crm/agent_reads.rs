//! Current scoped, bounded CRM source reads for the agent adapter.
use super::{CrmService, Opportunity};
use crate::{b2::common::DomainError, store::PgStore};
use business_query_contracts::{
    GetCrmOpportunityInput, SearchCrmOpportunitiesInput, ValidateInput,
};
use serde_json::{json, Value};
use uuid::Uuid;

impl CrmService {
    /// Locate scoped opportunity summaries before pagination.
    pub async fn agent_search(
        &self,
        actor: Uuid,
        mut input: SearchCrmOpportunitiesInput,
    ) -> Result<Value, DomainError> {
        input
            .validate_and_normalize(chrono::Utc::now().date_naive())
            .map_err(|_| DomainError::Invalid("invalid CRM lookup".into()))?;
        let scope = self.scope(actor, "crm:read").await?;
        let mut items:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'legalEntityId',legal_entity_id,'businessUnitId',business_unit_id,'customerId',customer_id,'title',title,'companyName',company_name,'contactName',contact_name,'stage',stage,'expectedAmountMinor',expected_amount_minor,'currency',currency,'nextFollowUp',next_follow_up,'version',version) FROM crm_opportunities WHERE legal_entity_id=ANY($1) AND business_unit_id=ANY($2) AND (customer_id IS NULL OR customer_id=ANY($3)) AND ($4::uuid IS NULL OR id=$4) AND ($5::text IS NULL OR strpos(lower(title||' '||company_name||' '||contact_name),lower($5))>0) AND ($6::uuid IS NULL OR legal_entity_id=$6) AND ($7::uuid IS NULL OR business_unit_id=$7) AND ($8::uuid IS NULL OR customer_id=$8) AND ($9::text IS NULL OR stage=$9) AND ($10::date IS NULL OR (next_follow_up<=$10 AND stage NOT IN ('won','lost'))) ORDER BY next_follow_up ASC NULLS LAST,created_at DESC,id LIMIT $11 OFFSET $12")
            .bind(scope.scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>()).bind(scope.scopes.business_unit_ids.iter().copied().collect::<Vec<_>>()).bind(scope.scopes.customer_ids.iter().copied().collect::<Vec<_>>())
            .bind(input.document_id).bind(input.query).bind(input.legal_entity_id).bind(input.business_unit_id).bind(input.customer_id).bind(input.stage).bind(input.due_by).bind(i64::from(input.limit)+1).bind(i64::from(input.offset)).fetch_all(self.store.pool()).await?;
        let has_more = items.len() > input.limit as usize;
        items.truncate(input.limit as usize);
        Ok(
            json!({"items":items,"hasMore":has_more,"nextOffset":has_more.then_some(input.offset+input.limit)}),
        )
    }
    /// Read one stable opportunity version and at most three full follow-up notes.
    pub async fn agent_detail(
        &self,
        actor: Uuid,
        mut input: GetCrmOpportunityInput,
    ) -> Result<Value, DomainError> {
        input
            .validate_and_normalize(chrono::Utc::now().date_naive())
            .map_err(|_| DomainError::Invalid("invalid CRM detail page".into()))?;
        let mut tx = self.store.pool().begin().await?;
        let item = sqlx::query_as::<_, Opportunity>(
            "SELECT * FROM crm_opportunities WHERE id=$1 FOR SHARE",
        )
        .bind(input.document_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
        sqlx::query(
            "SELECT revision FROM business_authorization_revision WHERE singleton FOR SHARE",
        )
        .fetch_one(&mut *tx)
        .await?;
        let scope = PgStore::snapshot_on(&mut tx, actor)
            .await
            .map_err(|_| DomainError::NotFoundOrForbidden)?;
        if !scope.permission_keys.contains("crm:read")
            || !scope
                .scopes
                .legal_entity_ids
                .contains(&item.legal_entity_id)
            || !scope
                .scopes
                .business_unit_ids
                .contains(&item.business_unit_id)
            || item
                .customer_id
                .is_some_and(|v| !scope.scopes.customer_ids.contains(&v))
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
        if input.expected_version.is_some_and(|v| v != item.version) {
            return Err(DomainError::VersionConflict);
        }
        let mut notes:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'note',note,'stage',stage,'nextAction',next_action,'nextFollowUp',next_follow_up,'createdAt',created_at,'authorUserId',author_user_id) FROM crm_followups WHERE opportunity_id=$1 ORDER BY created_at DESC,id DESC LIMIT $2 OFFSET $3")
            .bind(input.document_id).bind(i64::from(input.limit)+1).bind(i64::from(input.offset)).fetch_all(&mut *tx).await?;
        let has_more = notes.len() > input.limit as usize;
        notes.truncate(input.limit as usize);
        tx.commit().await?;
        Ok(
            json!({"item":item,"followups":notes,"hasMore":has_more,"nextOffset":has_more.then_some(input.offset+input.limit)}),
        )
    }
}
