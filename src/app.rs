use std::sync::Arc;

use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::{
    api::{error::ApiError, health::health, lobbies, matchmaking, players, sessions, ws},
    config::WebSocketConfig,
    service::{
        dev_session_service::DevSessionService, lobby_service::LobbyService,
        matchmaking_service::MatchmakingService, notification_service::NotificationHub,
        player_service::PlayerService,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub player_service: PlayerService,
    pub lobby_service: LobbyService,
    pub matchmaking_service: MatchmakingService,
    pub dev_session_service: DevSessionService,
    pub notification_hub: Arc<NotificationHub>,
    pub websocket_config: WebSocketConfig,
}

pub fn build_app(state: AppState) -> Router {
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
        .route("/api/v1/dev/sessions", post(sessions::create_dev_session))
        .route("/api/v1/ws", get(ws::connect))
        .fallback(ApiError::route_not_found)
        .with_state(state)
}
