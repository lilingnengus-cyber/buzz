//! Immutable operational adjustment posting intents with votes and execution in one transaction.
use super::permission_witness as authority;
use super::*;
use serde_json::Value;
mod command;
use super::snapshot_retry as retry;
mod vote;
use command::Command;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/agent-adjustment-previews/{kind}", post(dry_preview))
        .route("/v1/agent-adjustment-intents/{kind}", post(prepare))
        .route(
            "/v1/agent-approval-previews/adjustments/{kind}/{id}",
            get(preview),
        )
        .route("/v1/agent-approvals/adjustments/{kind}/{id}", post(approve))
}
fn envelope(id: Uuid, kind: &str, snapshot: Value, trace: Uuid) -> Value {
    let hash = hash_json(&snapshot);
    json!({"item":{"id":id,"version":1,"snapshot":snapshot},"document":snapshot,"previewHash":hash,"approvalCommand":format!("确认 {} {id} v1 {hash}",kind.replace('_',"-")),"rejectionCommand":format!("拒绝 {} {id} v1 {hash}",kind.replace('_',"-")),"traceId":trace})
}
async fn prepare(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(kind): Path<String>,
    headers: HeaderMap,
    Json(value): Json<Value>,
) -> Response {
    if !state.b4_enabled[2] {
        return approval_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "feature_disabled",
            c.trace_id,
        );
    }
    let Some(key) = idempotency_key(&headers) else {
        return approval_error(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            c.trace_id,
        );
    };
    let expected = match headers.get("x-business-preflight-hash") {
        Some(value) => match value.to_str() {
            Ok(value) if value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()) => {
                Some(value)
            }
            _ => {
                return approval_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_preflight_hash",
                    c.trace_id,
                )
            }
        },
        None => None,
    };
    match retry::run(|| {
        prepare_on(
            &state,
            c.actor_user_id,
            c.trace_id,
            &kind,
            key,
            value.clone(),
            expected,
        )
    })
    .await
    {
        Ok((id, snapshot)) => Json(envelope(id, &kind, snapshot, c.trace_id)).into_response(),
        Err(e) => store_error(e, c.trace_id),
    }
}
async fn prepare_on(
    state: &AppState,
    actor: Uuid,
    trace: Uuid,
    kind: &str,
    key: &str,
    value: Value,
    expected: Option<&str>,
) -> Result<(Uuid, Value), StoreError> {
    let command = Command::parse(kind, value)?;
    let input = command.value()?;
    let mut tx = state.store.pool().begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await?;
    let snapshot = command
        .preview_on(&state.adjustments, &mut tx, actor)
        .await?;
    if expected.is_some_and(|hash| hash_json(&snapshot) != hash) {
        return Err(StoreError::Conflict);
    }
    let id = Uuid::new_v4();
    let inserted = sqlx::query("INSERT INTO business_agent_adjustment_intents(id,kind,input,snapshot,created_by_user_id,idempotency_key,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(created_by_user_id,idempotency_key) DO NOTHING")
        .bind(id).bind(kind).bind(&input).bind(&snapshot).bind(actor).bind(key).bind(trace).execute(&mut *tx).await?.rows_affected();
    let row = sqlx::query("SELECT id,kind,input,snapshot,expires_at>clock_timestamp() current FROM business_agent_adjustment_intents WHERE created_by_user_id=$1 AND idempotency_key=$2")
        .bind(actor).bind(key).fetch_one(&mut *tx).await?;
    if !row.get::<bool, _>("current")
        || row.get::<String, _>("kind") != kind
        || row.get::<Value, _>("input") != input
        || row.get::<Value, _>("snapshot") != snapshot
    {
        return Err(StoreError::Conflict);
    }
    let id: Uuid = row.get("id");
    if inserted == 1 {
        sqlx::query("INSERT INTO business_core_audit_events(trace_id,actor_user_id,operation,target_type,target_id,details) VALUES($1,$2,'agent_adjustment_intent_prepared',$3,$4,$5)")
            .bind(trace).bind(actor).bind(kind).bind(id.to_string()).bind(json!({"previewHash":hash_json(&snapshot)})).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok((id, snapshot))
}
async fn load(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    kind: &str,
    id: Uuid,
) -> Result<(Command, Value, Uuid), StoreError> {
    let row = sqlx::query("SELECT input,snapshot,created_by_user_id FROM business_agent_adjustment_intents WHERE id=$1 AND kind=$2 AND expires_at>clock_timestamp()")
        .bind(id).bind(kind).fetch_optional(&mut **tx).await?.ok_or(StoreError::NotFoundOrForbidden)?;
    Ok((
        Command::parse(kind, row.get("input"))?,
        row.get("snapshot"),
        row.get("created_by_user_id"),
    ))
}
async fn preview(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path((kind, id)): Path<(String, Uuid)>,
) -> Response {
    if !state.b4_enabled[2] {
        return approval_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "feature_disabled",
            c.trace_id,
        );
    }
    let result = async {
        let mut tx = state.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        let (command, snapshot, creator) = load(&mut tx, &kind, id).await?;
        viewer(&mut tx, c.actor_user_id, &snapshot, &command).await?;
        if command
            .preview_on(&state.adjustments, &mut tx, creator)
            .await?
            != snapshot
        {
            return Err(StoreError::Conflict);
        }
        tx.rollback().await?;
        Ok(envelope(id, &kind, snapshot, c.trace_id))
    }
    .await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => store_error(e, c.trace_id),
    }
}
async fn approve(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path((kind, id)): Path<(String, Uuid)>,
    Json(input): Json<ChatApprovalInput>,
) -> Response {
    if !state.b4_enabled[2] {
        return approval_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "feature_disabled",
            c.trace_id,
        );
    }
    match retry::run(|| vote::execute(&state, (c.actor_user_id, c.trace_id), &kind, id, &input))
        .await
    {
        Ok(value) => Json(value).into_response(),
        Err(e) => store_error(e, c.trace_id),
    }
}

