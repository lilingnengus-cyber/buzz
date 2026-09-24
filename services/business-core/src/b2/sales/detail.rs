use super::*;

impl SalesService {
    /// Fetches one order within the actor's current read permissions and data scopes.
    pub async fn get_order(&self, actor: Uuid, id: Uuid) -> Result<SalesOrderSummary, DomainError> {
        let snapshot = authorize(
            &self.store,
            actor,
            "sales_order:read",
            None,
            None,
            None,
            None,
            None,
        )
        .await?;
        sqlx::query_as::<_, SalesOrderSummary>("SELECT id,order_number,legal_entity_id,business_unit_id,customer_id,currency::text,lifecycle_status,hold_status,fulfillment_status,gross_amount,order_date,updated_at,version FROM sales_orders WHERE id=$1 AND legal_entity_id=ANY($2) AND customer_id=ANY($3) AND business_unit_id=ANY($4)")
            .bind(id)
            .bind(snapshot.scopes.legal_entity_ids.into_iter().collect::<Vec<_>>())
            .bind(snapshot.scopes.customer_ids.into_iter().collect::<Vec<_>>())
            .bind(snapshot.scopes.business_unit_ids.into_iter().collect::<Vec<_>>())
            .fetch_optional(self.store.pool()).await?
            .ok_or(DomainError::NotFoundOrForbidden)
    }
}
