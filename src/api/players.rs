use axum::{
    extract::{
        rejection::{JsonRejection, PathRejection},
        Path, State,
    },
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{app::AppState, domain::player::Player};

use super::error::ApiError;

#[derive(Deserialize)]
pub struct CreatePlayerRequest {
    display_name: String,
    rating: Option<i32>,
}

#[derive(Deserialize, Serialize)]
pub struct PlayerResponse {
    id: Uuid,
    display_name: String,
    rating: u16,
}

pub async fn create_player(
    State(state): State<AppState>,
    payload: Result<Json<CreatePlayerRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<PlayerResponse>), ApiError> {
    let Json(request) = payload.map_err(|rejection| {
        tracing::debug!(%rejection, "invalid create-player request body");
        ApiError::invalid_json()
    })?;

    let player = state
        .player_service
        .create_player(request.display_name, request.rating)
        .await?;

    Ok((StatusCode::CREATED, Json(PlayerResponse::from(player))))
}

pub async fn get_player(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<PlayerResponse>, ApiError> {
    let Path(player_id) = path.map_err(|rejection| {
        tracing::debug!(%rejection, "invalid player id in request path");
        ApiError::invalid_player_id()
    })?;

    let player = state.player_service.get_player(player_id).await?;

    Ok(Json(PlayerResponse::from(player)))
}

impl From<Player> for PlayerResponse {
    fn from(player: Player) -> Self {
        Self {
            id: player.id(),
            display_name: player.display_name().to_owned(),
            rating: player.rating(),
        }
    }
}
