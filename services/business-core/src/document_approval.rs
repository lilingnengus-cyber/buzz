mod adjustment;
/// Immutable, scoped allocation preparation and signed execution.
pub mod allocation;
mod allocation_history;
mod crm;
mod financial_documents;
mod inventory_count_creation;
mod inventory_count_operation;
mod master;
mod operating_snapshot;
/// Bound cancellation of remaining order quantities.
pub mod order_cancellation;
mod order_hold;
mod permission_witness;
mod report_snapshot;
/// Immutable return inspection and logistics intents.
pub mod return_disposition;
mod returns;
/// Immutable reversal preparation and signed execution.
pub mod reversal;
mod settlement;
mod snapshot;
mod snapshot_retry;
pub(crate) mod stock;
mod stock_documents;
/// Immutable stock reversal preparation and signed execution.
pub mod stock_reversal;

use crate::{
    api::AppState,
    b2::model::VersionCommand as B2VersionCommand,
    b3::model::VersionCommand as B3VersionCommand,
    security::RequestContext,
    store::{PgStore, StoreError},
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Reject,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatApprovalInput {
    pub expected_version: i64,
    pub preview_hash: String,
    pub decision: ApprovalDecision,
    pub source_buzz_event_id: String,
    pub source_channel_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatApprovalResult {
    pub request_id: Uuid,
    pub document_type: &'static str,
    pub document_id: Uuid,
    pub decision: ApprovalDecision,
    pub status: String,
    pub approval_count: i64,
    pub minimum_approvers: i16,
    pub executed: bool,
    pub trace_id: Uuid,
}

#[derive(Debug)]
struct VoteOutcome {
    request_id: Uuid,
    approval_count: i64,
    minimum_approvers: i16,
    should_execute: bool,
    status: String,
}

pub fn service_routes() -> Router<Arc<AppState>> {
    Router::new()
        .merge(stock::routes())
        .merge(crm::routes())
        .merge(master::routes())
        .merge(order_hold::routes())
        .merge(report_snapshot::routes())
        .merge(operating_snapshot::routes())
        .merge(adjustment::routes())
        .merge(returns::routes())
        .merge(return_disposition::routes())
        .merge(inventory_count_creation::routes())
        .merge(inventory_count_operation::routes())
        .merge(settlement::routes())
        .merge(allocation::routes())
        .merge(reversal::routes())
        .merge(order_cancellation::routes())
        .merge(stock_reversal::routes())
        .route(
            "/v1/agent-stock-documents/{kind}",
            get(stock_documents::search),
        )
        .route(
            "/v1/agent-allocation-history/{kind}",
            get(allocation_history::search),
        )
        .route(
            "/v1/agent-financial-documents/{kind}",
            get(financial_documents::search),
        )
        .route(
            "/v1/agent-documents/sales-orders/{id}",
            get(snapshot::sales),
        )
        .route(
            "/v1/agent-documents/purchase-orders/{id}",
            get(snapshot::purchase),
        )
        .route(
            "/v1/agent-approvals/sales-orders/{id}",
            post(approve_sales_order),
        )
        .route(
            "/v1/agent-approval-previews/sales-orders/{id}",
            get(sales_order_preview),
        )
        .route(
            "/v1/agent-approvals/purchase-orders/{id}",
            post(approve_purchase_order),
        )
        .route(
            "/v1/agent-approval-previews/purchase-orders/{id}",
            get(purchase_order_preview),
        )
}

async fn sales_order_preview(
    State(state): State<Arc<AppState>>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> Response {
    match state
        .sales
        .confirmation_preview(context.actor_user_id, id)
        .await
    {
        Ok(preview) => {
            let details =
                match snapshot::order(&state, context.actor_user_id, "sales_order", id).await {
                    Ok(v) => v,
                    Err(e) => return store_error(e, context.trace_id),
                };
            let hash = hash_json(&preview);
            Json(json!({
                "item": preview,
                "document": details,
                "previewHash": hash,
                "approvalCommand": format!("确认 sales-order {id} v{} {hash}", preview.version),
                "rejectionCommand": format!("拒绝 sales-order {id} v{} {hash}", preview.version),
                "traceId": context.trace_id,
            }))
            .into_response()
        }
        Err(_) => approval_error(
            StatusCode::NOT_FOUND,
            "not_found_or_forbidden",
            context.trace_id,
        ),
    }
}

async fn purchase_order_preview(
    State(state): State<Arc<AppState>>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> Response {
    match state
        .purchasing
        .confirmation_preview(context.actor_user_id, id)
        .await
    {
        Ok(preview) => {
            let details =
                match snapshot::order(&state, context.actor_user_id, "purchase_order", id).await {
                    Ok(v) => v,
                    Err(e) => return store_error(e, context.trace_id),
                };
            let hash = hash_json(&preview);
            Json(json!({
                "item": preview,
                "document": details,
                "previewHash": hash,
                "approvalCommand": format!("确认 purchase-order {id} v{} {hash}", preview.version),
                "rejectionCommand": format!("拒绝 purchase-order {id} v{} {hash}", preview.version),
                "traceId": context.trace_id,
            }))
            .into_response()
        }
        Err(_) => approval_error(
            StatusCode::NOT_FOUND,
            "not_found_or_forbidden",
            context.trace_id,
        ),
    }
}

async fn approve_sales_order(
    State(state): State<Arc<AppState>>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<ChatApprovalInput>,
) -> Response {
    let preview = match state
        .sales
        .confirmation_preview(context.actor_user_id, id)
        .await
    {
        Ok(value) => value,
        Err(_) => {
            return approval_error(
                StatusCode::NOT_FOUND,
                "not_found_or_forbidden",
                context.trace_id,
            );
        }
    };
    if preview.version != input.expected_version || hash_json(&preview) != input.preview_hash {
        return approval_error(
            StatusCode::CONFLICT,
            "stale_approval_preview",
            context.trace_id,
        );
    }
    let Some(key) = idempotency_key(&headers) else {
        return approval_error(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            context.trace_id,
        );
    };
    let outcome = match cast_vote(
        &state.store,
        "sales_order",
        "sales_order:confirm",
        id,
        context.actor_user_id,
        context.trace_id,
        &input,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => return store_error(error, context.trace_id),
    };
    let mut executed = false;
    let mut status = outcome.status.clone();
    if outcome.should_execute {
        match state
            .sales
            .confirm_order(
                context.actor_user_id,
                context.trace_id,
                id,
                key,
                &B2VersionCommand {
                    expected_version: input.expected_version,
                    reason_code: None,
                },
            )
            .await
        {
            Ok(_) => {
                executed = true;
                status = "executed".into();
                let _ = finish_execution(&state.store, outcome.request_id, true).await;
            }
            Err(_) => {
                let _ = finish_execution(&state.store, outcome.request_id, false).await;
                return approval_error(
                    StatusCode::CONFLICT,
                    "approval_execution_failed",
                    context.trace_id,
                );
            }
        }
    }
    Json(ChatApprovalResult {
        request_id: outcome.request_id,
        document_type: "sales_order",
        document_id: id,
        decision: input.decision,
        status,
        approval_count: outcome.approval_count,
        minimum_approvers: outcome.minimum_approvers,
        executed,
        trace_id: context.trace_id,
    })
    .into_response()
}

async fn approve_purchase_order(
    State(state): State<Arc<AppState>>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<ChatApprovalInput>,
) -> Response {
    let preview = match state
        .purchasing
        .confirmation_preview(context.actor_user_id, id)
        .await
    {
        Ok(value) => value,
        Err(_) => {
            return approval_error(
                StatusCode::NOT_FOUND,
                "not_found_or_forbidden",
                context.trace_id,
            );
        }
    };
    if preview.version != input.expected_version || hash_json(&preview) != input.preview_hash {
        return approval_error(
            StatusCode::CONFLICT,
            "stale_approval_preview",
            context.trace_id,
        );
    }
    let Some(key) = idempotency_key(&headers) else {
        return approval_error(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            context.trace_id,
        );
    };
    let outcome = match cast_vote(
        &state.store,
        "purchase_order",
        "purchase_order:confirm",
        id,
        context.actor_user_id,
        context.trace_id,
        &input,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => return store_error(error, context.trace_id),
    };
    let mut executed = false;
    let mut status = outcome.status.clone();
    if outcome.should_execute {
        match state
            .purchasing
            .confirm_order(
                context.actor_user_id,
                context.trace_id,
                id,
                key,
                &B3VersionCommand {
                    expected_version: input.expected_version,
                    reason_code: None,
                },
            )
            .await
        {
            Ok(_) => {
                executed = true;
                status = "executed".into();
                let _ = finish_execution(&state.store, outcome.request_id, true).await;
            }
            Err(_) => {
                let _ = finish_execution(&state.store, outcome.request_id, false).await;
                return approval_error(
                    StatusCode::CONFLICT,
                    "approval_execution_failed",
                    context.trace_id,
                );
            }
        }
    }
    Json(ChatApprovalResult {
        request_id: outcome.request_id,
        document_type: "purchase_order",
        document_id: id,
        decision: input.decision,
        status,
        approval_count: outcome.approval_count,
        minimum_approvers: outcome.minimum_approvers,
        executed,
        trace_id: context.trace_id,
    })
    .into_response()
}

async fn cast_vote(
    store: &PgStore,
    document_type: &str,
    action_code: &str,
    document_id: Uuid,
    actor: Uuid,
    trace_id: Uuid,
    input: &ChatApprovalInput,
) -> Result<VoteOutcome, StoreError> {
    validate_input(input)?;
    let policy = store.approval_policy(action_code).await?;
    // No step-up credential is transported by this chat flow; do not downgrade such policies.
    if policy.step_up_amount_minor.is_some() {
        return Err(StoreError::NotFoundOrForbidden);
    }
    let minimum_approvers = effective_minimum_approvers(policy.min_approvers);
    let snapshot = store.snapshot(actor).await?;
    let eligible_role = snapshot.roles.iter().any(|role| {
        policy
            .eligible_role_keys
            .iter()
            .any(|eligible| eligible == &role.role_key)
    });
    if !snapshot
        .permission_keys
        .contains(&policy.required_permission)
        || !eligible_role
    {
        return Err(StoreError::NotFoundOrForbidden);
    }
    let row = match document_type {
        "sales_order" => sqlx::query(
            "SELECT created_by_user_id,legal_entity_id,business_unit_id,customer_id party_id,lifecycle_status,version FROM sales_orders WHERE id=$1",
        )
        .bind(document_id)
        .fetch_optional(store.pool())
        .await?,
        "purchase_order" => sqlx::query(
            "SELECT created_by_user_id,legal_entity_id,business_unit_id,supplier_id party_id,lifecycle_status,version FROM purchase_orders WHERE id=$1",
        )
        .bind(document_id)
        .fetch_optional(store.pool())
        .await?,
        "sales_return_inspection_intent" | "purchase_return_dispatch_intent" | "purchase_return_acknowledgment_intent" | "sales_return_cancellation_intent" | "purchase_return_cancellation_intent" | "sales_return_reversal_intent" | "purchase_return_reversal_intent" => Some(return_disposition::authority_row(store,document_type,document_id).await?),
        "inventory_count_submission_intent" | "inventory_count_posting_intent" | "inventory_count_cancellation_intent" => Some(inventory_count_operation::authority_row(store,document_type,document_id).await?),
        "crm_creation_intent" | "crm_update_intent" | "crm_followup_intent" => Some(crm::authority_row(store,document_type,document_id).await?),
        "inventory_count_creation_intent" => Some(inventory_count_creation::authority_row(store,document_type,document_id).await?),
        "sales_return" | "purchase_return" => Some(returns::authority_row(store,document_type,document_id).await?),
        "shipment" | "goods_receipt" | "inventory_opening" => Some(stock::authority_row(store, document_type, document_id).await?),
        "customer_receipt" | "supplier_payment" => Some(settlement::authority_row(store, document_type, document_id).await?),
        "shipment_reversal_intent" | "goods_receipt_reversal_intent" | "inventory_opening_reversal_intent" => Some(stock_reversal::authority_row(store,document_type,document_id).await?),
        "sales_order_cancellation_intent" | "purchase_order_cancellation_intent" => Some(order_cancellation::authority_row(store,document_type,document_id).await?),
        "customer_receipt_reversal_intent" | "supplier_payment_reversal_intent" | "receivable_allocation_reversal_intent" | "payable_allocation_reversal_intent" => Some(reversal::authority_row(store, document_type, document_id).await?),
        "receivable_allocation_intent" | "payable_allocation_intent" => Some(allocation::authority_row(store, document_type, document_id).await?),
        _ => return Err(StoreError::Invalid("document type".into())),
    }
    .ok_or(StoreError::NotFoundOrForbidden)?;
    let creator: Uuid = row.get("created_by_user_id");
    let wrong_party_scope = if matches!(
        document_type,
        "crm_creation_intent" | "crm_update_intent" | "crm_followup_intent"
    ) {
        row.get::<Option<Uuid>, _>("party_id")
            .is_some_and(|id| !snapshot.scopes.customer_ids.contains(&id))
    } else if matches!(
        document_type,
        "inventory_opening"
            | "inventory_opening_reversal_intent"
            | "inventory_count_creation_intent"
            | "inventory_count_submission_intent"
            | "inventory_count_posting_intent"
            | "inventory_count_cancellation_intent"
    ) {
        false
    } else if matches!(
        document_type,
        "sales_order"
            | "sales_return"
            | "sales_return_inspection_intent"
            | "sales_return_cancellation_intent"
            | "sales_return_reversal_intent"
            | "shipment"
            | "shipment_reversal_intent"
            | "customer_receipt"
            | "receivable_allocation_intent"
            | "sales_order_cancellation_intent"
            | "customer_receipt_reversal_intent"
            | "receivable_allocation_reversal_intent"
    ) {
        !snapshot
            .scopes
            .customer_ids
            .contains(&row.get::<Uuid, _>("party_id"))
    } else {
        !snapshot
            .scopes
            .supplier_ids
            .contains(&row.get::<Uuid, _>("party_id"))
    };
    if row.get::<String, _>("lifecycle_status") != "draft"
        || row.get::<i64, _>("version") != input.expected_version
        || (!policy.allow_self_approval && creator == actor)
        || !snapshot
            .scopes
            .legal_entity_ids
            .contains(&row.get::<Uuid, _>("legal_entity_id"))
        || row
            .get::<Option<Uuid>, _>("business_unit_id")
            .is_some_and(|unit| !snapshot.scopes.business_unit_ids.contains(&unit))
        || wrong_party_scope
    {
        return Err(StoreError::NotFoundOrForbidden);
    }
    if matches!(
        document_type,
        "shipment" | "goods_receipt" | "inventory_opening"
    ) {
        stock::check_stock_scope(store, actor, document_type, document_id).await?;
    }
    if policy.require_distinct_business_unit {
        let requester_units = store.snapshot(creator).await?.scopes.business_unit_ids;
        if !requester_units.is_disjoint(&snapshot.scopes.business_unit_ids) {
            return Err(StoreError::NotFoundOrForbidden);
        }
    }

    let mut tx = store.pool().begin().await?;
    let proposed_id = Uuid::new_v4();
    let request_id: Uuid = sqlx::query_scalar(
        "INSERT INTO business_document_approval_requests(id,document_type,document_id,action_code,expected_version,preview_hash,requester_user_id,minimum_approvers,trace_id)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)
         ON CONFLICT(document_type,document_id,expected_version,preview_hash) DO UPDATE SET preview_hash=business_document_approval_requests.preview_hash
         RETURNING id",
    )
    .bind(proposed_id)
    .bind(document_type)
    .bind(document_id)
    .bind(action_code)
    .bind(input.expected_version)
    .bind(&input.preview_hash)
    .bind(creator)
    .bind(minimum_approvers)
    .bind(trace_id)
    .fetch_one(&mut *tx)
    .await?;
    let request = sqlx::query(
        "SELECT status,preview_hash,minimum_approvers FROM business_document_approval_requests WHERE id=$1 FOR UPDATE",
    )
    .bind(request_id)
    .fetch_one(&mut *tx)
    .await?;
    if request.get::<String, _>("preview_hash") != input.preview_hash
        || request.get::<String, _>("status") != "pending"
    {
        return Err(StoreError::Conflict);
    }
    let duplicate_vote: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM business_document_approval_votes WHERE request_id=$1 AND approver_user_id=$2)",
    )
    .bind(request_id)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;
    if duplicate_vote {
        return Err(StoreError::Conflict);
    }
    sqlx::query(
        "INSERT INTO business_document_approval_votes(id,request_id,approver_user_id,decision,source_buzz_event_id,source_channel_id,trace_id)
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(Uuid::new_v4())
    .bind(request_id)
    .bind(actor)
    .bind(match input.decision {
        ApprovalDecision::Approve => "approve",
        ApprovalDecision::Reject => "reject",
    })
    .bind(&input.source_buzz_event_id)
    .bind(&input.source_channel_id)
    .bind(trace_id)
    .execute(&mut *tx)
    .await?;
    let minimum_approvers: i16 = request.get("minimum_approvers");
    let approval_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM business_document_approval_votes WHERE request_id=$1 AND decision='approve'",
    )
    .bind(request_id)
    .fetch_one(&mut *tx)
    .await?;
    let (status, should_execute) = match input.decision {
        ApprovalDecision::Reject => ("rejected", false),
        ApprovalDecision::Approve if approval_count >= i64::from(minimum_approvers) => {
            ("executing", true)
        }
        ApprovalDecision::Approve => ("pending", false),
    };
    if status != "pending" {
        sqlx::query(
            "UPDATE business_document_approval_requests SET status=$2,decided_at=now(),version=version+1 WHERE id=$1",
        )
        .bind(request_id)
        .bind(status)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "INSERT INTO business_core_audit_events(trace_id,actor_user_id,operation,target_type,target_id,details)
         VALUES($1,$2,'chat_document_approval_vote',$3,$4,$5)",
    )
    .bind(trace_id)
    .bind(actor)
    .bind(document_type)
    .bind(document_id.to_string())
    .bind(json!({"requestId":request_id,"decision":input.decision,"approvalCount":approval_count,"minimumApprovers":minimum_approvers,"sourceBuzzEventId":input.source_buzz_event_id}))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(VoteOutcome {
        request_id,
        approval_count,
        minimum_approvers,
        should_execute,
        status: status.into(),
    })
}

async fn finish_execution(
    store: &PgStore,
    request_id: Uuid,
    succeeded: bool,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE business_document_approval_requests SET status=$2,executed_at=CASE WHEN $3 THEN now() ELSE executed_at END,version=version+1 WHERE id=$1 AND status='executing'",
    )
    .bind(request_id)
    .bind(if succeeded {
        "executed"
    } else {
        "execution_failed"
    })
    .bind(succeeded)
    .execute(store.pool())
    .await?;
    Ok(())
}

fn validate_input(input: &ChatApprovalInput) -> Result<(), StoreError> {
    if input.expected_version <= 0
        || input.preview_hash.len() != 64
        || !input
            .preview_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || input.source_buzz_event_id.len() != 64
        || !input
            .source_buzz_event_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || input.source_channel_id.is_empty()
        || input.source_channel_id.len() > 200
    {
        return Err(StoreError::Invalid("chat approval input".into()));
    }
    Ok(())
}

fn hash_json<T: Serialize>(value: &T) -> String {
    let mut value = serde_json::to_value(value).unwrap_or_default();
    if let Some(object) = value.as_object_mut() {
        object.remove("inventoryAsOf");
        object.remove("checkedAt");
        object.remove("canConfirm");
    }
    let bytes = serde_json::to_vec(&value).unwrap_or_default();
    hex::encode(Sha256::digest(bytes))
}

fn idempotency_key(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .filter(|value| (16..=128).contains(&value.len()))
}

fn store_error(error: StoreError, trace_id: Uuid) -> Response {
    match error {
        StoreError::NotFoundOrForbidden => {
            approval_error(StatusCode::NOT_FOUND, "not_found_or_forbidden", trace_id)
        }
        StoreError::Conflict => approval_error(StatusCode::CONFLICT, "approval_conflict", trace_id),
        StoreError::Invalid(_) => approval_error(
            StatusCode::BAD_REQUEST,
            "invalid_approval_request",
            trace_id,
        ),
        _ => approval_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "approval_unavailable",
            trace_id,
        ),
    }
}

