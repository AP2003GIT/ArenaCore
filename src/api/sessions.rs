use axum::{
    extract::{rejection::JsonRejection, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app::AppState;

use super::error::ApiError;

#[derive(Deserialize)]
pub struct CreateDevSessionRequest {
    player_id: Uuid,
}

#[derive(Serialize)]
pub struct DevSessionResponse {
    player_id: Uuid,
    access_token: Uuid,
    token_type: &'static str,
}

pub async fn create_dev_session(
    State(state): State<AppState>,
    payload: Result<Json<CreateDevSessionRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<DevSessionResponse>), ApiError> {
    let request = payload.map(|Json(request)| request).map_err(|rejection| {
        tracing::debug!(%rejection, "invalid development session request body");
        ApiError::invalid_json()
    })?;
    let access_token = state.dev_session_service.issue(request.player_id).await?;

    Ok((
        StatusCode::CREATED,
        Json(DevSessionResponse {
            player_id: request.player_id,
            access_token,
            token_type: "Bearer",
        }),
    ))
}
