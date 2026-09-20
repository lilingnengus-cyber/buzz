//! Read-only complete draft previews and exact preview-bound transaction execution.
use super::*;
use guarded_drafts::{check_references, isolation, lock_references, references};
impl AdjustmentService {
    /// Preview creation (`source=None`) or complete replacement (`source=id, version`).
    /// No draft, numbering, audit, allocation or idempotency records are written.
    pub async fn draft_preview(
        &self,
        actor: Uuid,
        source: Option<(Uuid, i64)>,
        input: &CreateAdjustmentBatch,
    ) -> Result<Value, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        let result = self.draft_preview_on(&mut tx, actor, source, input).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Join an existing repeatable-read transaction without writing business records.
    pub async fn draft_preview_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        source: Option<(Uuid, i64)>,
        input: &CreateAdjustmentBatch,
    ) -> Result<Value, DomainError> {
        isolation(tx).await?;
        validate(input)?;
        if source.is_some_and(|(id, version)| id.is_nil() || version < 1) {
            return Err(DomainError::Invalid(
                "invalid adjustment draft source".into(),
            ));
        }
        let action = if source.is_some() {
            "profit_adjustment:update_draft"
        } else {
            "profit_adjustment:create"
        };
        let current = crate::master_write_authority::snapshot(tx, actor, action, false).await?;
        check_references(tx, input, &current).await?;
        let original = if let Some((id, version)) = source {
            let mut detail = self
                .detail_on(
                    tx,
                    id,
                    &AdjustmentDetailQuery {
                        offset: 0,
                        limit: 100,
                        expected_version: Some(version),
                    },
                    &current,
                )
                .await?;
            if !matches!(
                detail["batch"]["status"].as_str(),
                Some("draft" | "previewed")
            ) {
                return Err(DomainError::Invalid(
                    "only draft or previewed adjustments can be edited".into(),
                ));
            }
            let total = detail["pagination"]["total"]
                .as_u64()
                .ok_or(DomainError::StalePreview)?;
            if total > 200 {
                return Err(DomainError::Invalid(
                    "adjustment draft exceeds line limit".into(),
                ));
            }
            if total > 100 {
                let second = self
                    .detail_on(
                        tx,
                        id,
                        &AdjustmentDetailQuery {
                            offset: 100,
                            limit: 100,
                            expected_version: Some(version),
                        },
                        &current,
                    )
                    .await?;
                let rest = second["lines"]
                    .as_array()
                    .ok_or(DomainError::StalePreview)?;
                detail["lines"]
                    .as_array_mut()
                    .ok_or(DomainError::StalePreview)?
                    .extend(rest.iter().cloned());
            }
            // A confirmation binds all original lines, not a page cursor.
            json!({"batch":detail["batch"],"lines":detail["lines"],"totalAmount":detail["totalAmount"]})
        } else {
            Value::Null
        };
        let mut orders = Vec::new();
        for id in references(input) {
            let row:Value=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'version',version,'legalEntityId',legal_entity_id,'customerId',customer_id,'brandId',brand_id,'businessUnitId',business_unit_id,'currency',currency) FROM sales_orders WHERE id=$1").bind(id).fetch_one(&mut **tx).await?;
            orders.push(row);
        }
        let total = input
            .lines
            .iter()
            .try_fold(Decimal::ZERO, |sum, line| sum.checked_add(line.amount.0))
            .ok_or_else(|| DomainError::Invalid("adjustment amount overflow".into()))?;
        let preview = json!({"schemaVersion":1,"kind":if source.is_some(){"operational_adjustment_draft_replace"}else{"operational_adjustment_draft_create"},"ownerUserId":actor,"source":original,"input":input,"referencedOrders":orders,"scope":current.scopes,"totalAmount":total.to_string(),"currency":input.currency,"effects":{"createsBatch":source.is_none(),"replacesAllLines":source.is_some(),"postsAdjustment":false,"allocatesAmounts":false},"boundary":"management_only_not_general_ledger"});
        let hash = request_hash(&preview)?;
        Ok(json!({"preview":preview,"previewHash":hash}))
    }

    /// Apply exactly a verified draft preview in the caller's approval transaction.
    /// Human signatures, expiry, approval policy and terminal intent handling remain
    /// the caller's responsibility. Roll back the whole transaction on any error.
    #[allow(clippy::too_many_arguments)]
    pub async fn apply_draft_preview_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        trace: Uuid,
        key: &str,
        source: Option<(Uuid, i64)>,
        input: &CreateAdjustmentBatch,
        expected: &Value,
    ) -> Result<CommandResult, DomainError> {
        isolation(tx).await?;
        if let Some((id, _)) = source {
            sqlx::query("SELECT id FROM operational_adjustment_batches WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut **tx)
                .await?
                .ok_or(DomainError::NotFoundOrForbidden)?;
        }
        lock_references(tx, input).await?;
        let actual = self.draft_preview_on(tx, actor, source, input).await?;
        if actual != *expected {
            return Err(DomainError::StalePreview);
        }
        if let Some((id, version)) = source {
            self.replace_draft_guarded_on(
                tx,
                actor,
                trace,
                id,
                key,
                &ReplaceAdjustmentDraft {
                    expected_version: version,
                    batch: input.clone(),
                },
            )
            .await
        } else {
            self.create_guarded_on(tx, actor, trace, key, input).await
        }
    }
}
