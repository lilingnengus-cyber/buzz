//! Immutable master intents with votes and execution in one transaction.
use super::*;
use serde_json::Value;
mod authority;
mod command;
mod vote;
use command::Command;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/agent-master-intents/{kind}", post(prepare))
        .route(
            "/v1/agent-approval-previews/master/{kind}/{id}",
            get(preview),
        )
        .route("/v1/agent-approvals/master/{kind}/{id}", post(approve))
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
    let Some(key) = idempotency_key(&headers) else {
        return approval_error(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            c.trace_id,
        );
    };
    match prepare_on(&state.store, c.actor_user_id, c.trace_id, &kind, key, value).await {
        Ok((id, snapshot)) => Json(envelope(id, &kind, snapshot, c.trace_id)).into_response(),
        Err(e) => store_error(e, c.trace_id),
    }
}
async fn prepare_on(
    store: &PgStore,
    actor: Uuid,
    trace: Uuid,
    kind: &str,
    key: &str,
    value: Value,
) -> Result<(Uuid, Value), StoreError> {
    let command = Command::parse(kind, value)?;
    let input = command.value()?;
    let mut tx = store.pool().begin().await?;
    let snapshot = command.preview_on(store, &mut tx, actor).await?;
    let id = Uuid::new_v4();
    let inserted = sqlx::query("INSERT INTO business_agent_master_intents(id,kind,input,snapshot,created_by_user_id,idempotency_key,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(created_by_user_id,idempotency_key) DO NOTHING")
        .bind(id).bind(kind).bind(&input).bind(&snapshot).bind(actor).bind(key).bind(trace).execute(&mut *tx).await?.rows_affected();
    let row = sqlx::query("SELECT id,kind,input,snapshot,expires_at>clock_timestamp() current FROM business_agent_master_intents WHERE created_by_user_id=$1 AND idempotency_key=$2")
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
        sqlx::query("INSERT INTO business_core_audit_events(trace_id,actor_user_id,operation,target_type,target_id,details) VALUES($1,$2,'agent_master_intent_prepared',$3,$4,$5)")
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
    let row = sqlx::query("SELECT input,snapshot,created_by_user_id FROM business_agent_master_intents WHERE id=$1 AND kind=$2 AND expires_at>clock_timestamp()")
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
    let result = async {
        let mut tx = state.store.pool().begin().await?;
        let (command, snapshot, _) = load(&mut tx, &kind, id).await?;
        if command
            .preview_on(&state.store, &mut tx, c.actor_user_id)
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
    match vote::execute(
        &state.store,
        (c.actor_user_id, c.trace_id),
        &kind,
        id,
        &input,
    )
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(e) => store_error(e, c.trace_id),
    }
}
