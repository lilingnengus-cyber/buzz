use super::*;
use crate::b2::stock_reversal_guard::{BalanceExpectation, StockReversalGuard};
use rust_decimal::Decimal;
use std::collections::BTreeMap;

fn decimal(value: &Value, key: &str) -> Result<Decimal, StoreError> {
    value[key]
        .as_str()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| StoreError::Invalid(format!("missing exact {key}")))
}
fn uuid(value: &Value, key: &str) -> Result<Uuid, StoreError> {
    value[key]
        .as_str()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| StoreError::Invalid(format!("missing {key}")))
}

pub(super) async fn snapshot(
    state: &AppState,
    actor: Uuid,
    kind: &str,
    input: &PrepareStockReversal,
) -> Result<Value, StoreError> {
    let (source_kind, action) = family(kind)?;
    if input.reason.trim().is_empty()
        || input.reason.chars().count() > 500
        || input.reason.chars().any(char::is_control)
        || input.expected_source_version <= 0
    {
        return Err(StoreError::Invalid(
            "current version and bounded reversal reason required".into(),
        ));
    }
    stock::check_stock_scope(&state.store, actor, source_kind, input.source_document_id).await?;
    let authority = state.store.snapshot(actor).await?;
    if !authority.permission_keys.contains(action) {
        return Err(StoreError::NotFoundOrForbidden);
    }
    let (header, lines_sql) = queries(source_kind)?;
    let mut source: Value = sqlx::query_scalar(header)
        .bind(input.source_document_id)
        .fetch_one(state.store.pool())
        .await?;
    if source["version"].as_i64() != Some(input.expected_source_version)
        || source["status"]
            != if source_kind == "inventory_opening" {
                "posted"
            } else {
                "confirmed"
            }
    {
        return Err(StoreError::Conflict);
    }
    let mut lines: Vec<Value> = sqlx::query_scalar(lines_sql)
        .bind(input.source_document_id)
        .fetch_all(state.store.pool())
        .await?;
    if lines.is_empty() {
        return Err(StoreError::Conflict);
    }
    for line in &lines {
        if line["brandId"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .is_some_and(|id| !authority.scopes.brand_ids.contains(&id))
        {
            return Err(StoreError::NotFoundOrForbidden);
        }
    }
    let mut order = Value::Null;
    let mut financial = Value::Null;
    if source_kind != "inventory_opening" {
        order = super::super::snapshot::order(
            state,
            actor,
            if source_kind == "shipment" {
                "sales_order"
            } else {
                "purchase_order"
            },
            uuid(&source, "orderId")?,
        )
        .await?;
        source["businessUnitId"] = order["businessUnitId"].clone();
        source["customerId"] = order["customerId"].clone();
        source["supplierId"] = order["supplierId"].clone();
        let sql = if source_kind == "shipment" {
            "SELECT jsonb_build_object('id',id,'version',version,'originalAmount',original_amount::text,'settledAmount',settled_amount::text,'openAmount',open_amount::text,'status',status) FROM trade_receivables WHERE shipment_id=$1"
        } else {
            "SELECT jsonb_build_object('id',id,'version',version,'originalAmount',original_amount::text,'settledAmount',settled_amount::text,'openAmount',open_amount::text,'status',status) FROM trade_payables WHERE goods_receipt_id=$1"
        };
        financial = sqlx::query_scalar(sql)
            .bind(input.source_document_id)
            .fetch_one(state.store.pool())
            .await?;
        if decimal(&financial, "settledAmount")? != Decimal::ZERO
            || financial["status"] == "reversed"
        {
            return Err(StoreError::Invalid(
                "reverse allocations before stock reversal".into(),
            ));
        }
        let sql = if source_kind == "shipment" {
            "SELECT EXISTS(SELECT 1 FROM sales_returns WHERE shipment_id=$1 AND status IN ('draft','confirmed'))"
        } else {
            "SELECT EXISTS(SELECT 1 FROM purchase_returns WHERE goods_receipt_id=$1 AND status IN ('draft','confirmed'))"
        };
        let returns: bool = sqlx::query_scalar(sql)
            .bind(input.source_document_id)
            .fetch_one(state.store.pool())
            .await?;
        if returns {
            return Err(StoreError::Invalid("source has active returns".into()));
        }
    }
    let legal = uuid(&source, "legalEntityId")?;
    let mut totals: BTreeMap<(Uuid, Uuid), (Decimal, Decimal)> = BTreeMap::new();
    for line in &lines {
        let key = (uuid(line, "warehouseId")?, uuid(line, "skuId")?);
        let total = totals.entry(key).or_default();
        total.0 = total
            .0
            .checked_add(decimal(line, "quantity")?)
            .ok_or(StoreError::Conflict)?;
        total.1 = total
            .1
            .checked_add(decimal(line, "totalCost")?)
            .ok_or(StoreError::Conflict)?;
    }
    let mut guard = StockReversalGuard {
        order_version: order["version"].as_i64(),
        financial_version: financial["version"].as_i64(),
        balances: Vec::new(),
    };
    let mut effects = Vec::new();
    for ((warehouse, sku), (quantity, cost)) in totals {
        let row=sqlx::query("SELECT on_hand_quantity,reserved_quantity,quarantined_quantity,inventory_value,last_movement_id FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3").bind(legal).bind(warehouse).bind(sku).fetch_one(state.store.pool()).await?;
        let balance = BalanceExpectation {
            legal_entity_id: legal,
            warehouse_id: warehouse,
            sku_id: sku,
            on_hand_quantity: row.get("on_hand_quantity"),
            reserved_quantity: row.get("reserved_quantity"),
            quarantined_quantity: row.get("quarantined_quantity"),
            inventory_value: row.get("inventory_value"),
            last_movement_id: row.get("last_movement_id"),
        };
        let add = source_kind == "shipment";
        let delta = if add { quantity } else { -quantity };
        let cost_delta = if add { cost } else { -cost };
        let after = balance
            .on_hand_quantity
            .checked_add(delta)
            .ok_or(StoreError::Conflict)?;
        let after_reserved = balance
            .reserved_quantity
            .checked_add(if add { quantity } else { Decimal::ZERO })
            .ok_or(StoreError::Conflict)?;
        let after_value = balance
            .inventory_value
            .checked_add(cost_delta)
            .ok_or(StoreError::Conflict)?;
        if after < after_reserved + balance.quarantined_quantity
            || after_value < Decimal::ZERO
            || (after == Decimal::ZERO && after_value != Decimal::ZERO)
        {
            return Err(StoreError::Invalid(
                "reversal would invalidate stock quantity, value, reservations or quarantine"
                    .into(),
            ));
        }
        if !add {
            let latest_source: Option<Uuid> =
                sqlx::query_scalar("SELECT source_id FROM inventory_movements WHERE id=$1")
                    .bind(balance.last_movement_id)
                    .fetch_optional(state.store.pool())
                    .await?;
            let later: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM inventory_movements original JOIN inventory_movements later ON later.legal_entity_id=original.legal_entity_id AND later.warehouse_id=original.warehouse_id AND later.sku_id=original.sku_id AND later.posted_at>original.posted_at WHERE original.source_id=$1 AND original.warehouse_id=$2 AND original.sku_id=$3)")
                .bind(input.source_document_id).bind(warehouse).bind(sku).fetch_one(state.store.pool()).await?;
            if later || latest_source != Some(input.source_document_id) {
                return Err(StoreError::Invalid(
                    "source has subsequent inventory movements".into(),
                ));
            }
        }
        effects.push(json!({"warehouseId":warehouse,"skuId":sku,"quantityChange":delta.to_string(),"inventoryValueChange":cost_delta.to_string(),"reservedQuantityChange":if add {quantity.to_string()}else{"0".into()},"onHandQuantityAfter":after.to_string(),"reservedQuantityAfter":after_reserved.to_string(),"quarantinedQuantityAfter":balance.quarantined_quantity.to_string(),"inventoryValueAfter":after_value.to_string()}));
        guard.balances.push(balance);
    }
    for line in &mut lines {
        line["legalEntityId"] = json!(legal);
    }
    Ok(
        json!({"source":source,"lines":lines,"order":order,"financial":financial,"guard":guard,"inventoryEffects":effects,"reason":input.reason,"resultStatus":"reversed","effect":if source_kind=="shipment" {"恢复出库数量、原始成本与库存预留，逆转对应应收，订单恢复待履约；不删除历史、不退款"}else if source_kind=="goods_receipt" {"扣减原收货数量与成本，逆转对应应付，恢复采购待收货数量；不删除历史、不付款"}else{"扣减期初数量与原始成本，不产生应收应付或银行交易"}}),
    )
}

