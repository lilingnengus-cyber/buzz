use super::*;

pub(super) async fn get_order(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> Result<Json<impl serde::Serialize>, B2ApiError> {
    s.sales
        .get_order(c.actor_user_id, id)
        .await
        .map(Json)
        .map_err(|e| B2ApiError::domain(e, c.trace_id))
}
