use crate::{
    auth::{bearer, Authenticator},
    model::{CreateChangeRequest, DecisionRequest},
    no_store, Config, Error, Store,
};
use axum::{
    extract::{Path, Query, Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub authenticator: Authenticator,
    pub config: Config,
}

#[derive(Debug, Deserialize)]
struct ChangeQuery {
    status: Option<String>,
}

pub fn router(state: AppState) -> Router {
    let origins = state
        .config
        .allowed_origins
        .iter()
        .filter_map(|value| HeaderValue::from_str(value).ok())
        .collect::<Vec<_>>();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            axum::http::HeaderName::from_static("x-trace-id"),
        ]);
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/api/iam/catalog", get(catalog))
        .route(
            "/api/iam/change-requests",
            get(list_changes).post(create_change),
        )
        .route("/api/iam/change-requests/{id}/approve", post(approve))
        .route("/api/iam/change-requests/{id}/reject", post(reject))
        .with_state(Arc::new(state))
        .layer(middleware::from_fn(security_headers))
        .layer(cors)
        .layer(axum::extract::DefaultBodyLimit::max(32 * 1024))
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    no_store(response.headers_mut());
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
    );
    response
}

async fn live() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn ready(State(state): State<Arc<AppState>>) -> Result<StatusCode, Error> {
    state.store.ready().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn actor(state: &AppState, headers: &HeaderMap) -> Result<crate::model::Actor, Error> {
    state
        .authenticator
        .actor(state.store.pool(), bearer(headers)?)
        .await
}

fn trace_id(headers: &HeaderMap) -> Result<Uuid, Error> {
    headers
        .get("x-trace-id")
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| Uuid::parse_str(value).ok())
                .ok_or(Error::Invalid("invalid_trace_id"))
        })
        .transpose()
        .map(|value| value.unwrap_or_else(Uuid::new_v4))
}

async fn catalog(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<crate::model::CatalogView>, Error> {
    let actor = actor(&state, &headers).await?;
    Ok(Json(state.store.catalog(&actor).await?))
}

async fn list_changes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ChangeQuery>,
) -> Result<Json<Vec<crate::model::ChangeRequestView>>, Error> {
    let actor = actor(&state, &headers).await?;
    Ok(Json(
        state
            .store
            .list_changes(&actor, query.status.as_deref())
            .await?,
    ))
}

async fn create_change(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateChangeRequest>,
) -> Result<(StatusCode, Json<crate::model::ChangeRequestView>), Error> {
    let actor = actor(&state, &headers).await?;
    let change = state
        .store
        .create_change(&actor, request, trace_id(&headers)?)
        .await?;
    Ok((StatusCode::CREATED, Json(change)))
}

async fn approve(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<DecisionRequest>,
) -> Result<Json<crate::model::ChangeRequestView>, Error> {
    let actor = actor(&state, &headers).await?;
    Ok(Json(
        state
            .store
            .approve(&actor, id, request.comment.as_deref(), trace_id(&headers)?)
            .await?,
    ))
}

async fn reject(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<DecisionRequest>,
) -> Result<Json<crate::model::ChangeRequestView>, Error> {
    let actor = actor(&state, &headers).await?;
    Ok(Json(
        state
            .store
            .reject(&actor, id, request.comment.as_deref(), trace_id(&headers)?)
            .await?,
    ))
}
