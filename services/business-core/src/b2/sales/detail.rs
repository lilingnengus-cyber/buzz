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
        sqlx::query_as::<_, SalesOrderSummary>("SELECT o.id,o.order_number,o.legal_entity_id,le.code AS legal_entity_code,le.name AS legal_entity_name,o.customer_id,c.code AS customer_code,c.name AS customer_name,o.business_unit_id,bu.code AS business_unit_code,bu.name AS business_unit_name,o.currency::text,o.lifecycle_status,o.hold_status,o.fulfillment_status,o.gross_amount,o.order_date,o.updated_at,o.version FROM sales_orders o JOIN business_legal_entities le ON le.id=o.legal_entity_id JOIN business_customers c ON c.id=o.customer_id JOIN business_units bu ON bu.id=o.business_unit_id WHERE NOT EXISTS (SELECT 1 FROM sales_order_events de WHERE de.sales_order_id=o.id AND de.event_type='draft_deleted') AND o.id=$1 AND o.legal_entity_id=ANY($2) AND o.customer_id=ANY($3) AND o.business_unit_id=ANY($4)")
            .bind(id)
            .bind(snapshot.scopes.legal_entity_ids.into_iter().collect::<Vec<_>>())
            .bind(snapshot.scopes.customer_ids.into_iter().collect::<Vec<_>>())
            .bind(snapshot.scopes.business_unit_ids.into_iter().collect::<Vec<_>>())
            .fetch_optional(self.store.pool()).await?
            .ok_or(DomainError::NotFoundOrForbidden)
    }
}
