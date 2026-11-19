use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::service::{
    lobby_service::LobbyServiceError, matchmaking_service::MatchmakingServiceError,
    player_service::PlayerServiceError,
};

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl ApiError {
    pub fn invalid_json() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_json",
            message: "request body must be valid JSON with an application/json content type",
        }
    }

    pub fn invalid_player_id() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_player_id",
            message: "player_id must be a valid UUID",
        }
    }

    pub fn invalid_lobby_id() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_lobby_id",
            message: "lobby_id must be a valid UUID",
        }
    }

    pub fn invalid_match_id() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_match_id",
            message: "match_id must be a valid UUID",
        }
    }

    pub fn invalid_match_mode() -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "invalid_match_mode",
            message: "mode must be either 1v1 or 2v2",
        }
    }

    pub async fn route_not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "route_not_found",
            message: "the requested route does not exist",
        }
    }
}

impl From<LobbyServiceError> for ApiError {
    fn from(error: LobbyServiceError) -> Self {
        match error {
            LobbyServiceError::InvalidCapacity => Self {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                code: "invalid_lobby_capacity",
                message: "capacity must be an integer from 2 to 8",
            },
            LobbyServiceError::PlayerNotFound => Self {
                status: StatusCode::NOT_FOUND,
                code: "player_not_found",
                message: "player was not found",
            },
            LobbyServiceError::LobbyNotFound => Self {
                status: StatusCode::NOT_FOUND,
                code: "lobby_not_found",
                message: "lobby was not found",
            },
            LobbyServiceError::PlayerAlreadyInLobby => Self {
                status: StatusCode::CONFLICT,
                code: "player_already_in_lobby",
                message: "player is already a member of another lobby",
            },
            LobbyServiceError::PlayerQueued => Self {
                status: StatusCode::CONFLICT,
                code: "player_already_queued",
                message: "player is already queued for matchmaking",
            },
            LobbyServiceError::PlayerInMatch => Self {
                status: StatusCode::CONFLICT,
                code: "player_in_match",
                message: "player is already in a match",
            },
            LobbyServiceError::LobbyFull => Self {
                status: StatusCode::CONFLICT,
                code: "lobby_full",
                message: "lobby is full",
            },
            LobbyServiceError::LobbyClosed => Self {
                status: StatusCode::CONFLICT,
                code: "lobby_closed",
                message: "lobby is closed",
            },
            LobbyServiceError::PlayerNotInLobby => Self {
                status: StatusCode::CONFLICT,
                code: "player_not_in_lobby",
                message: "player is not a member of this lobby",
            },
            LobbyServiceError::PlayerRepository(source) => {
                tracing::error!(%source, "player lookup for lobby operation failed");
                Self::internal()
            }
            LobbyServiceError::LobbyRepository(source) => {
                tracing::error!(%source, "lobby repository operation failed");
                Self::internal()
            }
        }
    }
}

impl From<MatchmakingServiceError> for ApiError {
    fn from(error: MatchmakingServiceError) -> Self {
        match error {
            MatchmakingServiceError::PlayerNotFound => Self {
                status: StatusCode::NOT_FOUND,
                code: "player_not_found",
                message: "player was not found",
            },
            MatchmakingServiceError::MatchNotFound => Self {
                status: StatusCode::NOT_FOUND,
                code: "match_not_found",
                message: "match was not found",
            },
            MatchmakingServiceError::PlayerInLobby => Self {
                status: StatusCode::CONFLICT,
                code: "player_in_lobby",
                message: "player must leave their lobby before queueing",
            },
            MatchmakingServiceError::PlayerAlreadyQueued => Self {
                status: StatusCode::CONFLICT,
                code: "player_already_queued",
                message: "player is already queued for matchmaking",
            },
            MatchmakingServiceError::PlayerInMatch => Self {
                status: StatusCode::CONFLICT,
                code: "player_in_match",
                message: "player is already in a match",
            },
            MatchmakingServiceError::PlayerRepository(source) => {
                tracing::error!(%source, "player lookup for matchmaking failed");
                Self::internal()
            }
            MatchmakingServiceError::MatchmakingRepository(source) => {
                tracing::error!(%source, "matchmaking repository operation failed");
                Self::internal()
            }
        }
    }
}

impl From<PlayerServiceError> for ApiError {
    fn from(error: PlayerServiceError) -> Self {
        match error {
            PlayerServiceError::InvalidDisplayName => Self {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                code: "invalid_display_name",
                message: "display_name must contain between 3 and 24 Unicode characters",
            },
            PlayerServiceError::InvalidRating => Self {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                code: "invalid_rating",
                message: "rating must be an integer from 0 to 3000",
            },
            PlayerServiceError::PlayerNotFound => Self {
                status: StatusCode::NOT_FOUND,
                code: "player_not_found",
                message: "player was not found",
            },
            PlayerServiceError::Repository(source) => {
                tracing::error!(%source, "player repository operation failed");
                Self::internal()
            }
        }
    }
}

impl ApiError {
    fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: "an internal error occurred",
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorResponse {
            error: ErrorBody {
                code: self.code,
                message: self.message,
            },
        };

        (self.status, Json(body)).into_response()
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
}
