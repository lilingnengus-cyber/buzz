use super::*;

pub(super) async fn get_order(
    State(s): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> Result<Json<impl serde::Serialize>, B3ApiError> {
    enabled(&s, 0, c.trace_id)?;
    s.purchasing
        .get_order(c.actor_user_id, id)
        .await
        .map(Json)
        .map_err(|e| B3ApiError::domain(e, c.trace_id))
}
