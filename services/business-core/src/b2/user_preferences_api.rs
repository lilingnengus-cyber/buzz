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
    #[serde(default)]
    pinned: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreferenceResponse {
    context: String,
    business_unit_id: Option<Uuid>,
    pinned: bool,
}

/// Browser routes for the signed-in user's operating-unit defaults.
pub fn browser_routes() -> Router<Arc<AppState>> {
    Router::new().route(
        "/api/v1/preferences/operating-unit/{context}",
        get(get_preference)
            .put(save_preference)
            .delete(clear_preference),
    )
}

async fn clear_preference(
    State(state): State<Arc<AppState>>,
    Extension(context): Extension<RequestContext>,
    Path(preference_context): Path<String>,
) -> Result<Json<PreferenceResponse>, B2ApiError> {
    state
        .user_preferences
        .clear_operating_unit(context.actor_user_id, &preference_context)
        .await
        .map_err(|error| B2ApiError::domain(error, context.trace_id))?;
    Ok(Json(PreferenceResponse {
        context: preference_context,
        business_unit_id: None,
        pinned: false,
    }))
}

async fn get_preference(
    State(state): State<Arc<AppState>>,
    Extension(context): Extension<RequestContext>,
    Path(preference_context): Path<String>,
) -> Result<Json<PreferenceResponse>, B2ApiError> {
    let preference = state
        .user_preferences
        .operating_unit_preference(context.actor_user_id, &preference_context)
        .await
        .map_err(|error| B2ApiError::domain(error, context.trace_id))?;
    Ok(Json(PreferenceResponse {
        context: preference_context,
        business_unit_id: preference.as_ref().map(|item| item.business_unit_id),
        pinned: preference.is_some_and(|item| item.pinned),
    }))
}

async fn save_preference(
    State(state): State<Arc<AppState>>,
    Extension(context): Extension<RequestContext>,
    Path(preference_context): Path<String>,
    Json(input): Json<SavePreference>,
) -> Result<Json<PreferenceResponse>, B2ApiError> {
    let preference = state
        .user_preferences
        .save_operating_unit(
            context.actor_user_id,
            &preference_context,
            input.business_unit_id,
            input.pinned,
        )
        .await
        .map_err(|error| B2ApiError::domain(error, context.trace_id))?;
    Ok(Json(PreferenceResponse {
        context: preference_context,
        business_unit_id: Some(preference.business_unit_id),
        pinned: preference.pinned,
    }))
}
