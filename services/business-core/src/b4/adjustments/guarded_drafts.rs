//! Draft mutations that join a future signed approval transaction without committing it.
use super::*;
use std::collections::BTreeSet;
impl AdjustmentService {
    /// Create a draft inside the caller's repeatable-read transaction. The caller must
    /// verify human confirmation separately and roll back the entire transaction on error.
    pub async fn create_guarded_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        trace: Uuid,
        key: &str,
        input: &CreateAdjustmentBatch,
    ) -> Result<CommandResult, DomainError> {
        isolation(tx).await?;
        validate(input)?;
        let hash = request_hash(input)?;
        let operation = "profit_adjustment:create_guarded";
        let replay = begin_idempotent::<CommandResult>(tx, actor, operation, key, &hash).await?;
        lock_references(tx, input).await?;
        let current =
            crate::master_write_authority::snapshot(tx, actor, "profit_adjustment:create", false)
                .await?;
        check_references(tx, input, &current).await?;
        if let Some(mut result) = replay {
            // A replay may disclose a draft that has since moved out of scope.
            self.detail_on(tx, result.id, &page(), &current).await?;
            result.idempotent_replay = true;
            return Ok(result);
        }
        let result = self
            .persist_create_on(
                tx,
                actor,
                trace,
                &internal_key(operation, key, &hash),
                input,
            )
            .await?;
        finish_idempotent(tx, actor, operation, key, &result).await?;
        Ok(result)
    }

    /// Replace a complete authorized draft within the caller's transaction, preserving
    /// the current version and batch identity in idempotency. This is not an approval API.
    #[allow(clippy::too_many_arguments)]
    pub async fn replace_draft_guarded_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        trace: Uuid,
        id: Uuid,
        key: &str,
        input: &ReplaceAdjustmentDraft,
    ) -> Result<CommandResult, DomainError> {
        isolation(tx).await?;
        validate(&input.batch)?;
        let hash = request_hash(&json!({"batchId":id,"input":input}))?;
        let operation = "profit_adjustment:update_draft_guarded";
        let replay = begin_idempotent::<CommandResult>(tx, actor, operation, key, &hash).await?;
        sqlx::query("SELECT id FROM operational_adjustment_batches WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(DomainError::NotFoundOrForbidden)?;
        lock_references(tx, &input.batch).await?;
        let current = crate::master_write_authority::snapshot(
            tx,
            actor,
            "profit_adjustment:update_draft",
            false,
        )
        .await?;
        // Authorization covers every old line and target, including lines outside page one.
        self.detail_on(tx, id, &page(), &current).await?;
        check_references(tx, &input.batch, &current).await?;
        if let Some(mut result) = replay {
            result.idempotent_replay = true;
            return Ok(result);
        }
        let result = self
            .persist_replace_draft_on(
                tx,
                actor,
                trace,
                id,
                &internal_key(operation, key, &hash),
                input,
            )
            .await?;
        finish_idempotent(tx, actor, operation, key, &result).await?;
        Ok(result)
    }
}
fn page() -> AdjustmentDetailQuery {
    AdjustmentDetailQuery {
        offset: 0,
        limit: 1,
        expected_version: None,
    }
}
fn internal_key(operation: &str, key: &str, hash: &str) -> String {
    format!(
        "guarded-{}",
        hex::encode(Sha256::digest(format!("{operation}:{key}:{hash}")))
    )
}
pub(super) async fn isolation(tx: &mut Transaction<'_, Postgres>) -> Result<(), DomainError> {
    let isolation: String = sqlx::query_scalar("SHOW transaction_isolation")
        .fetch_one(&mut **tx)
        .await?;
    if isolation != "repeatable read" && isolation != "serializable" {
        return Err(DomainError::Invalid(
            "guarded adjustment draft requires repeatable-read isolation".into(),
        ));
    }
    Ok(())
}
pub(super) fn references(input: &CreateAdjustmentBatch) -> BTreeSet<Uuid> {
    input
        .lines
        .iter()
        .flat_map(|line| {
            line.direct_sales_order_id
                .into_iter()
                .chain(line.sales_order_ids.iter().copied())
                .chain(
                    line.fixed_weights
                        .iter()
                        .map(|weight| weight.sales_order_id),
                )
        })
        .collect()
}
pub(super) async fn lock_references(
    tx: &mut Transaction<'_, Postgres>,
    input: &CreateAdjustmentBatch,
) -> Result<(), DomainError> {
    for id in references(input) {
        sqlx::query("SELECT id FROM sales_orders WHERE id=$1 FOR SHARE")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(DomainError::NotFoundOrForbidden)?;
    }
    Ok(())
}
pub(super) async fn check_references(
    tx: &mut Transaction<'_, Postgres>,
    input: &CreateAdjustmentBatch,
    current: &AuthorizationSnapshot,
) -> Result<(), DomainError> {
    ensure_input_scope(current, input)?;
    for id in references(input) {
        detail::visible_order(tx, id, current).await?;
        let compatible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sales_orders WHERE id=$1 AND legal_entity_id=$2 AND currency=$3)")
            .bind(id).bind(input.legal_entity_id).bind(&input.currency).fetch_one(&mut **tx).await?;
        if !compatible {
            return Err(DomainError::NotFoundOrForbidden);
        }
    }
    Ok(())
}
