use super::*;
use crate::b2::{return_confirmation::ReturnConfirmationGuard, DomainError};

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/v1/agent-approval-previews/returns/{kind}/{id}",
            get(preview),
        )
        .route("/v1/agent-approvals/returns/{kind}/{id}", post(approve))
}
fn sales(kind: &str) -> Result<bool, StoreError> {
    match kind {
        "sales_return" => Ok(true),
        "purchase_return" => Ok(false),
        _ => Err(StoreError::NotFoundOrForbidden),
    }
}
pub(super) async fn authority_row(
    store: &PgStore,
    kind: &str,
    id: Uuid,
) -> Result<sqlx::postgres::PgRow, StoreError> {
    let sql = if sales(kind)? {
        "SELECT r.created_by_user_id,r.legal_entity_id,o.business_unit_id,r.customer_id party_id,r.status lifecycle_status,r.version FROM sales_returns r JOIN sales_orders o ON o.id=r.sales_order_id WHERE r.id=$1"
    } else {
        "SELECT r.created_by_user_id,r.legal_entity_id,o.business_unit_id,r.supplier_id party_id,r.status lifecycle_status,r.version FROM purchase_returns r JOIN purchase_orders o ON o.id=r.purchase_order_id WHERE r.id=$1"
    };
    sqlx::query(sql)
        .bind(id)
        .fetch_optional(store.pool())
        .await?
        .ok_or(StoreError::NotFoundOrForbidden)
}
fn domain(error: DomainError, trace: Uuid) -> Response {
    let code = match error {
        DomainError::NotFoundOrForbidden => {
            return store_error(StoreError::NotFoundOrForbidden, trace)
        }
        DomainError::VersionConflict => return store_error(StoreError::Conflict, trace),
        DomainError::InsufficientStock(_) => "insufficient_stock",
        DomainError::ReceivableAlreadySettled | DomainError::PayableAlreadySettled => {
            "return_exceeds_open_amount"
        }
        DomainError::MissingInventoryCost => "missing_inventory_cost",
        DomainError::Database(_) => {
            return approval_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "return_preview_unavailable",
                trace,
            )
        }
        _ => "invalid_return_state",
    };
    approval_error(StatusCode::BAD_REQUEST, code, trace)
}
async fn preview(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path((kind, id)): Path<(String, Uuid)>,
) -> Response {
    let sales = match sales(&kind) {
        Ok(v) => v,
        Err(e) => return store_error(e, c.trace_id),
    };
    match state
        .returns
        .confirmation_preview(c.actor_user_id, sales, id)
        .await
    {
        Ok(item) => {
            let hash = hash_json(&item);
            Json(json!({"item":item,"document":item,"previewHash":hash,"approvalCommand":format!("确认 {} {id} v{} {hash}",kind.replace('_',"-"),item["version"]),"rejectionCommand":format!("拒绝 {} {id} v{} {hash}",kind.replace('_',"-"),item["version"]),"traceId":c.trace_id})).into_response()
        }
        Err(e) => domain(e, c.trace_id),
    }
}
async fn approve(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path((kind, id)): Path<(String, Uuid)>,
    Json(input): Json<ChatApprovalInput>,
) -> Response {
    let sales = match sales(&kind) {
        Ok(v) => v,
        Err(e) => return store_error(e, c.trace_id),
    };
    let item = match state
        .returns
        .confirmation_preview(c.actor_user_id, sales, id)
        .await
    {
        Ok(v) => v,
        Err(e) => return domain(e, c.trace_id),
    };
    if item["version"].as_i64() != Some(input.expected_version)
        || hash_json(&item) != input.preview_hash
    {
        return approval_error(StatusCode::CONFLICT, "stale_approval_preview", c.trace_id);
    }
    let guard: ReturnConfirmationGuard = match serde_json::from_value(item["guard"].clone()) {
        Ok(v) => v,
        Err(_) => {
            return approval_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "return_preview_unavailable",
                c.trace_id,
            )
        }
    };
    let action = if sales {
        "shipment:reverse"
    } else {
        "goods_receipt:reverse"
    };
    let outcome = match cast_vote(
        &state.store,
        &kind,
        action,
        id,
        c.actor_user_id,
        c.trace_id,
        &input,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return store_error(e, c.trace_id),
    };
    let mut executed = false;
    let mut status = outcome.status.clone();
    if outcome.should_execute {
        let key = format!("agent-return-confirm:{}", outcome.request_id);
        let command = B2VersionCommand {
            expected_version: input.expected_version,
            reason_code: None,
        };
        let result = if sales {
            state
                .returns
                .confirm_sales_return_guarded(
                    c.actor_user_id,
                    c.trace_id,
                    id,
                    &key,
                    &command,
                    Some(&guard),
                )
                .await
        } else {
            state
                .returns
                .confirm_purchase_return_guarded(
                    c.actor_user_id,
                    c.trace_id,
                    id,
                    &key,
                    &command,
                    Some(&guard),
                )
                .await
        };
        if result.is_err() {
            let _ = finish_execution(&state.store, outcome.request_id, false).await;
            return approval_error(
                StatusCode::CONFLICT,
                "approval_execution_failed",
                c.trace_id,
            );
        }
        if let Err(e) = finish_execution(&state.store, outcome.request_id, true).await {
            return store_error(e, c.trace_id);
        }
        executed = true;
        status = "executed".into();
    }
    Json(json!({"requestId":outcome.request_id,"documentType":kind,"documentId":id,"decision":input.decision,"status":status,"approvalCount":outcome.approval_count,"minimumApprovers":outcome.minimum_approvers,"executed":executed,"traceId":c.trace_id})).into_response()
}
