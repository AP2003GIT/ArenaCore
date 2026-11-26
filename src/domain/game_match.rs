use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MatchMode {
    OneVsOne,
    TwoVsTwo,
}

impl MatchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OneVsOne => "1v1",
            Self::TwoVsTwo => "2v2",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "1v1" => Some(Self::OneVsOne),
            "2v2" => Some(Self::TwoVsTwo),
            _ => None,
        }
    }

    pub fn required_players(self) -> usize {
        match self {
            Self::OneVsOne => 2,
            Self::TwoVsTwo => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchStatus {
    Ready,
    Active,
    Completed,
    Cancelled,
}

impl MatchStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "ready" => Some(Self::Ready),
            "active" => Some(Self::Active),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchTeam {
    A,
    B,
}

impl MatchTeam {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
        }
    }

    pub fn number(self) -> i16 {
        match self {
            Self::A => 1,
            Self::B => 2,
        }
    }

    pub(crate) fn parse_number(value: i16) -> Option<Self> {
        match value {
            1 => Some(Self::A),
            2 => Some(Self::B),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchPlayer {
    player_id: Uuid,
    rating: u16,
}

impl MatchPlayer {
    pub fn new(player_id: Uuid, rating: u16) -> Self {
        Self { player_id, rating }
    }

    pub fn player_id(&self) -> Uuid {
        self.player_id
    }

    pub fn rating(&self) -> u16 {
        self.rating
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatingChange {
    player_id: Uuid,
    rating_before: u16,
    rating_after: u16,
}

impl RatingChange {
    pub fn new(player_id: Uuid, rating_before: u16, rating_after: u16) -> Self {
        Self {
            player_id,
            rating_before,
            rating_after,
        }
    }

    pub fn player_id(&self) -> Uuid {
        self.player_id
    }

    pub fn rating_before(&self) -> u16 {
        self.rating_before
    }

    pub fn rating_after(&self) -> u16 {
        self.rating_after
    }

    pub fn delta(&self) -> i16 {
        let difference = i32::from(self.rating_after) - i32::from(self.rating_before);
        i16::try_from(difference).expect("rating delta always fits in i16")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchResult {
    winning_team: MatchTeam,
    rating_changes: Vec<RatingChange>,
}

impl MatchResult {
    pub fn new(winning_team: MatchTeam, rating_changes: Vec<RatingChange>) -> Self {
        Self {
            winning_team,
            rating_changes,
        }
    }

    pub fn winning_team(&self) -> MatchTeam {
        self.winning_team
    }

    pub fn rating_changes(&self) -> &[RatingChange] {
        &self.rating_changes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameMatch {
    id: Uuid,
    mode: MatchMode,
    status: MatchStatus,
    team_a: Vec<MatchPlayer>,
    team_b: Vec<MatchPlayer>,
    result: Option<MatchResult>,
}

impl GameMatch {
    pub fn new(mode: MatchMode, team_a: Vec<MatchPlayer>, team_b: Vec<MatchPlayer>) -> Self {
        Self {
            id: Uuid::new_v4(),
            mode,
            status: MatchStatus::Ready,
            team_a,
            team_b,
            result: None,
        }
    }

    pub(crate) fn restore(
        id: Uuid,
        mode: MatchMode,
        status: MatchStatus,
        team_a: Vec<MatchPlayer>,
        team_b: Vec<MatchPlayer>,
        result: Option<MatchResult>,
    ) -> Self {
        Self {
            id,
            mode,
            status,
            team_a,
            team_b,
            result,
        }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn mode(&self) -> MatchMode {
        self.mode
    }

    pub fn status(&self) -> MatchStatus {
        self.status
    }

    pub fn team_a(&self) -> &[MatchPlayer] {
        &self.team_a
    }

    pub fn team_b(&self) -> &[MatchPlayer] {
        &self.team_b
    }

    pub fn result(&self) -> Option<&MatchResult> {
        self.result.as_ref()
    }

    pub fn players(&self) -> impl Iterator<Item = &MatchPlayer> {
        self.team_a.iter().chain(self.team_b.iter())
    }

    pub fn contains_player(&self, player_id: Uuid) -> bool {
        self.players().any(|player| player.player_id() == player_id)
    }

    pub fn team_a_rating(&self) -> u32 {
        total_rating(&self.team_a)
    }

    pub fn team_b_rating(&self) -> u32 {
        total_rating(&self.team_b)
    }

    pub fn start(&mut self, requested_by: Uuid) -> Result<bool, MatchLifecycleError> {
        self.ensure_participant(requested_by)?;
        match self.status {
            MatchStatus::Ready => {
                self.status = MatchStatus::Active;
                Ok(true)
            }
            MatchStatus::Active => Ok(false),
            MatchStatus::Completed | MatchStatus::Cancelled => {
                Err(MatchLifecycleError::InvalidTransition)
            }
        }
    }

    pub fn complete(
        &mut self,
        requested_by: Uuid,
        result: MatchResult,
    ) -> Result<bool, MatchLifecycleError> {
        self.ensure_participant(requested_by)?;
        match self.status {
            MatchStatus::Active => {
                self.status = MatchStatus::Completed;
                self.result = Some(result);
                Ok(true)
            }
            MatchStatus::Completed => {
                if self.result.as_ref().map(MatchResult::winning_team)
                    == Some(result.winning_team())
                {
                    Ok(false)
                } else {
                    Err(MatchLifecycleError::ResultConflict)
                }
            }
            MatchStatus::Ready | MatchStatus::Cancelled => {
                Err(MatchLifecycleError::InvalidTransition)
            }
        }
    }

    pub fn cancel(&mut self, requested_by: Uuid) -> Result<bool, MatchLifecycleError> {
        self.ensure_participant(requested_by)?;
        match self.status {
            MatchStatus::Ready | MatchStatus::Active => {
                self.status = MatchStatus::Cancelled;
                Ok(true)
            }
            MatchStatus::Cancelled => Ok(false),
            MatchStatus::Completed => Err(MatchLifecycleError::InvalidTransition),
        }
    }

    fn ensure_participant(&self, player_id: Uuid) -> Result<(), MatchLifecycleError> {
        if self.contains_player(player_id) {
            Ok(())
        } else {
            Err(MatchLifecycleError::PlayerNotParticipant)
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum MatchLifecycleError {
    #[error("player is not a participant in this match")]
    PlayerNotParticipant,
    #[error("match cannot perform that transition from its current status")]
    InvalidTransition,
    #[error("match was already completed with a different result")]
    ResultConflict,
}

fn total_rating(players: &[MatchPlayer]) -> u32 {
    players.iter().map(|player| u32::from(player.rating)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game_match() -> (GameMatch, Uuid, Uuid) {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        (
            GameMatch::new(
                MatchMode::OneVsOne,
                vec![MatchPlayer::new(first, 1200)],
                vec![MatchPlayer::new(second, 1200)],
            ),
            first,
            second,
        )
    }

    #[test]
    fn lifecycle_is_idempotent_but_rejects_conflicting_result() {
        let (mut game_match, first, second) = game_match();
        assert!(game_match.start(first).unwrap());
        assert!(!game_match.start(second).unwrap());

        let result = MatchResult::new(MatchTeam::A, Vec::new());
        assert!(game_match.complete(first, result.clone()).unwrap());
        assert!(!game_match.complete(second, result).unwrap());
        assert_eq!(game_match.status(), MatchStatus::Completed);
        assert!(matches!(
            game_match.complete(second, MatchResult::new(MatchTeam::B, Vec::new())),
            Err(MatchLifecycleError::ResultConflict)
        ));
    }

    #[test]
    fn non_participant_cannot_mutate_match() {
        let (mut game_match, _, _) = game_match();

        assert!(matches!(
            game_match.start(Uuid::new_v4()),
            Err(MatchLifecycleError::PlayerNotParticipant)
        ));
    }
}
