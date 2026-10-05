mod model;
use super::*;
pub use model::*;
impl CrmService {
    /// Lead search scoped to owner/creator unless explicitly granted read-all access.
    pub async fn leads(&self, actor: Uuid, f: &LeadFilters) -> Result<Value, DomainError> {
        let scope = self.scope(actor, "crm:read").await?;
        model::validate_filters(f)?;
        let read_all = scope.permission_keys.contains("crm:lead_read_all");
        let mut items=sqlx::query_as::<_,Lead>("SELECT l.*,u.display_name owner_name FROM crm_leads l JOIN enterprise_users u ON u.id=l.owner_user_id WHERE ($8 OR l.owner_user_id=$1 OR l.created_by_user_id=$1) AND ($2::text IS NULL OR strpos(lower(l.title||' '||l.company_name||' '||l.contact_name||' '||l.contact_details),lower($2))>0) AND ($3::text IS NULL OR l.status=$3) AND ($4::date IS NULL OR (l.next_follow_up<$4 AND l.status IN ('new','contacting'))) AND ($5::uuid IS NULL OR l.owner_user_id=$5) AND ($7::text IS NULL OR l.source=$7) ORDER BY l.next_follow_up NULLS LAST,l.created_at DESC,l.id LIMIT 51 OFFSET $6")
            .bind(actor).bind(&f.query).bind(&f.status).bind(f.due_by).bind(f.owner_user_id).bind(f.offset).bind(&f.source).bind(read_all).fetch_all(self.store.pool()).await?;
        let more = items.len() > 50;
        items.truncate(50);
        Ok(
            json!({"items":items,"hasMore":more,"canManage":scope.permission_keys.contains("crm:manage"),"canReadAll":read_all}),
        )
    }
    /// Read lead details and a bounded page of preserved notes.
    pub async fn lead_detail(
        &self,
        actor: Uuid,
        id: Uuid,
        offset: i64,
    ) -> Result<Value, DomainError> {
        let scope = self.scope(actor, "crm:read").await?;
        let read_all = scope.permission_keys.contains("crm:lead_read_all");
        if !(0..=100000).contains(&offset) {
            return Err(DomainError::Invalid("无效页码".into()));
        }
        let item=sqlx::query_as::<_,Lead>("SELECT l.*,u.display_name owner_name FROM crm_leads l JOIN enterprise_users u ON u.id=l.owner_user_id WHERE l.id=$1 AND ($3 OR l.owner_user_id=$2 OR l.created_by_user_id=$2)")
            .bind(id).bind(actor).bind(read_all).fetch_optional(self.store.pool()).await?.ok_or(DomainError::NotFoundOrForbidden)?;
        let mut notes:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',f.id,'note',f.note,'status',f.status,'disqualificationReason',f.disqualification_reason,'nextAction',f.next_action,'nextFollowUp',f.next_follow_up,'createdAt',f.created_at,'authorName',u.display_name) FROM crm_lead_followups f JOIN enterprise_users u ON u.id=f.author_user_id WHERE f.lead_id=$1 ORDER BY f.created_at DESC,f.id DESC LIMIT 101 OFFSET $2").bind(id).bind(offset).fetch_all(self.store.pool()).await?;
        let more = notes.len() > 100;
        notes.truncate(100);
        let duplicates:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'title',title) FROM crm_leads WHERE ($5 OR owner_user_id=$1 OR created_by_user_id=$1) AND id!=$2 AND status!='disqualified' AND (($3!='' AND lower(company_name)=lower($3)) OR ($4!='' AND contact_details=$4)) ORDER BY created_at DESC LIMIT 10")
            .bind(actor).bind(id).bind(&item.company_name).bind(&item.contact_details).bind(read_all).fetch_all(self.store.pool()).await?;
        let can_manage = scope.permission_keys.contains("crm:manage")
            && (item.owner_user_id == actor || item.created_by_user_id == actor);
        Ok(
            json!({"item":item,"followups":notes,"hasMore":more,"duplicates":duplicates,"canManage":can_manage}),
        )
    }
    /// CRM operators eligible to receive leads; no business scope is granted.
    pub async fn lead_owners(&self, actor: Uuid) -> Result<Value, DomainError> {
        self.scope(actor, "crm:read").await?;
        let candidates:Vec<(Uuid,String)>=sqlx::query_as("SELECT id,display_name FROM enterprise_users WHERE status='active' ORDER BY display_name,id LIMIT 200").fetch_all(self.store.pool()).await?;
        let mut items = Vec::new();
        for (id, name) in candidates {
            match self.scope(id, "crm:manage").await {
                Ok(_) => items.push(json!({"id":id,"name":name})),
                Err(DomainError::NotFoundOrForbidden) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(json!({"items":items,"currentUserId":actor}))
    }
    /// Save an owned lead without creating customer or contact master records.
    pub async fn save_lead(
        &self,
        actor: Uuid,
        trace: Uuid,
        id: Option<Uuid>,
        key: &str,
        input: &SaveLead,
    ) -> Result<Value, DomainError> {
        input.validate()?;
        self.scope(actor, "crm:manage").await?;
        let owner = input.owner_user_id.unwrap_or(actor);
        self.scope(owner, "crm:manage").await?;
        if let Some(customer) = input.customer_id {
            authorize(
                &self.store,
                actor,
                "crm:manage",
                None,
                None,
                Some(customer),
                None,
                None,
            )
            .await?;
            authorize(
                &self.store,
                owner,
                "crm:manage",
                None,
                None,
                Some(customer),
                None,
                None,
            )
            .await?;
        }
        if id.is_some() != input.expected_version.is_some() {
            return Err(DomainError::Invalid("更新需要当前版本".into()));
        }
        let mut tx = self.store.pool().begin().await?;
        let hash = request_hash(&(id, input))?;
        if let Some(result) =
            begin_idempotent::<Value>(&mut tx, actor, "crm:lead:save", key, &hash).await?
        {
            tx.commit().await?;
            return Ok(result);
        }
        if let Some(id) = id {
            self.lock_lead(&mut tx, actor, id, input.expected_version.unwrap_or(0))
                .await?;
        }
        let record_id = id.unwrap_or_else(Uuid::new_v4);
        let version: i64 = if id.is_none() {
            sqlx::query_scalar("INSERT INTO crm_leads(id,title,company_name,contact_name,contact_details,source,summary,next_action,next_follow_up,customer_id,owner_user_id,created_by_user_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING version")
                .bind(record_id).bind(input.title.trim()).bind(input.company_name.trim()).bind(input.contact_name.trim()).bind(input.contact_details.trim()).bind(input.source.trim()).bind(input.summary.trim()).bind(input.next_action.trim()).bind(input.next_follow_up).bind(input.customer_id).bind(owner).bind(actor).fetch_one(&mut *tx).await?
        } else {
            sqlx::query_scalar("UPDATE crm_leads SET title=$2,company_name=$3,contact_name=$4,contact_details=$5,source=$6,summary=$7,next_action=$8,next_follow_up=$9,customer_id=$10,owner_user_id=$11,version=version+1,updated_at=now() WHERE id=$1 RETURNING version")
                .bind(record_id).bind(input.title.trim()).bind(input.company_name.trim()).bind(input.contact_name.trim()).bind(input.contact_details.trim()).bind(input.source.trim()).bind(input.summary.trim()).bind(input.next_action.trim()).bind(input.next_follow_up).bind(input.customer_id).bind(owner).fetch_one(&mut *tx).await?
        };
        let result =
            json!({"id":record_id,"version":version,"traceId":trace,"transferred":owner!=actor});
        record(
            &mut tx,
            trace,
            actor,
            "crm.lead.saved",
            "crm.lead.saved",
            "crm_lead",
            record_id,
            json!({"version":version,"ownerUserId":owner}),
        )
        .await?;
        finish_idempotent(&mut tx, actor, "crm:lead:save", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub(super) async fn lock_lead(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        actor: Uuid,
        id: Uuid,
        version: i64,
    ) -> Result<String, DomainError> {
        let row: Option<(i64, String)> = sqlx::query_as(
            "SELECT version,status FROM crm_leads WHERE id=$1 AND (owner_user_id=$2 OR created_by_user_id=$2) FOR UPDATE",
        )
        .bind(id)
        .bind(actor)
        .fetch_optional(&mut **tx)
        .await?;
        let (current, status) = row.ok_or(DomainError::NotFoundOrForbidden)?;
        if current != version {
            return Err(DomainError::VersionConflict);
        }
        if status == "converted" {
            return Err(DomainError::Invalid(
                "线索已转为商机，请打开关联商机继续跟进".into(),
            ));
        }
        Ok(status)
    }
    /// Append screening history and advance the next follow-up atomically.
    pub async fn lead_followup(
        &self,
        actor: Uuid,
        trace: Uuid,
        id: Uuid,
        key: &str,
        input: &LeadFollowup,
    ) -> Result<Value, DomainError> {
        self.scope(actor, "crm:manage").await?;
        input.validate()?;
        let mut tx = self.store.pool().begin().await?;
        let hash = request_hash(&(id, input))?;
        if let Some(result) =
            begin_idempotent::<Value>(&mut tx, actor, "crm:lead:followup", key, &hash).await?
        {
            tx.commit().await?;
            return Ok(result);
        }
        self.lock_lead(&mut tx, actor, id, input.expected_version)
            .await?;
        let version:i64=sqlx::query_scalar("UPDATE crm_leads SET status=$2,next_action=$3,next_follow_up=$4,disqualification_reason=$5,version=version+1,updated_at=now() WHERE id=$1 RETURNING version")
            .bind(id).bind(&input.status).bind(input.next_action.trim()).bind(input.next_follow_up).bind(input.disqualification_reason.trim()).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO crm_lead_followups(id,lead_id,author_user_id,note,status,next_action,next_follow_up,disqualification_reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(Uuid::new_v4()).bind(id).bind(actor).bind(input.note.trim()).bind(&input.status).bind(input.next_action.trim()).bind(input.next_follow_up).bind(&input.disqualification_reason).execute(&mut *tx).await?;
        let result = json!({"id":id,"version":version,"traceId":trace});
        record(
            &mut tx,
            trace,
            actor,
            "crm.lead.followup",
            "crm.lead.followup",
            "crm_lead",
            id,
            json!({"version":version,"status":input.status}),
        )
        .await?;
        finish_idempotent(&mut tx, actor, "crm:lead:followup", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
    /// Qualify a lead into one opportunity within the same transaction.
    pub async fn convert_lead(
        &self,
        actor: Uuid,
        trace: Uuid,
        id: Uuid,
        key: &str,
        input: &ConvertLead,
    ) -> Result<Value, DomainError> {
        if input.opportunity.stage != "contacting" || input.opportunity.expected_version.is_some() {
            return Err(DomainError::Invalid("转商机初始阶段应为沟通中".into()));
        }
        self.save_from_lead(
            actor,
            trace,
            None,
            key,
            &input.opportunity,
            Some((id, input.expected_version)),
        )
        .await
    }
}
