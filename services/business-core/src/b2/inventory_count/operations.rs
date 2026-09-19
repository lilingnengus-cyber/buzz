//! Locked previews shared by count entry, posting and cancellation.
use super::*;

/// A closed family of commands that operate on one existing inventory count.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    content = "command",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum InventoryCountOperation {
    /// Record every physical count line while retaining the inventory freeze.
    Submit(SubmitInventoryCount),
    /// Post the recorded differences and release the inventory freeze.
    Post(VersionCommand),
    /// Cancel an active count with a reason and release its inventory freeze.
    Cancel(VersionCommand),
}
impl InventoryCountOperation {
    fn permission(&self) -> &'static str {
        match self {
            Self::Submit(_) => "inventory_opening:create",
            Self::Post(_) => "inventory_opening:post",
            Self::Cancel(_) => "inventory_opening:reverse",
        }
    }
    fn version(&self) -> i64 {
        match self {
            Self::Submit(i) => i.expected_version,
            Self::Post(i) | Self::Cancel(i) => i.expected_version,
        }
    }
}

pub(super) struct Effect {
    pub(super) unit: Option<Decimal>,
    pub(super) variance: Decimal,
    pub(super) value: Decimal,
    pub(super) new_value: Decimal,
    pub(super) average: Option<Decimal>,
}
pub(super) fn effect(
    balance: &sqlx::postgres::PgRow,
    actual: Decimal,
    surplus: Option<Decimal>,
) -> Result<Effect, DomainError> {
    let invalid = || DomainError::Invalid("invalid inventory count quantity or valuation".into());
    let protected = balance.get::<Decimal, _>("reserved_quantity")
        + balance.get::<Decimal, _>("quarantined_quantity");
    if actual < Decimal::ZERO || actual < protected || surplus.is_some_and(|v| v < Decimal::ZERO) {
        return Err(invalid());
    }
    let current: Decimal = balance.get("on_hand_quantity");
    let current_value: Decimal = balance.get("inventory_value");
    let unit = balance
        .get::<Option<Decimal>, _>("average_unit_cost")
        .or(surplus);
    let variance = actual.checked_sub(current).ok_or_else(invalid)?;
    let value = if variance == Decimal::ZERO {
        Decimal::ZERO
    } else if actual == Decimal::ZERO {
        -current_value
    } else {
        money(
            unit.ok_or(DomainError::MissingInventoryCost)?
                .checked_mul(variance)
                .ok_or_else(invalid)?,
        )
    };
    let new_value = if actual == Decimal::ZERO {
        Decimal::ZERO
    } else {
        money(current_value.checked_add(value).ok_or_else(invalid)?)
    };
    if new_value < Decimal::ZERO {
        return Err(invalid());
    }
    let average = if variance == Decimal::ZERO {
        balance.get::<Option<Decimal>, _>("average_unit_cost")
    } else if actual == Decimal::ZERO {
        None
    } else {
        Some(money(new_value.checked_div(actual).ok_or_else(invalid)?))
    };
    Ok(Effect {
        unit,
        variance,
        value,
        new_value,
        average,
    })
}

