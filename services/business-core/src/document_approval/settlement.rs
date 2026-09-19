use super::*;
use serde_json::Value;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/v1/agent-approval-previews/settlement/{kind}/{id}",
            get(preview),
        )
        .route("/v1/agent-approvals/settlement/{kind}/{id}", post(approve))
}

pub(super) async fn authority_row(
    store: &PgStore,
    kind: &str,
    id: Uuid,
) -> Result<sqlx::postgres::PgRow, StoreError> {
    let query = match kind {
        "customer_receipt" => "SELECT r.created_by_user_id,r.legal_entity_id,c.business_unit_id,r.customer_id party_id,r.status lifecycle_status,r.version FROM customer_receipts r JOIN business_customers c ON c.id=r.customer_id WHERE r.id=$1",
        "supplier_payment" => "SELECT r.created_by_user_id,r.legal_entity_id,c.business_unit_id,r.supplier_id party_id,r.status lifecycle_status,r.version FROM supplier_payments r JOIN business_suppliers c ON c.id=r.supplier_id WHERE r.id=$1",
        _ => return Err(StoreError::NotFoundOrForbidden),
    };
    sqlx::query(query)
        .bind(id)
        .fetch_optional(store.pool())
        .await?
        .ok_or(StoreError::NotFoundOrForbidden)
}

pub(super) async fn value(
    state: &AppState,
    actor: Uuid,
    kind: &str,
    id: Uuid,
) -> Result<Value, StoreError> {
    let row = authority_row(&state.store, kind, id).await?;
    let snapshot = state.store.snapshot(actor).await?;
    let party: Uuid = row.get("party_id");
    let permission = format!("{kind}:read");
    if !snapshot.permission_keys.contains(&permission)
        || !snapshot
            .scopes
            .legal_entity_ids
            .contains(&row.get("legal_entity_id"))
        || !snapshot
            .scopes
            .business_unit_ids
            .contains(&row.get("business_unit_id"))
        || !(if kind == "customer_receipt" {
            &snapshot.scopes.customer_ids
        } else {
            &snapshot.scopes.supplier_ids
        })
        .contains(&party)
    {
        return Err(StoreError::NotFoundOrForbidden);
    }
    // Keep decimal amounts as strings before JSON conversion; no bank credentials are exposed.
    let sql = match kind {
        "customer_receipt" => "SELECT jsonb_build_object('id',id,'number',receipt_number,'legalEntityId',legal_entity_id,'customerId',customer_id,'currency',currency,'businessDate',receipt_date,'amount',amount::text,'allocatedAmount',allocated_amount::text,'unappliedAmount',unapplied_amount::text,'paymentMethod',payment_method,'externalReference',external_reference,'status',status,'version',version) FROM customer_receipts WHERE id=$1",
        "supplier_payment" => "SELECT jsonb_build_object('id',id,'number',supplier_payment_number,'legalEntityId',legal_entity_id,'supplierId',supplier_id,'currency',currency,'businessDate',payment_date,'amount',amount::text,'allocatedAmount',allocated_amount::text,'unappliedAmount',unapplied_amount::text,'paymentMethod',payment_method,'externalReference',external_reference,'status',status,'version',version) FROM supplier_payments WHERE id=$1",
        _ => return Err(StoreError::NotFoundOrForbidden),
    };
    let mut item: Value = sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(state.store.pool())
        .await?;
    item["businessUnitId"] = json!(row.get::<Uuid, _>("business_unit_id"));
    item["effect"] = json!(if kind == "customer_receipt" {
        "确认已实际收到的款项，增加待核销收款金额；本操作不核销应收、不发起银行交易"
    } else {
        "确认已实际支付的款项，增加待核销付款金额；本操作不核销应付、不发起银行转账"
    });
    item["executesBankTransfer"] = json!(false);
    Ok(item)
}

async fn preview(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path((kind, id)): Path<(String, Uuid)>,
) -> Response {
    match value(&state, c.actor_user_id, &kind, id).await {
        Ok(item) => {
            let hash = hash_json(&item);
            Json(json!({"document":item,"item":item,"previewHash":hash,"approvalCommand":format!("确认 {} {id} v{} {hash}",kind.replace('_',"-"),item["version"]),"rejectionCommand":format!("拒绝 {} {id} v{} {hash}",kind.replace('_',"-"),item["version"]),"traceId":c.trace_id})).into_response()
        }
        Err(e) => store_error(e, c.trace_id),
    }
}

async fn approve(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path((kind, id)): Path<(String, Uuid)>,
    headers: HeaderMap,
    Json(input): Json<ChatApprovalInput>,
) -> Response {
    let action = match kind.as_str() {
        "customer_receipt" => "customer_receipt:confirm",
        "supplier_payment" => "supplier_payment:confirm",
        _ => return approval_error(StatusCode::NOT_FOUND, "not_found_or_forbidden", c.trace_id),
    };
    let item = match value(&state, c.actor_user_id, &kind, id).await {
        Ok(v) => v,
        Err(e) => return store_error(e, c.trace_id),
    };
    if item["version"].as_i64() != Some(input.expected_version)
        || hash_json(&item) != input.preview_hash
    {
        return approval_error(StatusCode::CONFLICT, "stale_approval_preview", c.trace_id);
    }
    let Some(key) = idempotency_key(&headers) else {
        return approval_error(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            c.trace_id,
        );
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
    let mut status = outcome.status.clone();
    let mut executed = false;
    if outcome.should_execute {
        let result = if kind == "customer_receipt" {
            state
                .settlement
                .confirm_receipt(
                    c.actor_user_id,
                    c.trace_id,
                    id,
                    key,
                    &B2VersionCommand {
                        expected_version: input.expected_version,
                        reason_code: None,
                    },
                )
                .await
                .map(|_| ())
        } else {
            state
                .payables
                .confirm_payment(
                    c.actor_user_id,
                    c.trace_id,
                    id,
                    key,
                    &B3VersionCommand {
                        expected_version: input.expected_version,
                        reason_code: None,
                    },
                )
                .await
                .map(|_| ())
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
        status = "executed".into();
        executed = true;
    }
    Json(json!({"requestId":outcome.request_id,"documentType":kind,"documentId":id,"decision":input.decision,"status":status,"approvalCount":outcome.approval_count,"minimumApprovers":outcome.minimum_approvers,"executed":executed,"resourceRefs":[{"type":kind,"id":id,"title":item["number"],"bizUri":format!("biz://{}/{id}",kind.replace('_',"-"))}],"traceId":c.trace_id})).into_response()
}
