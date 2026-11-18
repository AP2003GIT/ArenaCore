use axum::{
    routing::{get, post},
    Router,
};

use crate::{
    api::{error::ApiError, health::health, players},
    service::player_service::PlayerService,
};

#[derive(Clone)]
pub struct AppState {
    pub player_service: PlayerService,
}

pub fn build_app(player_service: PlayerService) -> Router {
    let state = AppState { player_service };

    Router::new()
        .route("/health", get(health))
        .route("/api/v1/players", post(players::create_player))
        .route("/api/v1/players/{player_id}", get(players::get_player))
        .fallback(ApiError::route_not_found)
        .with_state(state)
}
