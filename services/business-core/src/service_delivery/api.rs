//! Browser routes inherit the existing session, CSRF, origin and rate-limit middleware.
use super::{AcceptanceInput, DeliverableInput, Filters, ProjectInput, ServiceDelivery};
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
                "没有访问此 服务交付 记录的权限".to_string(),
            ),
            DomainError::VersionConflict => (
                StatusCode::CONFLICT,
                "VERSION_CONFLICT",
                "记录已更新，请刷新后重试".into(),
            ),
            DomainError::IdempotencyConflict => (
                StatusCode::CONFLICT,
                "IDEMPOTENCY_CONFLICT",
                "请求标识已用于其他内容".into(),
            ),
            DomainError::Invalid(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            other => {
                tracing::error!(error=%other,trace_id=%self.1,"服务交付 command failed");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "service_unavailable",
                    "暂时无法保存或读取 服务交付 记录，请重试".into(),
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
/// Mount service delivery routes inside existing authentication and CSRF middleware.
pub fn browser_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/service-project-options", get(options))
        .route("/api/v1/service-projects", get(list).post(create))
        .route("/api/v1/service-projects/{id}", get(detail).put(update))
        .route(
            "/api/v1/service-projects/{id}/deliverables",
            post(create_task),
        )
        .route(
            "/api/v1/service-projects/{project}/deliverables/{id}",
            axum::routing::put(update_task),
        )
        .route("/api/v1/service-projects/{id}/acceptances", post(accept))
        .route("/api/v1/service-deliverables", get(tasks))
}
fn service(state: &AppState) -> ServiceDelivery {
    ServiceDelivery::new(state.store.clone())
        .with_receivable_prefix(state.receivable_number_prefix.clone())
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
        .list(c.actor_user_id, &q, false)
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}
async fn tasks(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Query(q): Query<Filters>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .list(c.actor_user_id, &q, true)
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}
async fn detail(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .detail(c.actor_user_id, id)
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}
async fn create(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    h: HeaderMap,
    Json(input): Json<ProjectInput>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .save_project(
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
    Json(input): Json<ProjectInput>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .save_project(
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
async fn create_task(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(input): Json<DeliverableInput>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .save_deliverable(
            c.actor_user_id,
            c.trace_id,
            id,
            None,
            key(&h, c.trace_id)?,
            &input,
        )
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}
async fn update_task(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path((project, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(input): Json<DeliverableInput>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .save_deliverable(
            c.actor_user_id,
            c.trace_id,
            project,
            Some(id),
            key(&h, c.trace_id)?,
            &input,
        )
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}
async fn accept(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(input): Json<AcceptanceInput>,
) -> Result<Json<Value>, Error> {
    if input.result == "passed" && !s.b2_enabled[2] {
        return Err(Error(
            DomainError::Invalid("应收功能未启用，无法验收记账".into()),
            c.trace_id,
        ));
    }
    service(&s)
        .accept(
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

#[derive(serde::Deserialize)]
struct OptionsQuery {
    query: Option<String>,
}
async fn options(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Query(q): Query<OptionsQuery>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .options(c.actor_user_id, q.query.as_deref())
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}
