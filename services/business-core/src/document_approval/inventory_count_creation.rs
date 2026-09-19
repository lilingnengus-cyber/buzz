use super::*;
use crate::b2::CreateInventoryCount as PrepareInventoryCountCreation;
use serde_json::Value;
fn family(kind: &str) -> Result<(&'static str, &'static str), StoreError> {
    match kind {
        "inventory_count_creation_intent" => Ok(("inventory_count", "inventory_opening:create")),
        _ => Err(StoreError::NotFoundOrForbidden),
    }
}

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/v1/agent-inventory-count-creation-previews/{kind}",
            post(dry_preview),
        )
        .route(
            "/v1/agent-inventory-count-creation-intents/{kind}",
            post(prepare),
        )
        .route(
            "/v1/agent-approval-previews/inventory-count-creations/{kind}/{id}",
            get(preview),
        )
        .route(
            "/v1/agent-approvals/inventory-count-creations/{kind}/{id}",
            post(approve),
        )
}

pub(super) async fn authority_row(
    store: &PgStore,
    kind: &str,
    id: Uuid,
) -> Result<sqlx::postgres::PgRow, StoreError> {
    family(kind)?;
    sqlx::query("SELECT created_by_user_id,(snapshot->'command'->>'legalEntityId')::uuid legal_entity_id,(snapshot->>'businessUnitId')::uuid business_unit_id,NULL::uuid party_id,'draft'::text lifecycle_status,1::bigint version FROM business_agent_inventory_count_creation_intents WHERE id=$1 AND kind=$2 AND expires_at>now()")
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
) -> Result<(PrepareInventoryCountCreation, Value), StoreError> {
    family(kind)?;
    let row=sqlx::query("SELECT input,snapshot FROM business_agent_inventory_count_creation_intents WHERE id=$1 AND kind=$2 AND expires_at>now()")
        .bind(id).bind(kind).fetch_optional(state.store.pool()).await?.ok_or(StoreError::NotFoundOrForbidden)?;
    let input: PrepareInventoryCountCreation = serde_json::from_value(row.get("input"))
        .map_err(|_| StoreError::Invalid("invalid stored count creation".into()))?;
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
    Json(input): Json<PrepareInventoryCountCreation>,
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
    input: &PrepareInventoryCountCreation,
    snapshot: &Value,
) -> Result<Uuid, StoreError> {
    let input = serde_json::to_value(input)
        .map_err(|_| StoreError::Invalid("invalid count creation input".into()))?;
    let mut tx = store.pool().begin().await?;
    let id = Uuid::new_v4();
    let inserted=sqlx::query("INSERT INTO business_agent_inventory_count_creation_intents(id,kind,input,snapshot,created_by_user_id,idempotency_key,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(created_by_user_id,idempotency_key) DO NOTHING")
      .bind(id).bind(kind).bind(&input).bind(snapshot).bind(actor).bind(key).bind(trace).execute(&mut *tx).await?.rows_affected();
    let row=sqlx::query("SELECT id,kind,input,snapshot,expires_at>now() current FROM business_agent_inventory_count_creation_intents WHERE created_by_user_id=$1 AND idempotency_key=$2").bind(actor).bind(key).fetch_one(&mut *tx).await?;
    if !row.get::<bool, _>("current")
        || row.get::<String, _>("kind") != kind
        || row.get::<Value, _>("input") != input
        || row.get::<Value, _>("snapshot") != *snapshot
    {
        return Err(StoreError::Conflict);
    }
    let id: Uuid = row.get("id");
    if inserted == 1 {
        sqlx::query("INSERT INTO business_core_audit_events(trace_id,actor_user_id,operation,target_type,target_id,details) VALUES($1,$2,'agent_inventory_count_creation_prepared',$3,$4,$5)").bind(trace).bind(actor).bind(kind).bind(id.to_string()).bind(json!({"previewHash":hash_json(snapshot)})).execute(&mut *tx).await?;
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
    let (_, action) = match family(&kind) {
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
    let mut created = None;
    let mut status = outcome.status.clone();
    if outcome.should_execute {
        match state
            .inventory_count
            .create_approved(
                c.actor_user_id,
                c.trace_id,
                &input,
                &snapshot,
                outcome.request_id,
            )
            .await
        {
            Ok(result) => {
                created = Some(result);
                status = "executed".into();
            }
            Err(_) => {
                let _ = finish_execution(&state.store, outcome.request_id, false).await;
                return approval_error(
                    StatusCode::CONFLICT,
                    "approval_execution_failed",
                    c.trace_id,
                );
            }
        }
    }
    Json(json!({"documentId":id,"documentType":kind,"requestId":outcome.request_id,"status":status,"executed":created.is_some(),"createdDocument":created,"approvalCount":outcome.approval_count,"minimumApprovers":outcome.minimum_approvers,"traceId":c.trace_id})).into_response()
}

async fn dry_preview(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(kind): Path<String>,
    Json(input): Json<PrepareInventoryCountCreation>,
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
    input: &PrepareInventoryCountCreation,
) -> Result<Value, StoreError> {
    family(kind)?;
    state
        .inventory_count
        .creation_preview(actor, input)
        .await
        .map_err(|e| match e {
            crate::b2::DomainError::NotFoundOrForbidden => StoreError::NotFoundOrForbidden,
            crate::b2::DomainError::StalePreview | crate::b2::DomainError::VersionConflict => {
                StoreError::Conflict
            }
            crate::b2::DomainError::Database(e) => StoreError::Database(e),
            _ => StoreError::Invalid("invalid inventory count creation input or state".into()),
        })
}
