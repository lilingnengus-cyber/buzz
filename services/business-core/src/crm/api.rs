//! Browser routes inherit the existing session, CSRF, origin and rate-limit middleware.
use super::{AddFollowup, CrmService, Filters, SaveOpportunity};
use crate::{api::AppState, b2::common::DomainError, security::RequestContext};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;
struct Error(DomainError, Uuid);
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, code, message) = match self.0 {
            DomainError::NotFoundOrForbidden => (
                StatusCode::NOT_FOUND,
                "not_found_or_forbidden",
                "没有访问此商机的权限".to_string(),
            ),
            DomainError::VersionConflict => (
                StatusCode::CONFLICT,
                "VERSION_CONFLICT",
                "商机已更新，请刷新后重试".into(),
            ),
            DomainError::IdempotencyConflict => (
                StatusCode::CONFLICT,
                "IDEMPOTENCY_CONFLICT",
                "请求标识已用于其他内容".into(),
            ),
            DomainError::Invalid(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            other => {
                tracing::error!(error=%other,trace_id=%self.1,"CRM command failed");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "service_unavailable",
                    "暂时无法保存或读取商机，请重试".into(),
                )
            }
        };
        (
            status,
            Json(json!({"code":code,"message":message,"traceId":self.1})),
        )
            .into_response()
    }
}
/// Mount within the authenticated Business Core browser surface.
pub fn browser_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/crm/opportunities", get(list).post(create))
        .route("/api/v1/crm/options", get(options))
        .route("/api/v1/crm/followups", get(followups))
        .route("/api/v1/crm/contacts", get(contacts))
        .route("/api/v1/crm/opportunities/{id}", get(detail).put(update))
        .route("/api/v1/crm/opportunities/{id}/followups", post(followup))
}
fn service(state: &AppState) -> CrmService {
    CrmService::new(state.store.clone())
}
fn key(headers: &HeaderMap, trace: Uuid) -> Result<&str, Error> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            Error(
                DomainError::Invalid("Idempotency-Key is required".into()),
                trace,
            )
        })
}
async fn list(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Query(q): Query<Filters>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .list(c.actor_user_id, &q)
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}
async fn options(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .options(c.actor_user_id)
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}
async fn detail(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    Query(q): Query<super::Filters>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .detail(c.actor_user_id, id, q.offset)
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}
async fn create(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    h: HeaderMap,
    Json(input): Json<SaveOpportunity>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .save(
            c.actor_user_id,
            c.trace_id,
            None,
            key(&h, c.trace_id)?,
            &input,
        )
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}
async fn update(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(input): Json<SaveOpportunity>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .save(
            c.actor_user_id,
            c.trace_id,
            Some(id),
            key(&h, c.trace_id)?,
            &input,
        )
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}
async fn followup(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(input): Json<AddFollowup>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .followup(
            c.actor_user_id,
            c.trace_id,
            id,
            key(&h, c.trace_id)?,
            &input,
        )
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}

async fn followups(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Query(q): Query<Filters>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .register(c.actor_user_id, &q, false)
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}
async fn contacts(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Query(q): Query<Filters>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .register(c.actor_user_id, &q, true)
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}

/// Mount read-only CRM lookups behind the existing service authentication boundary.
pub fn service_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/agent-crm-opportunities", get(agent_search))
        .route("/v1/agent-crm-opportunity", get(agent_detail))
}
async fn agent_search(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Query(q): Query<business_query_contracts::SearchCrmOpportunitiesInput>,
) -> Result<Json<Value>, Error> {
    let mut result = service(&s)
        .agent_search(c.actor_user_id, q)
        .await
        .map_err(|e| Error(e, c.trace_id))?;
    result["traceId"] = json!(c.trace_id);
    Ok(Json(result))
}
async fn agent_detail(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Query(q): Query<business_query_contracts::GetCrmOpportunityInput>,
) -> Result<Json<Value>, Error> {
    let mut result = service(&s)
        .agent_detail(c.actor_user_id, q)
        .await
        .map_err(|e| Error(e, c.trace_id))?;
    result["traceId"] = json!(c.trace_id);
    Ok(Json(result))
}
