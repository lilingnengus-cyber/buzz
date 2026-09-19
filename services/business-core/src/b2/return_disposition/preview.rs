use super::*;
use crate::b2::stock_reversal_guard::{BalanceExpectation, StockReversalGuard};

impl ReturnDispositionService {
    pub(crate) async fn agent_preview(
        &self,
        actor: Uuid,
        id: Uuid,
        kind: &str,
        command: &Value,
    ) -> Result<Value, DomainError> {
        let sales = kind == "sales_return_inspection_intent";
        let (version, normalized) = match kind {
            "sales_return_inspection_intent" => {
                let input: InspectSalesReturn = serde_json::from_value(command.clone())
                    .map_err(|_| DomainError::Invalid("invalid inspection input".into()))?;
                validate_inspection(&input)?;
                (input.expected_version, serde_json::to_value(input))
            }
            "purchase_return_dispatch_intent" => {
                let mut input: DispatchPurchaseReturn = serde_json::from_value(command.clone())
                    .map_err(|_| DomainError::Invalid("invalid dispatch input".into()))?;
                input.carrier = input.carrier.trim().to_owned();
                input.tracking_number = input.tracking_number.trim().to_owned();
                if input.carrier.is_empty()
                    || input.carrier.len() > 120
                    || input.tracking_number.is_empty()
                    || input.tracking_number.len() > 120
                    || input.carrier.chars().any(char::is_control)
                    || input.tracking_number.chars().any(char::is_control)
                {
                    return Err(DomainError::Invalid(
                        "carrier and trackingNumber are required".into(),
                    ));
                }
                (input.expected_version, serde_json::to_value(input))
            }
            "purchase_return_acknowledgment_intent" => {
                let input: AcknowledgePurchaseReturn = serde_json::from_value(command.clone())
                    .map_err(|_| DomainError::Invalid("invalid acknowledgment input".into()))?;
                if input
                    .acknowledgment_note
                    .as_ref()
                    .is_some_and(|v| v.len() > 1000)
                {
                    return Err(DomainError::Invalid(
                        "acknowledgmentNote is too long".into(),
                    ));
                }
                (input.expected_version, serde_json::to_value(input))
            }
            _ => return Err(DomainError::NotFoundOrForbidden),
        };
        let normalized = normalized.map_err(|e| DomainError::Invalid(e.to_string()))?;
        crate::b2::return_scope::check_return(&self.store, actor, sales, id).await?;
        let auth = self
            .store
            .snapshot(actor)
            .await
            .map_err(|_| DomainError::NotFoundOrForbidden)?;
        let mut tx = self.store.pool().begin().await?;
        let sql = if sales {
            "SELECT r.id,r.return_number,r.legal_entity_id,r.warehouse_id,r.customer_id party_id,r.return_date,r.currency::text currency,r.status,r.version,r.inspection_status workflow_status,NULL::date dispatch_date,NULL::text carrier,NULL::text tracking_number,o.business_unit_id,o.brand_id FROM sales_returns r JOIN sales_orders o ON o.id=r.sales_order_id WHERE r.id=$1 FOR SHARE OF r,o"
        } else {
            "SELECT r.id,r.return_number,r.legal_entity_id,r.warehouse_id,r.supplier_id party_id,r.return_date,r.currency::text currency,r.status,r.version,r.logistics_status workflow_status,r.dispatch_date,r.carrier,r.tracking_number,o.business_unit_id,o.brand_id FROM purchase_returns r JOIN purchase_orders o ON o.id=r.purchase_order_id WHERE r.id=$1 FOR SHARE OF r,o"
        };
        let row = sqlx::query(sql)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(DomainError::NotFoundOrForbidden)?;
        let parties = if sales {
            &auth.scopes.customer_ids
        } else {
            &auth.scopes.supplier_ids
        };
        if !auth.permission_keys.contains(if sales {
            "shipment:reverse"
        } else {
            "goods_receipt:reverse"
        }) || !auth
            .scopes
            .legal_entity_ids
            .contains(&row.get("legal_entity_id"))
            || !auth.scopes.warehouse_ids.contains(&row.get("warehouse_id"))
            || !parties.contains(&row.get("party_id"))
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
        if row.get::<i64, _>("version") != version {
            return Err(DomainError::VersionConflict);
        }
        if row.get::<String, _>("status") != "confirmed" {
            return Err(DomainError::Invalid(
                "only confirmed returns have dispositions".into(),
            ));
        }
        let mut guard = StockReversalGuard {
            order_version: None,
            financial_version: None,
            balances: Vec::new(),
        };
        let mut effects = Vec::new();
        if sales {
            let input: InspectSalesReturn = serde_json::from_value(normalized.clone())
                .map_err(|_| DomainError::Invalid("invalid inspection".into()))?;
            if row.get::<String, _>("workflow_status") != "pending"
                || input.inspection_date < row.get::<NaiveDate, _>("return_date")
            {
                return Err(DomainError::Invalid(
                    "return is not ready for inspection".into(),
                ));
            }
            let lines=sqlx::query("SELECT l.id,l.sku_id,l.quantity,l.unit_cost,l.total_cost,ol.brand_id,p.brand_id current_brand_id FROM sales_return_lines l JOIN shipment_lines sl ON sl.id=l.shipment_line_id JOIN sales_order_lines ol ON ol.id=sl.sales_order_line_id JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id WHERE l.sales_return_id=$1 ORDER BY l.sku_id,l.id").bind(id).fetch_all(&mut *tx).await?;
            let requested = input
                .lines
                .iter()
                .map(|v| (v.return_line_id, v))
                .collect::<BTreeMap<_, _>>();
            if lines.len() != requested.len()
                || lines
                    .iter()
                    .any(|l| !requested.contains_key(&l.get::<Uuid, _>("id")))
            {
                return Err(DomainError::Invalid(
                    "inspection must cover every return line".into(),
                ));
            }
            let mut balances = BTreeMap::new();
            for line in &lines {
                let sku: Uuid = line.get("sku_id");
                if balances.contains_key(&sku) {
                    continue;
                }
                let balance=sqlx::query("SELECT on_hand_quantity,reserved_quantity,quarantined_quantity,inventory_value,last_movement_id FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3 FOR UPDATE").bind(row.get::<Uuid,_>("legal_entity_id")).bind(row.get::<Uuid,_>("warehouse_id")).bind(sku).fetch_one(&mut *tx).await?;
                let balance = BalanceExpectation {
                    legal_entity_id: row.get("legal_entity_id"),
                    warehouse_id: row.get("warehouse_id"),
                    sku_id: sku,
                    on_hand_quantity: balance.get("on_hand_quantity"),
                    reserved_quantity: balance.get("reserved_quantity"),
                    quarantined_quantity: balance.get("quarantined_quantity"),
                    inventory_value: balance.get("inventory_value"),
                    last_movement_id: balance.get("last_movement_id"),
                };
                guard.balances.push(balance.clone());
                balances.insert(sku, balance);
            }
            for line in lines {
                let id: Uuid = line.get("id");
                let sku: Uuid = line.get("sku_id");
                let requested = requested[&id];
                let qty: Decimal = line.get("quantity");
                let scrap = requested.scrap_quantity.0;
                if requested.accepted_quantity.0 + scrap != qty {
                    return Err(DomainError::Invalid(
                        "inspection quantities must equal returned quantity".into(),
                    ));
                }
                let cost = if scrap == qty {
                    line.get::<Decimal, _>("total_cost")
                } else {
                    money(line.get::<Decimal, _>("unit_cost") * scrap)
                };
                let balance = balances
                    .get_mut(&sku)
                    .ok_or(DomainError::NotFoundOrForbidden)?;
                balance.on_hand_quantity -= scrap;
                balance.quarantined_quantity -= qty;
                balance.inventory_value = money(balance.inventory_value - cost);
                if balance.quarantined_quantity < Decimal::ZERO
                    || balance.inventory_value < Decimal::ZERO
                    || balance.on_hand_quantity
                        < balance.reserved_quantity + balance.quarantined_quantity
                {
                    return Err(DomainError::Invalid(
                        "inspection would violate inventory balance".into(),
                    ));
                }
                effects.push(json!({"returnLineId":id,"skuId":sku,"brandId":line.get::<Option<Uuid>,_>("brand_id"),"currentBrandId":line.get::<Option<Uuid>,_>("current_brand_id"),"warehouseId":balance.warehouse_id,"acceptedQuantity":requested.accepted_quantity.0.to_string(),"scrapQuantity":scrap.to_string(),"scrapCost":cost.to_string(),"onHandQuantityAfter":balance.on_hand_quantity.to_string(),"quarantinedQuantityAfter":balance.quarantined_quantity.to_string(),"reservedQuantityAfter":balance.reserved_quantity.to_string(),"inventoryValueAfter":balance.inventory_value.to_string()}));
            }
        } else if kind == "purchase_return_dispatch_intent" {
            let input: DispatchPurchaseReturn = serde_json::from_value(normalized.clone())
                .map_err(|_| DomainError::Invalid("invalid dispatch".into()))?;
            if row.get::<String, _>("workflow_status") != "not_dispatched"
                || input.dispatch_date < row.get::<NaiveDate, _>("return_date")
            {
                return Err(DomainError::Invalid(
                    "purchase return is not ready for dispatch".into(),
                ));
            }
        } else {
            let input: AcknowledgePurchaseReturn = serde_json::from_value(normalized.clone())
                .map_err(|_| DomainError::Invalid("invalid acknowledgment".into()))?;
            if row.get::<String, _>("workflow_status") != "dispatched"
                || row
                    .get::<Option<NaiveDate>, _>("dispatch_date")
                    .is_none_or(|d| input.acknowledged_date < d)
            {
                return Err(DomainError::Invalid(
                    "supplier acknowledgment cannot precede dispatch".into(),
                ));
            }
        }
        let scope_lines_sql = if sales {
            "SELECT jsonb_build_object('skuId',l.sku_id,'quantity',l.quantity::text,'brandId',ol.brand_id,'currentBrandId',p.brand_id) FROM sales_return_lines l JOIN shipment_lines sl ON sl.id=l.shipment_line_id JOIN sales_order_lines ol ON ol.id=sl.sales_order_line_id JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id WHERE l.sales_return_id=$1 ORDER BY l.id"
        } else {
            "SELECT jsonb_build_object('skuId',l.sku_id,'quantity',l.quantity::text,'brandId',ol.brand_id,'currentBrandId',p.brand_id) FROM purchase_return_lines l JOIN goods_receipt_lines gl ON gl.id=l.goods_receipt_line_id JOIN purchase_order_lines ol ON ol.id=gl.purchase_order_line_id JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id WHERE l.purchase_return_id=$1 ORDER BY l.id"
        };
        let scope_lines: Vec<Value> = sqlx::query_scalar(scope_lines_sql)
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
        tx.rollback().await?;
        Ok(
            json!({"source":{"id":id,"lines":scope_lines,"number":row.get::<String,_>("return_number"),"version":version,"legalEntityId":row.get::<Uuid,_>("legal_entity_id"),"businessUnitId":row.get::<Uuid,_>("business_unit_id"),"warehouseId":row.get::<Uuid,_>("warehouse_id"),"brandId":row.get::<Option<Uuid>,_>("brand_id"),"customerId":if sales{Some(row.get::<Uuid,_>("party_id"))}else{None},"supplierId":if sales{None}else{Some(row.get::<Uuid,_>("party_id"))},"status":"confirmed","workflowStatus":row.get::<String,_>("workflow_status"),"returnDate":row.get::<NaiveDate,_>("return_date"),"currency":row.get::<String,_>("currency"),"dispatchDate":row.get::<Option<NaiveDate>,_>("dispatch_date"),"carrier":row.get::<Option<String>,_>("carrier"),"trackingNumber":row.get::<Option<String>,_>("tracking_number")},"command":normalized,"lines":effects,"guard":guard,"changesInventory":sales,"changesReceivableOrPayable":false}),
        )
    }
}
