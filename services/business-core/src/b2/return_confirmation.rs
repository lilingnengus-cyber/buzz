//! Return effects and the transaction expectations bound to approval.
use super::{
    common::money,
    stock_reversal_guard::{BalanceExpectation, StockReversalGuard},
    DomainError,
};
use crate::store::PgStore;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Postgres, Row, Transaction};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReturnConfirmationGuard {
    source_version: i64,
    order_version: i64,
    financial_version: i64,
    balances: Vec<BalanceExpectation>,
    average_unit_costs: BTreeMap<Uuid, Option<Decimal>>,
}
impl ReturnConfirmationGuard {
    pub async fn check_source(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        sales: bool,
        id: Uuid,
    ) -> Result<(), DomainError> {
        let sql = if sales {
            "SELECT s.version,o.version order_version FROM shipments s JOIN sales_orders o ON o.id=s.sales_order_id WHERE s.id=$1 FOR SHARE OF s,o"
        } else {
            "SELECT s.version,o.version order_version FROM goods_receipts s JOIN purchase_orders o ON o.id=s.purchase_order_id WHERE s.id=$1 FOR SHARE OF s,o"
        };
        let row = sqlx::query(sql).bind(id).fetch_one(&mut **tx).await?;
        if row.get::<i64, _>("version") != self.source_version
            || row.get::<i64, _>("order_version") != self.order_version
        {
            return Err(DomainError::VersionConflict);
        }
        Ok(())
    }
    pub async fn check_financial_and_balances(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        version: i64,
    ) -> Result<(), DomainError> {
        if version != self.financial_version {
            return Err(DomainError::VersionConflict);
        }
        StockReversalGuard {
            order_version: None,
            financial_version: None,
            balances: self.balances.clone(),
        }
        .check_balances(tx)
        .await?;
        for balance in &self.balances {
            let average:Option<Decimal>=sqlx::query_scalar("SELECT average_unit_cost FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3").bind(balance.legal_entity_id).bind(balance.warehouse_id).bind(balance.sku_id).fetch_one(&mut **tx).await?;
            if self.average_unit_costs.get(&balance.sku_id) != Some(&average) {
                return Err(DomainError::VersionConflict);
            }
        }
        Ok(())
    }
}

