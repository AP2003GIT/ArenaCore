pub mod lobby_memory;
pub mod memory;

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    lobby::{Lobby, LobbyError},
    player::Player,
};

#[async_trait]
pub trait PlayerRepository: Send + Sync {
    async fn insert(&self, player: Player) -> Result<Player, RepositoryError>;
    async fn find_by_id(&self, player_id: Uuid) -> Result<Option<Player>, RepositoryError>;
}

#[async_trait]
pub trait LobbyRepository: Send + Sync {
    async fn create(&self, lobby: Lobby) -> Result<Lobby, LobbyRepositoryError>;
    async fn find_by_id(&self, lobby_id: Uuid) -> Result<Option<Lobby>, LobbyRepositoryError>;
    async fn join(&self, lobby_id: Uuid, player_id: Uuid) -> Result<Lobby, LobbyRepositoryError>;
    async fn leave(&self, lobby_id: Uuid, player_id: Uuid) -> Result<Lobby, LobbyRepositoryError>;
    async fn set_ready(
        &self,
        lobby_id: Uuid,
        player_id: Uuid,
        ready: bool,
    ) -> Result<Lobby, LobbyRepositoryError>;
}

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("player already exists")]
    AlreadyExists,
    #[error("repository lock is unavailable")]
    Unavailable,
}

#[derive(Debug, Error)]
pub enum LobbyRepositoryError {
    #[error("lobby already exists")]
    AlreadyExists,
    #[error("lobby was not found")]
    LobbyNotFound,
    #[error("player is already a member of another lobby")]
    PlayerAlreadyInLobby,
    #[error("lobby repository lock is unavailable")]
    Unavailable,
    #[error(transparent)]
    Lobby(#[from] LobbyError),
}
