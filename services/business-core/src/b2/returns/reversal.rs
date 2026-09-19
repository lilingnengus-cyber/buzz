//! Read-only, transaction-consistent plan for correcting a confirmed return.
use super::*;
use serde_json::Value;
use sqlx::{AssertSqlSafe, Postgres, Transaction};
use std::collections::BTreeMap;

/// Requested business date and reason for reversing a recorded return.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReverseReturn {
    /// Current confirmed return version.
    pub expected_version: i64,
    /// Date on which compensating records should be recorded.
    pub reversal_date: NaiveDate,
    /// User-supplied explanation; original records remain available.
    pub reason: String,
}

impl ReturnService {
    /// Read exact compensating effects without inserting any business or approval record.
    pub async fn reversal_preview(
        &self,
        actor: Uuid,
        sales: bool,
        id: Uuid,
        input: &ReverseReturn,
    ) -> Result<Value, DomainError> {
        let mut input = input.clone();
        input.reason = input.reason.trim().to_owned();
        if input.expected_version < 1
            || input.reason.is_empty()
            || input.reason.len() > 500
            || input.reason.chars().any(char::is_control)
        {
            return Err(DomainError::Invalid(
                "positive version and correction reason are required".into(),
            ));
        }
        super::super::return_scope::check_return(&self.store, actor, sales, id).await?;
        let authority = authorize(
            &self.store,
            actor,
            if sales {
                "shipment:reverse"
            } else {
                "goods_receipt:reverse"
            },
            None,
            None,
            None,
            None,
            None,
        )
        .await?;
        let mut tx = self.store.pool().begin().await?;
        let (table, orders, order_fk, source_fk, financial_fk, party, amount, activity, workflow) =
            if sales {
                (
                    "sales_returns",
                    "sales_orders",
                    "sales_order_id",
                    "shipment_id",
                    "receivable_id",
                    "customer_id",
                    "sales_amount",
                    "GREATEST(r.return_date,r.inspection_date)",
                    "inspection_status",
                )
            } else {
                (
                    "purchase_returns",
                    "purchase_orders",
                    "purchase_order_id",
                    "goods_receipt_id",
                    "payable_id",
                    "supplier_id",
                    "gross_amount",
                    "GREATEST(r.return_date,r.dispatch_date,r.supplier_acknowledged_date)",
                    "logistics_status",
                )
            };
        let row=sqlx::query(AssertSqlSafe(format!("SELECT r.id,r.return_number,r.{source_fk} source_id,r.{order_fk} order_id,r.{financial_fk} financial_id,r.{party} party_id,r.legal_entity_id,r.warehouse_id,r.currency::text currency,r.status,r.version,r.{workflow} workflow_status,r.{amount} amount,{activity} activity_date,o.business_unit_id,o.brand_id FROM {table} r JOIN {orders} o ON o.id=r.{order_fk} WHERE r.id=$1 FOR UPDATE OF r"))).bind(id).fetch_optional(&mut *tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
        if !authority
            .scopes
            .legal_entity_ids
            .contains(&row.get("legal_entity_id"))
            || !authority
                .scopes
                .warehouse_ids
                .contains(&row.get("warehouse_id"))
            || !(if sales {
                &authority.scopes.customer_ids
            } else {
                &authority.scopes.supplier_ids
            })
            .contains(&row.get("party_id"))
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
        if row.get::<i64, _>("version") != input.expected_version {
            return Err(DomainError::VersionConflict);
        }
        if row.get::<String, _>("status") != "confirmed" {
            return Err(DomainError::Invalid(
                "only confirmed returns can be reversed".into(),
            ));
        }
        if input.reversal_date < row.get::<NaiveDate, _>("activity_date") {
            return Err(DomainError::Invalid(
                "correction date predates return activity".into(),
            ));
        }
        let parent=sqlx::query(if sales {"SELECT s.status,s.version,o.version order_version FROM shipments s JOIN sales_orders o ON o.id=s.sales_order_id WHERE s.id=$1 FOR SHARE OF s,o"}else{"SELECT s.status,s.version,o.version order_version FROM goods_receipts s JOIN purchase_orders o ON o.id=s.purchase_order_id WHERE s.id=$1 FOR SHARE OF s,o"}).bind(row.get::<Uuid,_>("source_id")).fetch_one(&mut *tx).await?;
        if parent.get::<String, _>("status") != "confirmed" {
            return Err(DomainError::Invalid(
                "return source is no longer confirmed".into(),
            ));
        }
        let financial=sqlx::query(if sales {"SELECT original_amount,settled_amount,open_amount,status,version FROM trade_receivables WHERE id=$1 FOR UPDATE"}else{"SELECT original_amount,settled_amount,open_amount,status,version FROM trade_payables WHERE id=$1 FOR UPDATE"}).bind(row.get::<Uuid,_>("financial_id")).fetch_one(&mut *tx).await?;
        if financial.get::<String, _>("status") == "reversed" {
            return Err(DomainError::Invalid(
                "return financial source is reversed".into(),
            ));
        }
        let lines = scope_lines(&mut tx, sales, id).await?;
        let movements=sqlx::query("SELECT id,legal_entity_id,warehouse_id,sku_id,source_line_id,movement_type,quantity,unit_cost,total_cost,currency::text currency,posting_sequence FROM inventory_movements WHERE source_type=$1 AND source_id=$2 ORDER BY sku_id,posting_sequence,id")
            .bind(if sales {"sales_return"}else{"purchase_return"}).bind(id).fetch_all(&mut *tx).await?;
        validate_movements(sales, &row, &lines, &movements)?;
        let mut by_sku: BTreeMap<Uuid, (Decimal, Decimal, Vec<Uuid>)> = BTreeMap::new();
        let mut inverse = Vec::new();
        for movement in &movements {
            let quantity = -movement.get::<Decimal, _>("quantity");
            let cost = -movement.get::<Decimal, _>("total_cost");
            let entry = by_sku.entry(movement.get("sku_id")).or_default();
            entry.0 += quantity;
            entry.1 += cost;
            entry.2.push(movement.get("id"));
            inverse.push(json!({"reversesMovementId":movement.get::<Uuid,_>("id"),"sourceLineId":movement.get::<Uuid,_>("source_line_id"),"skuId":movement.get::<Uuid,_>("sku_id"),"movementType":format!("{}_reversal",movement.get::<String,_>("movement_type")),"quantity":quantity.to_string(),"unitCost":movement.get::<Decimal,_>("unit_cost").to_string(),"totalCost":cost.to_string()}));
        }
        let mut effects = Vec::new();
        for (sku, (quantity, cost, ids)) in by_sku {
            let balance=sqlx::query("SELECT on_hand_quantity,reserved_quantity,quarantined_quantity,inventory_value,average_unit_cost,last_movement_id FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3 FOR UPDATE").bind(row.get::<Uuid,_>("legal_entity_id")).bind(row.get::<Uuid,_>("warehouse_id")).bind(sku).fetch_one(&mut *tx).await?;
            if !balance
                .get::<Option<Uuid>, _>("last_movement_id")
                .is_some_and(|last| ids.contains(&last))
            {
                return Err(DomainError::Invalid(
                    "later inventory movements prevent return reversal".into(),
                ));
            }
            // A later scrap must not hide an intervening movement from another document.
            let later:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM inventory_movements m WHERE m.legal_entity_id=$1 AND m.warehouse_id=$2 AND m.sku_id=$3 AND m.posting_sequence >= (SELECT min(posting_sequence) FROM inventory_movements WHERE id=ANY($4)) AND NOT(m.id=ANY($4)))").bind(row.get::<Uuid,_>("legal_entity_id")).bind(row.get::<Uuid,_>("warehouse_id")).bind(sku).bind(&ids).fetch_one(&mut *tx).await?;
            if later {
                return Err(DomainError::Invalid(
                    "intervening inventory movements prevent return reversal".into(),
                ));
            }
            let pending = sales && row.get::<String, _>("workflow_status") == "pending";
            let quarantine_delta = if pending { quantity } else { Decimal::ZERO };
            let new_qty = balance.get::<Decimal, _>("on_hand_quantity") + quantity;
            let new_quarantine =
                balance.get::<Decimal, _>("quarantined_quantity") + quarantine_delta;
            let new_value = money(balance.get::<Decimal, _>("inventory_value") + cost);
            if new_quarantine < Decimal::ZERO
                || new_qty < balance.get::<Decimal, _>("reserved_quantity") + new_quarantine
                || new_value < Decimal::ZERO
                || (new_qty == Decimal::ZERO && new_value != Decimal::ZERO)
            {
                return Err(DomainError::Invalid(
                    "return reversal conflicts with current inventory quantity or value".into(),
                ));
            }
            let average = if new_qty == Decimal::ZERO {
                None
            } else {
                Some(money(new_value / new_qty).to_string())
            };
            effects.push(json!({"skuId":sku,"warehouseId":row.get::<Uuid,_>("warehouse_id"),"onHandQuantityBefore":balance.get::<Decimal,_>("on_hand_quantity").to_string(),"onHandQuantityAfter":new_qty.to_string(),"reservedQuantity":balance.get::<Decimal,_>("reserved_quantity").to_string(),"quarantinedQuantityBefore":balance.get::<Decimal,_>("quarantined_quantity").to_string(),"quarantinedQuantityAfter":new_quarantine.to_string(),"inventoryValueBefore":balance.get::<Decimal,_>("inventory_value").to_string(),"inventoryValueAfter":new_value.to_string(),"averageUnitCostBefore":balance.get::<Option<Decimal>,_>("average_unit_cost").map(|v|v.to_string()),"averageUnitCostAfter":average,"lastMovementId":balance.get::<Option<Uuid>,_>("last_movement_id")}));
        }
        let restored: Decimal = row.get("amount");
        let open = financial.get::<Decimal, _>("open_amount") + restored;
        let snapshot = json!({"source":{"id":id,"number":row.get::<String,_>("return_number"),"version":input.expected_version,"legalEntityId":row.get::<Uuid,_>("legal_entity_id"),"businessUnitId":row.get::<Uuid,_>("business_unit_id"),"warehouseId":row.get::<Uuid,_>("warehouse_id"),"brandId":row.get::<Option<Uuid>,_>("brand_id"),"customerId":if sales{Some(row.get::<Uuid,_>("party_id"))}else{None},"supplierId":if sales{None}else{Some(row.get::<Uuid,_>("party_id"))},"currency":row.get::<String,_>("currency"),"status":"confirmed","workflowStatus":row.get::<String,_>("workflow_status"),"fulfillmentId":row.get::<Uuid,_>("source_id"),"fulfillmentVersion":parent.get::<i64,_>("version"),"orderId":row.get::<Uuid,_>("order_id"),"orderVersion":parent.get::<i64,_>("order_version"),"lines":lines},"command":input,"lines":effects,"inverseMovements":inverse,"financial":{"id":row.get::<Uuid,_>("financial_id"),"version":financial.get::<i64,_>("version"),"originalAmountBefore":financial.get::<Decimal,_>("original_amount").to_string(),"originalAmountAfter":(financial.get::<Decimal,_>("original_amount")+restored).to_string(),"openAmountBefore":financial.get::<Decimal,_>("open_amount").to_string(),"openAmountAfter":open.to_string(),"settledAmount":financial.get::<Decimal,_>("settled_amount").to_string(),"statusBefore":financial.get::<String,_>("status"),"statusAfter":balance_status(financial.get("settled_amount"),open)},"statusAfter":"reversed"});
        tx.rollback().await?;
        Ok(snapshot)
    }
}

