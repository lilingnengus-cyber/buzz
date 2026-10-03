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
                "没有访问此 CRM 记录的权限".to_string(),
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
                tracing::error!(error=%other,trace_id=%self.1,"CRM command failed");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "service_unavailable",
                    "暂时无法保存或读取 CRM 记录，请重试".into(),
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
        .route(
            "/api/v1/crm/opportunities/{id}/convert-customer",
            post(convert_customer),
        )
        .route("/api/v1/crm/owners", get(owners))
        .route("/api/v1/crm/followups", get(followups))
        .route("/api/v1/crm/contacts", get(contacts).post(create_contact))
        .route(
            "/api/v1/crm/contacts/{id}",
            axum::routing::put(update_contact),
        )
        .route("/api/v1/crm/accounts", get(accounts).post(create_account))
        .route(
            "/api/v1/crm/accounts/{id}",
            axum::routing::put(update_account),
        )
        .route(
            "/api/v1/crm/opportunities/{id}",
            get(detail).put(update).delete(delete),
        )
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

async fn accounts(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Query(q): Query<Filters>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .accounts(c.actor_user_id, &q)
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}
async fn create_account(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    h: HeaderMap,
    Json(input): Json<super::SaveAccount>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .save_account(
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
async fn update_account(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(input): Json<super::SaveAccount>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .save_account(
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
async fn create_contact(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    h: HeaderMap,
    Json(input): Json<super::SaveContact>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .save_contact(
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
async fn update_contact(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(input): Json<super::SaveContact>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .save_contact(
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

async fn owners(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Query(q): Query<super::OwnerScope>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .owners(c.actor_user_id, &q)
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}

async fn convert_customer(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<super::ConvertCustomer>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .convert_customer(
            c.actor_user_id,
            c.trace_id,
            id,
            key(&headers, c.trace_id)?,
            &input,
        )
        .await
        .map(Json)
        .map_err(|e| Error(e, c.trace_id))
}

async fn delete(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(input): Json<super::DeleteOpportunity>,
) -> Result<Json<Value>, Error> {
    service(&s)
        .delete(
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
