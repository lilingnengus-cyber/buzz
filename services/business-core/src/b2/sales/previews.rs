use super::*;

impl SalesService {
    /// Deletes a draft from operational reads while preserving its number and audit history.
    pub async fn delete_order_draft(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        id: Uuid,
        key: &str,
        expected_version: i64,
    ) -> Result<crate::b2::model::CommandResult, DomainError> {
        crate::b2::draft_deletion::delete_draft(
            &self.store,
            crate::b2::draft_deletion::OrderKind::Sales,
            actor,
            trace_id,
            id,
            key,
            expected_version,
        )
        .await
    }

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
        let readiness = if lifecycle_status != "draft" {
            "order_not_draft"
        } else if !has_permission {
            "permission_required"
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

    pub async fn shipment_draft_options(
        &self,
        actor: Uuid,
        limit: i64,
    ) -> Result<ShipmentDraftOptions, DomainError> {
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
        let can_create = snapshot.permission_keys.contains("shipment:create");
        let rows = sqlx::query_as::<_, ShipmentDraftOptionLine>(
            "WITH options AS (SELECT o.order_date,o.id order_id,o.order_number,c.code customer_code,c.name customer_name,o.currency::text currency,l.warehouse_id,w.code warehouse_code,w.name warehouse_name,l.id sales_order_line_id,l.line_number,l.sku_id,s.code sku_code,s.name sku_name,l.ordered_quantity,l.shipped_quantity,r.reserved_quantity-r.consumed_quantity-r.released_quantity reservation_open_quantity,COALESCE((SELECT sum(sl.quantity) FROM shipment_lines sl JOIN shipments sh ON sh.id=sl.shipment_id WHERE sl.sales_order_line_id=l.id AND sh.status='draft'),0) draft_allocated_quantity,GREATEST(LEAST(l.ordered_quantity-l.shipped_quantity-l.cancelled_quantity,r.reserved_quantity-r.consumed_quantity-r.released_quantity)-COALESCE((SELECT sum(sl.quantity) FROM shipment_lines sl JOIN shipments sh ON sh.id=sl.shipment_id WHERE sl.sales_order_line_id=l.id AND sh.status='draft'),0),0) shippable_quantity FROM sales_orders o JOIN business_customers c ON c.id=o.customer_id JOIN sales_order_lines l ON l.sales_order_id=o.id JOIN business_warehouses w ON w.id=l.warehouse_id JOIN business_skus s ON s.id=l.sku_id JOIN inventory_reservations r ON r.sales_order_line_id=l.id WHERE o.legal_entity_id=ANY($1) AND o.customer_id=ANY($2) AND o.business_unit_id=ANY($3) AND l.warehouse_id=ANY($4) AND o.lifecycle_status='confirmed' AND o.hold_status='none') SELECT order_id,order_number,customer_code,customer_name,currency,warehouse_id,warehouse_code,warehouse_name,sales_order_line_id,line_number,sku_id,sku_code,sku_name,ordered_quantity,shipped_quantity,reservation_open_quantity,draft_allocated_quantity,shippable_quantity FROM options WHERE shippable_quantity>0 ORDER BY order_date,order_number,warehouse_code,line_number LIMIT $5",
        )
        .bind(snapshot.scopes.legal_entity_ids.into_iter().collect::<Vec<_>>())
        .bind(snapshot.scopes.customer_ids.into_iter().collect::<Vec<_>>())
        .bind(snapshot.scopes.business_unit_ids.into_iter().collect::<Vec<_>>())
        .bind(snapshot.scopes.warehouse_ids.into_iter().collect::<Vec<_>>())
        .bind(limit.clamp(1, 500))
        .fetch_all(self.store.pool())
        .await?;
        Ok(ShipmentDraftOptions {
            can_create,
            data_as_of: Utc::now(),
            items: rows,
        })
    }

    pub async fn shipment_confirmation_preview(
        &self,
        actor: Uuid,
        shipment_id: Uuid,
    ) -> Result<ShipmentConfirmationPreview, DomainError> {
        let shipment = sqlx::query(
            "SELECT sh.shipment_number,sh.sales_order_id,sh.legal_entity_id,sh.warehouse_id,sh.customer_id,sh.shipment_date,sh.sales_amount,sh.currency::text currency,sh.status,sh.version,o.order_number,o.lifecycle_status,o.hold_status,o.payment_terms_days,o.business_unit_id,c.code customer_code,c.name customer_name,w.code warehouse_code,w.name warehouse_name FROM shipments sh JOIN sales_orders o ON o.id=sh.sales_order_id JOIN business_customers c ON c.id=sh.customer_id JOIN business_warehouses w ON w.id=sh.warehouse_id WHERE sh.id=$1",
        )
        .bind(shipment_id)
        .fetch_optional(self.store.pool())
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
        let snapshot = authorize(
            &self.store,
            actor,
            "sales_order:read",
            Some(shipment.get("legal_entity_id")),
            Some(shipment.get("warehouse_id")),
            Some(shipment.get("customer_id")),
            None,
            Some(shipment.get("business_unit_id")),
        )
        .await?;
        let rows = sqlx::query(
            "SELECT sl.sales_order_line_id,sl.sku_id,s.code sku_code,s.name sku_name,sl.quantity,r.reserved_quantity-r.consumed_quantity-r.released_quantity reservation_open_quantity,COALESCE(b.on_hand_quantity,0) on_hand_quantity,COALESCE(b.reserved_quantity,0) reserved_quantity,b.average_unit_cost FROM shipment_lines sl JOIN business_skus s ON s.id=sl.sku_id JOIN inventory_reservations r ON r.id=sl.inventory_reservation_id LEFT JOIN inventory_balances b ON b.legal_entity_id=$2 AND b.warehouse_id=$3 AND b.sku_id=sl.sku_id WHERE sl.shipment_id=$1 ORDER BY sl.id",
        )
        .bind(shipment_id)
        .bind(shipment.get::<Uuid, _>("legal_entity_id"))
        .bind(shipment.get::<Uuid, _>("warehouse_id"))
        .fetch_all(self.store.pool())
        .await?;
        let mut total_cost = Decimal::ZERO;
        let mut all_costed = !rows.is_empty();
        let lines = rows
            .into_iter()
            .map(|row| {
                let quantity: Decimal = row.get("quantity");
                let reservation_open: Decimal = row.get("reservation_open_quantity");
                let on_hand: Decimal = row.get("on_hand_quantity");
                let reserved: Decimal = row.get("reserved_quantity");
                let average: Option<Decimal> = row.get("average_unit_cost");
                let expected_cost = average.map(|value| money(value * quantity));
                if let Some(value) = expected_cost {
                    total_cost += value;
                } else {
                    all_costed = false;
                }
                let readiness = if average.is_none() {
                    "missing_inventory_cost"
                } else if quantity > reservation_open || quantity > on_hand || quantity > reserved {
                    "insufficient_inventory"
                } else {
                    "ready"
                };
                ShipmentConfirmationLine {
                    sales_order_line_id: row.get("sales_order_line_id"),
                    sku_id: row.get("sku_id"),
                    sku_code: row.get("sku_code"),
                    sku_name: row.get("sku_name"),
                    quantity: quantity.into(),
                    reservation_open_quantity: reservation_open.into(),
                    on_hand_quantity: on_hand.into(),
                    reserved_quantity: reserved.into(),
                    average_unit_cost: average.map(Into::into),
                    expected_cost_amount: expected_cost.map(Into::into),
                    ready: readiness == "ready",
                    readiness: readiness.into(),
                }
            })
            .collect::<Vec<_>>();
        let status: String = shipment.get("status");
        let hold_status: String = shipment.get("hold_status");
        let lifecycle_status: String = shipment.get("lifecycle_status");
        let has_inventory = !lines.is_empty() && lines.iter().all(|line| line.ready);
        let has_permission = snapshot.permission_keys.contains("shipment:confirm");
        let readiness = if status != "draft" {
            "shipment_not_draft"
        } else if hold_status != "none" {
            "order_on_hold"
        } else if lifecycle_status != "confirmed" {
            "order_not_fulfillable"
        } else if lines.iter().any(|line| line.average_unit_cost.is_none()) {
            "missing_inventory_cost"
        } else if !has_inventory {
            "insufficient_inventory"
        } else if !has_permission {
            "permission_required"
        } else {
            "ready"
        };
        let shipment_date = shipment.get::<chrono::NaiveDate, _>("shipment_date");
        let sales_amount = shipment.get::<Decimal, _>("sales_amount");
        Ok(ShipmentConfirmationPreview {
            shipment_id,
            shipment_number: shipment.get("shipment_number"),
            sales_order_id: shipment.get("sales_order_id"),
            order_number: shipment.get("order_number"),
            customer_code: shipment.get("customer_code"),
            customer_name: shipment.get("customer_name"),
            warehouse_code: shipment.get("warehouse_code"),
            warehouse_name: shipment.get("warehouse_name"),
            shipment_date,
            status,
            version: shipment.get("version"),
            currency: shipment.get("currency"),
            sales_amount: sales_amount.into(),
            expected_cost_amount: all_costed.then(|| money(total_cost).into()),
            expected_receivable_amount: sales_amount.into(),
            expected_due_date: shipment_date
                + Duration::days(i64::from(shipment.get::<i32, _>("payment_terms_days"))),
            can_confirm: readiness == "ready",
            readiness: readiness.into(),
            inventory_as_of: Utc::now(),
            lines,
        })
    }
}
