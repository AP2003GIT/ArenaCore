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
    domain::lobby::{Lobby, LobbyMember},
};

use super::error::ApiError;

#[derive(Deserialize)]
pub struct CreateLobbyRequest {
    owner_player_id: Uuid,
    capacity: Option<i32>,
}

#[derive(Deserialize)]
pub struct LobbyMembershipRequest {
    player_id: Uuid,
}

#[derive(Deserialize)]
pub struct SetReadyRequest {
    player_id: Uuid,
    ready: bool,
}

#[derive(Serialize)]
pub struct LobbyResponse {
    id: Uuid,
    owner_player_id: Option<Uuid>,
    capacity: u8,
    status: &'static str,
    members: Vec<LobbyMemberResponse>,
}

#[derive(Serialize)]
pub struct LobbyMemberResponse {
    player_id: Uuid,
    ready: bool,
}

pub async fn create_lobby(
    State(state): State<AppState>,
    payload: Result<Json<CreateLobbyRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<LobbyResponse>), ApiError> {
    let request = parse_json(payload)?;
    let lobby = state
        .lobby_service
        .create_lobby(request.owner_player_id, request.capacity)
        .await?;

    Ok((StatusCode::CREATED, Json(LobbyResponse::from(lobby))))
}

pub async fn get_lobby(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<LobbyResponse>, ApiError> {
    let lobby_id = parse_lobby_id(path)?;
    let lobby = state.lobby_service.get_lobby(lobby_id).await?;

    Ok(Json(LobbyResponse::from(lobby)))
}

pub async fn join_lobby(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    payload: Result<Json<LobbyMembershipRequest>, JsonRejection>,
) -> Result<Json<LobbyResponse>, ApiError> {
    let lobby_id = parse_lobby_id(path)?;
    let request = parse_json(payload)?;
    let lobby = state
        .lobby_service
        .join_lobby(lobby_id, request.player_id)
        .await?;

    Ok(Json(LobbyResponse::from(lobby)))
}

pub async fn leave_lobby(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    payload: Result<Json<LobbyMembershipRequest>, JsonRejection>,
) -> Result<Json<LobbyResponse>, ApiError> {
    let lobby_id = parse_lobby_id(path)?;
    let request = parse_json(payload)?;
    let lobby = state
        .lobby_service
        .leave_lobby(lobby_id, request.player_id)
        .await?;

    Ok(Json(LobbyResponse::from(lobby)))
}

pub async fn set_ready(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    payload: Result<Json<SetReadyRequest>, JsonRejection>,
) -> Result<Json<LobbyResponse>, ApiError> {
    let lobby_id = parse_lobby_id(path)?;
    let request = parse_json(payload)?;
    let lobby = state
        .lobby_service
        .set_ready(lobby_id, request.player_id, request.ready)
        .await?;

    Ok(Json(LobbyResponse::from(lobby)))
}

fn parse_lobby_id(path: Result<Path<Uuid>, PathRejection>) -> Result<Uuid, ApiError> {
    path.map(|Path(lobby_id)| lobby_id).map_err(|rejection| {
        tracing::debug!(%rejection, "invalid lobby id in request path");
        ApiError::invalid_lobby_id()
    })
}

fn parse_json<T>(payload: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    payload.map(|Json(request)| request).map_err(|rejection| {
        tracing::debug!(%rejection, "invalid lobby request body");
        ApiError::invalid_json()
    })
}

impl From<Lobby> for LobbyResponse {
    fn from(lobby: Lobby) -> Self {
        Self {
            id: lobby.id(),
            owner_player_id: lobby.owner_player_id(),
            capacity: lobby.capacity(),
            status: lobby.status().as_str(),
            members: lobby
                .members()
                .iter()
                .map(LobbyMemberResponse::from)
                .collect(),
        }
    }
}

impl From<&LobbyMember> for LobbyMemberResponse {
    fn from(member: &LobbyMember) -> Self {
        Self {
            player_id: member.player_id(),
            ready: member.is_ready(),
        }
    }
}
