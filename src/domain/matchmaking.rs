use std::collections::HashMap;

use thiserror::Error;
use uuid::Uuid;

use super::game_match::{GameMatch, MatchMode, MatchPlayer};

type GroupSelectionKey = (u16, u32, Vec<u128>);
type GroupSelection = (GroupSelectionKey, Vec<usize>);
type TeamBalanceKey = (u32, Vec<u128>, Vec<u128>);
type TeamBalanceCandidate = (TeamBalanceKey, Vec<MatchPlayer>, Vec<MatchPlayer>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MatchmakingConfig {
    base_rating_gap: u16,
    rating_gap_growth_per_second: u16,
    max_rating_gap: u16,
}

impl MatchmakingConfig {
    pub fn new(
        base_rating_gap: u16,
        rating_gap_growth_per_second: u16,
        max_rating_gap: u16,
    ) -> Result<Self, MatchmakingConfigError> {
        if base_rating_gap > max_rating_gap {
            return Err(MatchmakingConfigError::BaseExceedsMaximum);
        }

        Ok(Self {
            base_rating_gap,
            rating_gap_growth_per_second,
            max_rating_gap,
        })
    }

    pub fn base_rating_gap(self) -> u16 {
        self.base_rating_gap
    }

    pub fn rating_gap_growth_per_second(self) -> u16 {
        self.rating_gap_growth_per_second
    }

    pub fn max_rating_gap(self) -> u16 {
        self.max_rating_gap
    }

    fn allowed_gap(self, wait_seconds: u64) -> u16 {
        let expanded = u64::from(self.base_rating_gap).saturating_add(
            u64::from(self.rating_gap_growth_per_second).saturating_mul(wait_seconds),
        );
        expanded.min(u64::from(self.max_rating_gap)) as u16
    }
}

impl Default for MatchmakingConfig {
    fn default() -> Self {
        Self {
            base_rating_gap: 100,
            rating_gap_growth_per_second: 10,
            max_rating_gap: 1000,
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum MatchmakingConfigError {
    #[error("base rating gap cannot exceed maximum rating gap")]
    BaseExceedsMaximum,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueEntry {
    player_id: Uuid,
    rating: u16,
    mode: MatchMode,
    enqueued_at_seconds: u64,
}

impl QueueEntry {
    pub fn new(player_id: Uuid, rating: u16, mode: MatchMode, enqueued_at_seconds: u64) -> Self {
        Self {
            player_id,
            rating,
            mode,
            enqueued_at_seconds,
        }
    }

    pub fn player_id(&self) -> Uuid {
        self.player_id
    }

    pub fn rating(&self) -> u16 {
        self.rating
    }

    pub fn mode(&self) -> MatchMode {
        self.mode
    }

    pub fn enqueued_at_seconds(&self) -> u64 {
        self.enqueued_at_seconds
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerMatchmakingStatus {
    Idle,
    Queued { mode: MatchMode },
    Matched { match_id: Uuid, mode: MatchMode },
}

#[derive(Default)]
pub struct MatchmakingState {
    queues: HashMap<MatchMode, Vec<QueueEntry>>,
    queued_players: HashMap<Uuid, MatchMode>,
    matches: HashMap<Uuid, GameMatch>,
    player_matches: HashMap<Uuid, (Uuid, MatchMode)>,
}

impl MatchmakingState {
    pub fn enqueue(&mut self, entry: QueueEntry) -> Result<(), MatchmakingStateError> {
        if self.queued_players.contains_key(&entry.player_id) {
            return Err(MatchmakingStateError::AlreadyQueued);
        }

        if self.player_matches.contains_key(&entry.player_id) {
            return Err(MatchmakingStateError::PlayerInMatch);
        }

        let mode = entry.mode;
        let player_id = entry.player_id;
        let queue = self.queues.entry(mode).or_default();
        queue.push(entry);
        queue.sort_by_key(queue_order_key);
        self.queued_players.insert(player_id, mode);

        Ok(())
    }

    pub fn cancel(&mut self, player_id: Uuid) -> bool {
        let Some(mode) = self.queued_players.remove(&player_id) else {
            return false;
        };

        if let Some(queue) = self.queues.get_mut(&mode) {
            queue.retain(|entry| entry.player_id != player_id);
        }

        true
    }

    pub fn status(&self, player_id: Uuid) -> PlayerMatchmakingStatus {
        if let Some((match_id, mode)) = self.player_matches.get(&player_id) {
            return PlayerMatchmakingStatus::Matched {
                match_id: *match_id,
                mode: *mode,
            };
        }

        if let Some(mode) = self.queued_players.get(&player_id) {
            return PlayerMatchmakingStatus::Queued { mode: *mode };
        }

        PlayerMatchmakingStatus::Idle
    }

    pub fn find_match(&self, match_id: Uuid) -> Option<GameMatch> {
        self.matches.get(&match_id).cloned()
    }

    pub fn form_available_matches(
        &mut self,
        now_seconds: u64,
        config: MatchmakingConfig,
    ) -> Vec<GameMatch> {
        let mut formed = Vec::new();

        for mode in [MatchMode::OneVsOne, MatchMode::TwoVsTwo] {
            while let Some(game_match) = self.form_one_match(mode, now_seconds, config) {
                formed.push(game_match);
            }
        }

        formed
    }

    fn form_one_match(
        &mut self,
        mode: MatchMode,
        now_seconds: u64,
        config: MatchmakingConfig,
    ) -> Option<GameMatch> {
        let selected_entries = {
            let queue = self.queues.get_mut(&mode)?;
            let indices = select_group(queue, mode, now_seconds, config)?;
            remove_entries(queue, &indices)
        };

        let game_match = build_match(mode, &selected_entries);
        for entry in &selected_entries {
            self.queued_players.remove(&entry.player_id);
            self.player_matches
                .insert(entry.player_id, (game_match.id(), game_match.mode()));
        }
        self.matches.insert(game_match.id(), game_match.clone());

        Some(game_match)
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum MatchmakingStateError {
    #[error("player is already queued")]
    AlreadyQueued,
    #[error("player is already in a match")]
    PlayerInMatch,
}

fn select_group(
    queue: &[QueueEntry],
    mode: MatchMode,
    now_seconds: u64,
    config: MatchmakingConfig,
) -> Option<Vec<usize>> {
    if queue.len() < mode.required_players() {
        return None;
    }

    match mode {
        MatchMode::OneVsOne => select_one_vs_one(queue, now_seconds, config),
        MatchMode::TwoVsTwo => select_two_vs_two(queue, now_seconds, config),
    }
}

fn select_one_vs_one(
    queue: &[QueueEntry],
    now_seconds: u64,
    config: MatchmakingConfig,
) -> Option<Vec<usize>> {
    for anchor_index in 0..queue.len() {
        let anchor = &queue[anchor_index];
        let mut best: Option<((u16, u64, u128), usize)> = None;

        for (candidate_index, candidate) in queue.iter().enumerate() {
            if candidate_index == anchor_index {
                continue;
            }

            if !entries_compatible(anchor, candidate, now_seconds, config) {
                continue;
            }

            let key = (
                rating_difference(anchor.rating, candidate.rating),
                candidate.enqueued_at_seconds,
                candidate.player_id.as_u128(),
            );
            if best.as_ref().is_none_or(|(best_key, _)| key < *best_key) {
                best = Some((key, candidate_index));
            }
        }

        if let Some((_, candidate_index)) = best {
            return Some(vec![anchor_index, candidate_index]);
        }
    }

    None
}

fn select_two_vs_two(
    queue: &[QueueEntry],
    now_seconds: u64,
    config: MatchmakingConfig,
) -> Option<Vec<usize>> {
    for anchor_index in 0..queue.len() {
        let candidates: Vec<usize> = (0..queue.len())
            .filter(|index| *index != anchor_index)
            .collect();
        let mut best: Option<GroupSelection> = None;

        for first in 0..candidates.len() {
            for second in (first + 1)..candidates.len() {
                for third in (second + 1)..candidates.len() {
                    let indices = vec![
                        anchor_index,
                        candidates[first],
                        candidates[second],
                        candidates[third],
                    ];
                    let entries: Vec<&QueueEntry> =
                        indices.iter().map(|index| &queue[*index]).collect();

                    if !group_is_compatible(&entries, now_seconds, config) {
                        continue;
                    }

                    let min_rating = entries.iter().map(|entry| entry.rating).min().unwrap();
                    let max_rating = entries.iter().map(|entry| entry.rating).max().unwrap();
                    let anchor_distance = entries
                        .iter()
                        .map(|entry| {
                            u32::from(rating_difference(queue[anchor_index].rating, entry.rating))
                        })
                        .sum();
                    let mut player_ids: Vec<u128> = entries
                        .iter()
                        .map(|entry| entry.player_id.as_u128())
                        .collect();
                    player_ids.sort_unstable();
                    let key = (max_rating - min_rating, anchor_distance, player_ids);

                    if best.as_ref().is_none_or(|(best_key, _)| key < *best_key) {
                        best = Some((key, indices));
                    }
                }
            }
        }

        if let Some((_, indices)) = best {
            return Some(indices);
        }
    }

    None
}

fn entries_compatible(
    first: &QueueEntry,
    second: &QueueEntry,
    now_seconds: u64,
    config: MatchmakingConfig,
) -> bool {
    let first_wait = now_seconds.saturating_sub(first.enqueued_at_seconds);
    let second_wait = now_seconds.saturating_sub(second.enqueued_at_seconds);
    let allowed_gap = config
        .allowed_gap(first_wait)
        .max(config.allowed_gap(second_wait));

    rating_difference(first.rating, second.rating) <= allowed_gap
}

fn group_is_compatible(
    entries: &[&QueueEntry],
    now_seconds: u64,
    config: MatchmakingConfig,
) -> bool {
    for first in 0..entries.len() {
        for second in (first + 1)..entries.len() {
            if !entries_compatible(entries[first], entries[second], now_seconds, config) {
                return false;
            }
        }
    }

    true
}

fn remove_entries(queue: &mut Vec<QueueEntry>, indices: &[usize]) -> Vec<QueueEntry> {
    let mut indices = indices.to_vec();
    indices.sort_unstable_by(|left, right| right.cmp(left));
    let mut entries: Vec<QueueEntry> = indices
        .into_iter()
        .map(|index| queue.remove(index))
        .collect();
    entries.sort_by_key(queue_order_key);
    entries
}

fn build_match(mode: MatchMode, entries: &[QueueEntry]) -> GameMatch {
    match mode {
        MatchMode::OneVsOne => GameMatch::new(
            mode,
            vec![to_match_player(&entries[0])],
            vec![to_match_player(&entries[1])],
        ),
        MatchMode::TwoVsTwo => build_balanced_two_vs_two(entries),
    }
}

fn build_balanced_two_vs_two(entries: &[QueueEntry]) -> GameMatch {
    debug_assert_eq!(entries.len(), 4);
    let partitions = [([0, 1], [2, 3]), ([0, 2], [1, 3]), ([0, 3], [1, 2])];
    let mut best: Option<TeamBalanceCandidate> = None;

    for (team_a_indices, team_b_indices) in partitions {
        let mut team_a: Vec<MatchPlayer> = team_a_indices
            .iter()
            .map(|index| to_match_player(&entries[*index]))
            .collect();
        let mut team_b: Vec<MatchPlayer> = team_b_indices
            .iter()
            .map(|index| to_match_player(&entries[*index]))
            .collect();
        team_a.sort_by_key(|player| player.player_id().as_u128());
        team_b.sort_by_key(|player| player.player_id().as_u128());

        let team_a_rating: u32 = team_a.iter().map(|player| u32::from(player.rating())).sum();
        let team_b_rating: u32 = team_b.iter().map(|player| u32::from(player.rating())).sum();
        let key = (
            team_a_rating.abs_diff(team_b_rating),
            team_a
                .iter()
                .map(|player| player.player_id().as_u128())
                .collect(),
            team_b
                .iter()
                .map(|player| player.player_id().as_u128())
                .collect(),
        );

        if best.as_ref().is_none_or(|(best_key, _, _)| key < *best_key) {
            best = Some((key, team_a, team_b));
        }
    }

    let (_, team_a, team_b) = best.expect("2v2 always has a feasible team partition");
    GameMatch::new(MatchMode::TwoVsTwo, team_a, team_b)
}

fn to_match_player(entry: &QueueEntry) -> MatchPlayer {
    MatchPlayer::new(entry.player_id, entry.rating)
}

fn rating_difference(first: u16, second: u16) -> u16 {
    first.abs_diff(second)
}

fn queue_order_key(entry: &QueueEntry) -> (u64, u128) {
    (entry.enqueued_at_seconds, entry.player_id.as_u128())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rating_gap_expands_with_wait_time() {
        let mut state = MatchmakingState::default();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let config = MatchmakingConfig::default();

        state
            .enqueue(QueueEntry::new(first, 1000, MatchMode::OneVsOne, 0))
            .unwrap();
        state
            .enqueue(QueueEntry::new(second, 1600, MatchMode::OneVsOne, 0))
            .unwrap();

        assert!(state.form_available_matches(0, config).is_empty());
        let matches = state.form_available_matches(60, config);

        assert_eq!(matches.len(), 1);
        assert!(matches[0]
            .players()
            .any(|player| player.player_id() == first));
        assert!(matches[0]
            .players()
            .any(|player| player.player_id() == second));
    }

    #[test]
    fn two_vs_two_balances_team_rating_totals() {
        let mut state = MatchmakingState::default();
        let config = MatchmakingConfig::default();

        for rating in [1000, 1040, 1060, 1100] {
            state
                .enqueue(QueueEntry::new(
                    Uuid::new_v4(),
                    rating,
                    MatchMode::TwoVsTwo,
                    0,
                ))
                .unwrap();
        }

        let matches = state.form_available_matches(0, config);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].team_a_rating(), matches[0].team_b_rating());
    }

    #[test]
    fn cancellation_removes_queue_membership() {
        let mut state = MatchmakingState::default();
        let player_id = Uuid::new_v4();
        state
            .enqueue(QueueEntry::new(player_id, 1200, MatchMode::OneVsOne, 0))
            .unwrap();

        assert!(state.cancel(player_id));
        assert_eq!(state.status(player_id), PlayerMatchmakingStatus::Idle);
        assert!(!state.cancel(player_id));
    }
}
