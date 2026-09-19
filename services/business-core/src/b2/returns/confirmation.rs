use super::*;
use crate::b2::return_confirmation::ReturnConfirmationGuard;

impl ReturnService {
    /// Confirm a sales return using the established browser command contract.
    pub async fn confirm_sales_return(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        id: Uuid,
        key: &str,
        input: &VersionCommand,
    ) -> Result<CommandResult, DomainError> {
        self.confirm_sales_return_guarded(actor, trace_id, id, key, input, None)
            .await
    }

    pub(crate) async fn confirm_sales_return_guarded(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        id: Uuid,
        key: &str,
        input: &VersionCommand,
        guard: Option<&ReturnConfirmationGuard>,
    ) -> Result<CommandResult, DomainError> {
        let pre = sqlx::query(
            "SELECT legal_entity_id,warehouse_id,customer_id FROM sales_returns WHERE id=$1",
        )
        .bind(id)
        .fetch_optional(self.store.pool())
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
        authorize(
            &self.store,
            actor,
            "shipment:reverse",
            Some(pre.get("legal_entity_id")),
            Some(pre.get("warehouse_id")),
            Some(pre.get("customer_id")),
            None,
            None,
        )
        .await?;
        crate::b2::return_scope::check_return(&self.store, actor, true, id).await?;
        let hash = match guard {
            Some(guard) => request_hash(&(input, guard))?,
            None => request_hash(input)?,
        };
        let mut tx = self.store.pool().begin().await?;
        if let Some(mut replay) =
            begin_idempotent::<CommandResult>(&mut tx, actor, "sales_return:confirm", key, &hash)
                .await?
        {
            replay.idempotent_replay = true;
            tx.commit().await?;
            return Ok(replay);
        }
        let ret=sqlx::query("SELECT return_number,shipment_id,receivable_id,legal_entity_id,warehouse_id,return_date,currency::text,status,version FROM sales_returns WHERE id=$1 FOR UPDATE").bind(id).fetch_one(&mut *tx).await?;
        check_draft(&ret, input.expected_version)?;
        if let Some(guard) = guard {
            guard
                .check_source(&mut tx, true, ret.get("shipment_id"))
                .await?;
        }
        let receivable=sqlx::query("SELECT original_amount,settled_amount,open_amount,status,version FROM trade_receivables WHERE id=$1 FOR UPDATE").bind(ret.get::<Uuid,_>("receivable_id")).fetch_one(&mut *tx).await?;
        if let Some(guard) = guard {
            guard
                .check_financial_and_balances(&mut tx, receivable.get("version"))
                .await?;
        }
        if receivable.get::<String, _>("status") == "reversed" {
            return Err(DomainError::Invalid(
                "cannot return a reversed source".into(),
            ));
        }
        let lines=sqlx::query("SELECT rl.id,rl.shipment_line_id,rl.sku_id,rl.quantity,sl.quantity source_quantity,sl.sales_amount source_sales,sl.unit_cost,sl.total_cost FROM sales_return_lines rl JOIN shipment_lines sl ON sl.id=rl.shipment_line_id WHERE rl.sales_return_id=$1 ORDER BY rl.sku_id,rl.id FOR UPDATE OF rl").bind(id).fetch_all(&mut *tx).await?;
        let mut sales_total = Decimal::ZERO;
        let mut cost_total = Decimal::ZERO;
        for line in &lines {
            let qty: Decimal = line.get("quantity");
            let already = confirmed_sales_amount(&mut tx, line.get("shipment_line_id"), id).await?;
            let prior_qty:Decimal=sqlx::query_scalar("SELECT COALESCE(sum(rl.quantity),0) FROM sales_return_lines rl JOIN sales_returns r ON r.id=rl.sales_return_id WHERE rl.shipment_line_id=$1 AND r.status='confirmed'").bind(line.get::<Uuid,_>("shipment_line_id")).fetch_one(&mut *tx).await?;
            let final_line = prior_qty + qty == line.get::<Decimal, _>("source_quantity");
            let sales = if final_line {
                line.get::<Decimal, _>("source_sales") - already
            } else {
                money(
                    line.get::<Decimal, _>("source_sales") * qty
                        / line.get::<Decimal, _>("source_quantity"),
                )
            };
            let unit = line
                .get::<Option<Decimal>, _>("unit_cost")
                .ok_or(DomainError::MissingInventoryCost)?;
            let prior_cost: Decimal = sqlx::query_scalar("SELECT COALESCE(sum(rl.total_cost),0) FROM sales_return_lines rl JOIN sales_returns r ON r.id=rl.sales_return_id WHERE rl.shipment_line_id=$1 AND r.status='confirmed' AND r.id<>$2")
                .bind(line.get::<Uuid,_>("shipment_line_id")).bind(id).fetch_one(&mut *tx).await?;
            let cost = if final_line {
                line.get::<Decimal, _>("total_cost") - prior_cost
            } else {
                money(unit * qty)
            };
            sales_total += sales;
            cost_total += cost;
            let balance=sqlx::query("SELECT on_hand_quantity,inventory_value FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3 FOR UPDATE").bind(ret.get::<Uuid,_>("legal_entity_id")).bind(ret.get::<Uuid,_>("warehouse_id")).bind(line.get::<Uuid,_>("sku_id")).fetch_one(&mut *tx).await?;
            let new_qty = balance.get::<Decimal, _>("on_hand_quantity") + qty;
            let new_value = money(balance.get::<Decimal, _>("inventory_value") + cost);
            if new_value < Decimal::ZERO {
                return Err(DomainError::Invalid(
                    "return cost exceeds inventory value".into(),
                ));
            }
            let movement = Uuid::new_v4();
            sqlx::query("INSERT INTO inventory_movements(id,legal_entity_id,warehouse_id,sku_id,movement_type,quantity,unit_cost,total_cost,currency,source_type,source_id,source_line_id,business_date,created_by_user_id,trace_id) VALUES($1,$2,$3,$4,'sales_return',$5,$6,$7,$8,'sales_return',$9,$10,$11,$12,$13)").bind(movement).bind(ret.get::<Uuid,_>("legal_entity_id")).bind(ret.get::<Uuid,_>("warehouse_id")).bind(line.get::<Uuid,_>("sku_id")).bind(qty).bind(unit).bind(cost).bind(ret.get::<String,_>("currency")).bind(id).bind(line.get::<Uuid,_>("id")).bind(ret.get::<NaiveDate,_>("return_date")).bind(actor).bind(trace_id).execute(&mut *tx).await?;
            sqlx::query("UPDATE inventory_balances SET on_hand_quantity=$4,quarantined_quantity=quarantined_quantity+$5,inventory_value=$6,average_unit_cost=$7,last_movement_id=$8 WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3").bind(ret.get::<Uuid,_>("legal_entity_id")).bind(ret.get::<Uuid,_>("warehouse_id")).bind(line.get::<Uuid,_>("sku_id")).bind(new_qty).bind(qty).bind(new_value).bind(money(new_value/new_qty)).bind(movement).execute(&mut *tx).await?;
            sqlx::query("UPDATE sales_return_lines SET sales_amount=$2,unit_cost=$3,total_cost=$4,inventory_movement_id=$5 WHERE id=$1").bind(line.get::<Uuid,_>("id")).bind(sales).bind(unit).bind(cost).bind(movement).execute(&mut *tx).await?;
        }
        if sales_total > receivable.get::<Decimal, _>("open_amount") {
            return Err(DomainError::ReceivableAlreadySettled);
        }
        let new_original = receivable.get::<Decimal, _>("original_amount") - sales_total;
        let new_open = receivable.get::<Decimal, _>("open_amount") - sales_total;
        let status = balance_status(receivable.get("settled_amount"), new_open);
        sqlx::query("UPDATE trade_receivables SET original_amount=$2,open_amount=$3,status=$4,trace_id=$5 WHERE id=$1").bind(ret.get::<Uuid,_>("receivable_id")).bind(new_original).bind(new_open).bind(status).bind(trace_id).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO trade_receivable_events(id,receivable_id,event_type,amount,payload,actor_user_id,trace_id) VALUES($1,$2,'sales_return_reduced',$3,$4,$5,$6)").bind(Uuid::new_v4()).bind(ret.get::<Uuid,_>("receivable_id")).bind(sales_total).bind(json!({"salesReturnId":id})).bind(actor).bind(trace_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE sales_returns SET version=version+1,updated_at=now(),status='confirmed',inspection_status='pending',sales_amount=$2,cost_amount=$3,confirmed_by_user_id=$4,confirmed_at=now(),trace_id=$5 WHERE id=$1").bind(id).bind(money(sales_total)).bind(money(cost_total)).bind(actor).bind(trace_id).execute(&mut *tx).await?;
        let version = input.expected_version + 1;
        return_event(&mut tx,"sales",id,"confirmed",version,(actor,trace_id),json!({"salesAmount":money(sales_total).to_string(),"costAmount":money(cost_total).to_string()})).await?;
        record(
            &mut tx,
            trace_id,
            actor,
            "SALES_RETURN_CONFIRMED",
            "sales_return_confirmed",
            "sales_return",
            id,
            json!({"version":version}),
        )
        .await?;
        let result = CommandResult {
            id,
            number: ret.get("return_number"),
            status: "confirmed".into(),
            version,
            trace_id,
            idempotent_replay: false,
        };
        finish_idempotent(&mut tx, actor, "sales_return:confirm", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Confirm a purchase return using the established browser command contract.
    pub async fn confirm_purchase_return(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        id: Uuid,
        key: &str,
        input: &VersionCommand,
    ) -> Result<CommandResult, DomainError> {
        self.confirm_purchase_return_guarded(actor, trace_id, id, key, input, None)
            .await
    }

    pub(crate) async fn confirm_purchase_return_guarded(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        id: Uuid,
        key: &str,
        input: &VersionCommand,
        guard: Option<&ReturnConfirmationGuard>,
    ) -> Result<CommandResult, DomainError> {
        let pre = sqlx::query(
            "SELECT legal_entity_id,warehouse_id,supplier_id FROM purchase_returns WHERE id=$1",
        )
        .bind(id)
        .fetch_optional(self.store.pool())
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
        crate::b3::common::authorize(
            &self.store,
            actor,
            "goods_receipt:reverse",
            Some(pre.get("legal_entity_id")),
            Some(pre.get("warehouse_id")),
            Some(pre.get("supplier_id")),
            None,
            None,
        )
        .await?;
        crate::b2::return_scope::check_return(&self.store, actor, false, id).await?;
        let hash = match guard {
            Some(guard) => request_hash(&(input, guard))?,
            None => request_hash(input)?,
        };
        let mut tx = self.store.pool().begin().await?;
        if let Some(mut replay) =
            begin_idempotent::<CommandResult>(&mut tx, actor, "purchase_return:confirm", key, &hash)
                .await?
        {
            replay.idempotent_replay = true;
            tx.commit().await?;
            return Ok(replay);
        }
        let ret=sqlx::query("SELECT return_number,goods_receipt_id,payable_id,legal_entity_id,warehouse_id,return_date,currency::text,status,version FROM purchase_returns WHERE id=$1 FOR UPDATE").bind(id).fetch_one(&mut *tx).await?;
        check_draft(&ret, input.expected_version)?;
        if let Some(guard) = guard {
            guard
                .check_source(&mut tx, false, ret.get("goods_receipt_id"))
                .await?;
        }
        let payable=sqlx::query("SELECT original_amount,settled_amount,open_amount,status,version FROM trade_payables WHERE id=$1 FOR UPDATE").bind(ret.get::<Uuid,_>("payable_id")).fetch_one(&mut *tx).await?;
        if let Some(guard) = guard {
            guard
                .check_financial_and_balances(&mut tx, payable.get("version"))
                .await?;
        }
        if payable.get::<String, _>("status") == "reversed" {
            return Err(DomainError::Invalid(
                "cannot return a reversed source".into(),
            ));
        }
        let lines=sqlx::query("SELECT rl.id,rl.goods_receipt_line_id,rl.sku_id,rl.quantity,gl.received_quantity source_quantity,gl.net_amount source_net,gl.tax_amount source_tax,gl.gross_amount source_gross FROM purchase_return_lines rl JOIN goods_receipt_lines gl ON gl.id=rl.goods_receipt_line_id WHERE rl.purchase_return_id=$1 ORDER BY rl.sku_id,rl.id FOR UPDATE OF rl").bind(id).fetch_all(&mut *tx).await?;
        let mut net_total = Decimal::ZERO;
        let mut tax_total = Decimal::ZERO;
        let mut gross_total = Decimal::ZERO;
        let mut cost_total = Decimal::ZERO;
        for line in &lines {
            let qty: Decimal = line.get("quantity");
            let (prior_net, prior_tax, prior_gross) =
                confirmed_purchase_amounts(&mut tx, line.get("goods_receipt_line_id"), id).await?;
            let prior_qty:Decimal=sqlx::query_scalar("SELECT COALESCE(sum(rl.quantity),0) FROM purchase_return_lines rl JOIN purchase_returns r ON r.id=rl.purchase_return_id WHERE rl.goods_receipt_line_id=$1 AND r.status='confirmed'").bind(line.get::<Uuid,_>("goods_receipt_line_id")).fetch_one(&mut *tx).await?;
            let final_line = prior_qty + qty == line.get::<Decimal, _>("source_quantity");
            let net = if final_line {
                line.get::<Decimal, _>("source_net") - prior_net
            } else {
                money(
                    line.get::<Decimal, _>("source_net") * qty
                        / line.get::<Decimal, _>("source_quantity"),
                )
            };
            let tax = if final_line {
                line.get::<Decimal, _>("source_tax") - prior_tax
            } else {
                money(
                    line.get::<Decimal, _>("source_tax") * qty
                        / line.get::<Decimal, _>("source_quantity"),
                )
            };
            let gross = if final_line {
                line.get::<Decimal, _>("source_gross") - prior_gross
            } else {
                money(net + tax)
            };
            let balance=sqlx::query("SELECT on_hand_quantity,reserved_quantity,quarantined_quantity,inventory_value,average_unit_cost FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3 FOR UPDATE").bind(ret.get::<Uuid,_>("legal_entity_id")).bind(ret.get::<Uuid,_>("warehouse_id")).bind(line.get::<Uuid,_>("sku_id")).fetch_one(&mut *tx).await?;
            if balance.get::<Decimal, _>("on_hand_quantity")
                - balance.get::<Decimal, _>("reserved_quantity")
                - balance.get::<Decimal, _>("quarantined_quantity")
                < qty
            {
                return Err(DomainError::InsufficientStock(
                    json!({"skuId":line.get::<Uuid,_>("sku_id")}),
                ));
            }
            let unit = balance
                .get::<Option<Decimal>, _>("average_unit_cost")
                .ok_or(DomainError::MissingInventoryCost)?;
            let cost = crate::b2::return_confirmation::purchase_cost(
                balance.get("on_hand_quantity"),
                balance.get("inventory_value"),
                unit,
                qty,
            );
            let new_qty = balance.get::<Decimal, _>("on_hand_quantity") - qty;
            let new_value = if new_qty == Decimal::ZERO {
                Decimal::ZERO
            } else {
                money(balance.get::<Decimal, _>("inventory_value") - cost)
            };
            if new_value < Decimal::ZERO {
                return Err(DomainError::Invalid(
                    "return cost exceeds inventory value".into(),
                ));
            }
            let movement = Uuid::new_v4();
            sqlx::query("INSERT INTO inventory_movements(id,legal_entity_id,warehouse_id,sku_id,movement_type,quantity,unit_cost,total_cost,currency,source_type,source_id,source_line_id,business_date,created_by_user_id,trace_id) VALUES($1,$2,$3,$4,'purchase_return',$5,$6,$7,$8,'purchase_return',$9,$10,$11,$12,$13)").bind(movement).bind(ret.get::<Uuid,_>("legal_entity_id")).bind(ret.get::<Uuid,_>("warehouse_id")).bind(line.get::<Uuid,_>("sku_id")).bind(-qty).bind(unit).bind(-cost).bind(ret.get::<String,_>("currency")).bind(id).bind(line.get::<Uuid,_>("id")).bind(ret.get::<NaiveDate,_>("return_date")).bind(actor).bind(trace_id).execute(&mut *tx).await?;
            let avg = if new_qty == Decimal::ZERO {
                None
            } else {
                Some(money(new_value / new_qty))
            };
            sqlx::query("UPDATE inventory_balances SET on_hand_quantity=$4,inventory_value=$5,average_unit_cost=$6,last_movement_id=$7 WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3").bind(ret.get::<Uuid,_>("legal_entity_id")).bind(ret.get::<Uuid,_>("warehouse_id")).bind(line.get::<Uuid,_>("sku_id")).bind(new_qty).bind(new_value).bind(avg).bind(movement).execute(&mut *tx).await?;
            sqlx::query("UPDATE purchase_return_lines SET net_amount=$2,tax_amount=$3,gross_amount=$4,unit_cost=$5,total_cost=$6,inventory_movement_id=$7 WHERE id=$1").bind(line.get::<Uuid,_>("id")).bind(net).bind(tax).bind(gross).bind(unit).bind(cost).bind(movement).execute(&mut *tx).await?;
            net_total += net;
            tax_total += tax;
            gross_total += gross;
            cost_total += cost;
        }
        if gross_total > payable.get::<Decimal, _>("open_amount") {
            return Err(DomainError::PayableAlreadySettled);
        }
        let new_original = payable.get::<Decimal, _>("original_amount") - gross_total;
        let new_open = payable.get::<Decimal, _>("open_amount") - gross_total;
        let status = balance_status(payable.get("settled_amount"), new_open);
        sqlx::query("UPDATE trade_payables SET original_amount=$2,open_amount=$3,status=$4,trace_id=$5 WHERE id=$1").bind(ret.get::<Uuid,_>("payable_id")).bind(new_original).bind(new_open).bind(status).bind(trace_id).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO trade_payable_events(id,payable_id,event_type,amount,payload,actor_user_id,trace_id) VALUES($1,$2,'purchase_return_reduced',$3,$4,$5,$6)").bind(Uuid::new_v4()).bind(ret.get::<Uuid,_>("payable_id")).bind(gross_total).bind(json!({"purchaseReturnId":id})).bind(actor).bind(trace_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE purchase_returns SET version=version+1,updated_at=now(),status='confirmed',net_amount=$2,tax_amount=$3,gross_amount=$4,inventory_cost_amount=$5,confirmed_by_user_id=$6,confirmed_at=now(),trace_id=$7 WHERE id=$1").bind(id).bind(money(net_total)).bind(money(tax_total)).bind(money(gross_total)).bind(money(cost_total)).bind(actor).bind(trace_id).execute(&mut *tx).await?;
        let version = input.expected_version + 1;
        return_event(&mut tx,"purchase",id,"confirmed",version,(actor,trace_id),json!({"grossAmount":money(gross_total).to_string(),"inventoryCostAmount":money(cost_total).to_string()})).await?;
        record(
            &mut tx,
            trace_id,
            actor,
            "PURCHASE_RETURN_CONFIRMED",
            "purchase_return_confirmed",
            "purchase_return",
            id,
            json!({"version":version}),
        )
        .await?;
        let result = CommandResult {
            id,
            number: ret.get("return_number"),
            status: "confirmed".into(),
            version,
            trace_id,
            idempotent_replay: false,
        };
        finish_idempotent(&mut tx, actor, "purchase_return:confirm", key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
}
