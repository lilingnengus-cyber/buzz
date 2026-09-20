//! Confirmation-bound allocation and posting, without nested commits.
use super::*;
const OPERATION: &str = "profit_adjustment:post_guarded";
impl AdjustmentService {
    /// Post exactly the read-only preview, atomically persisting allocation, facts and audit.
    /// This domain command does not replace the caller's human approval or policy checks.
    pub async fn post_guarded(
        &self,
        actor: Uuid,
        trace: Uuid,
        batch_id: Uuid,
        key: &str,
        input: &VersionCommand,
        expected: &Value,
    ) -> Result<CommandResult, DomainError> {
        crate::snapshot_transaction::retry(|| async {
            let mut tx = self.store.pool().begin().await?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
                .execute(&mut *tx)
                .await?;
            let result = self
                .post_guarded_on(&mut tx, actor, trace, batch_id, key, input, expected)
                .await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
    }
    /// Join a caller-owned repeatable-read approval transaction. Roll back on any error;
    /// retry serialization failures only by restarting the entire approval transaction.
    #[allow(clippy::too_many_arguments)]
    pub async fn post_guarded_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        trace: Uuid,
        batch_id: Uuid,
        key: &str,
        input: &VersionCommand,
        expected: &Value,
    ) -> Result<CommandResult, DomainError> {
        let isolation: String = sqlx::query_scalar("SHOW transaction_isolation")
            .fetch_one(&mut **tx)
            .await?;
        if isolation != "repeatable read" && isolation != "serializable" {
            return Err(DomainError::Invalid(
                "guarded adjustment requires repeatable-read isolation".into(),
            ));
        }
        let preliminary =
            crate::master_write_authority::read(tx, actor, "profit_adjustment:post").await?;
        let legal: Uuid = sqlx::query_scalar(
            "SELECT legal_entity_id FROM operational_adjustment_batches WHERE id=$1",
        )
        .bind(batch_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
        if !preliminary.scopes.legal_entity_ids.contains(&legal) {
            return Err(DomainError::NotFoundOrForbidden);
        }
        let hash = request_hash(&json!({"batchId":batch_id,"input":input,"expected":expected}))?;
        let replay = begin_idempotent::<CommandResult>(tx, actor, OPERATION, key, &hash).await?;
        sqlx::query("SELECT id FROM operational_adjustment_batches WHERE id=$1 FOR UPDATE")
            .bind(batch_id)
            .fetch_one(&mut **tx)
            .await?;
        let mut targets = std::collections::BTreeSet::new();
        for target in expected["preview"]["targets"]
            .as_array()
            .ok_or(DomainError::StalePreview)?
        {
            let id = target["id"]
                .as_str()
                .and_then(|v| Uuid::parse_str(v).ok())
                .ok_or(DomainError::StalePreview)?;
            ensure_order_scope(tx, id, &preliminary).await?;
            targets.insert(id);
        }
        if targets.is_empty() {
            return Err(DomainError::StalePreview);
        }
        for id in &targets {
            sqlx::query("SELECT id FROM sales_orders WHERE id=$1 FOR SHARE")
                .bind(id)
                .fetch_one(&mut **tx)
                .await?;
        }
        let current =
            crate::master_write_authority::snapshot(tx, actor, "profit_adjustment:post", false)
                .await?;
        if !current.scopes.legal_entity_ids.contains(&legal) {
            return Err(DomainError::NotFoundOrForbidden);
        }
        for id in targets {
            ensure_order_scope(tx, id, &current).await?;
        }
        if let Some(mut result) = replay {
            result.idempotent_replay = true;
            return Ok(result);
        }
        let actual = self
            .allocation_preview_on(tx, actor, batch_id, input)
            .await?;
        if actual != *expected {
            return Err(DomainError::StalePreview);
        }
        // Derived keys keep browser idempotency contracts separate from the bound command.
        let internal_key = format!(
            "guarded-{}",
            hex::encode(Sha256::digest(format!("{batch_id}:{key}:{hash}")))
        );
        let preview = self
            .persist_preview_on(tx, actor, trace, batch_id, &internal_key, input, &current)
            .await?;
        let result = self
            .persist_post_on(
                tx,
                actor,
                trace,
                batch_id,
                &internal_key,
                &PostAdjustment {
                    expected_version: preview.batch_version,
                    preview_id: preview.preview_id,
                    preview_hash: preview.preview_hash,
                },
                &current,
            )
            .await?;
        finish_idempotent(tx, actor, OPERATION, key, &result).await?;
        Ok(result)
    }
}
