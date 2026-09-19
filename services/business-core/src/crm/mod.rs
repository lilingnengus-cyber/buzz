//! Minimal presales CRM, sharing Business Core identity, scopes and command audit.
pub mod api;
mod command;
mod model;
pub use command::CrmCommand;
mod registers;
mod write_authority;
use crate::{
    b2::common::{
        authorize, begin_idempotent, finish_idempotent, record, request_hash, DomainError,
    },
    store::PgStore,
};
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
        let mut items=sqlx::query_as::<_,Opportunity>("SELECT * FROM crm_opportunities WHERE legal_entity_id=ANY($1) AND business_unit_id=ANY($2) AND (customer_id IS NULL OR customer_id=ANY($3)) AND ($4::text IS NULL OR strpos(lower(title||' '||company_name||' '||contact_name),lower($4))>0) AND ($5::text IS NULL OR stage=$5) AND ($6::date IS NULL OR (next_follow_up <= $6 AND stage NOT IN ('won','lost'))) ORDER BY next_follow_up ASC NULLS LAST,created_at DESC,id LIMIT 51 OFFSET $7")
            .bind(s.scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>()).bind(s.scopes.business_unit_ids.iter().copied().collect::<Vec<_>>()).bind(s.scopes.customer_ids.iter().copied().collect::<Vec<_>>())
            .bind(filters.query.as_deref().map(str::trim)).bind(&filters.stage).bind(filters.due_by).bind(filters.offset)
            .fetch_all(self.store.pool()).await?;
        let has_more = items.len() > 50;
        items.truncate(50);
        Ok(
            json!({"items":items,"hasMore":has_more,"canManage":s.permission_keys.contains("crm:manage")}),
        )
    }
    async fn accessible(
        &self,
        actor: Uuid,
        id: Uuid,
        permission: &str,
    ) -> Result<Opportunity, DomainError> {
        let s = self.scope(actor, permission).await?;
        sqlx::query_as::<_,Opportunity>("SELECT * FROM crm_opportunities WHERE id=$1 AND legal_entity_id=ANY($2) AND business_unit_id=ANY($3) AND (customer_id IS NULL OR customer_id=ANY($4))")
            .bind(id).bind(s.scopes.legal_entity_ids.iter().copied().collect::<Vec<_>>()).bind(s.scopes.business_unit_ids.iter().copied().collect::<Vec<_>>()).bind(s.scopes.customer_ids.iter().copied().collect::<Vec<_>>())
            .fetch_optional(self.store.pool()).await?.ok_or(DomainError::NotFoundOrForbidden)
    }
    /// Read an opportunity and a bounded page of immutable follow-up notes.
    pub async fn detail(&self, actor: Uuid, id: Uuid, offset: i64) -> Result<Value, DomainError> {
        if !(0..=100000).contains(&offset) {
            return Err(DomainError::Invalid("无效页码".into()));
        }
        let item = self.accessible(actor, id, "crm:read").await?;
        let notes:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',f.id,'note',f.note,'stage',f.stage,'nextAction',f.next_action,'nextFollowUp',f.next_follow_up,'createdAt',f.created_at,'authorName',u.display_name) FROM crm_followups f JOIN enterprise_users u ON u.id=f.author_user_id WHERE opportunity_id=$1 ORDER BY f.created_at DESC,f.id DESC LIMIT 101 OFFSET $2")
            .bind(id).bind(offset).fetch_all(self.store.pool()).await?;
        let has_more = notes.len() > 100;
        Ok(
            json!({"item":item,"followups":notes.into_iter().take(100).collect::<Vec<_>>(),"hasOlderFollowups":has_more}),
        )
    }
    /// Return active, scoped choices without requiring unrelated master-data manage access.
    pub async fn options(&self, actor: Uuid) -> Result<Value, DomainError> {
        let s = self.scope(actor, "crm:read").await?;
        let items:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'name',name,'code',code,'resourceType',resource_type,'legalEntityId',legal_entity_id,'businessUnitId',business_unit_id) FROM business_master_data_directory WHERE status='active' AND ((resource_type='legal_entity' AND id=ANY($1)) OR (resource_type='business_unit' AND id=ANY($2) AND legal_entity_id=ANY($1)) OR (resource_type='customer' AND id=ANY($3) AND legal_entity_id=ANY($1) AND business_unit_id=ANY($2))) ORDER BY name,id")
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
        self.save_inner((actor, trace), id, key, input, None).await
    }

    async fn save_inner(
        &self,
        context: (Uuid, Uuid),
        id: Option<Uuid>,
        key: &str,
        input: &SaveOpportunity,
        guard: Option<command::Guard<'_>>,
    ) -> Result<Value, DomainError> {
        let (actor, trace) = context;
        input.validate()?;
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
        if let Some(id) = id {
            let old = self.accessible(actor, id, "crm:manage").await?;
            if old.legal_entity_id != input.legal_entity_id
                || old.business_unit_id != input.business_unit_id
            {
                return Err(DomainError::Invalid("商机所属主体不可更改".into()));
            }
        }
        if id.is_some() != input.expected_version.is_some() {
            return Err(DomainError::Invalid("更新需要当前版本".into()));
        }
        let mut tx = self.store.pool().begin().await?;
        let hash = match &guard {
            Some(g) => request_hash(&(id, input, g.snapshot))?,
            None => request_hash(&(id, input))?,
        };
        if let Some(result) =
            begin_idempotent::<Value>(&mut tx, actor, "crm:save", key, &hash).await?
        {
            let replay_id: Uuid = serde_json::from_value(result["id"].clone())
                .map_err(|_| DomainError::NotFoundOrForbidden)?;
            self.check_write_authority(&mut tx, actor, replay_id)
                .await?;
            tx.commit().await?;
            return Ok(result);
        }
        if let Some(g) = &guard {
            let command = match id {
                Some(opportunity_id) => CrmCommand::Update {
                    opportunity_id,
                    command: input.clone(),
                },
                None => CrmCommand::Create {
                    command: input.clone(),
                },
            };
            if self.preview_on(&mut tx, actor, &command).await? != *g.snapshot {
                return Err(DomainError::StalePreview);
            }
        }
        if let Some(existing_id) = id {
            // An edit can replace the customer. Recheck the old target after
            // taking its lock, before replacing its scope-bearing fields.
            self.check_write_authority(&mut tx, actor, existing_id)
                .await?;
        }
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM business_units u JOIN business_legal_entities e ON e.id=u.legal_entity_id WHERE u.id=$1 AND e.id=$2 AND u.status='active' AND e.status='active') AND ($3::uuid IS NULL OR EXISTS(SELECT 1 FROM business_customers WHERE id=$3 AND legal_entity_id=$2 AND business_unit_id=$1 AND status='active'))")
            .bind(input.business_unit_id).bind(input.legal_entity_id).bind(input.customer_id).fetch_one(&mut *tx).await?;
        if !valid {
            return Err(DomainError::NotFoundOrForbidden);
        }
        let record_id = id.unwrap_or_else(Uuid::new_v4);
        let version: Option<i64> = if id.is_none() {
            sqlx::query_scalar("INSERT INTO crm_opportunities(id,legal_entity_id,business_unit_id,customer_id,title,company_name,contact_name,contact_details,stage,expected_amount_minor,currency,next_action,next_follow_up,owner_user_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) RETURNING version")
                .bind(record_id).bind(input.legal_entity_id).bind(input.business_unit_id).bind(input.customer_id).bind(input.title.trim()).bind(input.company_name.trim()).bind(input.contact_name.trim()).bind(input.contact_details.trim()).bind(&input.stage).bind(input.expected_amount_minor).bind(&input.currency).bind(input.next_action.trim()).bind(input.next_follow_up).bind(actor).fetch_optional(&mut *tx).await?
        } else {
            sqlx::query_scalar("UPDATE crm_opportunities SET customer_id=$2,title=$3,company_name=$4,contact_name=$5,contact_details=$6,stage=$7,expected_amount_minor=$8,currency=$9,next_action=$10,next_follow_up=$11,version=version+1,updated_at=now() WHERE id=$1 AND version=$12 RETURNING version")
                .bind(record_id).bind(input.customer_id).bind(input.title.trim()).bind(input.company_name.trim()).bind(input.contact_name.trim()).bind(input.contact_details.trim()).bind(&input.stage).bind(input.expected_amount_minor).bind(&input.currency).bind(input.next_action.trim()).bind(input.next_follow_up).bind(input.expected_version).fetch_optional(&mut *tx).await?
        };
        let version = version.ok_or(DomainError::VersionConflict)?;
        self.check_write_authority(&mut tx, actor, record_id)
            .await?;
        let result = json!({"id":record_id,"version":version,"traceId":trace});
        record(
            &mut tx,
            trace,
            actor,
            "crm.opportunity.saved",
            "crm.opportunity.saved",
            "crm_opportunity",
            record_id,
            json!({"version":version,"stage":input.stage}),
        )
        .await?;
        command::finish_approval(&mut tx, guard.as_ref()).await?;
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
        self.followup_inner((actor, trace), id, key, input, None)
            .await
    }

    async fn followup_inner(
        &self,
        context: (Uuid, Uuid),
        id: Uuid,
        key: &str,
        input: &AddFollowup,
        guard: Option<command::Guard<'_>>,
    ) -> Result<Value, DomainError> {
        let (actor, trace) = context;
        model::text(&input.note, 4000, true)?;
        model::text(&input.next_action, 500, false)?;
        model::stage(&input.stage)?;
        self.accessible(actor, id, "crm:manage").await?;
        let mut tx = self.store.pool().begin().await?;
        let hash = match &guard {
            Some(g) => request_hash(&(id, input, g.snapshot))?,
            None => request_hash(&(id, input))?,
        };
        if let Some(result) =
            begin_idempotent::<Value>(&mut tx, actor, "crm:followup", key, &hash).await?
        {
            let replay_id: Uuid = serde_json::from_value(result["id"].clone())
                .map_err(|_| DomainError::NotFoundOrForbidden)?;
            self.check_write_authority(&mut tx, actor, replay_id)
                .await?;
            tx.commit().await?;
            return Ok(result);
        }
        if let Some(g) = &guard {
            let command = CrmCommand::Followup {
                opportunity_id: id,
                command: input.clone(),
            };
            if self.preview_on(&mut tx, actor, &command).await? != *g.snapshot {
                return Err(DomainError::StalePreview);
            }
        }
        let version:Option<i64>=sqlx::query_scalar("UPDATE crm_opportunities SET stage=$2,next_action=$3,next_follow_up=$4,version=version+1,updated_at=now() WHERE id=$1 AND version=$5 RETURNING version")
            .bind(id).bind(&input.stage).bind(input.next_action.trim()).bind(input.next_follow_up).bind(input.expected_version).fetch_optional(&mut *tx).await?;
        let version = version.ok_or(DomainError::VersionConflict)?;
        sqlx::query("INSERT INTO crm_followups(id,opportunity_id,author_user_id,note,stage,next_action,next_follow_up) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(Uuid::new_v4()).bind(id).bind(actor).bind(input.note.trim()).bind(&input.stage).bind(input.next_action.trim()).bind(input.next_follow_up).execute(&mut *tx).await?;
        self.check_write_authority(&mut tx, actor, id).await?;
        let result = json!({"id":id,"version":version,"traceId":trace});
        record(
            &mut tx,
            trace,
            actor,
            "crm.followup.added",
            "crm.followup.added",
            "crm_opportunity",
            id,
            json!({"version":version,"stage":input.stage}),
        )
        .await?;
        command::finish_approval(&mut tx, guard.as_ref()).await?;
        finish_idempotent(&mut tx, actor, "crm:followup", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
}
