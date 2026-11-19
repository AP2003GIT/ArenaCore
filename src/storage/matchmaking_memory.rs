use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    domain::{
        game_match::GameMatch,
        matchmaking::{MatchmakingConfig, MatchmakingState, PlayerMatchmakingStatus, QueueEntry},
    },
    storage::{
        activity::{PlayerActivity, PlayerActivityRegistry},
        EnqueueResult, MatchmakingRepository, MatchmakingRepositoryError,
    },
};

pub struct InMemoryMatchmakingRepository {
    state: RwLock<MatchmakingState>,
    activity_registry: Arc<PlayerActivityRegistry>,
}

impl InMemoryMatchmakingRepository {
    pub fn new(activity_registry: Arc<PlayerActivityRegistry>) -> Self {
        Self {
            state: RwLock::new(MatchmakingState::default()),
            activity_registry,
        }
    }
}

#[async_trait]
impl MatchmakingRepository for InMemoryMatchmakingRepository {
    async fn enqueue(
        &self,
        entry: QueueEntry,
        now_seconds: u64,
        config: MatchmakingConfig,
    ) -> Result<EnqueueResult, MatchmakingRepositoryError> {
        let player_id = entry.player_id();
        let mut activities = self
            .activity_registry
            .activities
            .write()
            .map_err(|_| MatchmakingRepositoryError::Unavailable)?;

        match activities.get(&player_id) {
            Some(PlayerActivity::Lobby(_)) => {
                return Err(MatchmakingRepositoryError::PlayerInLobby);
            }
            Some(PlayerActivity::Queued) => {
                return Err(MatchmakingRepositoryError::PlayerAlreadyQueued);
            }
            Some(PlayerActivity::Match(_)) => {
                return Err(MatchmakingRepositoryError::PlayerInMatch);
            }
            None => {}
        }

        let mut state = self
            .state
            .write()
            .map_err(|_| MatchmakingRepositoryError::Unavailable)?;
        state.enqueue(entry)?;
        activities.insert(player_id, PlayerActivity::Queued);

        let matches = state.form_available_matches(now_seconds, config);
        update_matched_activities(&mut activities, &matches);

        Ok(EnqueueResult {
            status: state.status(player_id),
            formed_matches: matches,
        })
    }

    async fn cancel(&self, player_id: Uuid) -> Result<bool, MatchmakingRepositoryError> {
        let mut activities = self
            .activity_registry
            .activities
            .write()
            .map_err(|_| MatchmakingRepositoryError::Unavailable)?;

        if activities.get(&player_id) != Some(&PlayerActivity::Queued) {
            return Ok(false);
        }

        let mut state = self
            .state
            .write()
            .map_err(|_| MatchmakingRepositoryError::Unavailable)?;
        if !state.cancel(player_id) {
            return Err(MatchmakingRepositoryError::InconsistentState);
        }

        activities.remove(&player_id);
        Ok(true)
    }

    async fn status(
        &self,
        player_id: Uuid,
    ) -> Result<PlayerMatchmakingStatus, MatchmakingRepositoryError> {
        let state = self
            .state
            .read()
            .map_err(|_| MatchmakingRepositoryError::Unavailable)?;
        Ok(state.status(player_id))
    }

    async fn find_match(
        &self,
        match_id: Uuid,
    ) -> Result<Option<GameMatch>, MatchmakingRepositoryError> {
        let state = self
            .state
            .read()
            .map_err(|_| MatchmakingRepositoryError::Unavailable)?;
        Ok(state.find_match(match_id))
    }

    async fn run_pending(
        &self,
        now_seconds: u64,
        config: MatchmakingConfig,
    ) -> Result<Vec<GameMatch>, MatchmakingRepositoryError> {
        let mut activities = self
            .activity_registry
            .activities
            .write()
            .map_err(|_| MatchmakingRepositoryError::Unavailable)?;
        let mut state = self
            .state
            .write()
            .map_err(|_| MatchmakingRepositoryError::Unavailable)?;
        let matches = state.form_available_matches(now_seconds, config);
        update_matched_activities(&mut activities, &matches);

        Ok(matches)
    }
}

fn update_matched_activities(
    activities: &mut HashMap<Uuid, PlayerActivity>,
    matches: &[GameMatch],
) {
    for game_match in matches {
        for player in game_match.players() {
            activities.insert(player.player_id(), PlayerActivity::Match(game_match.id()));
        }
    }
}
