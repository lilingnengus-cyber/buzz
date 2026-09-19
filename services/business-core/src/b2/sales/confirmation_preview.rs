use super::*;

impl SalesService {
    pub async fn confirmation_preview(
        &self,
        actor: Uuid,
        order_id: Uuid,
    ) -> Result<SalesOrderConfirmationPreview, DomainError> {
        let scope = self.order_scope(order_id).await?;
        let snapshot = authorize(
            &self.store,
            actor,
            "sales_order:read",
            Some(scope.0),
            None,
            Some(scope.1),
            None,
            Some(scope.2),
        )
        .await?;
        let order = sqlx::query(
            "SELECT order_number,lifecycle_status,version FROM sales_orders WHERE id=$1",
        )
        .bind(order_id)
        .fetch_one(self.store.pool())
        .await?;
        let rows = sqlx::query(
            "SELECT l.sku_id,s.code sku_code,s.name sku_name,l.warehouse_id,w.code warehouse_code,w.name warehouse_name,sum(l.ordered_quantity) required_quantity,COALESCE(b.on_hand_quantity,0) on_hand_quantity,COALESCE(b.reserved_quantity,0) reserved_quantity,COALESCE(b.on_hand_quantity-b.reserved_quantity-b.quarantined_quantity,0) available_quantity FROM sales_order_lines l JOIN business_skus s ON s.id=l.sku_id JOIN business_warehouses w ON w.id=l.warehouse_id LEFT JOIN inventory_balances b ON b.legal_entity_id=$2 AND b.warehouse_id=l.warehouse_id AND b.sku_id=l.sku_id WHERE l.sales_order_id=$1 GROUP BY l.sku_id,s.code,s.name,l.warehouse_id,w.code,w.name,b.on_hand_quantity,b.reserved_quantity,b.quarantined_quantity ORDER BY w.code,s.code",
        )
        .bind(order_id)
        .bind(scope.0)
        .fetch_all(self.store.pool())
        .await?;
        let lines = rows
            .into_iter()
            .map(|row| {
                let required: Decimal = row.get("required_quantity");
                let available: Decimal = row.get("available_quantity");
                SalesOrderConfirmationLine {
                    sku_id: row.get("sku_id"),
                    sku_code: row.get("sku_code"),
                    sku_name: row.get("sku_name"),
                    warehouse_id: row.get("warehouse_id"),
                    warehouse_code: row.get("warehouse_code"),
                    warehouse_name: row.get("warehouse_name"),
                    required_quantity: required.into(),
                    on_hand_quantity: row.get::<Decimal, _>("on_hand_quantity").into(),
                    reserved_quantity: row.get::<Decimal, _>("reserved_quantity").into(),
                    available_quantity: available.into(),
                    expected_reserved_quantity: required.min(available).max(Decimal::ZERO).into(),
                    shortage_quantity: (required - available).max(Decimal::ZERO).into(),
                }
            })
            .collect::<Vec<_>>();
        let all_available = !lines.is_empty()
            && lines
                .iter()
                .all(|line| line.shortage_quantity.0 == Decimal::ZERO);
        let lifecycle_status: String = order.get("lifecycle_status");
        let has_permission = snapshot.permission_keys.contains("sales_order:confirm");
        let masters_ready = master_status::ready(self.store.pool(), order_id).await?;
        let readiness = if lifecycle_status != "draft" {
            "order_not_draft"
        } else if !has_permission {
            "permission_required"
        } else if !masters_ready {
            "master_data_not_ready"
        } else if !all_available {
            "insufficient_stock"
        } else {
            "ready"
        };
        Ok(SalesOrderConfirmationPreview {
            order_id,
            order_number: order.get("order_number"),
            lifecycle_status,
            version: order.get("version"),
            can_confirm: readiness == "ready",
            readiness: readiness.into(),
            all_available,
            inventory_as_of: Utc::now(),
            lines,
        })
    }
}
