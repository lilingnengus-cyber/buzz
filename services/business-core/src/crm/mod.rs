//! Minimal presales CRM, sharing Business Core identity, scopes and command audit.
pub mod api;
mod conversion;
mod deletion;
pub use deletion::DeleteOpportunity;
mod directory;
pub use conversion::ConvertCustomer;
mod model;
mod ownership;
pub use ownership::OwnerScope;
mod leads;
mod registers;
use crate::{
    b2::common::{
        authorize, begin_idempotent, finish_idempotent, record, request_hash, DomainError,
    },
    store::PgStore,
};
pub use directory::{SaveAccount, SaveContact};
pub use leads::{ConvertLead, LeadFilters, LeadFollowup, SaveLead};
pub use model::{AddFollowup, Filters, Opportunity, SaveOpportunity};
use serde_json::{json, Value};
use uuid::Uuid;

/// Durable CRM operations; no financial document is created by this service.
#[derive(Clone)]
pub struct CrmService {
    store: PgStore,
}
impl CrmService {
    /// Reuse the current Business Core store and authorization boundary.
    pub fn new(store: PgStore) -> Self {
        Self { store }
    }
    async fn scope(
        &self,
        actor: Uuid,
        permission: &str,
    ) -> Result<crate::model::AuthorizationSnapshot, DomainError> {
        authorize(&self.store, actor, permission, None, None, None, None, None).await
    }
    /// List a page after scope filtering, with one lookahead row.
    pub async fn list(&self, actor: Uuid, filters: &Filters) -> Result<Value, DomainError> {
        let s = self.scope(actor, "crm:read").await?;
        if let Some(v) = &filters.stage {
            model::stage(v)?;
        }
        model::text(filters.query.as_deref().unwrap_or(""), 160, false)?;
        if !(0..=100000).contains(&filters.offset) {
            return Err(DomainError::Invalid("无效页码".into()));
        }
        if let Some(mode) = &filters.followup {
            if !["overdue", "today", "upcoming", "unscheduled", "open"].contains(&mode.as_str())
                || filters.today.is_none()
            {
                return Err(DomainError::Invalid("无效跟进筛选或本地日期".into()));
            }
        }
        let mut items=sqlx::query_as::<_,Opportunity>("SELECT * FROM crm_opportunity_current WHERE legal_entity_id=ANY($1) AND business_unit_id=ANY($2) AND (customer_id IS NULL OR customer_id=ANY($3)) AND ($4::text IS NULL OR strpos(lower(title||' '||company_name||' '||contact_name),lower($4))>0) AND ($5::text IS NULL OR stage=$5) AND ($6::date IS NULL OR (next_follow_up <= $6 AND stage NOT IN ('won','lost'))) AND ($8::text IS NULL OR (stage NOT IN ('won','lost') AND CASE $8 WHEN 'overdue' THEN next_follow_up < $9::date WHEN 'today' THEN next_follow_up = $9::date WHEN 'upcoming' THEN next_follow_up > $9::date AND next_follow_up <= $9::date + 7 WHEN 'unscheduled' THEN next_follow_up IS NULL WHEN 'open' THEN true ELSE false END)) AND (NOT $10::boolean OR owner_user_id=$11) ORDER BY next_follow_up ASC NULLS LAST,created_at DESC,id LIMIT 51 OFFSET $7")
            .bind(s.scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>()).bind(s.scopes.business_unit_ids.iter().copied().collect::<Vec<_>>()).bind(s.scopes.customer_ids.iter().copied().collect::<Vec<_>>())
            .bind(filters.query.as_deref().map(str::trim)).bind(&filters.stage).bind(filters.due_by).bind(filters.offset).bind(&filters.followup).bind(filters.today).bind(filters.mine).bind(actor)
            .fetch_all(self.store.pool()).await?;
        let has_more = items.len() > 50;
        items.truncate(50);
        Ok(
            json!({"items":items,"hasMore":has_more,"canManage":s.permission_keys.contains("crm:manage"),"businessUnitFilterMode":"subtree","businessUnitIds":s.scopes.business_unit_ids}),
        )
    }
    async fn accessible(
        &self,
        actor: Uuid,
        id: Uuid,
        permission: &str,
    ) -> Result<Opportunity, DomainError> {
        let s = self.scope(actor, permission).await?;
        sqlx::query_as::<_,Opportunity>("SELECT * FROM crm_opportunity_current WHERE id=$1 AND legal_entity_id=ANY($2) AND business_unit_id=ANY($3) AND (customer_id IS NULL OR customer_id=ANY($4))")
            .bind(id).bind(s.scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>()).bind(s.scopes.business_unit_ids.iter().copied().collect::<Vec<_>>()).bind(s.scopes.customer_ids.iter().copied().collect::<Vec<_>>())
            .fetch_optional(self.store.pool()).await?.ok_or(DomainError::NotFoundOrForbidden)
    }
    /// Read an opportunity and a bounded page of immutable follow-up notes.
    pub async fn detail(&self, actor: Uuid, id: Uuid, offset: i64) -> Result<Value, DomainError> {
        if !(0..=100000).contains(&offset) {
            return Err(DomainError::Invalid("无效页码".into()));
        }
        let item = self.accessible(actor, id, "crm:read").await?;
        let notes:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',f.id,'note',f.note,'stage',f.stage,'nextAction',f.next_action,'nextFollowUp',f.next_follow_up,'createdAt',f.created_at,'authorName',u.display_name,'lossReason',f.loss_reason) FROM crm_followups f JOIN enterprise_users u ON u.id=f.author_user_id WHERE opportunity_id=$1 ORDER BY f.created_at DESC,f.id DESC LIMIT 101 OFFSET $2")
            .bind(id).bind(offset).fetch_all(self.store.pool()).await?;
        let source_lead: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM crm_leads WHERE converted_opportunity_id=$1")
                .bind(id)
                .fetch_optional(self.store.pool())
                .await?;
        let has_more = notes.len() > 100;
        Ok(
            json!({"item":item,"sourceLeadId":source_lead,"followups":notes.into_iter().take(100).collect::<Vec<_>>(),"hasOlderFollowups":has_more}),
        )
    }
    /// Return active, scoped choices without requiring unrelated master-data manage access.
    pub async fn options(&self, actor: Uuid) -> Result<Value, DomainError> {
        let s = self.scope(actor, "crm:read").await?;
        let items:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'name',name,'code',code,'status',status,'resourceType',resource_type,'legalEntityId',CASE WHEN resource_type='business_unit' THEN NULL ELSE legal_entity_id END,'businessUnitId',business_unit_id,'parentBusinessUnitId',CASE WHEN resource_type='business_unit' THEN (SELECT parent_business_unit_id FROM business_units unit WHERE unit.id=business_master_data_directory.id) ELSE NULL END,'ancestorPath',CASE WHEN resource_type='business_unit' THEN (SELECT business_unit_path FROM core_master_data_maintenance tree WHERE tree.resource_type='business_unit' AND tree.id=business_master_data_directory.id) ELSE NULL END,'depth',CASE WHEN resource_type='business_unit' THEN (SELECT business_unit_depth FROM core_master_data_maintenance tree WHERE tree.resource_type='business_unit' AND tree.id=business_master_data_directory.id) ELSE NULL END,'descendantCount',CASE WHEN resource_type='business_unit' THEN (SELECT descendant_count FROM core_master_data_maintenance tree WHERE tree.resource_type='business_unit' AND tree.id=business_master_data_directory.id) ELSE NULL END) FROM business_master_data_directory WHERE status='active' AND ((resource_type='legal_entity' AND id=ANY($1)) OR (resource_type='business_unit' AND id=ANY($2)) OR (resource_type='customer' AND id=ANY($3))) ORDER BY name,id")
            .bind(s.scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>()).bind(s.scopes.business_unit_ids.iter().copied().collect::<Vec<_>>()).bind(s.scopes.customer_ids.iter().copied().collect::<Vec<_>>()).fetch_all(self.store.pool()).await?;
        Ok(json!({"items":items}))
    }
    /// Create or replace fields with idempotency and optimistic concurrency.
    pub async fn save(
        &self,
        actor: Uuid,
        trace: Uuid,
        id: Option<Uuid>,
        key: &str,
        input: &SaveOpportunity,
    ) -> Result<Value, DomainError> {
        self.save_from_lead(actor, trace, id, key, input, None)
            .await
    }
    async fn save_from_lead(
        &self,
        actor: Uuid,
        trace: Uuid,
        id: Option<Uuid>,
        key: &str,
        input: &SaveOpportunity,
        lead: Option<(Uuid, i64)>,
    ) -> Result<Value, DomainError> {
        input.validate()?;
        if input.stage == "won" && input.customer_id.is_none() {
            return Err(DomainError::Invalid(
                "请通过成交转客户确认正式客户与联系人资料".into(),
            ));
        }
        authorize(
            &self.store,
            actor,
            "crm:manage",
            Some(input.legal_entity_id),
            None,
            input.customer_id,
            None,
            Some(input.business_unit_id),
        )
        .await?;
        let previous = if let Some(id) = id {
            let old = self.accessible(actor, id, "crm:manage").await?;
            if old.legal_entity_id != input.legal_entity_id {
                return Err(DomainError::Invalid("商机法人主体不可更改".into()));
            }
            Some(old)
        } else {
            None
        };
        if id.is_some() != input.expected_version.is_some() {
            return Err(DomainError::Invalid("更新需要当前版本".into()));
        }
        let mut tx = self.store.pool().begin().await?;
        let hash = if let Some(lead) = lead {
            request_hash(&(id, input, lead))?
        } else {
            request_hash(&(id, input))?
        };
        if let Some(result) =
            begin_idempotent::<Value>(&mut tx, actor, "crm:save", key, &hash).await?
        {
            tx.commit().await?;
            return Ok(result);
        }
        if let Some((lead_id, version)) = lead {
            let status = self.lock_lead(&mut tx, actor, lead_id, version).await?;
            if status == "disqualified" {
                return Err(DomainError::Invalid("请先重新跟进，再转为商机".into()));
            }
        }
        let owner = input
            .owner_user_id
            .or(previous
                .as_ref()
                .filter(|old| old.business_unit_id == input.business_unit_id)
                .map(|old| old.owner_user_id))
            .unwrap_or(actor);
        if previous.as_ref().is_none_or(|old| {
            old.owner_user_id != owner
                || old.customer_id != input.customer_id
                || old.business_unit_id != input.business_unit_id
        }) {
            authorize(
                &self.store,
                owner,
                "crm:manage",
                Some(input.legal_entity_id),
                None,
                input.customer_id,
                None,
                Some(input.business_unit_id),
            )
            .await?;
        }
        let close_date = input
            .expected_close_date
            .unwrap_or_else(|| previous.as_ref().and_then(|old| old.expected_close_date));
        let loss_reason = model::loss_reason(
            &input.stage,
            input.loss_reason.as_deref(),
            previous.as_ref().map(|old| old.loss_reason.as_str()),
        )?;
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM business_units WHERE id=$1 AND status='active') AND EXISTS(SELECT 1 FROM business_legal_entities WHERE id=$2 AND status='active') AND ($3::uuid IS NULL OR EXISTS(SELECT 1 FROM business_customers WHERE id=$3 AND status='active'))")
            .bind(input.business_unit_id).bind(input.legal_entity_id).bind(input.customer_id).fetch_one(&mut *tx).await?;
        if !valid {
            return Err(DomainError::NotFoundOrForbidden);
        }
        let (account_id, contact_id) = self.resolve_directory(actor, input, &mut tx).await?;
        let record_id = id.unwrap_or_else(Uuid::new_v4);
        let version: Option<i64> = if id.is_none() {
            sqlx::query_scalar("INSERT INTO crm_opportunities(id,legal_entity_id,business_unit_id,customer_id,title,company_name,contact_name,contact_details,stage,expected_amount_minor,currency,next_action,next_follow_up,owner_user_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) RETURNING version")
                .bind(record_id).bind(input.legal_entity_id).bind(input.business_unit_id).bind(input.customer_id).bind(input.title.trim()).bind(input.company_name.trim()).bind(input.contact_name.trim()).bind(input.contact_details.trim()).bind(&input.stage).bind(input.expected_amount_minor).bind(&input.currency).bind(input.next_action.trim()).bind(input.next_follow_up).bind(actor).fetch_optional(&mut *tx).await?
        } else {
            sqlx::query_scalar("UPDATE crm_opportunities SET customer_id=$2,title=$3,company_name=$4,contact_name=$5,contact_details=$6,stage=$7,expected_amount_minor=$8,currency=$9,next_action=$10,next_follow_up=$11,version=version+1,updated_at=now() WHERE id=$1 AND version=$12 AND deleted_at IS NULL RETURNING version")
                .bind(record_id).bind(input.customer_id).bind(input.title.trim()).bind(input.company_name.trim()).bind(input.contact_name.trim()).bind(input.contact_details.trim()).bind(&input.stage).bind(input.expected_amount_minor).bind(&input.currency).bind(input.next_action.trim()).bind(input.next_follow_up).bind(input.expected_version).fetch_optional(&mut *tx).await?
        };
        let version = version.ok_or(DomainError::VersionConflict)?;
        sqlx::query("UPDATE crm_opportunities SET account_id=$2,contact_id=$3,owner_user_id=$4,expected_close_date=$5,loss_reason=$6,business_unit_id=$7 WHERE id=$1")
            .bind(record_id)
            .bind(account_id)
            .bind(contact_id)
            .bind(owner).bind(close_date).bind(&loss_reason).bind(input.business_unit_id)
            .execute(&mut *tx)
            .await?;
        if previous
            .as_ref()
            .is_some_and(|old| old.stage != input.stage || old.loss_reason != loss_reason)
        {
            let label = match input.stage.as_str() {
                "new" => "新线索",
                "contacting" => "沟通中",
                "quoting" => "报价中",
                "won" => "已成交",
                _ => "已流失",
            };
            sqlx::query("INSERT INTO crm_followups(id,opportunity_id,author_user_id,note,stage,next_action,next_follow_up,loss_reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
                .bind(Uuid::new_v4()).bind(record_id).bind(actor).bind(format!("商机资料更新：阶段为{label}。"))
                .bind(&input.stage).bind(input.next_action.trim()).bind(input.next_follow_up).bind(&loss_reason).execute(&mut *tx).await?;
        }
        if let Some((lead_id, _)) = lead {
            sqlx::query("UPDATE crm_leads SET status='converted',converted_opportunity_id=$2,version=version+1,updated_at=now() WHERE id=$1").bind(lead_id).bind(record_id).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO crm_followups(id,opportunity_id,author_user_id,note,stage,next_action,next_follow_up,created_at,source_lead_id,loss_reason) SELECT gen_random_uuid(),$2,author_user_id,note,'contacting',next_action,next_follow_up,created_at,lead_id,disqualification_reason FROM crm_lead_followups WHERE lead_id=$1").bind(lead_id).bind(record_id).execute(&mut *tx).await?;
            let summary: String = sqlx::query_scalar("SELECT summary FROM crm_leads WHERE id=$1")
                .bind(lead_id)
                .fetch_one(&mut *tx)
                .await?;
            if !summary.trim().is_empty() {
                sqlx::query("INSERT INTO crm_followups(id,opportunity_id,author_user_id,note,stage,next_action,next_follow_up,source_lead_id) VALUES($1,$2,$3,$4,'contacting',$5,$6,$7)").bind(Uuid::new_v4()).bind(record_id).bind(actor).bind(summary).bind(&input.next_action).bind(input.next_follow_up).bind(lead_id).execute(&mut *tx).await?;
            }
            record(
                &mut tx,
                trace,
                actor,
                "crm.lead.converted",
                "crm.lead.converted",
                "crm_lead",
                lead_id,
                json!({"opportunityId":record_id}),
            )
            .await?;
        }
        let result = json!({"id":record_id,"version":version,"traceId":trace});
        record(
            &mut tx,
            trace,
            actor,
            "crm.opportunity.saved",
            "crm.opportunity.saved",
            "crm_opportunity",
            record_id,
            json!({"version":version,"stage":input.stage,"ownerUserId":owner,"expectedCloseDate":close_date,"lossReason":loss_reason,"businessUnitId":input.business_unit_id,"previousBusinessUnitId":previous.as_ref().map(|old| old.business_unit_id)}),
        )
        .await?;
        finish_idempotent(&mut tx, actor, "crm:save", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
    /// Append a note and advance stage/next action exactly once.
    pub async fn followup(
        &self,
        actor: Uuid,
        trace: Uuid,
        id: Uuid,
        key: &str,
        input: &AddFollowup,
    ) -> Result<Value, DomainError> {
        model::text(&input.note, 4000, true)?;
        model::text(&input.next_action, 500, false)?;
        model::stage(&input.stage)?;
        let previous = self.accessible(actor, id, "crm:manage").await?;
        if input.stage == "won" && previous.customer_id.is_none() {
            return Err(DomainError::Invalid(
                "请通过成交转客户确认正式客户与联系人资料".into(),
            ));
        }
        let mut tx = self.store.pool().begin().await?;
        let hash = request_hash(&(id, input))?;
        if let Some(result) =
            begin_idempotent::<Value>(&mut tx, actor, "crm:followup", key, &hash).await?
        {
            tx.commit().await?;
            return Ok(result);
        }
        let loss_reason = model::loss_reason(
            &input.stage,
            input.loss_reason.as_deref(),
            Some(&previous.loss_reason),
        )?;
        let version:Option<i64>=sqlx::query_scalar("UPDATE crm_opportunities SET stage=$2,next_action=$3,next_follow_up=$4,loss_reason=$6,version=version+1,updated_at=now() WHERE id=$1 AND version=$5 AND deleted_at IS NULL RETURNING version")
            .bind(id).bind(&input.stage).bind(input.next_action.trim()).bind(input.next_follow_up).bind(input.expected_version).bind(&loss_reason).fetch_optional(&mut *tx).await?;
        let version = version.ok_or(DomainError::VersionConflict)?;
        sqlx::query("INSERT INTO crm_followups(id,opportunity_id,author_user_id,note,stage,next_action,next_follow_up,loss_reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(Uuid::new_v4()).bind(id).bind(actor).bind(input.note.trim()).bind(&input.stage).bind(input.next_action.trim()).bind(input.next_follow_up).bind(&loss_reason).execute(&mut *tx).await?;
        let result = json!({"id":id,"version":version,"traceId":trace});
        record(
            &mut tx,
            trace,
            actor,
            "crm.followup.added",
            "crm.followup.added",
            "crm_opportunity",
            id,
            json!({"version":version,"stage":input.stage,"lossReason":loss_reason}),
        )
        .await?;
        finish_idempotent(&mut tx, actor, "crm:followup", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
}