pub(super) async fn plan(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: Uuid,
    id: Uuid,
    op: &InventoryCountOperation,
) -> Result<Value, DomainError> {
    if let InventoryCountOperation::Cancel(input) = op {
        if input
            .reason_code
            .as_ref()
            .is_none_or(|s| s.trim().is_empty() || s.len() > 1000)
        {
            return Err(DomainError::Invalid(
                "cancellation reason is required (maximum 1000 bytes)".into(),
            ));
        }
    }
    let task=sqlx::query("SELECT count_number,legal_entity_id,warehouse_id,count_date,currency::text,status,version,scope_snapshot_captured,snapshot_business_unit_id FROM inventory_count_tasks WHERE id=$1 FOR UPDATE").bind(id).fetch_optional(&mut **tx).await?.ok_or(DomainError::NotFoundOrForbidden)?;
    if task.get::<i64, _>("version") != op.version() {
        return Err(DomainError::VersionConflict);
    }
    let status: String = task.get("status");
    let allowed = match op {
        InventoryCountOperation::Submit(_) => status == "counting",
        InventoryCountOperation::Post(_) => status == "counted",
        InventoryCountOperation::Cancel(_) => matches!(status.as_str(), "counting" | "counted"),
    };
    if !allowed {
        return Err(DomainError::Invalid(
            "inventory count is not in the required state".into(),
        ));
    }
    let warehouse: Uuid = task.get("warehouse_id");
    let legal: Uuid = task.get("legal_entity_id");
    let unit: Uuid = sqlx::query_scalar(
        "SELECT business_unit_id FROM business_warehouses WHERE id=$1 FOR SHARE",
    )
    .bind(warehouse)
    .fetch_one(&mut **tx)
    .await?;
    let rows=sqlx::query("SELECT l.*,s.product_id,p.brand_id current_brand_id FROM inventory_count_lines l JOIN business_skus s ON s.id=l.sku_id JOIN business_products p ON p.id=s.product_id WHERE l.inventory_count_id=$1 ORDER BY l.sku_id,l.id FOR UPDATE OF l FOR SHARE OF s,p").bind(id).fetch_all(&mut **tx).await?;
    if rows.is_empty() {
        return Err(DomainError::NotFoundOrForbidden);
    }
    let entered = if let InventoryCountOperation::Submit(input) = op {
        let values = input
            .lines
            .iter()
            .map(|line| (line.count_line_id, line))
            .collect::<BTreeMap<_, _>>();
        if values.len() != input.lines.len()
            || values.len() != rows.len()
            || rows
                .iter()
                .any(|r| !values.contains_key(&r.get::<Uuid, _>("id")))
        {
            return Err(DomainError::Invalid(
                "every count line must be entered exactly once".into(),
            ));
        }
        values
    } else {
        BTreeMap::new()
    };
    let mut lines = Vec::new();
    for line in &rows {
        let sku: Uuid = line.get("sku_id");
        let line_id: Uuid = line.get("id");
        let balance=sqlx::query("SELECT on_hand_quantity,reserved_quantity,quarantined_quantity,inventory_value,average_unit_cost,last_movement_id,version FROM inventory_balances WHERE legal_entity_id=$1 AND warehouse_id=$2 AND sku_id=$3 FOR UPDATE").bind(legal).bind(warehouse).bind(sku).fetch_one(&mut **tx).await?;
        if !matches!(op, InventoryCountOperation::Cancel(_)) {
            ensure_snapshot(&balance, line)?;
        }
        let (actual, surplus) = match op {
            InventoryCountOperation::Submit(_) => {
                let input = entered
                    .get(&line_id)
                    .ok_or(DomainError::NotFoundOrForbidden)?;
                (
                    Some(input.actual_on_hand_quantity.0),
                    input.surplus_unit_cost.map(|v| v.0),
                )
            }
            InventoryCountOperation::Post(_) => (
                line.get::<Option<Decimal>, _>("actual_on_hand_quantity"),
                line.get::<Option<Decimal>, _>("surplus_unit_cost"),
            ),
            InventoryCountOperation::Cancel(_) => (None, None),
        };
        let impact = if matches!(op, InventoryCountOperation::Cancel(_)) {
            Value::Null
        } else {
            let actual =
                actual.ok_or_else(|| DomainError::Invalid("missing physical count".into()))?;
            let effect = effect(&balance, actual, surplus)?;
            json!({"actualOnHandQuantity":actual.to_string(),"surplusUnitCost":surplus.map(|v|v.to_string()),"varianceQuantity":effect.variance.to_string(),"varianceValue":effect.value.to_string(),"resultingInventoryValue":effect.new_value.to_string(),"resultingAverageUnitCost":effect.average.map(|v|v.to_string()),"appliedUnitCost":effect.unit.map(|v|v.to_string())})
        };
        lines.push(json!({"id":line_id,"skuId":sku,"brandId":line.get::<Option<Uuid>,_>("current_brand_id"),"snapshotBrandId":line.get::<Option<Uuid>,_>("snapshot_brand_id"),"onHandQuantity":balance.get::<Decimal,_>("on_hand_quantity").to_string(),"reservedQuantity":balance.get::<Decimal,_>("reserved_quantity").to_string(),"quarantinedQuantity":balance.get::<Decimal,_>("quarantined_quantity").to_string(),"inventoryValue":balance.get::<Decimal,_>("inventory_value").to_string(),"averageUnitCost":balance.get::<Option<Decimal>,_>("average_unit_cost").map(|v|v.to_string()),"lastMovementId":balance.get::<Option<Uuid>,_>("last_movement_id"),"balanceVersion":balance.get::<i64,_>("version"),"recordedActualQuantity":line.get::<Option<Decimal>,_>("actual_on_hand_quantity").map(|v|v.to_string()),"recordedSurplusUnitCost":line.get::<Option<Decimal>,_>("surplus_unit_cost").map(|v|v.to_string()),"impact":impact}));
    }
    let authority = PgStore::snapshot_on(&mut *tx, actor)
        .await
        .map_err(|_| DomainError::NotFoundOrForbidden)?;
    if !authority.permission_keys.contains(op.permission())
        || !authority.scopes.legal_entity_ids.contains(&legal)
        || !authority.scopes.warehouse_ids.contains(&warehouse)
        || !authority.scopes.business_unit_ids.contains(&unit)
        || task
            .get::<Option<Uuid>, _>("snapshot_business_unit_id")
            .is_some_and(|id| !authority.scopes.business_unit_ids.contains(&id))
        || rows.iter().any(|r| {
            ["snapshot_brand_id", "current_brand_id"].iter().any(|key| {
                r.get::<Option<Uuid>, _>(*key)
                    .is_some_and(|brand| !authority.scopes.brand_ids.contains(&brand))
            })
        })
    {
        return Err(DomainError::NotFoundOrForbidden);
    }
    Ok(
        json!({"source":{"id":id,"number":task.get::<String,_>("count_number"),"version":op.version(),"status":status,"legalEntityId":legal,"warehouseId":warehouse,"businessUnitId":unit,"snapshotBusinessUnitId":task.get::<Option<Uuid>,_>("snapshot_business_unit_id"),"scopeSnapshotCaptured":task.get::<bool,_>("scope_snapshot_captured"),"currency":task.get::<String,_>("currency"),"countDate":task.get::<NaiveDate,_>("count_date")},"operation":op,"lines":lines,"retainsFreeze":matches!(op,InventoryCountOperation::Submit(_)),"postsInventoryDifferences":matches!(op,InventoryCountOperation::Post(_))}),
    )
}

