use std::sync::Arc;

use thiserror::Error;
use uuid::Uuid;

use crate::{
    domain::lobby::{Lobby, LobbyError},
    storage::{LobbyRepository, LobbyRepositoryError, PlayerRepository, RepositoryError},
};

const DEFAULT_CAPACITY: i32 = 4;
const MIN_CAPACITY: i32 = 2;
const MAX_CAPACITY: i32 = 8;

#[derive(Clone)]
pub struct LobbyService {
    player_repository: Arc<dyn PlayerRepository>,
    lobby_repository: Arc<dyn LobbyRepository>,
}

impl LobbyService {
    pub fn new(
        player_repository: Arc<dyn PlayerRepository>,
        lobby_repository: Arc<dyn LobbyRepository>,
    ) -> Self {
        Self {
            player_repository,
            lobby_repository,
        }
    }

    pub async fn create_lobby(
        &self,
        owner_player_id: Uuid,
        capacity: Option<i32>,
    ) -> Result<Lobby, LobbyServiceError> {
        self.ensure_player_exists(owner_player_id).await?;
        let capacity = capacity.unwrap_or(DEFAULT_CAPACITY);

        if !(MIN_CAPACITY..=MAX_CAPACITY).contains(&capacity) {
            return Err(LobbyServiceError::InvalidCapacity);
        }

        self.lobby_repository
            .create(Lobby::new(owner_player_id, capacity as u8))
            .await
            .map_err(map_repository_error)
    }

    pub async fn get_lobby(&self, lobby_id: Uuid) -> Result<Lobby, LobbyServiceError> {
        self.lobby_repository
            .find_by_id(lobby_id)
            .await
            .map_err(map_repository_error)?
            .ok_or(LobbyServiceError::LobbyNotFound)
    }

    pub async fn join_lobby(
        &self,
        lobby_id: Uuid,
        player_id: Uuid,
    ) -> Result<Lobby, LobbyServiceError> {
        self.ensure_player_exists(player_id).await?;
        self.lobby_repository
            .join(lobby_id, player_id)
            .await
            .map_err(map_repository_error)
    }

    pub async fn leave_lobby(
        &self,
        lobby_id: Uuid,
        player_id: Uuid,
    ) -> Result<Lobby, LobbyServiceError> {
        self.ensure_player_exists(player_id).await?;
        self.lobby_repository
            .leave(lobby_id, player_id)
            .await
            .map_err(map_repository_error)
    }

    pub async fn set_ready(
        &self,
        lobby_id: Uuid,
        player_id: Uuid,
        ready: bool,
    ) -> Result<Lobby, LobbyServiceError> {
        self.ensure_player_exists(player_id).await?;
        self.lobby_repository
            .set_ready(lobby_id, player_id, ready)
            .await
            .map_err(map_repository_error)
    }

    async fn ensure_player_exists(&self, player_id: Uuid) -> Result<(), LobbyServiceError> {
        let player = self
            .player_repository
            .find_by_id(player_id)
            .await
            .map_err(LobbyServiceError::PlayerRepository)?;

        if player.is_none() {
            return Err(LobbyServiceError::PlayerNotFound);
        }

        Ok(())
    }
}

fn map_repository_error(error: LobbyRepositoryError) -> LobbyServiceError {
    match error {
        LobbyRepositoryError::LobbyNotFound => LobbyServiceError::LobbyNotFound,
        LobbyRepositoryError::PlayerAlreadyInLobby => LobbyServiceError::PlayerAlreadyInLobby,
        LobbyRepositoryError::Lobby(LobbyError::Full) => LobbyServiceError::LobbyFull,
        LobbyRepositoryError::Lobby(LobbyError::Closed) => LobbyServiceError::LobbyClosed,
        LobbyRepositoryError::Lobby(LobbyError::MemberNotFound) => {
            LobbyServiceError::PlayerNotInLobby
        }
        other => LobbyServiceError::LobbyRepository(other),
    }
}

#[derive(Debug, Error)]
pub enum LobbyServiceError {
    #[error("lobby capacity is invalid")]
    InvalidCapacity,
    #[error("player was not found")]
    PlayerNotFound,
    #[error("lobby was not found")]
    LobbyNotFound,
    #[error("player is already in a lobby")]
    PlayerAlreadyInLobby,
    #[error("lobby is full")]
    LobbyFull,
    #[error("lobby is closed")]
    LobbyClosed,
    #[error("player is not in this lobby")]
    PlayerNotInLobby,
    #[error("player repository operation failed: {0}")]
    PlayerRepository(RepositoryError),
    #[error("lobby repository operation failed: {0}")]
    LobbyRepository(LobbyRepositoryError),
}
