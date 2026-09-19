mod execute;
mod snapshot;
use super::*;
use serde_json::Value;

/// Prepare a reversal bound to the source and all affected balances.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrepareStockReversal {
    /// Shipment, goods receipt or inventory opening ID from a scoped read.
    pub source_document_id: Uuid,
    /// Current version of the source fulfillment document.
    pub expected_source_version: i64,
    /// Human-provided reversal reason; persisted in the business audit.
    pub reason: String,
}
fn family(kind: &str) -> Result<(&'static str, &'static str), StoreError> {
    match kind {
        "shipment_reversal_intent" => Ok(("shipment", "shipment:reverse")),
        "goods_receipt_reversal_intent" => Ok(("goods_receipt", "goods_receipt:reverse")),
        "inventory_opening_reversal_intent" => {
            Ok(("inventory_opening", "inventory_opening:reverse"))
        }
        _ => Err(StoreError::NotFoundOrForbidden),
    }
}

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/v1/agent-stock-reversal-previews/{kind}",
            post(dry_preview),
        )
        .route("/v1/agent-stock-reversal-intents/{kind}", post(prepare))
        .route(
            "/v1/agent-approval-previews/stock-reversals/{kind}/{id}",
            get(preview),
        )
        .route(
            "/v1/agent-approvals/stock-reversals/{kind}/{id}",
            post(approve),
        )
}

pub(super) async fn authority_row(
    store: &PgStore,
    kind: &str,
    id: Uuid,
) -> Result<sqlx::postgres::PgRow, StoreError> {
    family(kind)?;
    sqlx::query("SELECT created_by_user_id,(snapshot->'source'->>'legalEntityId')::uuid legal_entity_id,(snapshot->'source'->>'businessUnitId')::uuid business_unit_id,COALESCE(snapshot->'source'->>'customerId',snapshot->'source'->>'supplierId')::uuid party_id,'draft'::text lifecycle_status,1::bigint version FROM business_agent_stock_reversal_intents WHERE id=$1 AND kind=$2 AND expires_at>now()")
    .bind(id).bind(kind).fetch_optional(store.pool()).await?.ok_or(StoreError::NotFoundOrForbidden)
}

fn envelope(id: Uuid, kind: &str, snapshot: Value, trace: Uuid) -> Value {
    let hash = hash_json(&snapshot);
    json!({"item":{"id":id,"version":1,"snapshot":snapshot},"document":snapshot,"previewHash":hash,"approvalCommand":format!("确认 {} {id} v1 {hash}",kind.replace('_',"-")),"rejectionCommand":format!("拒绝 {} {id} v1 {hash}",kind.replace('_',"-")),"traceId":trace})
}

async fn load(
    state: &AppState,
    actor: Uuid,
    kind: &str,
    id: Uuid,
) -> Result<(PrepareStockReversal, Value), StoreError> {
    family(kind)?;
    let row=sqlx::query("SELECT input,snapshot FROM business_agent_stock_reversal_intents WHERE id=$1 AND kind=$2 AND expires_at>now()")
        .bind(id).bind(kind).fetch_optional(state.store.pool()).await?.ok_or(StoreError::NotFoundOrForbidden)?;
    let input: PrepareStockReversal = serde_json::from_value(row.get("input"))
        .map_err(|_| StoreError::Invalid("invalid stored reversal".into()))?;
    let current = snapshot::snapshot(state, actor, kind, &input).await?;
    if current != row.get::<Value, _>("snapshot") {
        return Err(StoreError::Conflict);
    }
    Ok((input, current))
}

async fn prepare(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(kind): Path<String>,
    headers: HeaderMap,
    Json(mut input): Json<PrepareStockReversal>,
) -> Response {
    let Some(key) = idempotency_key(&headers) else {
        return approval_error(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            c.trace_id,
        );
    };
    input.reason = input.reason.trim().to_owned();
    let snapshot = match snapshot::snapshot(&state, c.actor_user_id, &kind, &input).await {
        Ok(v) => v,
        Err(e) => return stock_error(e, c.trace_id),
    };
    match save(
        &state.store,
        c.actor_user_id,
        c.trace_id,
        &kind,
        key,
        &input,
        &snapshot,
    )
    .await
    {
        Ok(id) => Json(envelope(id, &kind, snapshot, c.trace_id)).into_response(),
        Err(e) => stock_error(e, c.trace_id),
    }
}

