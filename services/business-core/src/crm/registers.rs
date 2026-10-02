use super::{model, CrmService, Filters};
use crate::b2::common::DomainError;
use serde_json::{json, Value};
use uuid::Uuid;

impl CrmService {
    /// Read scoped contact groups or follow-up history, filtering before pagination.
    pub async fn register(
        &self,
        actor: Uuid,
        filters: &Filters,
        contacts: bool,
    ) -> Result<Value, DomainError> {
        let scope = self.scope(actor, "crm:read").await?;
        model::text(filters.query.as_deref().unwrap_or(""), 160, false)?;
        if !(0..=100000).contains(&filters.offset) {
            return Err(DomainError::Invalid("无效页码".into()));
        }
        let sql = if contacts {
            r#"SELECT jsonb_build_object('id',c.id,'accountId',c.account_id,'customerId',a.customer_id,'version',c.version,
              'companyName',COALESCE(b.name,a.name),'contactName',c.name,'contactDetails',c.details,
              'opportunities',COALESCE((SELECT jsonb_agg(jsonb_build_object('id',o.id,'title',o.title) ORDER BY o.title,o.id)
                FROM crm_opportunities o WHERE o.contact_id=c.id AND o.legal_entity_id=ANY($1) AND o.business_unit_id=ANY($2) AND (o.customer_id IS NULL OR o.customer_id=ANY($3))),'[]'::jsonb))
              FROM crm_contacts c JOIN crm_accounts a ON a.id=c.account_id LEFT JOIN business_customers b ON b.id=a.customer_id
              WHERE ((a.customer_id IS NOT NULL AND a.customer_id=ANY($3)) OR (a.customer_id IS NULL AND (a.owner_user_id=$6 OR EXISTS(SELECT 1 FROM crm_opportunities o WHERE o.account_id=a.id AND o.legal_entity_id=ANY($1) AND o.business_unit_id=ANY($2) AND o.customer_id IS NULL))))
                AND ($4::text IS NULL OR strpos(lower(COALESCE(b.name,a.name)||' '||c.name||' '||c.details),lower($4))>0)
                AND ($7::uuid IS NULL OR c.account_id=$7)
              ORDER BY COALESCE(b.name,a.name),c.name,c.id LIMIT 51 OFFSET $5"#
        } else {
            r#"SELECT jsonb_build_object('id',f.id,'note',f.note,'stage',f.stage,
                'nextAction',f.next_action,'nextFollowUp',f.next_follow_up,'createdAt',f.created_at,
                'authorName',u.display_name,'lossReason',f.loss_reason,'opportunityId',o.id,'opportunityTitle',o.title,
                'companyName',o.company_name,'contactName',o.contact_name)
              FROM crm_followups f JOIN crm_opportunity_current o ON o.id=f.opportunity_id
                JOIN enterprise_users u ON u.id=f.author_user_id
              WHERE o.legal_entity_id=ANY($1) AND o.business_unit_id=ANY($2)
                AND (o.customer_id IS NULL OR o.customer_id=ANY($3))
                AND ($4::text IS NULL OR strpos(lower(o.title||' '||o.company_name||' '||o.contact_name||' '||f.note),lower($4))>0)
              AND $6::uuid IS NOT NULL AND ($7::uuid IS NULL OR o.account_id=$7) ORDER BY f.created_at DESC,f.id DESC LIMIT 51 OFFSET $5"#
        };
        let mut items: Vec<Value> = sqlx::query_scalar(sql)
            .bind(
                scope
                    .scopes
                    .legal_entity_ids
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
            )
            .bind(
                scope
                    .scopes
                    .business_unit_ids
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
            )
            .bind(
                scope
                    .scopes
                    .customer_ids
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
            )
            .bind(filters.query.as_deref().map(str::trim))
            .bind(filters.offset)
            .bind(actor)
            .bind(filters.account_id)
            .fetch_all(self.store.pool())
            .await?;
        let has_more = items.len() > 50;
        items.truncate(50);
        Ok(
            json!({"items":items,"hasMore":has_more,"canManage":scope.permission_keys.contains("crm:manage"),"businessUnitFilterMode":"subtree","businessUnitIds":scope.scopes.business_unit_ids}),
        )
    }
}