fn queries(kind: &str) -> Result<(&'static str, &'static str), StoreError> {
    match kind {
        "shipment"=>Ok((
            "SELECT jsonb_build_object('id',id,'number',shipment_number,'legalEntityId',legal_entity_id,'warehouseId',warehouse_id,'orderId',sales_order_id,'currency',currency,'businessDate',shipment_date,'status',status,'version',version) FROM shipments WHERE id=$1",
            "SELECT jsonb_build_object('id',l.id,'skuId',l.sku_id,'skuName',s.name,'brandId',ol.brand_id,'warehouseId',h.warehouse_id,'quantity',l.quantity::text,'unitCost',l.unit_cost::text,'totalCost',l.total_cost::text,'orderLineId',l.sales_order_line_id) FROM shipment_lines l JOIN shipments h ON h.id=l.shipment_id JOIN sales_order_lines ol ON ol.id=l.sales_order_line_id JOIN business_skus s ON s.id=l.sku_id WHERE l.shipment_id=$1 ORDER BY l.id")),
        "goods_receipt"=>Ok((
            "SELECT jsonb_build_object('id',id,'number',goods_receipt_number,'legalEntityId',legal_entity_id,'warehouseId',warehouse_id,'orderId',purchase_order_id,'currency',currency,'businessDate',receipt_date,'status',status,'version',version) FROM goods_receipts WHERE id=$1",
            "SELECT jsonb_build_object('id',l.id,'skuId',l.sku_id,'skuName',s.name,'brandId',ol.brand_id,'warehouseId',h.warehouse_id,'quantity',l.received_quantity::text,'unitCost',l.provisional_unit_cost::text,'totalCost',l.provisional_total_cost::text,'orderLineId',l.purchase_order_line_id) FROM goods_receipt_lines l JOIN goods_receipts h ON h.id=l.goods_receipt_id JOIN purchase_order_lines ol ON ol.id=l.purchase_order_line_id JOIN business_skus s ON s.id=l.sku_id WHERE l.goods_receipt_id=$1 ORDER BY l.id")),
        "inventory_opening"=>Ok((
            "SELECT jsonb_build_object('id',id,'number',batch_number,'legalEntityId',legal_entity_id,'currency',currency,'businessDate',business_date,'status',status,'version',version) FROM inventory_opening_batches WHERE id=$1",
            "SELECT jsonb_build_object('id',l.id,'skuId',l.sku_id,'skuName',s.name,'brandId',p.brand_id,'warehouseId',l.warehouse_id,'quantity',l.quantity::text,'unitCost',l.unit_cost::text,'totalCost',l.total_cost::text) FROM inventory_opening_lines l JOIN business_skus s ON s.id=l.sku_id JOIN business_products p ON p.id=s.product_id WHERE l.batch_id=$1 ORDER BY l.id")),
        _=>Err(StoreError::NotFoundOrForbidden),
    }
}
