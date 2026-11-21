use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LobbyStatus {
    Open,
    Ready,
    Closed,
}

impl LobbyStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Ready => "ready",
            Self::Closed => "closed",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "ready" => Some(Self::Ready),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LobbyMember {
    player_id: Uuid,
    ready: bool,
}

impl LobbyMember {
    pub(crate) fn restore(player_id: Uuid, ready: bool) -> Self {
        Self { player_id, ready }
    }

    pub fn player_id(&self) -> Uuid {
        self.player_id
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lobby {
    id: Uuid,
    owner_player_id: Option<Uuid>,
    capacity: u8,
    status: LobbyStatus,
    members: Vec<LobbyMember>,
}

impl Lobby {
    pub fn new(owner_player_id: Uuid, capacity: u8) -> Self {
        Self {
            id: Uuid::new_v4(),
            owner_player_id: Some(owner_player_id),
            capacity,
            status: LobbyStatus::Open,
            members: vec![LobbyMember {
                player_id: owner_player_id,
                ready: false,
            }],
        }
    }

    pub(crate) fn restore(
        id: Uuid,
        owner_player_id: Option<Uuid>,
        capacity: u8,
        status: LobbyStatus,
        members: Vec<LobbyMember>,
    ) -> Self {
        Self {
            id,
            owner_player_id,
            capacity,
            status,
            members,
        }
    }

    pub fn join(&mut self, player_id: Uuid) -> Result<bool, LobbyError> {
        self.ensure_active()?;

        if self.contains_player(player_id) {
            return Ok(false);
        }

        if self.members.len() >= usize::from(self.capacity) {
            return Err(LobbyError::Full);
        }

        self.members.push(LobbyMember {
            player_id,
            ready: false,
        });
        self.refresh_status();

        Ok(true)
    }

    pub fn leave(&mut self, player_id: Uuid) -> Result<(), LobbyError> {
        self.ensure_active()?;

        let member_index = self
            .members
            .iter()
            .position(|member| member.player_id == player_id)
            .ok_or(LobbyError::MemberNotFound)?;

        self.members.remove(member_index);

        if self.members.is_empty() {
            self.owner_player_id = None;
            self.status = LobbyStatus::Closed;
            return Ok(());
        }

        if self.owner_player_id == Some(player_id) {
            self.owner_player_id = self.members.first().map(|member| member.player_id);
        }

        self.refresh_status();
        Ok(())
    }

    pub fn set_ready(&mut self, player_id: Uuid, ready: bool) -> Result<(), LobbyError> {
        self.ensure_active()?;

        let member = self
            .members
            .iter_mut()
            .find(|member| member.player_id == player_id)
            .ok_or(LobbyError::MemberNotFound)?;

        member.ready = ready;
        self.refresh_status();
        Ok(())
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn owner_player_id(&self) -> Option<Uuid> {
        self.owner_player_id
    }

    pub fn capacity(&self) -> u8 {
        self.capacity
    }

    pub fn status(&self) -> LobbyStatus {
        self.status
    }

    pub fn members(&self) -> &[LobbyMember] {
        &self.members
    }

    pub fn contains_player(&self, player_id: Uuid) -> bool {
        self.members
            .iter()
            .any(|member| member.player_id == player_id)
    }

    fn ensure_active(&self) -> Result<(), LobbyError> {
        if self.status == LobbyStatus::Closed {
            return Err(LobbyError::Closed);
        }

        Ok(())
    }

    fn refresh_status(&mut self) {
        self.status = if self.members.len() >= 2 && self.members.iter().all(LobbyMember::is_ready) {
            LobbyStatus::Ready
        } else {
            LobbyStatus::Open
        };
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum LobbyError {
    #[error("lobby is full")]
    Full,
    #[error("lobby is closed")]
    Closed,
    #[error("player is not a lobby member")]
    MemberNotFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_members_ready_transitions_lobby_to_ready() {
        let owner = Uuid::new_v4();
        let member = Uuid::new_v4();
        let mut lobby = Lobby::new(owner, 4);

        lobby.join(member).unwrap();
        lobby.set_ready(owner, true).unwrap();
        assert_eq!(lobby.status(), LobbyStatus::Open);

        lobby.set_ready(member, true).unwrap();
        assert_eq!(lobby.status(), LobbyStatus::Ready);

        lobby.set_ready(member, false).unwrap();
        assert_eq!(lobby.status(), LobbyStatus::Open);
    }

    #[test]
    fn owner_leave_transfers_ownership_to_longest_tenured_member() {
        let owner = Uuid::new_v4();
        let first_member = Uuid::new_v4();
        let second_member = Uuid::new_v4();
        let mut lobby = Lobby::new(owner, 4);

        lobby.join(first_member).unwrap();
        lobby.join(second_member).unwrap();
        lobby.leave(owner).unwrap();

        assert_eq!(lobby.owner_player_id(), Some(first_member));
    }

    #[test]
    fn last_member_leave_closes_lobby() {
        let owner = Uuid::new_v4();
        let mut lobby = Lobby::new(owner, 2);

        lobby.leave(owner).unwrap();

        assert_eq!(lobby.status(), LobbyStatus::Closed);
        assert_eq!(lobby.owner_player_id(), None);
        assert_eq!(lobby.join(Uuid::new_v4()), Err(LobbyError::Closed));
    }
}
