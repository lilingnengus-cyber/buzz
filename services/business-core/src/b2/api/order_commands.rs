use super::*;

pub(super) async fn create_order(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    h: HeaderMap,
    Json(i): Json<CreateSalesOrder>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    enabled(&s, 0, c.trace_id)?;
    s.sales
        .create_order(c.actor_user_id, c.trace_id, key(&h, c.trace_id)?, &i)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn replace_order(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ReplaceSalesOrderDraft>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    enabled(&s, 0, c.trace_id)?;
    s.sales
        .replace_order_draft(c.actor_user_id, c.trace_id, id, key(&h, c.trace_id)?, &i)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn confirm_order(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<VersionCommand>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.sales
        .confirm_order(c.actor_user_id, c.trace_id, id, key(&h, c.trace_id)?, &i)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn confirmation_preview(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.sales
        .confirmation_preview(c.actor_user_id, id)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn place_hold(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<VersionCommand>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.sales
        .set_hold(
            c.actor_user_id,
            c.trace_id,
            id,
            key(&h, c.trace_id)?,
            &i,
            true,
        )
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn release_hold(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<VersionCommand>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.sales
        .set_hold(
            c.actor_user_id,
            c.trace_id,
            id,
            key(&h, c.trace_id)?,
            &i,
            false,
        )
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn delete_draft(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<VersionCommand>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.sales
        .delete_order_draft(
            c.actor_user_id,
            c.trace_id,
            id,
            key(&h, c.trace_id)?,
            i.expected_version,
        )
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn cancel_remaining(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<VersionCommand>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.sales
        .cancel_remaining(c.actor_user_id, c.trace_id, id, key(&h, c.trace_id)?, &i)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn list_orders(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Query(q): Query<ListQuery>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.sales
        .list_orders(c.actor_user_id, q.limit)
        .await
        .map(|items| {
            Json(json!({"items":items,"dataAsOf":chrono::Utc::now(),"source":"business-core-b2"}))
        })
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn create_opening(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    h: HeaderMap,
    Json(i): Json<CreateInventoryOpening>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.inventory
        .create_opening(c.actor_user_id, c.trace_id, key(&h, c.trace_id)?, &i)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn post_opening(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<VersionCommand>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.inventory
        .post_opening(c.actor_user_id, c.trace_id, id, key(&h, c.trace_id)?, &i)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn reverse_opening(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<VersionCommand>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.inventory
        .reverse_opening(c.actor_user_id, c.trace_id, id, key(&h, c.trace_id)?, &i)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn list_sales_returns(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Query(q): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, B2ApiError> {
    s.returns
        .sales_returns(c.actor_user_id, q.limit)
        .await
        .map(|items| {
            Json(json!({"items":items,"dataAsOf":chrono::Utc::now(),"source":"business-core-b2"}))
        })
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn list_purchase_returns(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Query(q): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, B2ApiError> {
    s.returns
        .purchase_returns(c.actor_user_id, q.limit)
        .await
        .map(|items| {
            Json(json!({"items":items,"dataAsOf":chrono::Utc::now(),"source":"business-core-b3"}))
        })
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn sales_return_options(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.returns
        .sales_options(c.actor_user_id)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn purchase_return_options(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.returns
        .purchase_options(c.actor_user_id)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn create_sales_return(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    h: HeaderMap,
    Json(i): Json<CreateReturn>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.returns
        .create_sales_return(c.actor_user_id, c.trace_id, key(&h, c.trace_id)?, &i)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn create_purchase_return(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    h: HeaderMap,
    Json(i): Json<CreateReturn>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.returns
        .create_purchase_return(c.actor_user_id, c.trace_id, key(&h, c.trace_id)?, &i)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn confirm_sales_return(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<VersionCommand>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.returns
        .confirm_sales_return(c.actor_user_id, c.trace_id, id, key(&h, c.trace_id)?, &i)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn confirm_purchase_return(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<VersionCommand>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.returns
        .confirm_purchase_return(c.actor_user_id, c.trace_id, id, key(&h, c.trace_id)?, &i)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn cancel_sales_return(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<VersionCommand>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.returns
        .cancel_sales_return(c.actor_user_id, c.trace_id, id, key(&h, c.trace_id)?, &i)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
pub(super) async fn cancel_purchase_return(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<VersionCommand>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.returns
        .cancel_purchase_return(c.actor_user_id, c.trace_id, id, key(&h, c.trace_id)?, &i)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}

pub(super) async fn sales_draft_options(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.sales
        .draft_options(c.actor_user_id, id)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
