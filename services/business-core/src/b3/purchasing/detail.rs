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
        sqlx::query_as::<_, PurchaseOrderView>("SELECT o.id,o.purchase_order_number,o.legal_entity_id,le.code AS legal_entity_code,le.name AS legal_entity_name,o.supplier_id,s.code AS supplier_code,s.name AS supplier_name,o.business_unit_id,bu.code AS business_unit_code,bu.name AS business_unit_name,ARRAY(SELECT DISTINCT concat(w.code,' · ',w.name) FROM purchase_order_lines pol JOIN business_warehouses w ON w.id=pol.warehouse_id WHERE pol.purchase_order_id=o.id ORDER BY concat(w.code,' · ',w.name)) AS warehouse_labels,o.currency::text,o.lifecycle_status,o.receiving_status,o.gross_amount,o.order_date,o.updated_at,o.version FROM purchase_orders o JOIN business_legal_entities le ON le.id=o.legal_entity_id JOIN business_suppliers s ON s.id=o.supplier_id JOIN business_units bu ON bu.id=o.business_unit_id WHERE o.id=$1 AND o.legal_entity_id=ANY($2) AND o.supplier_id=ANY($3) AND o.business_unit_id=ANY($4)")
            .bind(id)
            .bind(snapshot.scopes.legal_entity_ids.into_iter().collect::<Vec<_>>())
            .bind(snapshot.scopes.supplier_ids.into_iter().collect::<Vec<_>>())
            .bind(snapshot.scopes.business_unit_ids.into_iter().collect::<Vec<_>>())
            .fetch_optional(self.store.pool()).await?
            .ok_or(DomainError::NotFoundOrForbidden)
    }
}