async fn scope_lines(
    tx: &mut Transaction<'_, Postgres>,
    sales: bool,
    id: Uuid,
) -> Result<Vec<Value>, DomainError> {
    let sql = if sales {
        "SELECT jsonb_build_object('id',l.id,'skuId',l.sku_id,'quantity',l.quantity::text,'cost',l.total_cost::text,'amount',l.sales_amount::text,'scrapQuantity',l.scrap_quantity::text,'scrapCost',l.scrap_cost_amount::text,'brandId',ol.brand_id,'currentBrandId',p.brand_id) FROM sales_return_lines l JOIN shipment_lines sl ON sl.id=l.shipment_line_id JOIN sales_order_lines ol ON ol.id=sl.sales_order_line_id JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id WHERE l.sales_return_id=$1 ORDER BY l.id"
    } else {
        "SELECT jsonb_build_object('id',l.id,'skuId',l.sku_id,'quantity',l.quantity::text,'cost',l.total_cost::text,'amount',l.gross_amount::text,'scrapQuantity','0','scrapCost','0','brandId',ol.brand_id,'currentBrandId',p.brand_id) FROM purchase_return_lines l JOIN goods_receipt_lines sl ON sl.id=l.goods_receipt_line_id JOIN purchase_order_lines ol ON ol.id=sl.purchase_order_line_id JOIN business_skus sku ON sku.id=l.sku_id JOIN business_products p ON p.id=sku.product_id WHERE l.purchase_return_id=$1 ORDER BY l.id"
    };
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_all(&mut **tx)
        .await
        .map_err(Into::into)
}