pub(super) async fn preview(
    store: &PgStore,
    actor: Uuid,
    sales: bool,
    id: Uuid,
) -> Result<Value, DomainError> {
    let (header, source_sql, financial_sql, lines_sql) = if sales {
        ("SELECT r.*,o.business_unit_id,o.brand_id,o.version initial_order_version,r.shipment_id source_id,r.receivable_id financial_id,r.customer_id party_id FROM sales_returns r JOIN sales_orders o ON o.id=r.sales_order_id WHERE r.id=$1 FOR UPDATE OF r",
        "SELECT s.version,s.status,o.version order_version FROM shipments s JOIN sales_orders o ON o.id=s.sales_order_id WHERE s.id=$1 FOR SHARE OF s,o",
        "SELECT version,original_amount,settled_amount,open_amount,status FROM trade_receivables WHERE id=$1 FOR UPDATE",
        "SELECT r.id,r.sku_id,r.quantity,ol.brand_id,p.brand_id current_brand_id,l.quantity source_quantity,l.sales_amount source_amount,l.unit_cost,l.total_cost source_cost,COALESCE(a.quantity,0) prior_quantity,COALESCE(a.amount,0) prior_amount,COALESCE(a.cost,0) prior_cost FROM sales_return_lines r JOIN shipment_lines l ON l.id=r.shipment_line_id JOIN sales_order_lines ol ON ol.id=l.sales_order_line_id JOIN business_skus sku ON sku.id=r.sku_id JOIN business_products p ON p.id=sku.product_id LEFT JOIN LATERAL(SELECT sum(x.quantity) quantity,sum(x.sales_amount) amount,sum(x.total_cost) cost FROM sales_return_lines x JOIN sales_returns h ON h.id=x.sales_return_id WHERE x.shipment_line_id=l.id AND h.status='confirmed' AND h.id<>$1) a ON true WHERE r.sales_return_id=$1 ORDER BY r.sku_id,r.id")
    } else {
        ("SELECT r.*,o.business_unit_id,o.brand_id,o.version initial_order_version,r.goods_receipt_id source_id,r.payable_id financial_id,r.supplier_id party_id FROM purchase_returns r JOIN purchase_orders o ON o.id=r.purchase_order_id WHERE r.id=$1 FOR UPDATE OF r",
        "SELECT s.version,s.status,o.version order_version FROM goods_receipts s JOIN purchase_orders o ON o.id=s.purchase_order_id WHERE s.id=$1 FOR SHARE OF s,o",
        "SELECT version,original_amount,settled_amount,open_amount,status FROM trade_payables WHERE id=$1 FOR UPDATE",
        "SELECT r.id,r.sku_id,r.quantity,ol.brand_id,p.brand_id current_brand_id,l.received_quantity source_quantity,l.net_amount source_net,l.tax_amount source_tax,l.gross_amount source_gross,COALESCE(a.quantity,0) prior_quantity,COALESCE(a.net,0) prior_net,COALESCE(a.tax,0) prior_tax,COALESCE(a.gross,0) prior_gross FROM purchase_return_lines r JOIN goods_receipt_lines l ON l.id=r.goods_receipt_line_id JOIN purchase_order_lines ol ON ol.id=l.purchase_order_line_id JOIN business_skus sku ON sku.id=r.sku_id JOIN business_products p ON p.id=sku.product_id LEFT JOIN LATERAL(SELECT sum(x.quantity) quantity,sum(x.net_amount) net,sum(x.tax_amount) tax,sum(x.gross_amount) gross FROM purchase_return_lines x JOIN purchase_returns h ON h.id=x.purchase_return_id WHERE x.goods_receipt_line_id=l.id AND h.status='confirmed' AND h.id<>$1) a ON true WHERE r.purchase_return_id=$1 ORDER BY r.sku_id,r.id")
    };
    super::return_scope::check_return(store, actor, sales, id).await?;
    let auth = store
        .snapshot(actor)
        .await
        .map_err(|_| DomainError::NotFoundOrForbidden)?;
    let mut tx = store.pool().begin().await?;
    let ret = sqlx::query(header)
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
        .contains(&ret.get("legal_entity_id"))
        || !auth.scopes.warehouse_ids.contains(&ret.get("warehouse_id"))
        || !parties.contains(&ret.get("party_id"))
    {
        return Err(DomainError::NotFoundOrForbidden);
    }
    if ret.get::<String, _>("status") != "draft" {
        return Err(DomainError::Invalid(
            "only draft returns can be confirmed".into(),
        ));
    }
    let source = sqlx::query(source_sql)
        .bind(ret.get::<Uuid, _>("source_id"))
        .fetch_one(&mut *tx)
        .await?;
    if source.get::<i64, _>("order_version") != ret.get::<i64, _>("initial_order_version") {
        return Err(DomainError::VersionConflict);
    }
    if source.get::<String, _>("status") != "confirmed" {
        return Err(DomainError::Invalid(
            "return source is not confirmed".into(),
        ));
    }
    let financial = sqlx::query(financial_sql)
        .bind(ret.get::<Uuid, _>("financial_id"))
        .fetch_one(&mut *tx)
        .await?;
    if financial.get::<String, _>("status") == "reversed" {
        return Err(DomainError::Invalid(
            "cannot return a reversed source".into(),
        ));
    }
    let lines = sqlx::query(lines_sql).bind(id).fetch_all(&mut *tx).await?;
    let mut guard = ReturnConfirmationGuard {
        source_version: source.get("version"),
        order_version: source.get("order_version"),
        financial_version: financial.get("version"),
        balances: Vec::new(),
        average_unit_costs: BTreeMap::new(),
    };
    let mut working = BTreeMap::<Uuid, (BalanceExpectation, Option<Decimal>)>::new();
    for line in &lines {
        let sku: Uuid = line.get("sku_id");
        if working.contains_key(&sku) {
            continue;
        }
        let row=sqlx::query("SELECT on_hand_quantity,reserved_quantity,quarantined_quantity,inventory_value,last_movement_id,average_unit_cost FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3 FOR UPDATE").bind(ret.get::<Uuid,_>("legal_entity_id")).bind(ret.get::<Uuid,_>("warehouse_id")).bind(sku).fetch_one(&mut *tx).await?;
        let balance = BalanceExpectation {
            legal_entity_id: ret.get("legal_entity_id"),
            warehouse_id: ret.get("warehouse_id"),
            sku_id: sku,
            on_hand_quantity: row.get("on_hand_quantity"),
            reserved_quantity: row.get("reserved_quantity"),
            quarantined_quantity: row.get("quarantined_quantity"),
            inventory_value: row.get("inventory_value"),
            last_movement_id: row.get("last_movement_id"),
        };
        let average = row.get::<Option<Decimal>, _>("average_unit_cost");
        guard.balances.push(balance.clone());
        guard.average_unit_costs.insert(sku, average);
        working.insert(sku, (balance, average));
    }
    let mut effects = Vec::new();
    let mut amount = Decimal::ZERO;
    let mut cost = Decimal::ZERO;
    for line in &lines {
        let sku: Uuid = line.get("sku_id");
        let (balance, average) = working
            .get_mut(&sku)
            .ok_or(DomainError::NotFoundOrForbidden)?;
        let quantity: Decimal = line.get("quantity");
        let total: Decimal = line.get("source_quantity");
        let last = line.get::<Decimal, _>("prior_quantity") + quantity == total;
        let (line_amount, line_cost) = if sales {
            let unit = line
                .get::<Option<Decimal>, _>("unit_cost")
                .ok_or(DomainError::MissingInventoryCost)?;
            (
                if last {
                    line.get::<Decimal, _>("source_amount") - line.get::<Decimal, _>("prior_amount")
                } else {
                    money(line.get::<Decimal, _>("source_amount") * quantity / total)
                },
                if last {
                    line.get::<Decimal, _>("source_cost") - line.get::<Decimal, _>("prior_cost")
                } else {
                    money(unit * quantity)
                },
            )
        } else {
            if balance.on_hand_quantity - balance.reserved_quantity - balance.quarantined_quantity
                < quantity
            {
                return Err(DomainError::InsufficientStock(json!({"skuId":sku})));
            }
            let net = if last {
                line.get::<Decimal, _>("source_net") - line.get::<Decimal, _>("prior_net")
            } else {
                money(line.get::<Decimal, _>("source_net") * quantity / total)
            };
            let tax = if last {
                line.get::<Decimal, _>("source_tax") - line.get::<Decimal, _>("prior_tax")
            } else {
                money(line.get::<Decimal, _>("source_tax") * quantity / total)
            };
            (
                if last {
                    line.get::<Decimal, _>("source_gross") - line.get::<Decimal, _>("prior_gross")
                } else {
                    money(net + tax)
                },
                purchase_cost(
                    balance.on_hand_quantity,
                    balance.inventory_value,
                    average.ok_or(DomainError::MissingInventoryCost)?,
                    quantity,
                ),
            )
        };
        if sales {
            balance.on_hand_quantity += quantity;
            balance.quarantined_quantity += quantity;
            balance.inventory_value = money(balance.inventory_value + line_cost);
        } else {
            balance.on_hand_quantity -= quantity;
            balance.inventory_value = money(balance.inventory_value - line_cost);
        }
        if balance.inventory_value < Decimal::ZERO {
            return Err(DomainError::Invalid(
                "return cost exceeds inventory value".into(),
            ));
        }
        *average = if balance.on_hand_quantity == Decimal::ZERO {
            None
        } else {
            Some(money(balance.inventory_value / balance.on_hand_quantity))
        };
        effects.push(json!({"returnLineId":line.get::<Uuid,_>("id"),"skuId":sku,"brandId":line.get::<Option<Uuid>,_>("brand_id"),"currentBrandId":line.get::<Option<Uuid>,_>("current_brand_id"),"warehouseId":ret.get::<Uuid,_>("warehouse_id"),"quantity":quantity.to_string(),"amount":line_amount.to_string(),"cost":line_cost.to_string(),"onHandQuantityAfter":balance.on_hand_quantity.to_string(),"quarantinedQuantityAfter":balance.quarantined_quantity.to_string(),"reservedQuantityAfter":balance.reserved_quantity.to_string(),"inventoryValueAfter":balance.inventory_value.to_string()}));
        amount += line_amount;
        cost += line_cost;
    }
    let open: Decimal = financial.get("open_amount");
    if amount > open {
        return Err(if sales {
            DomainError::ReceivableAlreadySettled
        } else {
            DomainError::PayableAlreadySettled
        });
    }
    tx.rollback().await?;
    Ok(
        json!({"id":id,"number":ret.get::<String,_>("return_number"),"version":ret.get::<i64,_>("version"),"status":"draft","legalEntityId":ret.get::<Uuid,_>("legal_entity_id"),"businessUnitId":ret.get::<Uuid,_>("business_unit_id"),"warehouseId":ret.get::<Uuid,_>("warehouse_id"),"brandId":ret.get::<Option<Uuid>,_>("brand_id"),"customerId":if sales{Some(ret.get::<Uuid,_>("party_id"))}else{None},"supplierId":if sales{None}else{Some(ret.get::<Uuid,_>("party_id"))},"sourceId":ret.get::<Uuid,_>("source_id"),"returnDate":ret.get::<chrono::NaiveDate,_>("return_date"),"reasonCode":ret.get::<String,_>("reason_code"),"businessNote":ret.get::<Option<String>,_>("business_note"),"currency":ret.get::<String,_>("currency"),"amount":amount.to_string(),"cost":cost.to_string(),"financialId":ret.get::<Uuid,_>("financial_id"),"originalAmountBefore":financial.get::<Decimal,_>("original_amount").to_string(),"originalAmountAfter":(financial.get::<Decimal,_>("original_amount")-amount).to_string(),"settledAmount":financial.get::<Decimal,_>("settled_amount").to_string(),"openAmountBefore":open.to_string(),"openAmountAfter":(open-amount).to_string(),"lines":effects,"guard":guard}),
    )
}

pub(super) fn purchase_cost(
    on_hand: Decimal,
    value: Decimal,
    unit: Decimal,
    quantity: Decimal,
) -> Decimal {
    if on_hand == quantity {
        value
    } else {
        money(unit * quantity)
    }
}