async fn dry_preview(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(kind): Path<String>,
    Json(value): Json<Value>,
) -> Response {
    if !state.b4_enabled[2] {
        return approval_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "feature_disabled",
            c.trace_id,
        );
    }
    let result = async {
        let command = Command::parse(&kind, value)?;
        let mut tx = state.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        let snapshot = command
            .preview_on(&state.adjustments, &mut tx, c.actor_user_id)
            .await?;
        tx.rollback().await?;
        Ok(snapshot)
    }
    .await;
    match result {
        Ok(snapshot) => Json(json!({"document":snapshot,"traceId":c.trace_id})).into_response(),
        Err(e) => store_error(e, c.trace_id),
    }
}

async fn viewer(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: Uuid,
    snapshot: &Value,
    command: &Command,
) -> Result<crate::model::AuthorizationSnapshot, StoreError> {
    let current = crate::master_write_authority::snapshot(tx, actor, command.action(), false)
        .await
        .map_err(command::domain_error)?;
    if !current
        .permission_keys
        .contains(command.preview_permission())
    {
        return Err(StoreError::NotFoundOrForbidden);
    }
    let source: crate::model::DataScopes = serde_json::from_value(snapshot["scope"].clone())
        .map_err(|_| StoreError::NotFoundOrForbidden)?;
    if !source
        .legal_entity_ids
        .is_subset(&current.scopes.legal_entity_ids)
        || !source
            .warehouse_ids
            .is_subset(&current.scopes.warehouse_ids)
        || !source.customer_ids.is_subset(&current.scopes.customer_ids)
        || !source.supplier_ids.is_subset(&current.scopes.supplier_ids)
        || !source.brand_ids.is_subset(&current.scopes.brand_ids)
        || !source
            .business_unit_ids
            .is_subset(&current.scopes.business_unit_ids)
    {
        return Err(StoreError::NotFoundOrForbidden);
    }
    Ok(current)
}
