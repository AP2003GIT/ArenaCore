use std::sync::Arc;

use axum::{
    middleware,
    routing::{delete, get, post},
    Router,
};

use crate::{
    api::{error::ApiError, health, lobbies, matches, matchmaking, players, sessions, ws},
    config::WebSocketConfig,
    operations::{enforce_rate_limit, observe_requests, OperationalMetrics, RateLimiter},
    service::{
        dev_session_service::DevSessionService, lobby_service::LobbyService,
        match_authority::MatchAuthority, match_lifecycle_service::MatchLifecycleService,
        matchmaking_service::MatchmakingService, notification_service::NotificationHub,
        player_service::PlayerService,
    },
    storage::readiness::ReadinessProbe,
};

#[derive(Clone)]
pub struct AppState {
    pub player_service: PlayerService,
    pub lobby_service: LobbyService,
    pub matchmaking_service: MatchmakingService,
    pub match_lifecycle_service: MatchLifecycleService,
    pub match_authority: MatchAuthority,
    pub dev_session_service: DevSessionService,
    pub notification_hub: Arc<NotificationHub>,
    pub websocket_config: WebSocketConfig,
    pub readiness_probe: Arc<dyn ReadinessProbe>,
    pub metrics: Arc<OperationalMetrics>,
    pub rate_limiter: Arc<RateLimiter>,
}

pub fn build_app(state: AppState) -> Router {
    let api = Router::new()
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
        .route("/api/v1/matches/{match_id}/start", post(matches::start))
        .route(
            "/api/v1/matches/{match_id}/complete",
            post(matches::complete),
        )
        .route("/api/v1/matches/{match_id}/cancel", post(matches::cancel))
        .route("/api/v1/dev/sessions", post(sessions::create_dev_session))
        .route("/api/v1/ws", get(ws::connect))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state.rate_limiter),
            enforce_rate_limit,
        ));
    Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(health::readiness))
        .route("/metrics", get(health::metrics))
        .merge(api)
        .fallback(ApiError::route_not_found)
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state.metrics),
            observe_requests,
        ))
        .with_state(state)
}
