use axum::{
    extract::{
        rejection::{JsonRejection, PathRejection},
        Path, State,
    },
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{app::AppState, domain::game_match::MatchTeam};

use super::{error::ApiError, matchmaking::GameMatchResponse};

#[derive(Deserialize)]
pub struct MatchCommandRequest {
    player_id: Uuid,
}

#[derive(Deserialize)]
pub struct CompleteMatchRequest {
    player_id: Uuid,
    winning_team: String,
}

pub async fn start(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    payload: Result<Json<MatchCommandRequest>, JsonRejection>,
) -> Result<Json<GameMatchResponse>, ApiError> {
    let match_id = parse_match_id(path)?;
    let request = parse_json(payload)?;
    let game_match = state
        .match_lifecycle_service
        .start(match_id, request.player_id)
        .await?;
    Ok(Json(GameMatchResponse::from(game_match)))
}

pub async fn complete(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    payload: Result<Json<CompleteMatchRequest>, JsonRejection>,
) -> Result<Json<GameMatchResponse>, ApiError> {
    let match_id = parse_match_id(path)?;
    let request = parse_json(payload)?;
    let winning_team = match request.winning_team.as_str() {
        "a" => MatchTeam::A,
        "b" => MatchTeam::B,
        _ => return Err(ApiError::invalid_winning_team()),
    };
    let game_match = state
        .match_lifecycle_service
        .complete(match_id, request.player_id, winning_team)
        .await?;
    Ok(Json(GameMatchResponse::from(game_match)))
}

pub async fn cancel(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    payload: Result<Json<MatchCommandRequest>, JsonRejection>,
) -> Result<Json<GameMatchResponse>, ApiError> {
    let match_id = parse_match_id(path)?;
    let request = parse_json(payload)?;
    let game_match = state
        .match_lifecycle_service
        .cancel(match_id, request.player_id)
        .await?;
    Ok(Json(GameMatchResponse::from(game_match)))
}

fn parse_match_id(path: Result<Path<Uuid>, PathRejection>) -> Result<Uuid, ApiError> {
    path.map(|Path(id)| id).map_err(|rejection| {
        tracing::debug!(%rejection, "invalid match id in request path");
        ApiError::invalid_match_id()
    })
}

fn parse_json<T>(payload: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    payload.map(|Json(request)| request).map_err(|rejection| {
        tracing::debug!(%rejection, "invalid match lifecycle request body");
        ApiError::invalid_json()
    })
}
