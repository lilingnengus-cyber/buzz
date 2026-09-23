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
            r#"WITH visible AS (
                SELECT * FROM crm_opportunities
                WHERE legal_entity_id=ANY($1) AND business_unit_id=ANY($2)
                  AND (customer_id IS NULL OR customer_id=ANY($3))
                  AND contact_name <> ''
            ), grouped AS (
                SELECT legal_entity_id,business_unit_id,customer_id,company_name,contact_name,contact_details,
                    jsonb_agg(jsonb_build_object('id',id,'title',title) ORDER BY title,id) AS opportunities
                FROM visible
                GROUP BY legal_entity_id,business_unit_id,customer_id,company_name,contact_name,contact_details
            ) SELECT jsonb_build_object('companyName',company_name,'contactName',contact_name,
                'contactDetails',contact_details,'opportunities',opportunities)
              FROM grouped
              WHERE ($4::text IS NULL OR strpos(lower(company_name||' '||contact_name||' '||contact_details),lower($4))>0)
              ORDER BY company_name,contact_name,contact_details,legal_entity_id,business_unit_id,customer_id
              LIMIT 51 OFFSET $5"#
        } else {
            r#"SELECT jsonb_build_object('id',f.id,'note',f.note,'stage',f.stage,
                'nextAction',f.next_action,'nextFollowUp',f.next_follow_up,'createdAt',f.created_at,
                'authorName',u.display_name,'opportunityId',o.id,'opportunityTitle',o.title,
                'companyName',o.company_name,'contactName',o.contact_name)
              FROM crm_followups f JOIN crm_opportunities o ON o.id=f.opportunity_id
                JOIN enterprise_users u ON u.id=f.author_user_id
              WHERE o.legal_entity_id=ANY($1) AND o.business_unit_id=ANY($2)
                AND (o.customer_id IS NULL OR o.customer_id=ANY($3))
                AND ($4::text IS NULL OR strpos(lower(o.title||' '||o.company_name||' '||o.contact_name||' '||f.note),lower($4))>0)
              ORDER BY f.created_at DESC,f.id DESC LIMIT 51 OFFSET $5"#
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
            .fetch_all(self.store.pool())
            .await?;
        let has_more = items.len() > 50;
        items.truncate(50);
        Ok(
            json!({"items":items,"hasMore":has_more,"businessUnitFilterMode":"subtree","businessUnitIds":scope.scopes.business_unit_ids}),
        )
    }
}
