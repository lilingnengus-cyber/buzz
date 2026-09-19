//! Atomic compensation of a confirmed return against an approved effects snapshot.
use super::*;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::AssertSqlSafe;
use std::collections::BTreeMap;

impl ReturnService {
    /// Reverse a confirmed return only when its locked effects match the supplied approval snapshot.
    /// Callers must obtain approval before invoking this command; it exposes no unguarded HTTP route.
    pub async fn reverse_return_guarded(
        &self,
        actor_trace: (Uuid, Uuid),
        sales: bool,
        id: Uuid,
        key: &str,
        input: &ReverseReturn,
        approved: &Value,
    ) -> Result<CommandResult, DomainError> {
        let (actor, trace) = actor_trace;
        super::super::return_scope::check_return(&self.store, actor, sales, id).await?;
        authorize(
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
        let kind = if sales {
            "sales_return"
        } else {
            "purchase_return"
        };
        let command = if sales {
            "sales_return:reverse"
        } else {
            "purchase_return:reverse"
        };
        let hash = request_hash(&(id, input, approved))?;
        let mut tx = self.store.pool().begin().await?;
        if let Some(mut replay) =
            begin_idempotent::<CommandResult>(&mut tx, actor, command, key, &hash).await?
        {
            replay.idempotent_replay = true;
            tx.commit().await?;
            return Ok(replay);
        }
        let plan = self.reversal_plan(&mut tx, actor, sales, id, input).await?;
        if &plan != approved {
            return Err(DomainError::VersionConflict);
        }
        let source = &plan["source"];
        let legal_entity: Uuid = field(source, "legalEntityId")?;
        let warehouse: Uuid = field(source, "warehouseId")?;
        let currency: String = field(source, "currency")?;
        let mut last = BTreeMap::new();
        let inverses = plan["inverseMovements"]
            .as_array()
            .ok_or(DomainError::VersionConflict)?;
        // Undo original postings in reverse order, keeping an explicit link to each original.
        for inverse in inverses.iter().rev() {
            let movement = Uuid::new_v4();
            let sku: Uuid = field(inverse, "skuId")?;
            sqlx::query("INSERT INTO inventory_movements(id,legal_entity_id,warehouse_id,sku_id,movement_type,quantity,unit_cost,total_cost,currency,source_type,source_id,source_line_id,business_date,reversal_of_movement_id,created_by_user_id,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)")
                .bind(movement).bind(legal_entity).bind(warehouse).bind(sku)
                .bind(field::<String>(inverse,"movementType")?).bind(field::<Decimal>(inverse,"quantity")?)
                .bind(field::<Decimal>(inverse,"unitCost")?).bind(field::<Decimal>(inverse,"totalCost")?)
                .bind(&currency).bind(format!("{kind}_reversal")).bind(id)
                .bind(field::<Uuid>(inverse,"sourceLineId")?).bind(input.reversal_date)
                .bind(field::<Uuid>(inverse,"reversesMovementId")?).bind(actor).bind(trace)
                .execute(&mut *tx).await?;
            last.insert(sku, movement);
        }
        for effect in plan["lines"]
            .as_array()
            .ok_or(DomainError::VersionConflict)?
        {
            let sku: Uuid = field(effect, "skuId")?;
            let movement = last.get(&sku).ok_or(DomainError::VersionConflict)?;
            sqlx::query("UPDATE inventory_balances SET on_hand_quantity=$4,quarantined_quantity=$5,inventory_value=$6,average_unit_cost=$7,last_movement_id=$8 WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3")
                .bind(legal_entity).bind(warehouse).bind(sku)
                .bind(field::<Decimal>(effect,"onHandQuantityAfter")?)
                .bind(field::<Decimal>(effect,"quarantinedQuantityAfter")?)
                .bind(field::<Decimal>(effect,"inventoryValueAfter")?)
                .bind(field::<Option<Decimal>>(effect,"averageUnitCostAfter")?).bind(movement)
                .execute(&mut *tx).await?;
        }
        let financial = &plan["financial"];
        let financial_id: Uuid = field(financial, "id")?;
        let original: Decimal = field(financial, "originalAmountAfter")?;
        let before: Decimal = field(financial, "originalAmountBefore")?;
        let (balances, events, fk) = if sales {
            (
                "trade_receivables",
                "trade_receivable_events",
                "receivable_id",
            )
        } else {
            ("trade_payables", "trade_payable_events", "payable_id")
        };
        sqlx::query(AssertSqlSafe(format!("UPDATE {balances} SET original_amount=$2,open_amount=$3,status=$4,trace_id=$5 WHERE id=$1")))
            .bind(financial_id).bind(original).bind(field::<Decimal>(financial,"openAmountAfter")?)
            .bind(field::<String>(financial,"statusAfter")?).bind(trace).execute(&mut *tx).await?;
        let payload = json!({"returnId":id,"reason":input.reason,"reversalDate":input.reversal_date,"effects":plan});
        sqlx::query(AssertSqlSafe(format!("INSERT INTO {events}(id,{fk},event_type,amount,payload,actor_user_id,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7)")))
            .bind(Uuid::new_v4()).bind(financial_id).bind(format!("{kind}_restored")).bind(original-before)
            .bind(&payload).bind(actor).bind(trace).execute(&mut *tx).await?;
        let version = input.expected_version + 1;
        sqlx::query(AssertSqlSafe(format!("UPDATE {kind}s SET status='reversed',version=$2,updated_at=now(),trace_id=$3 WHERE id=$1")))
            .bind(id).bind(version).bind(trace).execute(&mut *tx).await?;
        return_event(
            &mut tx,
            if sales { "sales" } else { "purchase" },
            id,
            "reversed",
            version,
            actor_trace,
            payload.clone(),
        )
        .await?;
        record(
            &mut tx,
            trace,
            actor,
            if sales {
                "SALES_RETURN_REVERSED"
            } else {
                "PURCHASE_RETURN_REVERSED"
            },
            &format!("{kind}_reversed"),
            kind,
            id,
            payload,
        )
        .await?;
        let result = CommandResult {
            id,
            number: field(source, "number")?,
            status: "reversed".into(),
            version,
            trace_id: trace,
            idempotent_replay: false,
        };
        finish_idempotent(&mut tx, actor, command, key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
}

fn field<T: DeserializeOwned>(value: &Value, key: &str) -> Result<T, DomainError> {
    serde_json::from_value(value[key].clone()).map_err(|_| DomainError::VersionConflict)
}
