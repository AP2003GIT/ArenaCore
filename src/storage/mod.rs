pub mod activity;
pub mod lobby_memory;
pub mod match_lifecycle_memory;
pub mod matchmaking_memory;
pub mod memory;
pub mod postgres;
pub mod readiness;

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    game_match::{GameMatch, MatchActor, MatchLifecycleError, MatchTeam},
    lobby::{Lobby, LobbyError},
    matchmaking::{MatchmakingConfig, MatchmakingStateError, PlayerMatchmakingStatus, QueueEntry},
    player::Player,
    rating::RatingConfig,
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

#[async_trait]
pub trait MatchmakingRepository: Send + Sync {
    async fn enqueue(
        &self,
        entry: QueueEntry,
        now_seconds: u64,
        config: MatchmakingConfig,
    ) -> Result<EnqueueResult, MatchmakingRepositoryError>;
    async fn cancel(&self, player_id: Uuid) -> Result<bool, MatchmakingRepositoryError>;
    async fn status(
        &self,
        player_id: Uuid,
    ) -> Result<PlayerMatchmakingStatus, MatchmakingRepositoryError>;
    async fn find_match(
        &self,
        match_id: Uuid,
    ) -> Result<Option<GameMatch>, MatchmakingRepositoryError>;
    async fn run_pending(
        &self,
        now_seconds: u64,
        config: MatchmakingConfig,
    ) -> Result<Vec<GameMatch>, MatchmakingRepositoryError>;
}

#[async_trait]
pub trait MatchLifecycleRepository: Send + Sync {
    async fn start_match(
        &self,
        match_id: Uuid,
        actor: MatchActor,
    ) -> Result<MatchUpdate, MatchLifecycleRepositoryError>;
    async fn complete_match(
        &self,
        match_id: Uuid,
        actor: MatchActor,
        winning_team: MatchTeam,
        rating_config: RatingConfig,
    ) -> Result<MatchUpdate, MatchLifecycleRepositoryError>;
    async fn cancel_match(
        &self,
        match_id: Uuid,
        actor: MatchActor,
    ) -> Result<MatchUpdate, MatchLifecycleRepositoryError>;
}

pub struct MatchUpdate {
    pub game_match: GameMatch,
    pub changed: bool,
}

pub struct EnqueueResult {
    pub status: PlayerMatchmakingStatus,
    pub formed_matches: Vec<GameMatch>,
}

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("player already exists")]
    AlreadyExists,
    #[error("repository lock is unavailable")]
    Unavailable,
    #[error("database operation failed")]
    Database(#[source] sqlx::Error),
}

#[derive(Debug, Error)]
pub enum LobbyRepositoryError {
    #[error("lobby already exists")]
    AlreadyExists,
    #[error("lobby was not found")]
    LobbyNotFound,
    #[error("player is already a member of another lobby")]
    PlayerAlreadyInLobby,
    #[error("player is queued for matchmaking")]
    PlayerQueued,
    #[error("player is already in a match")]
    PlayerInMatch,
    #[error("lobby repository lock is unavailable")]
    Unavailable,
    #[error("database operation failed")]
    Database(#[source] sqlx::Error),
    #[error("database contained an invalid lobby state")]
    InvalidStoredState,
    #[error(transparent)]
    Lobby(#[from] LobbyError),
}

#[derive(Debug, Error)]
pub enum MatchmakingRepositoryError {
    #[error("player is currently in a lobby")]
    PlayerInLobby,
    #[error("player is already queued")]
    PlayerAlreadyQueued,
    #[error("player is already in a match")]
    PlayerInMatch,
    #[error("matchmaking state lock is unavailable")]
    Unavailable,
    #[error("player activity and queue state are inconsistent")]
    InconsistentState,
    #[error("database operation failed")]
    Database(#[source] sqlx::Error),
    #[error("database contained an invalid matchmaking state")]
    InvalidStoredState,
    #[error(transparent)]
    State(#[from] MatchmakingStateError),
}

#[derive(Debug, Error)]
pub enum MatchLifecycleRepositoryError {
    #[error("match was not found")]
    MatchNotFound,
    #[error("match lifecycle state is inconsistent")]
    InconsistentState,
    #[error("match lifecycle lock is unavailable")]
    Unavailable,
    #[error("database operation failed")]
    Database(#[source] sqlx::Error),
    #[error("database contained an invalid match lifecycle state")]
    InvalidStoredState,
    #[error(transparent)]
    Domain(#[from] MatchLifecycleError),
}
