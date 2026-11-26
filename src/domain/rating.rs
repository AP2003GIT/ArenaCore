use thiserror::Error;

use super::game_match::{GameMatch, MatchResult, MatchTeam, RatingChange};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RatingConfig {
    k_factor: u16,
}

impl RatingConfig {
    pub fn new(k_factor: u16) -> Result<Self, RatingConfigError> {
        if !(1..=128).contains(&k_factor) {
            return Err(RatingConfigError::InvalidKFactor);
        }
        Ok(Self { k_factor })
    }

    pub fn k_factor(self) -> u16 {
        self.k_factor
    }
}

impl Default for RatingConfig {
    fn default() -> Self {
        Self { k_factor: 32 }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RatingConfigError {
    #[error("rating K-factor must be between 1 and 128")]
    InvalidKFactor,
}

pub fn calculate_match_result(
    game_match: &GameMatch,
    winning_team: MatchTeam,
    config: RatingConfig,
) -> MatchResult {
    let team_a_average = average_rating(game_match.team_a());
    let team_b_average = average_rating(game_match.team_b());
    let expected_a = 1.0 / (1.0 + 10_f64.powf((team_b_average - team_a_average) / 400.0));
    let actual_a = if winning_team == MatchTeam::A {
        1.0
    } else {
        0.0
    };
    let team_a_delta = (f64::from(config.k_factor()) * (actual_a - expected_a)).round() as i32;
    let mut changes = Vec::with_capacity(game_match.mode().required_players());

    for player in game_match.team_a() {
        changes.push(rating_change(
            player.player_id(),
            player.rating(),
            team_a_delta,
        ));
    }
    for player in game_match.team_b() {
        changes.push(rating_change(
            player.player_id(),
            player.rating(),
            -team_a_delta,
        ));
    }
    changes.sort_by_key(|change| change.player_id().as_u128());

    MatchResult::new(winning_team, changes)
}

fn average_rating(players: &[super::game_match::MatchPlayer]) -> f64 {
    let total: u32 = players
        .iter()
        .map(|player| u32::from(player.rating()))
        .sum();
    f64::from(total) / players.len() as f64
}

fn rating_change(player_id: uuid::Uuid, before: u16, delta: i32) -> RatingChange {
    let after = (i32::from(before) + delta).clamp(0, 3000) as u16;
    RatingChange::new(player_id, before, after)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::domain::game_match::{MatchMode, MatchPlayer};

    #[test]
    fn equal_players_exchange_sixteen_points_at_k_thirty_two() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let game_match = GameMatch::new(
            MatchMode::OneVsOne,
            vec![MatchPlayer::new(first, 1200)],
            vec![MatchPlayer::new(second, 1200)],
        );

        let result = calculate_match_result(&game_match, MatchTeam::A, RatingConfig::default());

        assert!(result
            .rating_changes()
            .contains(&RatingChange::new(first, 1200, 1216)));
        assert!(result
            .rating_changes()
            .contains(&RatingChange::new(second, 1200, 1184)));
    }

    #[test]
    fn rating_changes_are_clamped_to_supported_range() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let game_match = GameMatch::new(
            MatchMode::OneVsOne,
            vec![MatchPlayer::new(first, 0)],
            vec![MatchPlayer::new(second, 3000)],
        );

        let result = calculate_match_result(&game_match, MatchTeam::B, RatingConfig::default());

        assert!(result
            .rating_changes()
            .contains(&RatingChange::new(first, 0, 0)));
        assert!(result
            .rating_changes()
            .contains(&RatingChange::new(second, 3000, 3000)));
    }
}
