use super::*;
use crate::b2::common::{begin_idempotent, finish_idempotent, record, request_hash};
impl ServiceDelivery {
    /// Save a project with version checks and a single authoritative order-line link.
    pub async fn save_project(
        &self,
        actor: Uuid,
        trace: Uuid,
        id: Option<Uuid>,
        key: &str,
        input: &ProjectInput,
    ) -> Result<Value, DomainError> {
        input.validate()?;
        authorize(
            &self.store,
            actor,
            "service_delivery:manage",
            Some(input.legal_entity_id),
            None,
            Some(input.customer_id),
            None,
            Some(input.business_unit_id),
        )
        .await?;
        authorize(
            &self.store,
            input.owner_user_id,
            "service_delivery:manage",
            Some(input.legal_entity_id),
            None,
            Some(input.customer_id),
            None,
            Some(input.business_unit_id),
        )
        .await?;
        if id.is_some() != input.expected_version.is_some() {
            return Err(DomainError::Invalid("更新需要当前版本".into()));
        }
        let mut tx = self.store.pool().begin().await?;
        let hash = request_hash(&(id, input))?;
        if let Some(value) =
            begin_idempotent::<Value>(&mut tx, actor, "service_project:save", key, &hash).await?
        {
            tx.commit().await?;
            return Ok(value);
        }
        let project_id = id.unwrap_or_else(Uuid::new_v4);
        if let Some(id) = id {
            self.accessible(actor, id, "service_delivery:manage")
                .await?;
            let old = sqlx::query("SELECT * FROM service_projects WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
            if Some(old.get::<i64, _>("version")) != input.expected_version {
                return Err(DomainError::VersionConflict);
            }
            if old.get::<Uuid, _>("legal_entity_id") != input.legal_entity_id
                || old.get::<Uuid, _>("business_unit_id") != input.business_unit_id
                || old.get::<Uuid, _>("customer_id") != input.customer_id
                || old.get::<String, _>("service_kind") != input.service_kind
                || old.get::<Option<Uuid>, _>("renewal_of_project_id")
                    != input.renewal_of_project_id
                || old
                    .get::<Option<Uuid>, _>("sales_order_line_id")
                    .is_some_and(|v| Some(v) != input.sales_order_line_id)
            {
                return Err(DomainError::Invalid(
                    "客户、主体、服务类型及已关联来源不可修改".into(),
                ));
            }
            if ["completed", "cancelled"].contains(&old.get::<String, _>("status").as_str()) {
                return Err(DomainError::Invalid("已完成或取消的项目不可修改".into()));
            }
        }
        let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM business_legal_entities WHERE id=$1 AND status='active') AND EXISTS(SELECT 1 FROM business_units WHERE id=$2 AND status='active') AND EXISTS(SELECT 1 FROM business_customers WHERE id=$3 AND status='active')").bind(input.legal_entity_id).bind(input.business_unit_id).bind(input.customer_id).fetch_one(&mut *tx).await?;
        if !active {
            return Err(DomainError::NotFoundOrForbidden);
        }
        if let Some(source) = input.renewal_of_project_id {
            let old = self
                .accessible(actor, source, "service_delivery:read")
                .await?;
            if source == project_id
                || old.get::<Uuid, _>("customer_id") != input.customer_id
                || old.get::<String, _>("service_kind") != "software_service"
                || input.service_kind != "software_service"
            {
                return Err(DomainError::Invalid(
                    "续费须关联同一客户的软件服务项目".into(),
                ));
            }
        }
        if let Some(line_id) = input.sales_order_line_id {
            let line=sqlx::query("SELECT o.id,o.legal_entity_id,o.business_unit_id,o.customer_id,o.lifecycle_status,p.service_kind FROM sales_order_lines l JOIN sales_orders o ON o.id=l.sales_order_id JOIN business_skus s ON s.id=l.sku_id JOIN business_products p ON p.id=s.product_id WHERE l.id=$1 FOR SHARE OF o,l,p").bind(line_id).fetch_optional(&mut *tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
            authorize(
                &self.store,
                actor,
                "sales_order:read",
                Some(line.get("legal_entity_id")),
                None,
                Some(line.get("customer_id")),
                None,
                Some(line.get("business_unit_id")),
            )
            .await?;
            if line.get::<String, _>("lifecycle_status") != "confirmed"
                || line.get::<Uuid, _>("legal_entity_id") != input.legal_entity_id
                || line.get::<Uuid, _>("business_unit_id") != input.business_unit_id
                || line.get::<Uuid, _>("customer_id") != input.customer_id
                || line.get::<String, _>("service_kind") != input.service_kind
            {
                return Err(DomainError::Invalid(
                    "请选择客户、主体及服务类型一致的已确认订单行".into(),
                ));
            }
            let occupied:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM service_projects WHERE sales_order_line_id=$1 AND id<>$2)").bind(line_id).bind(project_id).fetch_one(&mut *tx).await?;
            if occupied {
                return Err(DomainError::Invalid("该服务订单行已创建项目".into()));
            }
        }
        let version:i64=sqlx::query_scalar("INSERT INTO service_projects(id,title,legal_entity_id,business_unit_id,customer_id,owner_user_id,contact_name,service_kind,sales_order_line_id,renewal_of_project_id,starts_on,ends_on,status,description) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) ON CONFLICT(id) DO UPDATE SET title=excluded.title,owner_user_id=excluded.owner_user_id,contact_name=excluded.contact_name,sales_order_line_id=excluded.sales_order_line_id,starts_on=excluded.starts_on,ends_on=excluded.ends_on,status=excluded.status,description=excluded.description,version=service_projects.version+1,updated_at=now() RETURNING version")
            .bind(project_id).bind(input.title.trim()).bind(input.legal_entity_id).bind(input.business_unit_id).bind(input.customer_id).bind(input.owner_user_id).bind(&input.contact_name).bind(&input.service_kind).bind(input.sales_order_line_id).bind(input.renewal_of_project_id).bind(input.starts_on).bind(input.ends_on).bind(&input.status).bind(&input.description).fetch_one(&mut *tx).await?;
        record(
            &mut tx,
            trace,
            actor,
            "SERVICE_PROJECT_SAVED",
            "service_project_saved",
            "service_project",
            project_id,
            json!({"version":version}),
        )
        .await?;
        let result = json!({"id":project_id,"version":version,"traceId":trace});
        finish_idempotent(&mut tx, actor, "service_project:save", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
    /// Create or update an item; a parent lock serializes delivery against acceptance.
    #[allow(clippy::too_many_arguments)]
    pub async fn save_deliverable(
        &self,
        actor: Uuid,
        trace: Uuid,
        project: Uuid,
        id: Option<Uuid>,
        key: &str,
        input: &DeliverableInput,
    ) -> Result<Value, DomainError> {
        model::text(&input.title, 200, true)?;
        model::text(&input.description, 4000, false)?;
        model::evidence(&input.evidence_url)?;
        if !["pending", "active", "completed", "cancelled"].contains(&input.status.as_str())
            || id.is_some() != input.expected_version.is_some()
        {
            return Err(DomainError::Invalid("无效状态或版本".into()));
        }
        let parent = self
            .accessible(actor, project, "service_delivery:manage")
            .await?;
        authorize(
            &self.store,
            input.owner_user_id,
            "service_delivery:manage",
            Some(parent.get("legal_entity_id")),
            None,
            Some(parent.get("customer_id")),
            None,
            Some(parent.get("business_unit_id")),
        )
        .await?;
        let mut tx = self.store.pool().begin().await?;
        let hash = request_hash(&(project, id, input))?;
        if let Some(value) =
            begin_idempotent::<Value>(&mut tx, actor, "service_deliverable:save", key, &hash)
                .await?
        {
            tx.commit().await?;
            return Ok(value);
        }
        let status: String =
            sqlx::query_scalar("SELECT status FROM service_projects WHERE id=$1 FOR UPDATE")
                .bind(project)
                .fetch_one(&mut *tx)
                .await?;
        if ["completed", "cancelled"].contains(&status.as_str()) {
            return Err(DomainError::Invalid("项目已关闭".into()));
        }
        let target = id.unwrap_or_else(Uuid::new_v4);
        if let Some(id) = id {
            let version: Option<i64> = sqlx::query_scalar(
                "SELECT version FROM service_deliverables WHERE id=$1 AND project_id=$2 FOR UPDATE",
            )
            .bind(id)
            .bind(project)
            .fetch_optional(&mut *tx)
            .await?;
            if version.is_none() {
                return Err(DomainError::NotFoundOrForbidden);
            }
            if version != input.expected_version {
                return Err(DomainError::VersionConflict);
            }
        }
        let version:i64=sqlx::query_scalar("INSERT INTO service_deliverables(id,project_id,title,owner_user_id,due_on,status,description,evidence_url) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(id) DO UPDATE SET title=excluded.title,owner_user_id=excluded.owner_user_id,due_on=excluded.due_on,status=excluded.status,description=excluded.description,evidence_url=excluded.evidence_url,version=service_deliverables.version+1,updated_at=now() RETURNING version").bind(target).bind(project).bind(input.title.trim()).bind(input.owner_user_id).bind(input.due_on).bind(&input.status).bind(&input.description).bind(&input.evidence_url).fetch_one(&mut *tx).await?;
        sqlx::query("UPDATE service_projects SET version=version+1,updated_at=now() WHERE id=$1")
            .bind(project)
            .execute(&mut *tx)
            .await?;
        record(
            &mut tx,
            trace,
            actor,
            "SERVICE_DELIVERABLE_SAVED",
            "service_deliverable_saved",
            "service_project",
            project,
            json!({"itemId":target,"version":version}),
        )
        .await?;
        let result = json!({"id":target,"version":version,"traceId":trace});
        finish_idempotent(&mut tx, actor, "service_deliverable:save", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
    /// Append acceptance evidence and atomically recognize passed service revenue and receivables.
    pub async fn accept(
        &self,
        actor: Uuid,
        trace: Uuid,
        project: Uuid,
        key: &str,
        input: &AcceptanceInput,
    ) -> Result<Value, DomainError> {
        self.accessible(actor, project, "service_delivery:manage")
            .await?;
        model::text(&input.customer_reviewer, 200, true)?;
        model::text(&input.note, 4000, false)?;
        model::evidence(&input.evidence_url)?;
        if !["passed", "rejected"].contains(&input.result.as_str())
            || (input.evidence_url.trim().is_empty() && input.note.trim().is_empty())
        {
            return Err(DomainError::Invalid("验收须填写结果及说明或凭据".into()));
        }
        let mut tx = self.store.pool().begin().await?;
        let hash = request_hash(&(project, input))?;
        if let Some(value) =
            begin_idempotent::<Value>(&mut tx, actor, "service_project:accept", key, &hash).await?
        {
            tx.commit().await?;
            return Ok(value);
        }
        let row = sqlx::query("SELECT status,version FROM service_projects WHERE id=$1 FOR UPDATE")
            .bind(project)
            .fetch_one(&mut *tx)
            .await?;
        if row.get::<i64, _>("version") != input.expected_version {
            return Err(DomainError::VersionConflict);
        }
        if row.get::<String, _>("status") != "acceptance" {
            return Err(DomainError::Invalid("请先将项目提交为待验收".into()));
        }
        let unfinished:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM service_deliverables WHERE project_id=$1 AND status NOT IN ('completed','cancelled'))").bind(project).fetch_one(&mut *tx).await?;
        if input.result == "passed" && unfinished {
            return Err(DomainError::Invalid("仍有未完成的交付事项".into()));
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO service_acceptances(id,project_id,accepted_on,customer_reviewer,result,note,evidence_url,actor_user_id,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)").bind(id).bind(project).bind(input.accepted_on).bind(input.customer_reviewer.trim()).bind(&input.result).bind(&input.note).bind(&input.evidence_url).bind(actor).bind(trace).execute(&mut *tx).await?;
        let receivable = if input.result == "passed" {
            Some(
                self.recognize(&mut tx, actor, trace, project, id, input.accepted_on)
                    .await?,
            )
        } else {
            None
        };
        sqlx::query(
            "UPDATE service_projects SET status=$2,version=version+1,updated_at=now() WHERE id=$1",
        )
        .bind(project)
        .bind(if input.result == "passed" {
            "completed"
        } else {
            "active"
        })
        .execute(&mut *tx)
        .await?;
        record(
            &mut tx,
            trace,
            actor,
            "SERVICE_PROJECT_ACCEPTED",
            "service_project_accepted",
            "service_project",
            project,
            json!({"acceptanceId":id,"result":input.result}),
        )
        .await?;
        let result = json!({"id":id,"version":input.expected_version+1,"traceId":trace,"receivable":receivable});
        finish_idempotent(&mut tx, actor, "service_project:accept", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
}
