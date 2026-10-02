//! Service delivery is independent of physical inventory and settlement.
mod accounting;
pub mod api;
mod model;
mod options;
mod writes;
use crate::{
    b2::common::{authorize, DomainError},
    store::PgStore,
};
pub use model::{AcceptanceInput, DeliverableInput, Filters, ProjectInput};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

/// Service project and delivery registers sharing Business Core scopes.
#[derive(Clone)]
pub struct ServiceDelivery {
    store: PgStore,
    receivable_prefix: String,
}
impl ServiceDelivery {
    /// Bind the existing authenticated business store.
    pub fn new(store: PgStore) -> Self {
        Self {
            store,
            receivable_prefix: "AR".into(),
        }
    }
    /// Use the configured receivable numbering fallback; governed rules still take precedence.
    pub fn with_receivable_prefix(mut self, prefix: String) -> Self {
        self.receivable_prefix = prefix;
        self
    }
    async fn scope(
        &self,
        actor: Uuid,
        permission: &str,
    ) -> Result<crate::model::AuthorizationSnapshot, DomainError> {
        authorize(&self.store, actor, permission, None, None, None, None, None).await
    }
    async fn accessible(
        &self,
        actor: Uuid,
        id: Uuid,
        permission: &str,
    ) -> Result<sqlx::postgres::PgRow, DomainError> {
        let row = sqlx::query("SELECT * FROM service_projects WHERE id=$1")
            .bind(id)
            .fetch_optional(self.store.pool())
            .await?
            .ok_or(DomainError::NotFoundOrForbidden)?;
        authorize(
            &self.store,
            actor,
            permission,
            Some(row.get("legal_entity_id")),
            None,
            Some(row.get("customer_id")),
            None,
            Some(row.get("business_unit_id")),
        )
        .await?;
        Ok(row)
    }
    /// Return service projects or delivery items after authorization filtering.
    pub async fn list(&self, actor: Uuid, q: &Filters, tasks: bool) -> Result<Value, DomainError> {
        let scope = self.scope(actor, "service_delivery:read").await?;
        model::text(q.query.as_deref().unwrap_or(""), 160, false)?;
        if !(0..=100000).contains(&q.offset)
            || q.expiry
                .as_deref()
                .is_some_and(|v| !["expired", "upcoming"].contains(&v) || q.today.is_none())
        {
            return Err(DomainError::Invalid("无效筛选条件".into()));
        }
        let sql = if tasks {
            "SELECT to_jsonb(d)||jsonb_build_object('project_title',p.title,'customer_name',c.name) FROM service_deliverables d JOIN service_projects p ON p.id=d.project_id JOIN business_customers c ON c.id=p.customer_id WHERE p.legal_entity_id=ANY($1) AND p.business_unit_id=ANY($2) AND p.customer_id=ANY($3) AND ($4::text IS NULL OR strpos(lower(d.title||' '||p.title||' '||c.name),lower($4))>0) AND ($5::text IS NULL OR d.status=$5) AND ($6::text IS NULL OR CASE $6 WHEN 'expired' THEN d.due_on<$7::date WHEN 'upcoming' THEN d.due_on BETWEEN $7::date AND $7::date+30 ELSE false END) ORDER BY d.created_at DESC,d.id LIMIT 51 OFFSET $8"
        } else {
            "SELECT to_jsonb(p)||jsonb_build_object('customer_name',c.name,'owner_name',u.display_name) FROM service_projects p JOIN business_customers c ON c.id=p.customer_id JOIN enterprise_users u ON u.id=p.owner_user_id WHERE p.legal_entity_id=ANY($1) AND p.business_unit_id=ANY($2) AND p.customer_id=ANY($3) AND ($4::text IS NULL OR strpos(lower(p.title||' '||c.name),lower($4))>0) AND ($5::text IS NULL OR p.status=$5) AND ($6::text IS NULL OR (p.service_kind='software_service' AND p.status<>'cancelled' AND CASE $6 WHEN 'expired' THEN p.ends_on<$7::date WHEN 'upcoming' THEN p.ends_on BETWEEN $7::date AND $7::date+30 ELSE false END)) ORDER BY p.created_at DESC,p.id LIMIT 51 OFFSET $8"
        };
        let mut rows: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
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
            .bind(&q.query)
            .bind(&q.status)
            .bind(&q.expiry)
            .bind(q.today)
            .bind(q.offset)
            .fetch_all(self.store.pool())
            .await?;
        let has_more = rows.len() > 50;
        rows.truncate(50);
        Ok(
            json!({"items":rows,"hasMore":has_more,"canManage":scope.permission_keys.contains("service_delivery:manage")}),
        )
    }
    /// Read immutable acceptance history and the project delivery items.
    pub async fn detail(&self, actor: Uuid, id: Uuid) -> Result<Value, DomainError> {
        let scope = self.accessible(actor, id, "service_delivery:read").await?;
        let item:Value=sqlx::query_scalar("SELECT to_jsonb(p)||jsonb_build_object('customer_name',c.name,'owner_name',u.display_name,'sales_order_id',l.sales_order_id,'order_number',o.order_number) FROM service_projects p JOIN business_customers c ON c.id=p.customer_id JOIN enterprise_users u ON u.id=p.owner_user_id LEFT JOIN sales_order_lines l ON l.id=p.sales_order_line_id LEFT JOIN sales_orders o ON o.id=l.sales_order_id WHERE p.id=$1").bind(id).fetch_one(self.store.pool()).await?;
        let tasks:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(d) FROM service_deliverables d WHERE project_id=$1 ORDER BY created_at,id LIMIT 501").bind(id).fetch_all(self.store.pool()).await?;
        let acceptances:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(a) FROM service_acceptances a WHERE project_id=$1 ORDER BY created_at DESC,id LIMIT 101").bind(id).fetch_all(self.store.pool()).await?;
        let can_accept = self
            .accessible(actor, id, "service_delivery:accept")
            .await
            .is_ok();
        let can_read_ar = authorize(
            &self.store,
            actor,
            "receivable:read",
            Some(scope.get("legal_entity_id")),
            None,
            Some(scope.get("customer_id")),
            None,
            Some(scope.get("business_unit_id")),
        )
        .await
        .is_ok();
        let receivable = if can_read_ar {
            sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'number',receivable_number,'amount',original_amount::text,'openAmount',open_amount::text,'currency',currency,'dueDate',due_date,'status',status) FROM trade_receivables WHERE service_project_id=$1").bind(id).fetch_optional(self.store.pool()).await?
        } else {
            None
        };
        Ok(
            json!({"item":item,"canAccept":can_accept,"receivable":receivable,"hasMoreTasks":tasks.len()>500,"tasks":tasks.into_iter().take(500).collect::<Vec<_>>(),"hasMoreAcceptances":acceptances.len()>100,"acceptances":acceptances.into_iter().take(100).collect::<Vec<_>>()}),
        )
    }
}
