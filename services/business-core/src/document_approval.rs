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
            let hash = hash_json(&preview);
            Json(json!({
                "item": preview,
                "previewHash": hash,
                "approvalCommand": format!("/approve sales-order {id} v{} {hash}", preview.version),
                "rejectionCommand": format!("/reject sales-order {id} v{} {hash}", preview.version),
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
            let hash = hash_json(&preview);
            Json(json!({
                "item": preview,
                "previewHash": hash,
                "approvalCommand": format!("/approve purchase-order {id} v{} {hash}", preview.version),
                "rejectionCommand": format!("/reject purchase-order {id} v{} {hash}", preview.version),
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
        _ => return Err(StoreError::Invalid("document type".into())),
    }
    .ok_or(StoreError::NotFoundOrForbidden)?;
    let creator: Uuid = row.get("created_by_user_id");
    let wrong_party_scope = if document_type == "sales_order" {
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
        || !snapshot
            .scopes
            .business_unit_ids
            .contains(&row.get::<Uuid, _>("business_unit_id"))
        || wrong_party_scope
    {
        return Err(StoreError::NotFoundOrForbidden);
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
         ON CONFLICT(document_type,document_id,expected_version) DO UPDATE SET preview_hash=business_document_approval_requests.preview_hash
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
