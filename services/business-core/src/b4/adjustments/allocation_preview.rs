//! Pure allocation previews shared with the persisted browser preview.
use super::*;
pub(super) struct Calculation {
    pub(super) watermark: i64,
    pub(super) lines: Vec<Value>,
    pub(super) total: Decimal,
    pub(super) allocated: Decimal,
}
pub(super) async fn calculate(
    tx: &mut Transaction<'_, Postgres>,
    batch: &sqlx::postgres::PgRow,
    batch_id: Uuid,
    authorization: &AuthorizationSnapshot,
    max_targets: usize,
) -> Result<Calculation, DomainError> {
    let lines=sqlx::query("SELECT id,metric_type,amount,business_date,allocation_basis,allocation_scope FROM operational_adjustment_lines WHERE batch_id=$1 ORDER BY line_number").bind(batch_id).fetch_all(&mut **tx).await?;
    let watermark: i64 =
        sqlx::query_scalar("SELECT COALESCE(max(fact_sequence),0) FROM profit_facts")
            .fetch_one(&mut **tx)
            .await?;
    let mut payload_lines = Vec::new();
    let mut total = Decimal::ZERO;
    let mut allocated = Decimal::ZERO;
    for line in lines {
        let amount: Decimal = line.get("amount");
        total += amount;
        let targets = targets(tx, batch, &line, max_targets).await?;
        for target in &targets {
            ensure_order_scope(tx, target.0, authorization).await?;
            ensure_line_target_scope(tx, target.0, &line.get::<Value, _>("allocation_scope"))
                .await?;
        }
        let allocations = largest_remainder(
            amount,
            &targets
                .iter()
                .map(|row| AllocationTarget {
                    sales_order_id: row.0,
                    weight: row.1,
                })
                .collect::<Vec<_>>(),
        )?;
        allocated += allocations.iter().map(|row| row.amount).sum::<Decimal>();
        payload_lines.push(json!({"lineId":line.get::<Uuid,_>("id"),"metricType":line.get::<String,_>("metric_type"),"businessDate":line.get::<chrono::NaiveDate,_>("business_date"),"targets":allocations.into_iter().map(|row|json!({"salesOrderId":row.sales_order_id,"weight":row.weight.to_string(),"amount":row.amount.to_string(),"remainderRank":row.remainder_rank})).collect::<Vec<_>>() }));
    }
    Ok(Calculation {
        watermark,
        lines: payload_lines,
        total,
        allocated,
    })
}
impl AdjustmentService {
    /// Compute a stable confirmation preview without updating the batch or persisting a preview.
    pub async fn allocation_preview(
        &self,
        actor: Uuid,
        batch_id: Uuid,
        input: &VersionCommand,
    ) -> Result<Value, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        let result = self
            .allocation_preview_on(&mut tx, actor, batch_id, input)
            .await?;
        tx.commit().await?;
        Ok(result)
    }
    /// Compute in the caller's repeatable-read transaction; never commits or writes business rows.
    pub async fn allocation_preview_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        batch_id: Uuid,
        input: &VersionCommand,
    ) -> Result<Value, DomainError> {
        let isolation: String = sqlx::query_scalar("SHOW transaction_isolation")
            .fetch_one(&mut **tx)
            .await?;
        if isolation != "repeatable read" && isolation != "serializable" {
            return Err(DomainError::Invalid(
                "allocation preview requires repeatable-read isolation".into(),
            ));
        }
        let authorization =
            crate::master_write_authority::snapshot(tx, actor, "profit_adjustment:preview", false)
                .await?;
        let batch = sqlx::query(
            "SELECT b.*,to_jsonb(b) record FROM operational_adjustment_batches b WHERE id=$1",
        )
        .bind(batch_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
        if !authorization
            .scopes
            .legal_entity_ids
            .contains(&batch.get::<Uuid, _>("legal_entity_id"))
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
        if batch.get::<i64, _>("version") != input.expected_version {
            return Err(DomainError::VersionConflict);
        }
        if !matches!(
            batch.get::<String, _>("status").as_str(),
            "draft" | "previewed"
        ) {
            return Err(DomainError::Invalid("adjustment is not previewable".into()));
        }
        let calculation = calculate(tx, &batch, batch_id, &authorization, self.max_targets).await?;
        let lines:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(l)||jsonb_build_object('amount',l.amount::text) FROM operational_adjustment_lines l WHERE batch_id=$1 ORDER BY line_number").bind(batch_id).fetch_all(&mut **tx).await?;
        let mut targets = std::collections::BTreeMap::<Uuid, Value>::new();
        for line in &calculation.lines {
            for target in line["targets"]
                .as_array()
                .ok_or(DomainError::StalePreview)?
            {
                let id = target["salesOrderId"]
                    .as_str()
                    .and_then(|v| Uuid::parse_str(v).ok())
                    .ok_or(DomainError::StalePreview)?;
                if let std::collections::btree_map::Entry::Vacant(entry) = targets.entry(id) {
                    let dimensions:Value=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'version',version,'legalEntityId',legal_entity_id,'customerId',customer_id,'brandId',brand_id,'currency',currency,'salespersonUserId',salesperson_user_id,'businessUnitId',business_unit_id,'departmentId',department_id) FROM sales_orders WHERE id=$1").bind(id).fetch_one(&mut **tx).await?;
                    entry.insert(dimensions);
                }
            }
        }
        let preview = json!({"schemaVersion":1,"kind":"operational_adjustment_allocation","batch":batch.get::<Value,_>("record"),"lines":lines,"targets":targets.into_values().collect::<Vec<_>>(),"allocations":calculation.lines,"sourceWatermark":calculation.watermark,"totalAmount":money(calculation.total).to_string(),"allocatedAmount":money(calculation.allocated).to_string(),"unallocatedAmount":money(calculation.total-calculation.allocated).to_string(),"scope":authorization.scopes,"boundary":"management_only_not_general_ledger"});
        let hash = hex::encode(Sha256::digest(serde_json::to_vec(&preview)?));
        Ok(json!({"preview":preview,"previewHash":hash}))
    }
}
