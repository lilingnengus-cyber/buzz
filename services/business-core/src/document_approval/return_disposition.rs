use super::*;
use serde_json::Value;

/// Prepare a return disposition with typed, fixed-family command parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrepareReturnDisposition {
    /// Confirmed return ID from a scoped read.
    pub source_document_id: Uuid,
    /// Strictly decoded inspection, dispatch or acknowledgment command.
    pub command: Value,
}
fn family(kind: &str) -> Result<(&'static str, &'static str), StoreError> {
    match kind {
        "sales_return_inspection_intent" => Ok(("sales_return", "shipment:reverse")),
        "purchase_return_dispatch_intent" | "purchase_return_acknowledgment_intent" => {
            Ok(("purchase_return", "goods_receipt:reverse"))
        }
        _ => Err(StoreError::NotFoundOrForbidden),
    }
}

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/v1/agent-return-disposition-previews/{kind}",
            post(dry_preview),
        )
        .route("/v1/agent-return-disposition-intents/{kind}", post(prepare))
        .route(
            "/v1/agent-approval-previews/return-dispositions/{kind}/{id}",
            get(preview),
        )
        .route(
            "/v1/agent-approvals/return-dispositions/{kind}/{id}",
            post(approve),
        )
}

pub(super) async fn authority_row(
    store: &PgStore,
    kind: &str,
    id: Uuid,
) -> Result<sqlx::postgres::PgRow, StoreError> {
    family(kind)?;
    sqlx::query("SELECT created_by_user_id,(snapshot->'source'->>'legalEntityId')::uuid legal_entity_id,(snapshot->'source'->>'businessUnitId')::uuid business_unit_id,COALESCE(snapshot->'source'->>'customerId',snapshot->'source'->>'supplierId')::uuid party_id,'draft'::text lifecycle_status,1::bigint version FROM business_agent_return_disposition_intents WHERE id=$1 AND kind=$2 AND expires_at>now()")
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
) -> Result<(PrepareReturnDisposition, Value), StoreError> {
    family(kind)?;
    let row=sqlx::query("SELECT input,snapshot FROM business_agent_return_disposition_intents WHERE id=$1 AND kind=$2 AND expires_at>now()")
        .bind(id).bind(kind).fetch_optional(state.store.pool()).await?.ok_or(StoreError::NotFoundOrForbidden)?;
    let input: PrepareReturnDisposition = serde_json::from_value(row.get("input"))
        .map_err(|_| StoreError::Invalid("invalid stored disposition".into()))?;
    let current = snapshot(state, actor, kind, &input).await?;
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
    Json(input): Json<PrepareReturnDisposition>,
) -> Response {
    let Some(key) = idempotency_key(&headers) else {
        return approval_error(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            c.trace_id,
        );
    };
    let snapshot = match snapshot(&state, c.actor_user_id, &kind, &input).await {
        Ok(v) => v,
        Err(e) => return store_error(e, c.trace_id),
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
        Err(e) => store_error(e, c.trace_id),
    }
}

