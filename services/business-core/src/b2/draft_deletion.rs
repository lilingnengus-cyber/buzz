use super::common::{begin_idempotent, finish_idempotent, record, request_hash, DomainError};
use super::model::CommandResult;
use crate::store::PgStore;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

pub(crate) enum OrderKind {
    Sales,
    Purchase,
}

pub(crate) async fn delete_draft(
    store: &PgStore,
    kind: OrderKind,
    actor: Uuid,
    trace_id: Uuid,
    id: Uuid,
    key: &str,
    expected_version: i64,
) -> Result<CommandResult, DomainError> {
    let (order_type, permission, select, update, event_insert) = match kind {
        OrderKind::Sales => (
            "sales_order", "sales_order:cancel",
            "SELECT order_number AS number,legal_entity_id,customer_id AS party_id,business_unit_id,brand_id,lifecycle_status,version FROM sales_orders WHERE id=$1 FOR UPDATE",
            "UPDATE sales_orders SET lifecycle_status='cancelled',cancelled_at=now(),updated_by_user_id=$2,trace_id=$3 WHERE id=$1 RETURNING version",
            "INSERT INTO sales_order_events(id,sales_order_id,event_type,order_version,payload,actor_user_id,trace_id) VALUES($1,$2,'draft_deleted',$3,$4,$5,$6)",
        ),
        OrderKind::Purchase => (
            "purchase_order", "purchase_order:cancel_remaining",
            "SELECT purchase_order_number AS number,legal_entity_id,supplier_id AS party_id,business_unit_id,brand_id,lifecycle_status,version FROM purchase_orders WHERE id=$1 FOR UPDATE",
            "UPDATE purchase_orders SET lifecycle_status='cancelled',cancelled_at=now(),updated_by_user_id=$2,trace_id=$3 WHERE id=$1 RETURNING version",
            "INSERT INTO purchase_order_events(id,purchase_order_id,event_type,order_version,payload,actor_user_id,trace_id) VALUES($1,$2,'draft_deleted',$3,$4,$5,$6)",
        ),
    };
    let snapshot = store
        .snapshot(actor)
        .await
        .map_err(|_| DomainError::NotFoundOrForbidden)?;
    if !snapshot.permission_keys.contains(permission) {
        return Err(DomainError::NotFoundOrForbidden);
    }
    let mut tx = store.pool().begin().await?;
    let order = sqlx::query(select)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DomainError::NotFoundOrForbidden)?;
    let party: Uuid = order.get("party_id");
    let party_allowed = match kind {
        OrderKind::Sales => snapshot.scopes.customer_ids.contains(&party),
        OrderKind::Purchase => snapshot.scopes.supplier_ids.contains(&party),
    };
    if !party_allowed
        || !snapshot
            .scopes
            .legal_entity_ids
            .contains(&order.get("legal_entity_id"))
        || !snapshot
            .scopes
            .business_unit_ids
            .contains(&order.get("business_unit_id"))
        || order
            .get::<Option<Uuid>, _>("brand_id")
            .is_some_and(|id| !snapshot.scopes.brand_ids.contains(&id))
    {
        return Err(DomainError::NotFoundOrForbidden);
    }
    let operation = format!("{order_type}:delete_draft");
    let hash = request_hash(&json!({"id":id,"expectedVersion":expected_version}))?;
    if let Some(mut replay) =
        begin_idempotent::<CommandResult>(&mut tx, actor, &operation, key, &hash).await?
    {
        replay.idempotent_replay = true;
        tx.commit().await?;
        return Ok(replay);
    }
    if order.get::<i64, _>("version") != expected_version {
        return Err(DomainError::VersionConflict);
    }
    if order.get::<String, _>("lifecycle_status") != "draft" {
        return Err(DomainError::Invalid(
            "only draft orders can be deleted".into(),
        ));
    }
    let version: i64 = sqlx::query_scalar(update)
        .bind(id)
        .bind(actor)
        .bind(trace_id)
        .fetch_one(&mut *tx)
        .await?;
    let number: String = order.get("number");
    let details = json!({"number":number,"version":version});
    sqlx::query(event_insert)
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(version)
        .bind(&details)
        .bind(actor)
        .bind(trace_id)
        .execute(&mut *tx)
        .await?;
    record(
        &mut tx,
        trace_id,
        actor,
        &format!("{}_DRAFT_DELETED", order_type.to_uppercase()),
        &format!("{order_type}_draft_deleted"),
        order_type,
        id,
        details,
    )
    .await?;
    let result = CommandResult {
        id,
        number,
        status: "deleted".into(),
        version,
        trace_id,
        idempotent_replay: false,
    };
    finish_idempotent(&mut tx, actor, &operation, key, &result).await?;
    tx.commit().await?;
    Ok(result)
}