fn approval_error(status: StatusCode, code: &'static str, trace_id: Uuid) -> Response {
    (status, Json(json!({"code":code,"traceId":trace_id}))).into_response()
}

fn effective_minimum_approvers(policy_minimum: i16) -> i16 {
    policy_minimum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_approval_uses_the_configured_policy_threshold() {
        assert_eq!(effective_minimum_approvers(1), 1);
        assert_eq!(effective_minimum_approvers(2), 2);
        assert_eq!(effective_minimum_approvers(4), 4);
    }

    #[test]
    fn preview_hash_ignores_observation_time_and_actor_hint_only() {
        let first = json!({
            "orderId": Uuid::nil(),
            "version": 3,
            "grossAmount": "100.00",
            "inventoryAsOf": "2026-08-31T01:00:00Z",
            "canConfirm": true
        });
        let second = json!({
            "orderId": Uuid::nil(),
            "version": 3,
            "grossAmount": "100.00",
            "inventoryAsOf": "2026-08-31T02:00:00Z",
            "canConfirm": false
        });
        assert_eq!(hash_json(&first), hash_json(&second));
        let changed = json!({
            "orderId": Uuid::nil(),
            "version": 3,
            "grossAmount": "101.00"
        });
        assert_ne!(hash_json(&first), hash_json(&changed));
    }
}
