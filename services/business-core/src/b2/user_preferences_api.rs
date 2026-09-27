use super::api::B2ApiError;
use crate::{api::AppState, security::RequestContext};
use axum::{
    extract::{Path, State},
    routing::get,
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SavePreference {
    business_unit_id: Uuid,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreferenceResponse {
    context: String,
    business_unit_id: Option<Uuid>,
}

/// Browser routes for the signed-in user's operating-unit defaults.
pub fn browser_routes() -> Router<Arc<AppState>> {
    Router::new().route(
        "/api/v1/preferences/operating-unit/{context}",
        get(get_preference).put(save_preference),
    )
}

async fn get_preference(
    State(state): State<Arc<AppState>>,
    Extension(context): Extension<RequestContext>,
    Path(preference_context): Path<String>,
) -> Result<Json<PreferenceResponse>, B2ApiError> {
    let business_unit_id = state
        .user_preferences
        .operating_unit(context.actor_user_id, &preference_context)
        .await
        .map_err(|error| B2ApiError::domain(error, context.trace_id))?;
    Ok(Json(PreferenceResponse {
        context: preference_context,
        business_unit_id,
    }))
}

async fn save_preference(
    State(state): State<Arc<AppState>>,
    Extension(context): Extension<RequestContext>,
    Path(preference_context): Path<String>,
    Json(input): Json<SavePreference>,
) -> Result<Json<PreferenceResponse>, B2ApiError> {
    state
        .user_preferences
        .save_operating_unit(
            context.actor_user_id,
            &preference_context,
            input.business_unit_id,
        )
        .await
        .map_err(|error| B2ApiError::domain(error, context.trace_id))?;
    Ok(Json(PreferenceResponse {
        context: preference_context,
        business_unit_id: Some(input.business_unit_id),
    }))
}
