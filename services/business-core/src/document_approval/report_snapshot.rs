//! Immutable management report snapshot intents with votes and execution in one transaction.
use super::permission_witness as authority;
use super::*;
use serde_json::Value;
mod command;
mod retry;
mod vote;
use command::Command;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/v1/agent-report-snapshot-previews/{kind}",
            post(dry_preview),
        )
        .route("/v1/agent-report-snapshot-intents/{kind}", post(prepare))
        .route(
            "/v1/agent-approval-previews/report-snapshots/{kind}/{id}",
            get(preview),
        )
        .route(
            "/v1/agent-approvals/report-snapshots/{kind}/{id}",
            post(approve),
        )
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
        .preview_on(&state.profit_reporting, &mut tx, actor)
        .await?;
    if expected.is_some_and(|hash| hash_json(&snapshot) != hash) {
        return Err(StoreError::Conflict);
    }
    let id = Uuid::new_v4();
    let inserted = sqlx::query("INSERT INTO business_agent_report_snapshot_intents(id,kind,input,snapshot,created_by_user_id,idempotency_key,trace_id) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(created_by_user_id,idempotency_key) DO NOTHING")
        .bind(id).bind(kind).bind(&input).bind(&snapshot).bind(actor).bind(key).bind(trace).execute(&mut *tx).await?.rows_affected();
    let row = sqlx::query("SELECT id,kind,input,snapshot,expires_at>clock_timestamp() current FROM business_agent_report_snapshot_intents WHERE created_by_user_id=$1 AND idempotency_key=$2")
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
        sqlx::query("INSERT INTO business_core_audit_events(trace_id,actor_user_id,operation,target_type,target_id,details) VALUES($1,$2,'agent_report_snapshot_intent_prepared',$3,$4,$5)")
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
    let row = sqlx::query("SELECT input,snapshot,created_by_user_id FROM business_agent_report_snapshot_intents WHERE id=$1 AND kind=$2 AND expires_at>clock_timestamp()")
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
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        let (command, snapshot, _) = load(&mut tx, &kind, id).await?;
        if command
            .preview_on(&state.profit_reporting, &mut tx, c.actor_user_id)
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
    let result = async {
        let command = Command::parse(&kind, value)?;
        let mut tx = state.store.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        let snapshot = command
            .preview_on(&state.profit_reporting, &mut tx, c.actor_user_id)
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
