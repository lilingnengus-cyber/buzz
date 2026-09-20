//! Exact immutable-fact reversal previews and atomic preview-bound execution.
use super::*;
use std::collections::BTreeSet;
const OPERATION: &str = "profit_adjustment:reverse_guarded";
fn validate_command(input: &VersionCommand, reason: &str) -> Result<(), DomainError> {
    if input.expected_version < 1
        || reason.trim().is_empty()
        || reason.chars().count() > 1000
        || reason
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(DomainError::Invalid(
            "invalid reversal version or reason".into(),
        ));
    }
    Ok(())
}
impl AdjustmentService {
    /// Read the exact immutable facts to offset. Never persists a preview or reversal.
    pub async fn reversal_preview(
        &self,
        actor: Uuid,
        id: Uuid,
        input: &VersionCommand,
        reason: &str,
    ) -> Result<Value, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        let value = self
            .reversal_preview_on(&mut tx, actor, id, input, reason)
            .await?;
        tx.commit().await?;
        Ok(value)
    }
    /// Join the caller's repeatable-read transaction, preserving all frozen fact dimensions.
    pub async fn reversal_preview_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        id: Uuid,
        input: &VersionCommand,
        reason: &str,
    ) -> Result<Value, DomainError> {
        guarded_drafts::isolation(tx).await?;
        validate_command(input, reason)?;
        let current =
            crate::master_write_authority::snapshot(tx, actor, "profit_adjustment:reverse", false)
                .await?;
        let detail = self
            .detail_on(
                tx,
                id,
                &AdjustmentDetailQuery {
                    offset: 0,
                    limit: 1,
                    expected_version: Some(input.expected_version),
                },
                &current,
            )
            .await?;
        let batch = &detail["batch"];
        if batch["status"] != "posted" {
            return Err(DomainError::Invalid(
                "only posted adjustments can be reversed".into(),
            ));
        }
        let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('allocation',to_jsonb(a)||jsonb_build_object('allocated_amount',a.allocated_amount::text,'weight',a.weight::text),'fact',to_jsonb(f)||jsonb_build_object('amount',f.amount::text,'quantity',f.quantity::text)) FROM operational_adjustment_allocations a JOIN profit_facts f ON f.id=a.profit_fact_id WHERE a.batch_id=$1 ORDER BY a.id").bind(id).fetch_all(&mut **tx).await?;
        if rows.is_empty() {
            return Err(DomainError::Invalid(
                "posted adjustment has no reversible facts".into(),
            ));
        }
        let allocation_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM operational_adjustment_allocations WHERE batch_id=$1",
        )
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;
        if usize::try_from(allocation_count).ok() != Some(rows.len()) {
            return Err(DomainError::StalePreview);
        }
        let mut total = Decimal::ZERO;
        let mut orders = BTreeSet::new();
        for row in &rows {
            let allocation = &row["allocation"];
            let fact = &row["fact"];
            if fact["direction"] != "normal"
                || fact["source_type"] != "operational_adjustment"
                || fact["source_id"] != json!(id)
                || fact["source_line_id"] != allocation["id"]
                || fact["id"] != allocation["profit_fact_id"]
                || fact["sales_order_id"] != allocation["sales_order_id"]
                || fact["legal_entity_id"] != batch["legal_entity_id"]
                || fact["currency"] != batch["currency"]
            {
                return Err(DomainError::StalePreview);
            }
            let amount = fact["amount"]
                .as_str()
                .and_then(|s| s.parse::<Decimal>().ok())
                .ok_or(DomainError::StalePreview)?;
            let allocated = allocation["allocated_amount"]
                .as_str()
                .and_then(|s| s.parse::<Decimal>().ok())
                .ok_or(DomainError::StalePreview)?;
            if amount < Decimal::ZERO || amount.round_dp(2) != amount || allocated != amount {
                return Err(DomainError::StalePreview);
            }
            total = total.checked_add(amount).ok_or(DomainError::StalePreview)?;
            orders.insert(
                fact["sales_order_id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .ok_or(DomainError::StalePreview)?,
            );
        }
        if detail["totalAmount"]
            .as_str()
            .and_then(|s| s.parse::<Decimal>().ok())
            != Some(total)
        {
            return Err(DomainError::StalePreview);
        }
        let preview = json!({"schemaVersion":1,"kind":"operational_adjustment_reversal","ownerUserId":actor,"batch":batch,"reason":reason,"scope":current.scopes,"facts":rows,"targetOrderIds":orders,"totalAmount":total.to_string(),"currency":batch["currency"],"effects":{"reversesAdjustment":true,"preservesOriginalFacts":true,"reallocatesAmounts":false,"bankRefund":false},"boundary":"management_only_not_general_ledger"});
        let hash = request_hash(&preview)?;
        Ok(json!({"preview":preview,"previewHash":hash}))
    }
    /// Reverse exactly the preview in a retryable standalone transaction. Human approval
    /// remains an independent responsibility of the service exposing this domain command.
    #[allow(clippy::too_many_arguments)]
    pub async fn reverse_guarded(
        &self,
        actor: Uuid,
        trace: Uuid,
        id: Uuid,
        key: &str,
        input: &VersionCommand,
        reason: &str,
        expected: &Value,
    ) -> Result<CommandResult, DomainError> {
        crate::snapshot_transaction::retry(|| async {
            let mut tx = self.store.pool().begin().await?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
                .execute(&mut *tx)
                .await?;
            let result = self
                .reverse_guarded_on(&mut tx, actor, trace, id, key, input, reason, expected)
                .await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
    }
    /// Join an atomic approval transaction. Roll back on error and retry only the whole
    /// transaction on serialization failure. No partial reversal can commit here.
    #[allow(clippy::too_many_arguments)]
    pub async fn reverse_guarded_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        trace: Uuid,
        id: Uuid,
        key: &str,
        input: &VersionCommand,
        reason: &str,
        expected: &Value,
    ) -> Result<CommandResult, DomainError> {
        guarded_drafts::isolation(tx).await?;
        validate_command(input, reason)?;
        let hash =
            request_hash(&json!({"batchId":id,"input":input,"reason":reason,"expected":expected}))?;
        let replay = begin_idempotent::<CommandResult>(tx, actor, OPERATION, key, &hash).await?;
        sqlx::query("SELECT id FROM operational_adjustment_batches WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(DomainError::NotFoundOrForbidden)?;
        let mut orders = BTreeSet::new();
        for order in expected["preview"]["targetOrderIds"]
            .as_array()
            .ok_or(DomainError::StalePreview)?
        {
            orders.insert(
                order
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .ok_or(DomainError::StalePreview)?,
            );
        }
        for order in orders {
            sqlx::query("SELECT id FROM sales_orders WHERE id=$1 FOR SHARE")
                .bind(order)
                .fetch_optional(&mut **tx)
                .await?
                .ok_or(DomainError::NotFoundOrForbidden)?;
        }
        let current =
            crate::master_write_authority::snapshot(tx, actor, "profit_adjustment:reverse", false)
                .await?;
        self.detail_on(
            tx,
            id,
            &AdjustmentDetailQuery {
                offset: 0,
                limit: 1,
                expected_version: None,
            },
            &current,
        )
        .await?;
        if let Some(mut result) = replay {
            result.idempotent_replay = true;
            return Ok(result);
        }
        let actual = self
            .reversal_preview_on(tx, actor, id, input, reason)
            .await?;
        if actual != *expected {
            return Err(DomainError::StalePreview);
        }
        let internal_key = format!(
            "guarded-{}",
            hex::encode(Sha256::digest(format!("{id}:{key}:{hash}")))
        );
        let result = self
            .persist_reverse_on(
                tx,
                actor,
                trace,
                id,
                &internal_key,
                input,
                &current,
                Some(reason),
            )
            .await?;
        finish_idempotent(tx, actor, OPERATION, key, &result).await?;
        Ok(result)
    }
}
