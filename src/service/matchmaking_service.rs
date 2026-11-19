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

use super::notification_service::{NotificationHub, PlayerEvent};

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
    notifications: Arc<NotificationHub>,
}

impl MatchmakingService {
    pub fn new(
        player_repository: Arc<dyn PlayerRepository>,
        matchmaking_repository: Arc<dyn MatchmakingRepository>,
        clock: Arc<dyn Clock>,
        config: MatchmakingConfig,
        notifications: Arc<NotificationHub>,
    ) -> Self {
        Self {
            player_repository,
            matchmaking_repository,
            clock,
            config,
            notifications,
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

        let result = self
            .matchmaking_repository
            .enqueue(entry, now_seconds, self.config)
            .await
            .map_err(map_repository_error)?;

        self.publish_matchmaking_result(player_id, result.status, &result.formed_matches);
        Ok(result.status)
    }

    pub async fn cancel(&self, player_id: Uuid) -> Result<bool, MatchmakingServiceError> {
        self.ensure_player_exists(player_id).await?;
        let cancelled = self
            .matchmaking_repository
            .cancel(player_id)
            .await
            .map_err(map_repository_error)?;
        self.notifications.publish(
            player_id,
            PlayerEvent::queue_updated(player_id, PlayerMatchmakingStatus::Idle),
        );
        Ok(cancelled)
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
        for game_match in &matches {
            self.publish_match(game_match);
        }
        Ok(matches.len())
    }

    fn publish_matchmaking_result(
        &self,
        requested_player_id: Uuid,
        requested_status: PlayerMatchmakingStatus,
        matches: &[GameMatch],
    ) {
        if matches.is_empty() {
            self.notifications.publish(
                requested_player_id,
                PlayerEvent::queue_updated(requested_player_id, requested_status),
            );
            return;
        }

        let requested_player_matched = matches.iter().any(|game_match| {
            game_match
                .players()
                .any(|player| player.player_id() == requested_player_id)
        });
        if !requested_player_matched {
            self.notifications.publish(
                requested_player_id,
                PlayerEvent::queue_updated(requested_player_id, requested_status),
            );
        }
        for game_match in matches {
            self.publish_match(game_match);
        }
    }

    fn publish_match(&self, game_match: &GameMatch) {
        for player in game_match.players() {
            let player_id = player.player_id();
            self.notifications.publish(
                player_id,
                PlayerEvent::queue_updated(
                    player_id,
                    PlayerMatchmakingStatus::Matched {
                        match_id: game_match.id(),
                        mode: game_match.mode(),
                    },
                ),
            );
            self.notifications
                .publish(player_id, PlayerEvent::match_found(game_match));
        }
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