async fn save(
    store: &PgStore,
    actor: Uuid,
    trace: Uuid,
    kind: &str,
    key: &str,
    input: &PrepareReturnDisposition,
    snapshot: &Value,
) -> Result<Uuid, StoreError> {
    let input = serde_json::to_value(input)
        .map_err(|_| StoreError::Invalid("invalid disposition input".into()))?;
    let mut tx = store.pool().begin().await?;
    let id = Uuid::new_v4();
    let inserted=sqlx::query("INSERT INTO business_agent_return_disposition_intents(id,kind,source_document_id,input,snapshot,created_by_user_id,idempotency_key,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(created_by_user_id,idempotency_key) DO NOTHING")
      .bind(id).bind(kind).bind(input["sourceDocumentId"].as_str().and_then(|s|s.parse::<Uuid>().ok()).ok_or_else(||StoreError::Invalid("invalid source".into()))?).bind(&input).bind(snapshot).bind(actor).bind(key).bind(trace).execute(&mut *tx).await?.rows_affected();
    let row=sqlx::query("SELECT id,kind,input,snapshot,expires_at>now() current FROM business_agent_return_disposition_intents WHERE created_by_user_id=$1 AND idempotency_key=$2").bind(actor).bind(key).fetch_one(&mut *tx).await?;
    if !row.get::<bool, _>("current")
        || row.get::<String, _>("kind") != kind
        || row.get::<Value, _>("input") != input
        || row.get::<Value, _>("snapshot") != *snapshot
    {
        return Err(StoreError::Conflict);
    }
    let id: Uuid = row.get("id");
    if inserted == 1 {
        sqlx::query("INSERT INTO business_core_audit_events(trace_id,actor_user_id,operation,target_type,target_id,details) VALUES($1,$2,'agent_return_disposition_prepared',$3,$4,$5)").bind(trace).bind(actor).bind(kind).bind(id.to_string()).bind(json!({"previewHash":hash_json(snapshot)})).execute(&mut *tx).await?;
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
        Err(e) => store_error(e, c.trace_id),
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
        Err(e) => return store_error(e, c.trace_id),
    };
    if command.expected_version != 1 || command.preview_hash != hash_json(&snapshot) {
        return approval_error(StatusCode::CONFLICT, "stale_approval_preview", c.trace_id);
    }
    let (source_kind, action) = match family(&kind) {
        Ok(v) => v,
        Err(e) => return store_error(e, c.trace_id),
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
        Err(e) => return store_error(e, c.trace_id),
    };
    let mut executed = false;
    let mut status = outcome.status.clone();
    if outcome.should_execute {
        let result = execute(
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
            return store_error(e, c.trace_id);
        }
        executed = true;
        status = "executed".into();
    }
    Json(json!({"documentId":id,"documentType":kind,"requestId":outcome.request_id,"status":status,"executed":executed,"approvalCount":outcome.approval_count,"minimumApprovers":outcome.minimum_approvers,"traceId":c.trace_id,"resourceRefs":[{"type":source_kind,"id":input.source_document_id,"title":"查看退货单","bizUri":format!("biz://{}/{}",source_kind.replace('_',"-"),input.source_document_id)}]})).into_response()
}

async fn dry_preview(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(kind): Path<String>,
    Json(input): Json<PrepareReturnDisposition>,
) -> Response {
    match snapshot(&state, c.actor_user_id, &kind, &input).await {
        Ok(document) => Json(json!({"document":document,"traceId":c.trace_id})).into_response(),
        Err(e) => store_error(e, c.trace_id),
    }
}

async fn snapshot(
    state: &AppState,
    actor: Uuid,
    kind: &str,
    input: &PrepareReturnDisposition,
) -> Result<Value, StoreError> {
    family(kind)?;
    state
        .return_disposition
        .agent_preview(actor, input.source_document_id, kind, &input.command)
        .await
        .map_err(|e| match e {
            crate::b2::DomainError::NotFoundOrForbidden => StoreError::NotFoundOrForbidden,
            crate::b2::DomainError::VersionConflict => StoreError::Conflict,
            crate::b2::DomainError::Database(e) => StoreError::Database(e),
            _ => StoreError::Invalid("invalid return disposition input or state".into()),
        })
}
async fn execute(
    state: &AppState,
    actor: Uuid,
    trace: Uuid,
    id: Uuid,
    kind: &str,
    input: &PrepareReturnDisposition,
    snapshot: &Value,
) -> Result<(), String> {
    let key = format!("agent-return-disposition:{id}");
    let command = snapshot["command"].clone();
    match kind {
        "sales_return_inspection_intent" => {
            let command = serde_json::from_value(command).map_err(|e| e.to_string())?;
            let guard =
                serde_json::from_value(snapshot["guard"].clone()).map_err(|e| e.to_string())?;
            state
                .return_disposition
                .inspect_sales_return_guarded(
                    actor,
                    trace,
                    input.source_document_id,
                    &key,
                    &command,
                    Some(&guard),
                )
                .await
        }
        "purchase_return_dispatch_intent" => {
            let command = serde_json::from_value(command).map_err(|e| e.to_string())?;
            state
                .return_disposition
                .dispatch_purchase_return(actor, trace, input.source_document_id, &key, &command)
                .await
        }
        "purchase_return_acknowledgment_intent" => {
            let command = serde_json::from_value(command).map_err(|e| e.to_string())?;
            state
                .return_disposition
                .acknowledge_purchase_return(actor, trace, input.source_document_id, &key, &command)
                .await
        }
        _ => return Err("unknown disposition".into()),
    }
    .map(|_| ())
    .map_err(|e| e.to_string())
}