impl InventoryCountService {
    /// Preview an existing count command without saving changes or releasing its freeze.
    pub async fn operation_preview(
        &self,
        actor: Uuid,
        id: Uuid,
        operation: &InventoryCountOperation,
    ) -> Result<Value, DomainError> {
        self.pre_authorize(actor, id, operation.permission())
            .await?;
        let mut tx = self.store.pool().begin().await?;
        let snapshot = plan(&mut tx, actor, id, operation).await?;
        tx.rollback().await?;
        Ok(snapshot)
    }
    /// Execute only the exact operation and locked state represented by the approved snapshot.
    pub async fn execute_guarded(
        &self,
        context: (Uuid, Uuid),
        id: Uuid,
        key: &str,
        operation: &InventoryCountOperation,
        approved: &Value,
    ) -> Result<CommandResult, DomainError> {
        match operation {
            InventoryCountOperation::Submit(input) => {
                self.submit_inner(context, id, key, input, Some(approved), None)
                    .await
            }
            InventoryCountOperation::Post(input) => {
                self.post_inner(context, id, key, input, Some(approved), None)
                    .await
            }
            InventoryCountOperation::Cancel(input) => {
                self.cancel_inner(context, id, key, input, Some(approved), None)
                    .await
            }
        }
    }
}

pub(super) async fn finish_approval(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request: Uuid,
    id: Uuid,
    kind: &str,
    snapshot: &Value,
) -> Result<(), DomainError> {
    let updated=sqlx::query("UPDATE business_document_approval_requests SET status='executed',executed_at=now(),version=version+1 WHERE id=$1 AND document_type=$2 AND status='executing' AND preview_hash=$3 AND EXISTS(SELECT 1 FROM business_agent_inventory_count_operation_intents i WHERE i.id=business_document_approval_requests.document_id AND i.inventory_count_id=$4 AND i.kind=$2 AND i.expires_at>clock_timestamp())")
        .bind(request).bind(kind).bind(request_hash(snapshot)?).bind(id).execute(&mut **tx).await?.rows_affected();
    if updated != 1 {
        return Err(DomainError::StalePreview);
    }
    Ok(())
}
impl InventoryCountService {
    /// Commit an approved count operation and its executed approval outcome atomically.
    pub(crate) async fn execute_approved(
        &self,
        context: (Uuid, Uuid),
        id: Uuid,
        operation: &InventoryCountOperation,
        approved: &Value,
        request: Uuid,
    ) -> Result<CommandResult, DomainError> {
        let key = format!("agent-count-operation:{request}");
        match operation {
            InventoryCountOperation::Submit(input) => {
                self.submit_inner(context, id, &key, input, Some(approved), Some(request))
                    .await
            }
            InventoryCountOperation::Post(input) => {
                self.post_inner(context, id, &key, input, Some(approved), Some(request))
                    .await
            }
            InventoryCountOperation::Cancel(input) => {
                self.cancel_inner(context, id, &key, input, Some(approved), Some(request))
                    .await
            }
        }
    }
}