async fn save(
    store: &PgStore,
    actor: Uuid,
    trace: Uuid,
    kind: &str,
    key: &str,
    input: &PrepareStockReversal,
    snapshot: &Value,
) -> Result<Uuid, StoreError> {
    let input = serde_json::to_value(input)
        .map_err(|_| StoreError::Invalid("invalid reversal input".into()))?;
    let mut tx = store.pool().begin().await?;
    let id = Uuid::new_v4();
    let inserted=sqlx::query("INSERT INTO business_agent_stock_reversal_intents(id,kind,source_document_id,input,snapshot,created_by_user_id,idempotency_key,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(created_by_user_id,idempotency_key) DO NOTHING")
      .bind(id).bind(kind).bind(input["sourceDocumentId"].as_str().and_then(|s|s.parse::<Uuid>().ok()).ok_or_else(||StoreError::Invalid("invalid source".into()))?).bind(&input).bind(snapshot).bind(actor).bind(key).bind(trace).execute(&mut *tx).await?.rows_affected();
    let row=sqlx::query("SELECT id,kind,input,snapshot,expires_at>now() current FROM business_agent_stock_reversal_intents WHERE created_by_user_id=$1 AND idempotency_key=$2").bind(actor).bind(key).fetch_one(&mut *tx).await?;
    if !row.get::<bool, _>("current")
        || row.get::<String, _>("kind") != kind
        || row.get::<Value, _>("input") != input
        || row.get::<Value, _>("snapshot") != *snapshot
    {
        return Err(StoreError::Conflict);
    }
    let id: Uuid = row.get("id");
    if inserted == 1 {
        sqlx::query("INSERT INTO business_core_audit_events(trace_id,actor_user_id,operation,target_type,target_id,details) VALUES($1,$2,'agent_stock_reversal_prepared',$3,$4,$5)").bind(trace).bind(actor).bind(kind).bind(id.to_string()).bind(json!({"previewHash":hash_json(snapshot)})).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(id)
}

async fn preview(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path((kind, id)): Path<(String, Uuid)>,
) -> Response {
    match load(&state, c.actor_user_id, &kind, id).await {
        Ok((_, snapshot)) => Json(envelope(id, &kind, snapshot, c.trace_id)).into_response(),
        Err(e) => stock_error(e, c.trace_id),
    }
}

async fn approve(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path((kind, id)): Path<(String, Uuid)>,
    Json(command): Json<ChatApprovalInput>,
) -> Response {
    let (input, snapshot) = match load(&state, c.actor_user_id, &kind, id).await {
        Ok(v) => v,
        Err(e) => return stock_error(e, c.trace_id),
    };
    if command.expected_version != 1 || command.preview_hash != hash_json(&snapshot) {
        return approval_error(StatusCode::CONFLICT, "stale_approval_preview", c.trace_id);
    }
    let (source_kind, action) = match family(&kind) {
        Ok(v) => v,
        Err(e) => return stock_error(e, c.trace_id),
    };
    let outcome = match cast_vote(
        &state.store,
        &kind,
        action,
        id,
        c.actor_user_id,
        c.trace_id,
        &command,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return stock_error(e, c.trace_id),
    };
    let mut executed = false;
    let mut status = outcome.status.clone();
    if outcome.should_execute {
        let result = execute::execute(
            &state,
            c.actor_user_id,
            c.trace_id,
            id,
            &kind,
            &input,
            &snapshot,
        )
        .await;
        if result.is_err() {
            let _ = finish_execution(&state.store, outcome.request_id, false).await;
            return approval_error(
                StatusCode::CONFLICT,
                "approval_execution_failed",
                c.trace_id,
            );
        }
        if let Err(e) = finish_execution(&state.store, outcome.request_id, true).await {
            return stock_error(e, c.trace_id);
        }
        executed = true;
        status = "executed".into();
    }
    Json(json!({"documentId":id,"documentType":kind,"requestId":outcome.request_id,"status":status,"executed":executed,"approvalCount":outcome.approval_count,"minimumApprovers":outcome.minimum_approvers,"traceId":c.trace_id,"resourceRefs":[{"type":source_kind,"id":input.source_document_id,"title":"查看履约单据","bizUri":format!("biz://{}/{}",source_kind.replace('_',"-"),input.source_document_id)}]})).into_response()
}

async fn dry_preview(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(kind): Path<String>,
    Json(mut input): Json<PrepareStockReversal>,
) -> Response {
    input.reason = input.reason.trim().to_owned();
    match snapshot::snapshot(&state, c.actor_user_id, &kind, &input).await {
        Ok(document) => Json(json!({"document":document,"traceId":c.trace_id})).into_response(),
        Err(e) => stock_error(e, c.trace_id),
    }
}

fn stock_error(error: StoreError, trace_id: Uuid) -> Response {
    if let StoreError::Invalid(ref reason) = error {
        let details = match reason.as_str() {
            "reverse allocations before stock reversal" => Some((
                "allocations_exist",
                "请先逆转该履约单据对应的核销，再准备履约逆转。",
            )),
            "source has active returns" => Some((
                "active_returns_exist",
                "来源单据存在有效退货记录，请先处理退货。",
            )),
            "source has subsequent inventory movements" => Some((
                "subsequent_inventory_movements",
                "该库存已有后续流水，当前不能逆转来源单据。",
            )),
            "reversal would invalidate stock quantity, value, reservations or quarantine" => {
                Some((
                    "stock_balance_conflict",
                    "逆转后的库存数量或成本不能覆盖现有预留、隔离库存，请先处理相关业务。",
                ))
            }
            "current version and bounded reversal reason required" => Some((
                "invalid_reversal_input",
                "请提供当前版本及不超过 500 字的非空逆转原因。",
            )),
            _ => None,
        };
        if let Some((code, message)) = details {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"code":code,"message":message,"traceId":trace_id})),
            )
                .into_response();
        }
    }
    super::store_error(error, trace_id)
}
