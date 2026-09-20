//! Scoped, version-bound manual review holds for sales orders.
use super::*;
use serde_json::Value;

impl SalesService {
    /// Place or release a manual review hold through the existing workbench contract.
    pub async fn set_hold(
        &self,
        actor: Uuid,
        trace_id: Uuid,
        order_id: Uuid,
        key: &str,
        input: &VersionCommand,
        place: bool,
    ) -> Result<CommandResult, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        let result = self
            .set_hold_on(
                &mut tx,
                (actor, trace_id),
                (order_id, place),
                key,
                input,
                None,
            )
            .await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Preview a hold transition, including its immutable reason and current scope.
    pub async fn hold_preview(
        &self,
        actor: Uuid,
        order_id: Uuid,
        input: &VersionCommand,
        place: bool,
    ) -> Result<Value, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        let result = self
            .hold_preview_on(&mut tx, actor, order_id, input, place)
            .await?;
        tx.rollback().await?;
        Ok(result)
    }

    pub(crate) async fn hold_preview_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        order_id: Uuid,
        input: &VersionCommand,
        place: bool,
    ) -> Result<Value, DomainError> {
        let row = self.hold_row(tx, actor, order_id, place).await?;
        hold_snapshot(tx, &row, order_id, input, place).await
    }

    /// Execute only the exact reviewed hold transition; ordinary workbench hashes stay compatible.
    pub async fn set_hold_guarded(
        &self,
        context: (Uuid, Uuid),
        order_id: Uuid,
        key: &str,
        input: &VersionCommand,
        place: bool,
        expected: &Value,
    ) -> Result<CommandResult, DomainError> {
        let mut tx = self.store.pool().begin().await?;
        let result = self
            .set_hold_on(
                &mut tx,
                context,
                (order_id, place),
                key,
                input,
                Some(expected),
            )
            .await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn hold_row(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: Uuid,
        order_id: Uuid,
        place: bool,
    ) -> Result<sqlx::postgres::PgRow, DomainError> {
        let permission = if place {
            "sales_order:place_hold"
        } else {
            "sales_order:release_hold"
        };
        crate::master_write_authority::read(tx, actor, permission).await?;
        let row = sqlx::query("SELECT order_number,legal_entity_id,customer_id,business_unit_id,brand_id,lifecycle_status,hold_status,version FROM sales_orders WHERE id=$1 FOR UPDATE")
            .bind(order_id).fetch_optional(&mut **tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
        let current = crate::master_write_authority::snapshot(tx, actor, permission, false).await?;
        if !current
            .scopes
            .legal_entity_ids
            .contains(&row.get("legal_entity_id"))
            || !current
                .scopes
                .customer_ids
                .contains(&row.get("customer_id"))
            || !current
                .scopes
                .business_unit_ids
                .contains(&row.get("business_unit_id"))
        {
            return Err(DomainError::NotFoundOrForbidden);
        }
        Ok(row)
    }

    pub(crate) async fn set_hold_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        context: (Uuid, Uuid),
        target: (Uuid, bool),
        key: &str,
        input: &VersionCommand,
        guard: Option<&Value>,
    ) -> Result<CommandResult, DomainError> {
        let (actor, trace_id) = context;
        let (order_id, place) = target;
        if input
            .reason_code
            .as_deref()
            .is_none_or(|value| value.trim().is_empty() || value.len() > 64)
        {
            return Err(DomainError::Invalid("reasonCode is required".into()));
        }
        let operation = if place {
            "sales_order:place_hold"
        } else {
            "sales_order:release_hold"
        };
        let hash = match guard {
            Some(expected) => {
                request_hash(&("guarded-sales-hold-v1", order_id, place, input, expected))?
            }
            None => request_hash(input)?,
        };
        if let Some(mut replay) =
            begin_idempotent::<CommandResult>(tx, actor, operation, key, &hash).await?
        {
            if replay.id != order_id {
                return Err(DomainError::IdempotencyConflict);
            }
            self.hold_row(tx, actor, order_id, place).await?;
            replay.idempotent_replay = true;
            return Ok(replay);
        }
        let row = self.hold_row(tx, actor, order_id, place).await?;
        if let Some(expected) = guard {
            if hold_snapshot(tx, &row, order_id, input, place).await? != *expected {
                return Err(DomainError::StalePreview);
            }
        }
        if row.get::<i64, _>("version") != input.expected_version {
            return Err(DomainError::VersionConflict);
        }
        if row.get::<String, _>("lifecycle_status") != "confirmed" {
            return Err(DomainError::Invalid(
                "hold applies only to confirmed orders".into(),
            ));
        }
        let expected = if place { "none" } else { "manual_review_hold" };
        if row.get::<String, _>("hold_status") != expected {
            return Err(DomainError::Invalid(
                "hold transition is not allowed".into(),
            ));
        }
        let status = if place { "manual_review_hold" } else { "none" };
        sqlx::query(
            "UPDATE sales_orders SET hold_status=$2,updated_by_user_id=$3,trace_id=$4 WHERE id=$1",
        )
        .bind(order_id)
        .bind(status)
        .bind(actor)
        .bind(trace_id)
        .execute(&mut **tx)
        .await?;
        let version = input.expected_version + 1;
        let event = if place {
            "manual_review_hold_placed"
        } else {
            "manual_review_hold_released"
        };
        let audit = if place {
            "SALES_ORDER_HOLD_PLACED"
        } else {
            "SALES_ORDER_HOLD_RELEASED"
        };
        sqlx::query("INSERT INTO sales_order_events(id,sales_order_id,event_type,order_version,payload,actor_user_id,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(Uuid::new_v4()).bind(order_id).bind(event).bind(version).bind(json!({"reasonCode":input.reason_code})).bind(actor).bind(trace_id).execute(&mut **tx).await?;
        record(
            tx,
            trace_id,
            actor,
            audit,
            event,
            "sales_order",
            order_id,
            json!({"reasonCode":input.reason_code,"version":version}),
        )
        .await?;
        let result = CommandResult {
            id: order_id,
            number: row.get("order_number"),
            status: status.into(),
            version,
            trace_id,
            idempotent_replay: false,
        };
        finish_idempotent(tx, actor, operation, key, &result).await?;
        Ok(result)
    }
}

async fn hold_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    row: &sqlx::postgres::PgRow,
    id: Uuid,
    input: &VersionCommand,
    place: bool,
) -> Result<Value, DomainError> {
    let lifecycle: String = row.get("lifecycle_status");
    let hold: String = row.get("hold_status");
    let version: i64 = row.get("version");
    let expected = if place { "none" } else { "manual_review_hold" };
    let reason_valid = input
        .reason_code
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty() && s.len() <= 64);
    let lines: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'skuId',sku_id,'warehouseId',warehouse_id,'businessUnitId',business_unit_id,'brandId',brand_id) FROM sales_order_lines WHERE sales_order_id=$1 ORDER BY line_number,id").bind(id).fetch_all(&mut **tx).await?;
    Ok(json!({
        "lines":lines,
        "source": {"id":id,"orderNumber":row.get::<String,_>("order_number"),
            "legalEntityId":row.get::<Uuid,_>("legal_entity_id"),"customerId":row.get::<Uuid,_>("customer_id"),
            "businessUnitId":row.get::<Uuid,_>("business_unit_id"),"brandId":row.get::<Option<Uuid>,_>("brand_id"),"version":version,
            "lifecycleStatus":lifecycle,"holdStatus":hold},
        "operation":if place {"place_hold"} else {"release_hold"},
        "expectedVersion":input.expected_version,"reasonCode":input.reason_code,
        "targetHoldStatus":if place {"manual_review_hold"} else {"none"},
        "blocksShipmentCreationAndConfirmation":place,
        "changesInventoryReservation":false,
        "canExecute":lifecycle=="confirmed" && hold==expected && version==input.expected_version && reason_valid
    }))
}
