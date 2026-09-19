use super::*;
use sqlx::AssertSqlSafe;

/// Full replacement of a draft's editable fields, bound to both current versions.
/// The original shipment or receipt is immutable; use a new draft to change it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceReturnDraft {
    /// Version read from the selected return.
    pub expected_version: i64,
    /// Version read from the original shipment or receipt.
    pub expected_source_version: i64,
    /// Business date supplied by the user.
    pub return_date: NaiveDate,
    /// Reason supplied by the user.
    pub reason_code: String,
    /// Optional replacement note; omission clears the previous note.
    #[serde(default)]
    pub business_note: Option<String>,
    /// Complete desired set of original fulfillment lines and return quantities.
    pub lines: Vec<ReturnLineInput>,
}

impl ReturnService {
    /// Replace editable draft fields while serializing quantity allocation with creation.
    /// `sales` selects sales returns; false selects purchase returns.
    pub async fn replace_draft(
        &self,
        actor: Uuid,
        trace: Uuid,
        sales: bool,
        id: Uuid,
        key: &str,
        input: &ReplaceReturnDraft,
    ) -> Result<CommandResult, DomainError> {
        if input.expected_version < 1 || input.expected_source_version < 1 {
            return Err(DomainError::Invalid(
                "positive return and source versions are required".into(),
            ));
        }
        let (
            side,
            table,
            source_table,
            source_fk,
            line_table,
            return_fk,
            source_lines,
            source_line_fk,
            quantity,
            permission,
            operation,
            event,
        ) = if sales {
            (
                "sales",
                "sales_returns",
                "shipments",
                "shipment_id",
                "sales_return_lines",
                "sales_return_id",
                "shipment_lines",
                "shipment_line_id",
                "quantity",
                "shipment:reverse",
                "sales_return:update_draft",
                "SALES_RETURN_DRAFT_UPDATED",
            )
        } else {
            (
                "purchase",
                "purchase_returns",
                "goods_receipts",
                "goods_receipt_id",
                "purchase_return_lines",
                "purchase_return_id",
                "goods_receipt_lines",
                "goods_receipt_line_id",
                "received_quantity",
                "goods_receipt:reverse",
                "purchase_return:update_draft",
                "PURCHASE_RETURN_DRAFT_UPDATED",
            )
        };
        let party = if sales { "customer_id" } else { "supplier_id" };
        let pre = sqlx::query(AssertSqlSafe(format!("SELECT legal_entity_id,warehouse_id,{party} party_id,{source_fk} source_id FROM {table} WHERE id=$1")))
            .bind(id).fetch_optional(self.store.pool()).await?.ok_or(DomainError::NotFoundOrForbidden)?;
        let authority = authorize(
            &self.store,
            actor,
            permission,
            Some(pre.get("legal_entity_id")),
            Some(pre.get("warehouse_id")),
            None,
            None,
            None,
        )
        .await?;
        let partner: Uuid = pre.get("party_id");
        if !(if sales {
            &authority.scopes.customer_ids
        } else {
            &authority.scopes.supplier_ids
        })
        .contains(&partner)
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
        let source: Uuid = pre.get("source_id");
        super::super::return_scope::check_source(&self.store, actor, sales, source).await?;
        Self::validate_input(&CreateReturn {
            source_id: source,
            expected_source_version: Some(input.expected_source_version),
            return_date: input.return_date,
            reason_code: input.reason_code.clone(),
            business_note: input.business_note.clone(),
            lines: input.lines.clone(),
        })?;
        // Bind the target as well as the replacement to prevent cross-document key replay.
        let hash = request_hash(&(id, input))?;
        let mut tx = self.store.pool().begin().await?;
        if let Some(mut replay) =
            begin_idempotent::<CommandResult>(&mut tx, actor, operation, key, &hash).await?
        {
            replay.idempotent_replay = true;
            tx.commit().await?;
            return Ok(replay);
        }
        // Match confirmation's return -> source lock order. Creation only locks the source.
        let ret = sqlx::query(AssertSqlSafe(format!("SELECT return_number,status,version,{source_fk} source_id FROM {table} WHERE id=$1 FOR UPDATE")))
            .bind(id).fetch_one(&mut *tx).await?;
        check_draft(&ret, input.expected_version)?;
        if ret.get::<Uuid, _>("source_id") != source {
            return Err(DomainError::VersionConflict);
        }
        let original = sqlx::query(AssertSqlSafe(format!(
            "SELECT status,version FROM {source_table} WHERE id=$1 FOR UPDATE"
        )))
        .bind(source)
        .fetch_one(&mut *tx)
        .await?;
        if original.get::<i64, _>("version") != input.expected_source_version {
            return Err(DomainError::VersionConflict);
        }
        if original.get::<String, _>("status") != "confirmed" {
            return Err(DomainError::Invalid(
                "return requires a confirmed source".into(),
            ));
        }
        let mut desired = Vec::with_capacity(input.lines.len());
        for line in &input.lines {
            let original = sqlx::query(AssertSqlSafe(format!("SELECT sku_id,{quantity} quantity FROM {source_lines} WHERE id=$1 AND {source_fk}=$2")))
                .bind(line.source_line_id).bind(source).fetch_optional(&mut *tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
            let allocated: Decimal = sqlx::query_scalar(AssertSqlSafe(format!("SELECT COALESCE(sum(l.quantity),0) FROM {line_table} l JOIN {table} r ON r.id=l.{return_fk} WHERE l.{source_line_fk}=$1 AND r.id<>$2 AND r.status IN ('draft','confirmed')")))
                .bind(line.source_line_id).bind(id).fetch_one(&mut *tx).await?;
            if line.quantity.0 > original.get::<Decimal, _>("quantity") - allocated {
                return Err(DomainError::Invalid(
                    "return quantity exceeds source remainder".into(),
                ));
            }
            desired.push((line, original.get::<Uuid, _>("sku_id")));
        }
        sqlx::query(AssertSqlSafe(format!(
            "DELETE FROM {line_table} WHERE {return_fk}=$1"
        )))
        .bind(id)
        .execute(&mut *tx)
        .await?;
        for (line, sku) in desired {
            sqlx::query(AssertSqlSafe(format!("INSERT INTO {line_table}(id,{return_fk},{source_line_fk},sku_id,quantity) VALUES($1,$2,$3,$4,$5)")))
                .bind(Uuid::new_v4()).bind(id).bind(line.source_line_id).bind(sku).bind(line.quantity.0).execute(&mut *tx).await?;
        }
        let version = input.expected_version + 1;
        sqlx::query(AssertSqlSafe(format!("UPDATE {table} SET return_date=$2,reason_code=$3,business_note=$4,version=$5,updated_at=now(),trace_id=$6 WHERE id=$1")))
            .bind(id).bind(input.return_date).bind(&input.reason_code).bind(&input.business_note).bind(version).bind(trace).execute(&mut *tx).await?;
        return_event(
            &mut tx,
            side,
            id,
            "draft_updated",
            version,
            (actor, trace),
            json!({"lineCount":input.lines.len(),"sourceId":source}),
        )
        .await?;
        record(
            &mut tx,
            trace,
            actor,
            event,
            &format!("{side}_return_draft_updated"),
            &format!("{side}_return"),
            id,
            json!({"version":version}),
        )
        .await?;
        let result = CommandResult {
            id,
            number: ret.get("return_number"),
            status: "draft".into(),
            version,
            trace_id: trace,
            idempotent_replay: false,
        };
        finish_idempotent(&mut tx, actor, operation, key, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
}
