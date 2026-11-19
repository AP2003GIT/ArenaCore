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
}

impl MatchStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
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
pub struct GameMatch {
    id: Uuid,
    mode: MatchMode,
    status: MatchStatus,
    team_a: Vec<MatchPlayer>,
    team_b: Vec<MatchPlayer>,
}

impl GameMatch {
    pub fn new(mode: MatchMode, team_a: Vec<MatchPlayer>, team_b: Vec<MatchPlayer>) -> Self {
        Self {
            id: Uuid::new_v4(),
            mode,
            status: MatchStatus::Ready,
            team_a,
            team_b,
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

    pub fn players(&self) -> impl Iterator<Item = &MatchPlayer> {
        self.team_a.iter().chain(self.team_b.iter())
    }

    pub fn team_a_rating(&self) -> u32 {
        total_rating(&self.team_a)
    }

    pub fn team_b_rating(&self) -> u32 {
        total_rating(&self.team_b)
    }
}

fn total_rating(players: &[MatchPlayer]) -> u32 {
    players.iter().map(|player| u32::from(player.rating)).sum()
}
