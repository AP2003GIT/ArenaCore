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

use crate::{
    app::AppState,
    domain::{
        game_match::{GameMatch, MatchMode, MatchPlayer},
        matchmaking::PlayerMatchmakingStatus,
    },
};

use super::error::ApiError;

#[derive(Deserialize)]
pub struct EnqueueRequest {
    player_id: Uuid,
    mode: String,
}

#[derive(Serialize)]
pub struct MatchmakingStatusResponse {
    player_id: Uuid,
    status: &'static str,
    mode: Option<&'static str>,
    match_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct GameMatchResponse {
    id: Uuid,
    mode: &'static str,
    status: &'static str,
    team_a: TeamResponse,
    team_b: TeamResponse,
    result: Option<MatchResultResponse>,
}

#[derive(Serialize)]
pub struct TeamResponse {
    total_rating: u32,
    players: Vec<MatchPlayerResponse>,
}

#[derive(Serialize)]
pub struct MatchPlayerResponse {
    player_id: Uuid,
    rating: u16,
}

#[derive(Serialize)]
pub struct MatchResultResponse {
    winning_team: &'static str,
    rating_changes: Vec<RatingChangeResponse>,
}

#[derive(Serialize)]
pub struct RatingChangeResponse {
    player_id: Uuid,
    rating_before: u16,
    rating_after: u16,
    delta: i16,
}

pub async fn enqueue(
    State(state): State<AppState>,
    payload: Result<Json<EnqueueRequest>, JsonRejection>,
) -> Result<Json<MatchmakingStatusResponse>, ApiError> {
    let request = parse_json(payload)?;
    let mode = parse_mode(&request.mode)?;
    let status = state
        .matchmaking_service
        .enqueue(request.player_id, mode)
        .await?;

    Ok(Json(MatchmakingStatusResponse::new(
        request.player_id,
        status,
    )))
}

pub async fn cancel(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let player_id = parse_uuid_path(path, ApiError::invalid_player_id)?;
    state.matchmaking_service.cancel(player_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_status(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<MatchmakingStatusResponse>, ApiError> {
    let player_id = parse_uuid_path(path, ApiError::invalid_player_id)?;
    let status = state.matchmaking_service.status(player_id).await?;

    Ok(Json(MatchmakingStatusResponse::new(player_id, status)))
}

pub async fn get_match(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<GameMatchResponse>, ApiError> {
    let match_id = parse_uuid_path(path, ApiError::invalid_match_id)?;
    let game_match = state.matchmaking_service.get_match(match_id).await?;

    Ok(Json(GameMatchResponse::from(game_match)))
}

fn parse_mode(mode: &str) -> Result<MatchMode, ApiError> {
    match mode {
        "1v1" => Ok(MatchMode::OneVsOne),
        "2v2" => Ok(MatchMode::TwoVsTwo),
        _ => Err(ApiError::invalid_match_mode()),
    }
}

fn parse_json<T>(payload: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    payload.map(|Json(request)| request).map_err(|rejection| {
        tracing::debug!(%rejection, "invalid matchmaking request body");
        ApiError::invalid_json()
    })
}

fn parse_uuid_path(
    path: Result<Path<Uuid>, PathRejection>,
    error: fn() -> ApiError,
) -> Result<Uuid, ApiError> {
    path.map(|Path(id)| id).map_err(|rejection| {
        tracing::debug!(%rejection, "invalid UUID in request path");
        error()
    })
}

impl MatchmakingStatusResponse {
    fn new(player_id: Uuid, status: PlayerMatchmakingStatus) -> Self {
        match status {
            PlayerMatchmakingStatus::Idle => Self {
                player_id,
                status: "idle",
                mode: None,
                match_id: None,
            },
            PlayerMatchmakingStatus::Queued { mode } => Self {
                player_id,
                status: "queued",
                mode: Some(mode.as_str()),
                match_id: None,
            },
            PlayerMatchmakingStatus::Matched { match_id, mode } => Self {
                player_id,
                status: "matched",
                mode: Some(mode.as_str()),
                match_id: Some(match_id),
            },
        }
    }
}

impl From<GameMatch> for GameMatchResponse {
    fn from(game_match: GameMatch) -> Self {
        Self {
            id: game_match.id(),
            mode: game_match.mode().as_str(),
            status: game_match.status().as_str(),
            team_a: TeamResponse::new(game_match.team_a()),
            team_b: TeamResponse::new(game_match.team_b()),
            result: game_match.result().map(|result| MatchResultResponse {
                winning_team: result.winning_team().as_str(),
                rating_changes: result
                    .rating_changes()
                    .iter()
                    .map(|change| RatingChangeResponse {
                        player_id: change.player_id(),
                        rating_before: change.rating_before(),
                        rating_after: change.rating_after(),
                        delta: change.delta(),
                    })
                    .collect(),
            }),
        }
    }
}

impl TeamResponse {
    fn new(players: &[MatchPlayer]) -> Self {
        Self {
            total_rating: players
                .iter()
                .map(|player| u32::from(player.rating()))
                .sum(),
            players: players.iter().map(MatchPlayerResponse::from).collect(),
        }
    }
}

impl From<&MatchPlayer> for MatchPlayerResponse {
    fn from(player: &MatchPlayer) -> Self {
        Self {
            player_id: player.player_id(),
            rating: player.rating(),
        }
    }
}
