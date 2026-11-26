use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    domain::{
        game_match::{GameMatch, MatchActor, MatchTeam},
        rating::{calculate_match_result, RatingConfig},
    },
    storage::{
        activity::{PlayerActivity, PlayerActivityRegistry},
        matchmaking_memory::InMemoryMatchmakingRepository,
        memory::InMemoryPlayerRepository,
        MatchLifecycleRepository, MatchLifecycleRepositoryError, MatchUpdate,
    },
};

pub struct InMemoryMatchLifecycleRepository {
    players: Arc<InMemoryPlayerRepository>,
    matchmaking: Arc<InMemoryMatchmakingRepository>,
    activities: Arc<PlayerActivityRegistry>,
}

impl InMemoryMatchLifecycleRepository {
    pub fn new(
        players: Arc<InMemoryPlayerRepository>,
        matchmaking: Arc<InMemoryMatchmakingRepository>,
        activities: Arc<PlayerActivityRegistry>,
    ) -> Self {
        Self {
            players,
            matchmaking,
            activities,
        }
    }

    fn release_terminal_match(
        &self,
        game_match: GameMatch,
        activities: &mut std::collections::HashMap<Uuid, PlayerActivity>,
        state: &mut crate::domain::matchmaking::MatchmakingState,
    ) -> Result<(), MatchLifecycleRepositoryError> {
        for player in game_match.players() {
            if activities.get(&player.player_id()) != Some(&PlayerActivity::Match(game_match.id()))
            {
                return Err(MatchLifecycleRepositoryError::InconsistentState);
            }
        }
        for player in game_match.players() {
            activities.remove(&player.player_id());
        }
        state.release_match_players(game_match.id());
        state.replace_match(game_match);
        Ok(())
    }

    fn ensure_match_activities(
        &self,
        game_match: &GameMatch,
        activities: &std::collections::HashMap<Uuid, PlayerActivity>,
    ) -> Result<(), MatchLifecycleRepositoryError> {
        if game_match.players().all(|player| {
            activities.get(&player.player_id()) == Some(&PlayerActivity::Match(game_match.id()))
        }) {
            Ok(())
        } else {
            Err(MatchLifecycleRepositoryError::InconsistentState)
        }
    }
}

#[async_trait]
impl MatchLifecycleRepository for InMemoryMatchLifecycleRepository {
    async fn start_match(
        &self,
        match_id: Uuid,
        actor: MatchActor,
    ) -> Result<MatchUpdate, MatchLifecycleRepositoryError> {
        let mut state = self
            .matchmaking
            .state
            .write()
            .map_err(|_| MatchLifecycleRepositoryError::Unavailable)?;
        let mut game_match = state
            .find_match(match_id)
            .ok_or(MatchLifecycleRepositoryError::MatchNotFound)?;
        let changed = game_match.start(actor)?;
        state.replace_match(game_match.clone());
        Ok(MatchUpdate {
            game_match,
            changed,
        })
    }

    async fn complete_match(
        &self,
        match_id: Uuid,
        actor: MatchActor,
        winning_team: MatchTeam,
        rating_config: RatingConfig,
    ) -> Result<MatchUpdate, MatchLifecycleRepositoryError> {
        let mut activities = self
            .activities
            .activities
            .write()
            .map_err(|_| MatchLifecycleRepositoryError::Unavailable)?;
        let mut state = self
            .matchmaking
            .state
            .write()
            .map_err(|_| MatchLifecycleRepositoryError::Unavailable)?;
        let mut game_match = state
            .find_match(match_id)
            .ok_or(MatchLifecycleRepositoryError::MatchNotFound)?;
        let result = calculate_match_result(&game_match, winning_team, rating_config);
        let changed = game_match.complete(actor, result)?;
        if !changed {
            return Ok(MatchUpdate {
                game_match,
                changed: false,
            });
        }
        self.ensure_match_activities(&game_match, &activities)?;

        let mut players = self
            .players
            .players
            .write()
            .map_err(|_| MatchLifecycleRepositoryError::Unavailable)?;
        let result = game_match
            .result()
            .expect("a completed match always contains a result");
        for change in result.rating_changes() {
            let player = players
                .get(&change.player_id())
                .ok_or(MatchLifecycleRepositoryError::InconsistentState)?;
            if player.rating() != change.rating_before() {
                return Err(MatchLifecycleRepositoryError::InconsistentState);
            }
        }
        for change in result.rating_changes() {
            players
                .get_mut(&change.player_id())
                .expect("player existence was checked")
                .set_rating(change.rating_after());
        }
        self.release_terminal_match(game_match.clone(), &mut activities, &mut state)?;

        Ok(MatchUpdate {
            game_match,
            changed: true,
        })
    }

    async fn cancel_match(
        &self,
        match_id: Uuid,
        actor: MatchActor,
    ) -> Result<MatchUpdate, MatchLifecycleRepositoryError> {
        let mut activities = self
            .activities
            .activities
            .write()
            .map_err(|_| MatchLifecycleRepositoryError::Unavailable)?;
        let mut state = self
            .matchmaking
            .state
            .write()
            .map_err(|_| MatchLifecycleRepositoryError::Unavailable)?;
        let mut game_match = state
            .find_match(match_id)
            .ok_or(MatchLifecycleRepositoryError::MatchNotFound)?;
        let changed = game_match.cancel(actor)?;
        if changed {
            self.release_terminal_match(game_match.clone(), &mut activities, &mut state)?;
        }

        Ok(MatchUpdate {
            game_match,
            changed,
        })
    }
}
