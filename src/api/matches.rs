use axum::{
    extract::{
        rejection::{JsonRejection, PathRejection},
        Path, State,
    },
    http::{header::AUTHORIZATION, HeaderMap},
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    app::AppState,
    domain::game_match::{MatchActor, MatchTeam},
    service::match_authority::{MatchAuthorizationError, MatchCommand},
};

use super::{error::ApiError, matchmaking::GameMatchResponse};

#[derive(Deserialize)]
pub struct MatchCommandRequest {
    player_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub struct CompleteMatchRequest {
    player_id: Option<Uuid>,
    winning_team: String,
}

pub async fn start(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    headers: HeaderMap,
    payload: Result<Json<MatchCommandRequest>, JsonRejection>,
) -> Result<Json<GameMatchResponse>, ApiError> {
    let match_id = parse_match_id(path)?;
    let request = parse_json(payload)?;
    let actor = authorize(
        &state,
        &headers,
        request.player_id,
        match_id,
        MatchCommand::Start,
    )?;
    let game_match = state.match_lifecycle_service.start(match_id, actor).await?;
    Ok(Json(GameMatchResponse::from(game_match)))
}

pub async fn complete(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    headers: HeaderMap,
    payload: Result<Json<CompleteMatchRequest>, JsonRejection>,
) -> Result<Json<GameMatchResponse>, ApiError> {
    let match_id = parse_match_id(path)?;
    let request = parse_json(payload)?;
    let winning_team = match request.winning_team.as_str() {
        "a" => MatchTeam::A,
        "b" => MatchTeam::B,
        _ => return Err(ApiError::invalid_winning_team()),
    };
    let actor = authorize(
        &state,
        &headers,
        request.player_id,
        match_id,
        MatchCommand::Complete(winning_team),
    )?;
    let game_match = state
        .match_lifecycle_service
        .complete(match_id, actor, winning_team)
        .await?;
    Ok(Json(GameMatchResponse::from(game_match)))
}

pub async fn cancel(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    headers: HeaderMap,
    payload: Result<Json<MatchCommandRequest>, JsonRejection>,
) -> Result<Json<GameMatchResponse>, ApiError> {
    let match_id = parse_match_id(path)?;
    let request = parse_json(payload)?;
    let actor = authorize(
        &state,
        &headers,
        request.player_id,
        match_id,
        MatchCommand::Cancel,
    )?;
    let game_match = state
        .match_lifecycle_service
        .cancel(match_id, actor)
        .await?;
    Ok(Json(GameMatchResponse::from(game_match)))
}

fn authorize(
    state: &AppState,
    headers: &HeaderMap,
    player_id: Option<Uuid>,
    match_id: Uuid,
    command: MatchCommand,
) -> Result<MatchActor, MatchAuthorizationError> {
    let invalid = MatchAuthorizationError::InvalidCredential;
    let mut values = headers.get_all(AUTHORIZATION).iter();
    let value = values
        .next()
        .ok_or(invalid)?
        .to_str()
        .map_err(|_| invalid)?;
    let (scheme, token) = value.split_once(' ').ok_or(invalid)?;
    if values.next().is_some() || !scheme.eq_ignore_ascii_case("Bearer") {
        return Err(invalid);
    }

    if let Some(player_id) = player_id {
        if !state.match_authority.allows_dev_participants() {
            return Err(invalid);
        }
        let token = Uuid::parse_str(token).map_err(|_| invalid)?;
        return state
            .dev_session_service
            .authenticates(token, player_id)
            .then_some(MatchActor::DevelopmentPlayer(player_id))
            .ok_or(invalid);
    }
    state.match_authority.verify(token, match_id, command)
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
