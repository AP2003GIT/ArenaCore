use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::service::player_service::PlayerServiceError;

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

    pub async fn route_not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "route_not_found",
            message: "the requested route does not exist",
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
                Self {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    code: "internal_error",
                    message: "an internal error occurred",
                }
            }
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
