use super::*;

impl PurchasingService {
    /// Fetches one order within the actor's current read permissions and data scopes.
    pub async fn get_order(&self, actor: Uuid, id: Uuid) -> Result<PurchaseOrderView, DomainError> {
        let snapshot = authorize(
            &self.store,
            actor,
            "purchase_order:read",
            None,
            None,
            None,
            None,
            None,
        )
        .await?;
        sqlx::query_as::<_, PurchaseOrderView>("SELECT id,purchase_order_number,legal_entity_id,supplier_id,currency::text,lifecycle_status,receiving_status,gross_amount,order_date,updated_at,version FROM purchase_orders WHERE id=$1 AND legal_entity_id=ANY($2) AND supplier_id=ANY($3)")
            .bind(id)
            .bind(snapshot.scopes.legal_entity_ids.into_iter().collect::<Vec<_>>())
            .bind(snapshot.scopes.supplier_ids.into_iter().collect::<Vec<_>>())
            .fetch_optional(self.store.pool()).await?
            .ok_or(DomainError::NotFoundOrForbidden)
    }
}