fn validate_movements(
    sales: bool,
    header: &sqlx::postgres::PgRow,
    lines: &[Value],
    movements: &[sqlx::postgres::PgRow],
) -> Result<(), DomainError> {
    let invalid =
        || DomainError::Invalid("return inventory evidence is incomplete or inconsistent".into());
    if lines.is_empty() {
        return Err(invalid());
    }
    if movements
        .iter()
        .any(|m| m.get::<Option<i64>, _>("posting_sequence").is_none())
    {
        return Err(DomainError::Invalid("historical movement ordering is unavailable; automatic return reversal is not supported".into()));
    }
    let mut expected_count = 0;
    let mut amount = Decimal::ZERO;
    for line in lines {
        let id = line["id"]
            .as_str()
            .and_then(|v| v.parse::<Uuid>().ok())
            .ok_or_else(invalid)?;
        let decimal = |key: &str| {
            line[key]
                .as_str()
                .and_then(|v| v.parse::<Decimal>().ok())
                .ok_or_else(invalid)
        };
        let quantity = decimal("quantity")?;
        amount += decimal("amount")?;
        let cost = decimal("cost")?;
        let scrap = decimal("scrapQuantity")?;
        let scrap_cost = decimal("scrapCost")?;
        for (kind, qty, value) in [
            (
                if sales {
                    "sales_return"
                } else {
                    "purchase_return"
                },
                if sales { quantity } else { -quantity },
                if sales { cost } else { -cost },
            ),
            ("sales_return_scrap", -scrap, -scrap_cost),
        ] {
            if kind == "sales_return_scrap" && (!sales || scrap == Decimal::ZERO) {
                continue;
            }
            let matches: Vec<_> = movements
                .iter()
                .filter(|m| {
                    m.get::<Option<Uuid>, _>("source_line_id") == Some(id)
                        && m.get::<String, _>("movement_type") == kind
                })
                .collect();
            if matches.len() != 1 {
                return Err(invalid());
            }
            let m = matches[0];
            if m.get::<Uuid, _>("legal_entity_id") != header.get::<Uuid, _>("legal_entity_id")
                || m.get::<Uuid, _>("warehouse_id") != header.get::<Uuid, _>("warehouse_id")
                || m.get::<Decimal, _>("quantity") != qty
                || m.get::<Decimal, _>("total_cost") != value
                || m.get::<String, _>("currency") != header.get::<String, _>("currency")
                || m.get::<Uuid, _>("sku_id").to_string()
                    != line["skuId"].as_str().ok_or_else(invalid)?
            {
                return Err(invalid());
            }
            expected_count += 1;
        }
    }
    if movements.len() != expected_count || amount != header.get::<Decimal, _>("amount") {
        return Err(invalid());
    }
    Ok(())
}
