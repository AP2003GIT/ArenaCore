use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::{
    api::{error::ApiError, health::health, lobbies, matchmaking, players},
    service::{
        lobby_service::LobbyService, matchmaking_service::MatchmakingService,
        player_service::PlayerService,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub player_service: PlayerService,
    pub lobby_service: LobbyService,
    pub matchmaking_service: MatchmakingService,
}

pub fn build_app(
    player_service: PlayerService,
    lobby_service: LobbyService,
    matchmaking_service: MatchmakingService,
) -> Router {
    let state = AppState {
        player_service,
        lobby_service,
        matchmaking_service,
    };

    Router::new()
        .route("/health", get(health))
        .route("/api/v1/players", post(players::create_player))
        .route("/api/v1/players/{player_id}", get(players::get_player))
        .route("/api/v1/lobbies", post(lobbies::create_lobby))
        .route("/api/v1/lobbies/{lobby_id}", get(lobbies::get_lobby))
        .route("/api/v1/lobbies/{lobby_id}/join", post(lobbies::join_lobby))
        .route(
            "/api/v1/lobbies/{lobby_id}/leave",
            post(lobbies::leave_lobby),
        )
        .route("/api/v1/lobbies/{lobby_id}/ready", post(lobbies::set_ready))
        .route("/api/v1/matchmaking/enqueue", post(matchmaking::enqueue))
        .route(
            "/api/v1/matchmaking/queue/{player_id}",
            delete(matchmaking::cancel),
        )
        .route(
            "/api/v1/matchmaking/status/{player_id}",
            get(matchmaking::get_status),
        )
        .route("/api/v1/matches/{match_id}", get(matchmaking::get_match))
        .fallback(ApiError::route_not_found)
        .with_state(state)
}
