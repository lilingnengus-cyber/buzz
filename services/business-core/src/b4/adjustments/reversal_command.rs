//! Existing reversal persistence shared by the guarded approval transaction.
use super::*;
impl AdjustmentService {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn persist_reverse_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        trace_id: Uuid,
        batch_id: Uuid,
        key: &str,
        input: &VersionCommand,
        authorization: &AuthorizationSnapshot,
        reason: Option<&str>,
    ) -> Result<CommandResult, DomainError> {
        let hash = request_hash(input)?;
        if let Some(mut replay) =
            begin_idempotent::<CommandResult>(tx, actor, "profit_adjustment:reverse", key, &hash)
                .await?
        {
            replay.idempotent_replay = true;
            return Ok(replay);
        }
        let batch=sqlx::query("SELECT adjustment_number,status,version FROM operational_adjustment_batches WHERE id=$1 FOR UPDATE").bind(batch_id).fetch_one(&mut **tx).await?;
        let target_orders = sqlx::query_scalar::<_, Uuid>("SELECT DISTINCT sales_order_id FROM operational_adjustment_allocations WHERE batch_id=$1")
            .bind(batch_id).fetch_all(&mut **tx).await?;
        for order_id in target_orders {
            ensure_order_scope(tx, order_id, authorization).await?;
        }
        if batch.get::<i64, _>("version") != input.expected_version {
            return Err(DomainError::VersionConflict);
        }
        if batch.get::<String, _>("status") != "posted" {
            return Err(DomainError::Invalid(
                "only posted adjustments can be reversed".into(),
            ));
        }
        let reverse_event = Uuid::new_v4();
        let version = input.expected_version + 1;
        let inserted=sqlx::query("INSERT INTO profit_facts(id,metric_type,direction,amount,currency,quantity,legal_entity_id,sales_order_id,sales_order_line_id,shipment_id,shipment_line_id,customer_id,sku_id,product_category_id,brand_id,salesperson_user_id,business_unit_id,department_id,warehouse_id,business_date,management_period,source_system,source_type,source_id,source_line_id,source_event_id,source_event_version,data_as_of,trace_id) SELECT gen_random_uuid(),f.metric_type,'reversal',f.amount,f.currency,f.quantity,f.legal_entity_id,f.sales_order_id,f.sales_order_line_id,f.shipment_id,f.shipment_line_id,f.customer_id,f.sku_id,f.product_category_id,f.brand_id,f.salesperson_user_id,f.business_unit_id,f.department_id,f.warehouse_id,f.business_date,f.management_period,'business_core_b4','operational_adjustment',$1,a.id,$2,$3,now(),$4 FROM operational_adjustment_allocations a JOIN profit_facts f ON f.id=a.profit_fact_id WHERE a.batch_id=$1").bind(batch_id).bind(reverse_event).bind(version).bind(trace_id).execute(&mut **tx).await?;
        sqlx::query("UPDATE operational_adjustment_batches SET status='reversed',reversed_at=now(),updated_by_user_id=$2,trace_id=$3 WHERE id=$1").bind(batch_id).bind(actor).bind(trace_id).execute(&mut **tx).await?;
        event_with_id(
            tx,
            reverse_event,
            batch_id,
            "reversed",
            version,
            actor,
            trace_id,
            details(inserted.rows_affected(), None, reason),
        )
        .await?;
        record(
            tx,
            trace_id,
            actor,
            "OPERATIONAL_ADJUSTMENT_REVERSED",
            "operational_adjustment_reversed",
            "operational_adjustment",
            batch_id,
            details(inserted.rows_affected(), Some(version), reason),
        )
        .await?;
        let result = CommandResult {
            id: batch_id,
            number: batch.get("adjustment_number"),
            status: "reversed".into(),
            version,
            trace_id,
            idempotent_replay: false,
        };
        finish_idempotent(tx, actor, "profit_adjustment:reverse", key, &result).await?;
        Ok(result)
    }
}
fn details(count: u64, version: Option<i64>, reason: Option<&str>) -> Value {
    let mut v = json!({"factCount":count});
    if let Some(version) = version {
        v["version"] = json!(version);
    }
    if let Some(reason) = reason {
        v["reason"] = json!(reason);
    }
    v
}
