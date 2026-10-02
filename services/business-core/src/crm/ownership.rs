use super::{model, CrmService};
use crate::b2::common::{authorize, DomainError};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

/// Scope of an opportunity; candidates never receive new grants through assignment.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerScope {
    pub legal_entity_id: Uuid,
    pub business_unit_id: Uuid,
    pub customer_id: Option<Uuid>,
    pub query: Option<String>,
}
impl CrmService {
    /// List active CRM operators who can already manage the requested scope.
    pub async fn owners(&self, actor: Uuid, scope: &OwnerScope) -> Result<Value, DomainError> {
        model::text(scope.query.as_deref().unwrap_or(""), 100, false)?;
        authorize(
            &self.store,
            actor,
            "crm:manage",
            Some(scope.legal_entity_id),
            None,
            scope.customer_id,
            None,
            Some(scope.business_unit_id),
        )
        .await?;
        let candidates: Vec<(Uuid,String)>=sqlx::query_as("SELECT u.id,u.display_name FROM enterprise_users u WHERE u.status='active' AND ($1::text IS NULL OR strpos(lower(u.display_name),lower($1))>0) ORDER BY u.display_name,u.id LIMIT 201")
            .bind(scope.query.as_deref().map(str::trim)).fetch_all(self.store.pool()).await?;
        let has_more = candidates.len() > 200;
        let mut items = Vec::new();
        for (id, name) in candidates.into_iter().take(200) {
            match authorize(
                &self.store,
                id,
                "crm:manage",
                Some(scope.legal_entity_id),
                None,
                scope.customer_id,
                None,
                Some(scope.business_unit_id),
            )
            .await
            {
                Ok(_) => items.push(json!({"id":id,"name":name})),
                Err(DomainError::NotFoundOrForbidden) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(json!({"items":items,"hasMore":has_more,"currentUserId":actor}))
    }
}
