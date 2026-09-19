//! Transaction-bound expectations from an immutable agent reversal preview.
use super::common::DomainError;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BalanceExpectation {
    pub legal_entity_id: Uuid,
    pub warehouse_id: Uuid,
    pub sku_id: Uuid,
    pub on_hand_quantity: Decimal,
    pub reserved_quantity: Decimal,
    pub quarantined_quantity: Decimal,
    pub inventory_value: Decimal,
    pub last_movement_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StockReversalGuard {
    pub order_version: Option<i64>,
    pub financial_version: Option<i64>,
    pub balances: Vec<BalanceExpectation>,
}

impl StockReversalGuard {
    pub async fn check_order(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        sales: bool,
        id: Uuid,
    ) -> Result<(), DomainError> {
        let sql = if sales {
            "SELECT version FROM sales_orders WHERE id=$1 FOR UPDATE"
        } else {
            "SELECT version FROM purchase_orders WHERE id=$1 FOR UPDATE"
        };
        let current: i64 = sqlx::query_scalar(sql)
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
        if self.order_version != Some(current) {
            return Err(DomainError::VersionConflict);
        }
        Ok(())
    }

    pub fn check_financial(&self, version: i64) -> Result<(), DomainError> {
        if self.financial_version != Some(version) {
            return Err(DomainError::VersionConflict);
        }
        Ok(())
    }

    pub async fn check_balances(
        &self,
        tx: &mut Transaction<'_, Postgres>,
    ) -> Result<(), DomainError> {
        let mut balances: Vec<_> = self.balances.iter().collect();
        balances.sort_by_key(|b| (b.legal_entity_id, b.warehouse_id, b.sku_id));
        for expected in balances {
            let row = sqlx::query("SELECT on_hand_quantity,reserved_quantity,quarantined_quantity,inventory_value,last_movement_id FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3 FOR UPDATE")
                .bind(expected.legal_entity_id).bind(expected.warehouse_id).bind(expected.sku_id)
                .fetch_optional(&mut **tx).await?.ok_or(DomainError::VersionConflict)?;
            if row.get::<Decimal, _>("on_hand_quantity") != expected.on_hand_quantity
                || row.get::<Decimal, _>("reserved_quantity") != expected.reserved_quantity
                || row.get::<Decimal, _>("quarantined_quantity") != expected.quarantined_quantity
                || row.get::<Decimal, _>("inventory_value") != expected.inventory_value
                || row.get::<Option<Uuid>, _>("last_movement_id") != expected.last_movement_id
            {
                return Err(DomainError::VersionConflict);
            }
        }
        Ok(())
    }
}
