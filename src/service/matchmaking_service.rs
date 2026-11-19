use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use thiserror::Error;
use uuid::Uuid;

use crate::{
    domain::{
        game_match::{GameMatch, MatchMode},
        matchmaking::{MatchmakingConfig, PlayerMatchmakingStatus, QueueEntry},
    },
    storage::{
        MatchmakingRepository, MatchmakingRepositoryError, PlayerRepository, RepositoryError,
    },
};

pub trait Clock: Send + Sync {
    fn now_seconds(&self) -> u64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_seconds(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

#[derive(Clone)]
pub struct MatchmakingService {
    player_repository: Arc<dyn PlayerRepository>,
    matchmaking_repository: Arc<dyn MatchmakingRepository>,
    clock: Arc<dyn Clock>,
    config: MatchmakingConfig,
}

impl MatchmakingService {
    pub fn new(
        player_repository: Arc<dyn PlayerRepository>,
        matchmaking_repository: Arc<dyn MatchmakingRepository>,
        clock: Arc<dyn Clock>,
        config: MatchmakingConfig,
    ) -> Self {
        Self {
            player_repository,
            matchmaking_repository,
            clock,
            config,
        }
    }

    pub async fn enqueue(
        &self,
        player_id: Uuid,
        mode: MatchMode,
    ) -> Result<PlayerMatchmakingStatus, MatchmakingServiceError> {
        let player = self
            .player_repository
            .find_by_id(player_id)
            .await
            .map_err(MatchmakingServiceError::PlayerRepository)?
            .ok_or(MatchmakingServiceError::PlayerNotFound)?;
        let now_seconds = self.clock.now_seconds();
        let entry = QueueEntry::new(player_id, player.rating(), mode, now_seconds);

        self.matchmaking_repository
            .enqueue(entry, now_seconds, self.config)
            .await
            .map_err(map_repository_error)
    }

    pub async fn cancel(&self, player_id: Uuid) -> Result<bool, MatchmakingServiceError> {
        self.ensure_player_exists(player_id).await?;
        self.matchmaking_repository
            .cancel(player_id)
            .await
            .map_err(map_repository_error)
    }

    pub async fn status(
        &self,
        player_id: Uuid,
    ) -> Result<PlayerMatchmakingStatus, MatchmakingServiceError> {
        self.ensure_player_exists(player_id).await?;
        self.matchmaking_repository
            .status(player_id)
            .await
            .map_err(map_repository_error)
    }

    pub async fn get_match(&self, match_id: Uuid) -> Result<GameMatch, MatchmakingServiceError> {
        self.matchmaking_repository
            .find_match(match_id)
            .await
            .map_err(map_repository_error)?
            .ok_or(MatchmakingServiceError::MatchNotFound)
    }

    pub async fn run_pending(&self) -> Result<usize, MatchmakingServiceError> {
        let matches = self
            .matchmaking_repository
            .run_pending(self.clock.now_seconds(), self.config)
            .await
            .map_err(map_repository_error)?;
        Ok(matches.len())
    }

    async fn ensure_player_exists(&self, player_id: Uuid) -> Result<(), MatchmakingServiceError> {
        let player = self
            .player_repository
            .find_by_id(player_id)
            .await
            .map_err(MatchmakingServiceError::PlayerRepository)?;

        if player.is_none() {
            return Err(MatchmakingServiceError::PlayerNotFound);
        }

        Ok(())
    }
}

fn map_repository_error(error: MatchmakingRepositoryError) -> MatchmakingServiceError {
    match error {
        MatchmakingRepositoryError::PlayerInLobby => MatchmakingServiceError::PlayerInLobby,
        MatchmakingRepositoryError::PlayerAlreadyQueued => {
            MatchmakingServiceError::PlayerAlreadyQueued
        }
        MatchmakingRepositoryError::PlayerInMatch => MatchmakingServiceError::PlayerInMatch,
        other => MatchmakingServiceError::MatchmakingRepository(other),
    }
}

#[derive(Debug, Error)]
pub enum MatchmakingServiceError {
    #[error("player was not found")]
    PlayerNotFound,
    #[error("match was not found")]
    MatchNotFound,
    #[error("player is currently in a lobby")]
    PlayerInLobby,
    #[error("player is already queued")]
    PlayerAlreadyQueued,
    #[error("player is already in a match")]
    PlayerInMatch,
    #[error("player repository operation failed: {0}")]
    PlayerRepository(RepositoryError),
    #[error("matchmaking repository operation failed: {0}")]
    MatchmakingRepository(MatchmakingRepositoryError),
}
