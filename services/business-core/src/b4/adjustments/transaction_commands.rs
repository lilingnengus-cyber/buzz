//! Existing browser commands, reusable inside an atomic confirmation transaction.
use super::*;
impl AdjustmentService {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn persist_preview_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        trace_id: Uuid,
        batch_id: Uuid,
        key: &str,
        input: &VersionCommand,
        authorization: &AuthorizationSnapshot,
    ) -> Result<PreviewResult, DomainError> {
        let hash = request_hash(input)?;
        if let Some(mut replay) =
            begin_idempotent::<PreviewResult>(tx, actor, "profit_adjustment:preview", key, &hash)
                .await?
        {
            replay.idempotent_replay = true;
            return Ok(replay);
        }
        let batch=sqlx::query("SELECT adjustment_number,legal_entity_id,currency::text,management_period,status,version FROM operational_adjustment_batches WHERE id=$1 FOR UPDATE").bind(batch_id).fetch_one(&mut **tx).await?;
        if batch.get::<i64, _>("version") != input.expected_version {
            return Err(DomainError::VersionConflict);
        }
        if !matches!(
            batch.get::<String, _>("status").as_str(),
            "draft" | "previewed"
        ) {
            return Err(DomainError::Invalid("adjustment is not previewable".into()));
        }
        let calculation =
            allocation_preview::calculate(tx, &batch, batch_id, authorization, self.max_targets)
                .await?;
        let watermark = calculation.watermark;
        let payload_lines = calculation.lines;
        let total = calculation.total;
        let allocated = calculation.allocated;
        let source_hash = hex::encode(Sha256::digest(serde_json::to_vec(
            &json!({"watermark":watermark,"lines":payload_lines}),
        )?));
        let next_version = input.expected_version + 1;
        let preview_hash = hex::encode(Sha256::digest(serde_json::to_vec(
            &json!({"batchId":batch_id,"batchVersion":next_version,"sourceHash":source_hash,"allocations":payload_lines}),
        )?));
        let preview_id = Uuid::new_v4();
        let payload =
            json!({"lines":payload_lines,"boundary":"management_only_not_general_ledger"});
        sqlx::query("INSERT INTO operational_adjustment_previews(id,batch_id,preview_hash,source_hash,source_watermark,batch_version,total_amount,allocated_amount,unallocated_amount,payload,created_by_user_id,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
            .bind(preview_id).bind(batch_id).bind(&preview_hash).bind(&source_hash).bind(watermark).bind(next_version).bind(money(total)).bind(money(allocated)).bind(money(total-allocated)).bind(&payload).bind(actor).bind(trace_id).execute(&mut **tx).await?;
        sqlx::query("UPDATE operational_adjustment_batches SET status='previewed',previewed_at=now(),updated_by_user_id=$2,trace_id=$3 WHERE id=$1").bind(batch_id).bind(actor).bind(trace_id).execute(&mut **tx).await?;
        event(
            tx,
            batch_id,
            "previewed",
            next_version,
            actor,
            trace_id,
            json!({"previewId":preview_id,"previewHash":preview_hash,"sourceWatermark":watermark}),
        )
        .await?;
        record(tx,trace_id,actor,"PROFIT_ALLOCATION_PREVIEWED","profit_allocation_previewed","operational_adjustment",batch_id,json!({"previewId":preview_id,"targetCount":payload["lines"].as_array().map_or(0,Vec::len),"sourceWatermark":watermark})).await?;
        let result = PreviewResult {
            preview_id,
            batch_id,
            preview_hash,
            source_hash,
            source_watermark: watermark,
            batch_version: next_version,
            total_amount: DecimalString(money(total)),
            allocated_amount: DecimalString(money(allocated)),
            unallocated_amount: DecimalString(money(total - allocated)),
            allocations: payload,
            data_as_of: Utc::now(),
            trace_id,
            idempotent_replay: false,
        };
        finish_idempotent(tx, actor, "profit_adjustment:preview", key, &result).await?;
        Ok(result)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn persist_post_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        trace_id: Uuid,
        batch_id: Uuid,
        key: &str,
        input: &PostAdjustment,
        authorization: &AuthorizationSnapshot,
    ) -> Result<CommandResult, DomainError> {
        let hash = request_hash(input)?;
        if let Some(mut replay) =
            begin_idempotent::<CommandResult>(tx, actor, "profit_adjustment:post", key, &hash)
                .await?
        {
            replay.idempotent_replay = true;
            return Ok(replay);
        }
        let batch=sqlx::query("SELECT adjustment_number,legal_entity_id,currency::text,management_period,status,version FROM operational_adjustment_batches WHERE id=$1 FOR UPDATE").bind(batch_id).fetch_one(&mut **tx).await?;
        if batch.get::<i64, _>("version") != input.expected_version {
            return Err(DomainError::VersionConflict);
        }
        if batch.get::<String, _>("status") != "previewed" {
            return Err(DomainError::Invalid(
                "only previewed adjustments can be posted".into(),
            ));
        }
        let preview=sqlx::query("SELECT preview_hash,source_watermark,batch_version,payload,unallocated_amount FROM operational_adjustment_previews WHERE id=$1 AND batch_id=$2").bind(input.preview_id).bind(batch_id).fetch_optional(&mut **tx).await?.ok_or(DomainError::StalePreview)?;
        let watermark: i64 =
            sqlx::query_scalar("SELECT COALESCE(max(fact_sequence),0) FROM profit_facts")
                .fetch_one(&mut **tx)
                .await?;
        if preview.get::<String, _>("preview_hash") != input.preview_hash
            || preview.get::<i64, _>("batch_version") != input.expected_version
            || preview.get::<i64, _>("source_watermark") != watermark
            || preview.get::<Decimal, _>("unallocated_amount") != Decimal::ZERO
        {
            return Err(DomainError::StalePreview);
        }
        let post_event = Uuid::new_v4();
        let payload: Value = preview.get("payload");
        let mut fact_count = 0_i64;
        for line in payload["lines"]
            .as_array()
            .ok_or_else(|| DomainError::Invalid("preview payload is invalid".into()))?
        {
            let line_id = Uuid::parse_str(line["lineId"].as_str().unwrap_or_default())
                .map_err(|_| DomainError::StalePreview)?;
            let metric = line["metricType"]
                .as_str()
                .ok_or(DomainError::StalePreview)?;
            let business_date = chrono::NaiveDate::parse_from_str(
                line["businessDate"].as_str().unwrap_or_default(),
                "%Y-%m-%d",
            )
            .map_err(|_| DomainError::StalePreview)?;
            for target in line["targets"]
                .as_array()
                .ok_or(DomainError::StalePreview)?
            {
                let order_id = Uuid::parse_str(target["salesOrderId"].as_str().unwrap_or_default())
                    .map_err(|_| DomainError::StalePreview)?;
                let amount = target["amount"]
                    .as_str()
                    .unwrap_or_default()
                    .parse::<Decimal>()
                    .map_err(|_| DomainError::StalePreview)?;
                let weight = target["weight"]
                    .as_str()
                    .unwrap_or_default()
                    .parse::<Decimal>()
                    .map_err(|_| DomainError::StalePreview)?;
                let rank = target["remainderRank"]
                    .as_i64()
                    .ok_or(DomainError::StalePreview)? as i32;
                let dims = order_dimensions(tx, order_id).await?;
                ensure_order_scope(tx, order_id, authorization).await?;
                if dims.get::<Uuid, _>("legal_entity_id") != batch.get::<Uuid, _>("legal_entity_id")
                    || dims.get::<String, _>("currency") != batch.get::<String, _>("currency")
                {
                    return Err(DomainError::NotFoundOrForbidden);
                }
                let allocation_id = Uuid::new_v4();
                let fact_id = Uuid::new_v4();
                sqlx::query("INSERT INTO profit_facts(id,metric_type,direction,amount,currency,legal_entity_id,sales_order_id,customer_id,brand_id,salesperson_user_id,business_unit_id,department_id,business_date,management_period,source_system,source_type,source_id,source_line_id,source_event_id,source_event_version,data_as_of,trace_id) VALUES($1,$2,'normal',$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,'business_core_b4','operational_adjustment',$14,$15,$16,$17,now(),$18)")
                    .bind(fact_id).bind(metric).bind(amount).bind(batch.get::<String,_>("currency")).bind(batch.get::<Uuid,_>("legal_entity_id")).bind(order_id).bind(dims.get::<Uuid,_>("customer_id")).bind(dims.get::<Option<Uuid>,_>("brand_id")).bind(dims.get::<Uuid,_>("salesperson_user_id")).bind(dims.get::<Uuid,_>("business_unit_id")).bind(dims.get::<Option<Uuid>,_>("department_id")).bind(business_date).bind(batch.get::<String,_>("management_period")).bind(batch_id).bind(allocation_id).bind(post_event).bind(input.expected_version+1).bind(trace_id).execute(&mut **tx).await?;
                sqlx::query("INSERT INTO operational_adjustment_allocations(id,batch_id,adjustment_line_id,preview_id,sales_order_id,weight,allocated_amount,remainder_rank,profit_fact_id,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
                    .bind(allocation_id).bind(batch_id).bind(line_id).bind(input.preview_id).bind(order_id).bind(weight).bind(amount).bind(rank).bind(fact_id).bind(trace_id).execute(&mut **tx).await?;
                fact_count += 1;
            }
        }
        sqlx::query("UPDATE operational_adjustment_batches SET status='posted',posted_at=now(),updated_by_user_id=$2,trace_id=$3 WHERE id=$1").bind(batch_id).bind(actor).bind(trace_id).execute(&mut **tx).await?;
        let version = input.expected_version + 1;
        event_with_id(
            tx,
            post_event,
            batch_id,
            "posted",
            version,
            actor,
            trace_id,
            json!({"previewId":input.preview_id,"factCount":fact_count}),
        )
        .await?;
        record(
            tx,
            trace_id,
            actor,
            "OPERATIONAL_ADJUSTMENT_POSTED",
            "operational_adjustment_posted",
            "operational_adjustment",
            batch_id,
            json!({"previewId":input.preview_id,"factCount":fact_count,"version":version}),
        )
        .await?;
        let result = CommandResult {
            id: batch_id,
            number: batch.get("adjustment_number"),
            status: "posted".into(),
            version,
            trace_id,
            idempotent_replay: false,
        };
        finish_idempotent(tx, actor, "profit_adjustment:post", key, &result).await?;
        Ok(result)
    }
}
