use std::sync::Arc;

use thiserror::Error;
use uuid::Uuid;

use crate::{
    domain::{
        game_match::{GameMatch, MatchLifecycleError, MatchTeam},
        rating::RatingConfig,
    },
    storage::{MatchLifecycleRepository, MatchLifecycleRepositoryError, MatchUpdate},
};

use super::notification_service::{NotificationHub, PlayerEvent};

#[derive(Clone)]
pub struct MatchLifecycleService {
    repository: Arc<dyn MatchLifecycleRepository>,
    rating_config: RatingConfig,
    notifications: Arc<NotificationHub>,
}

impl MatchLifecycleService {
    pub fn new(
        repository: Arc<dyn MatchLifecycleRepository>,
        rating_config: RatingConfig,
        notifications: Arc<NotificationHub>,
    ) -> Self {
        Self {
            repository,
            rating_config,
            notifications,
        }
    }

    pub async fn start(
        &self,
        match_id: Uuid,
        requested_by: Uuid,
    ) -> Result<GameMatch, MatchLifecycleServiceError> {
        let update = self
            .repository
            .start_match(match_id, requested_by)
            .await
            .map_err(map_repository_error)?;
        self.publish_if_changed(&update, PlayerEvent::match_started);
        Ok(update.game_match)
    }

    pub async fn complete(
        &self,
        match_id: Uuid,
        requested_by: Uuid,
        winning_team: MatchTeam,
    ) -> Result<GameMatch, MatchLifecycleServiceError> {
        let update = self
            .repository
            .complete_match(match_id, requested_by, winning_team, self.rating_config)
            .await
            .map_err(map_repository_error)?;
        self.publish_if_changed(&update, PlayerEvent::match_completed);
        Ok(update.game_match)
    }

    pub async fn cancel(
        &self,
        match_id: Uuid,
        requested_by: Uuid,
    ) -> Result<GameMatch, MatchLifecycleServiceError> {
        let update = self
            .repository
            .cancel_match(match_id, requested_by)
            .await
            .map_err(map_repository_error)?;
        self.publish_if_changed(&update, PlayerEvent::match_cancelled);
        Ok(update.game_match)
    }

    fn publish_if_changed(&self, update: &MatchUpdate, event: fn(&GameMatch) -> PlayerEvent) {
        if !update.changed {
            return;
        }
        for player in update.game_match.players() {
            self.notifications
                .publish(player.player_id(), event(&update.game_match));
        }
    }
}

fn map_repository_error(error: MatchLifecycleRepositoryError) -> MatchLifecycleServiceError {
    match error {
        MatchLifecycleRepositoryError::MatchNotFound => MatchLifecycleServiceError::MatchNotFound,
        MatchLifecycleRepositoryError::Domain(MatchLifecycleError::PlayerNotParticipant) => {
            MatchLifecycleServiceError::PlayerNotParticipant
        }
        MatchLifecycleRepositoryError::Domain(MatchLifecycleError::InvalidTransition) => {
            MatchLifecycleServiceError::InvalidTransition
        }
        MatchLifecycleRepositoryError::Domain(MatchLifecycleError::ResultConflict) => {
            MatchLifecycleServiceError::ResultConflict
        }
        other => MatchLifecycleServiceError::Repository(other),
    }
}

#[derive(Debug, Error)]
pub enum MatchLifecycleServiceError {
    #[error("match was not found")]
    MatchNotFound,
    #[error("player is not a participant in this match")]
    PlayerNotParticipant,
    #[error("match cannot perform that transition from its current status")]
    InvalidTransition,
    #[error("match was already completed with a different result")]
    ResultConflict,
    #[error("match lifecycle repository operation failed: {0}")]
    Repository(MatchLifecycleRepositoryError),
}
