//! Draft persistence shared with caller-owned guarded transactions.
use super::*;
impl AdjustmentService {
    pub(super) async fn persist_create_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        trace_id: Uuid,
        key: &str,
        input: &CreateAdjustmentBatch,
    ) -> Result<CommandResult, DomainError> {
        let hash = request_hash(input)?;
        if let Some(mut replay) =
            begin_idempotent::<CommandResult>(tx, actor, "profit_adjustment:create", key, &hash)
                .await?
        {
            replay.idempotent_replay = true;
            return Ok(replay);
        }
        let id = Uuid::new_v4();
        let number = next_number(
            tx,
            "profit_adjustment",
            &self.prefix,
            id,
            crate::numbering::NumberingContext::new(input.legal_entity_id, None),
        )
        .await?;
        sqlx::query("INSERT INTO operational_adjustment_batches(id,adjustment_number,legal_entity_id,currency,management_period,created_by_user_id,updated_by_user_id,trace_id) VALUES($1,$2,$3,$4,$5,$6,$6,$7)")
            .bind(id).bind(&number).bind(input.legal_entity_id).bind(&input.currency).bind(&input.management_period).bind(actor).bind(trace_id).execute(&mut **tx).await?;
        insert_lines(tx, id, input).await?;
        event(
            tx,
            id,
            "created",
            1,
            actor,
            trace_id,
            json!({"lineCount":input.lines.len()}),
        )
        .await?;
        record(tx,trace_id,actor,"OPERATIONAL_ADJUSTMENT_CREATED","operational_adjustment_created","operational_adjustment",id,json!({"adjustmentNumber":number,"lineCount":input.lines.len(),"managementPeriod":input.management_period})).await?;
        let result = CommandResult {
            id,
            number,
            status: "draft".into(),
            version: 1,
            trace_id,
            idempotent_replay: false,
        };
        finish_idempotent(tx, actor, "profit_adjustment:create", key, &result).await?;
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn persist_replace_draft_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        trace_id: Uuid,
        batch_id: Uuid,
        key: &str,
        input: &ReplaceAdjustmentDraft,
    ) -> Result<CommandResult, DomainError> {
        let hash = request_hash(input)?;
        if let Some(mut replay) = begin_idempotent::<CommandResult>(
            tx,
            actor,
            "profit_adjustment:update_draft",
            key,
            &hash,
        )
        .await?
        {
            replay.idempotent_replay = true;
            return Ok(replay);
        }
        let row=sqlx::query("SELECT adjustment_number,status,version FROM operational_adjustment_batches WHERE id=$1 FOR UPDATE").bind(batch_id).fetch_one(&mut **tx).await?;
        if row.get::<i64, _>("version") != input.expected_version {
            return Err(DomainError::VersionConflict);
        }
        if !matches!(
            row.get::<String, _>("status").as_str(),
            "draft" | "previewed"
        ) {
            return Err(DomainError::Invalid(
                "only draft or previewed adjustments can be edited".into(),
            ));
        }
        sqlx::query("DELETE FROM operational_adjustment_lines WHERE batch_id=$1")
            .bind(batch_id)
            .execute(&mut **tx)
            .await?;
        sqlx::query("UPDATE operational_adjustment_batches SET legal_entity_id=$2,currency=$3,management_period=$4,status='draft',previewed_at=NULL,updated_by_user_id=$5,trace_id=$6 WHERE id=$1").bind(batch_id).bind(input.batch.legal_entity_id).bind(&input.batch.currency).bind(&input.batch.management_period).bind(actor).bind(trace_id).execute(&mut **tx).await?;
        insert_lines(tx, batch_id, &input.batch).await?;
        let version = input.expected_version + 1;
        event(
            tx,
            batch_id,
            "updated",
            version,
            actor,
            trace_id,
            json!({"lineCount":input.batch.lines.len()}),
        )
        .await?;
        record(
            tx,
            trace_id,
            actor,
            "OPERATIONAL_ADJUSTMENT_UPDATED",
            "operational_adjustment_updated",
            "operational_adjustment",
            batch_id,
            json!({"version":version,"lineCount":input.batch.lines.len()}),
        )
        .await?;
        let result = CommandResult {
            id: batch_id,
            number: row.get("adjustment_number"),
            status: "draft".into(),
            version,
            trace_id,
            idempotent_replay: false,
        };
        finish_idempotent(tx, actor, "profit_adjustment:update_draft", key, &result).await?;
        Ok(result)
    }
}
