use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    domain::lobby::Lobby,
    storage::{
        activity::{PlayerActivity, PlayerActivityRegistry},
        LobbyRepository, LobbyRepositoryError,
    },
};

pub struct InMemoryLobbyRepository {
    state: RwLock<LobbyStore>,
    activity_registry: Arc<PlayerActivityRegistry>,
}

#[derive(Default)]
struct LobbyStore {
    lobbies: HashMap<Uuid, Lobby>,
    player_lobbies: HashMap<Uuid, Uuid>,
}

impl InMemoryLobbyRepository {
    pub fn new(activity_registry: Arc<PlayerActivityRegistry>) -> Self {
        Self {
            state: RwLock::new(LobbyStore::default()),
            activity_registry,
        }
    }
}

#[async_trait]
impl LobbyRepository for InMemoryLobbyRepository {
    async fn create(&self, lobby: Lobby) -> Result<Lobby, LobbyRepositoryError> {
        let mut activities = self
            .activity_registry
            .activities
            .write()
            .map_err(|_| LobbyRepositoryError::Unavailable)?;
        let mut store = self
            .state
            .write()
            .map_err(|_| LobbyRepositoryError::Unavailable)?;
        let lobby_id = lobby.id();
        let owner_player_id = lobby
            .owner_player_id()
            .expect("a new lobby always has an owner");

        if store.lobbies.contains_key(&lobby_id) {
            return Err(LobbyRepositoryError::AlreadyExists);
        }

        match activities.get(&owner_player_id) {
            Some(PlayerActivity::Lobby(_)) => {
                return Err(LobbyRepositoryError::PlayerAlreadyInLobby);
            }
            Some(PlayerActivity::Queued) => return Err(LobbyRepositoryError::PlayerQueued),
            Some(PlayerActivity::Match(_)) => return Err(LobbyRepositoryError::PlayerInMatch),
            None => {}
        }

        activities.insert(owner_player_id, PlayerActivity::Lobby(lobby_id));
        store.player_lobbies.insert(owner_player_id, lobby_id);
        store.lobbies.insert(lobby_id, lobby.clone());

        Ok(lobby)
    }

    async fn find_by_id(&self, lobby_id: Uuid) -> Result<Option<Lobby>, LobbyRepositoryError> {
        let store = self
            .state
            .read()
            .map_err(|_| LobbyRepositoryError::Unavailable)?;

        Ok(store.lobbies.get(&lobby_id).cloned())
    }

    async fn join(&self, lobby_id: Uuid, player_id: Uuid) -> Result<Lobby, LobbyRepositoryError> {
        let mut activities = self
            .activity_registry
            .activities
            .write()
            .map_err(|_| LobbyRepositoryError::Unavailable)?;
        let mut store = self
            .state
            .write()
            .map_err(|_| LobbyRepositoryError::Unavailable)?;

        if let Some(activity) = activities.get(&player_id) {
            if *activity == PlayerActivity::Lobby(lobby_id) {
                return store
                    .lobbies
                    .get(&lobby_id)
                    .cloned()
                    .ok_or(LobbyRepositoryError::LobbyNotFound);
            }

            return match activity {
                PlayerActivity::Lobby(_) => Err(LobbyRepositoryError::PlayerAlreadyInLobby),
                PlayerActivity::Queued => Err(LobbyRepositoryError::PlayerQueued),
                PlayerActivity::Match(_) => Err(LobbyRepositoryError::PlayerInMatch),
            };
        }

        let updated_lobby = {
            let lobby = store
                .lobbies
                .get_mut(&lobby_id)
                .ok_or(LobbyRepositoryError::LobbyNotFound)?;
            lobby.join(player_id)?;
            lobby.clone()
        };

        activities.insert(player_id, PlayerActivity::Lobby(lobby_id));
        store.player_lobbies.insert(player_id, lobby_id);
        Ok(updated_lobby)
    }

    async fn leave(&self, lobby_id: Uuid, player_id: Uuid) -> Result<Lobby, LobbyRepositoryError> {
        let mut activities = self
            .activity_registry
            .activities
            .write()
            .map_err(|_| LobbyRepositoryError::Unavailable)?;
        let mut store = self
            .state
            .write()
            .map_err(|_| LobbyRepositoryError::Unavailable)?;

        if !store.lobbies.contains_key(&lobby_id) {
            return Err(LobbyRepositoryError::LobbyNotFound);
        }

        if activities.get(&player_id) != Some(&PlayerActivity::Lobby(lobby_id)) {
            return Err(LobbyRepositoryError::Lobby(
                crate::domain::lobby::LobbyError::MemberNotFound,
            ));
        }

        let updated_lobby = {
            let lobby = store
                .lobbies
                .get_mut(&lobby_id)
                .ok_or(LobbyRepositoryError::LobbyNotFound)?;
            lobby.leave(player_id)?;
            lobby.clone()
        };

        activities.remove(&player_id);
        store.player_lobbies.remove(&player_id);
        Ok(updated_lobby)
    }

    async fn set_ready(
        &self,
        lobby_id: Uuid,
        player_id: Uuid,
        ready: bool,
    ) -> Result<Lobby, LobbyRepositoryError> {
        let mut store = self
            .state
            .write()
            .map_err(|_| LobbyRepositoryError::Unavailable)?;

        if !store.lobbies.contains_key(&lobby_id) {
            return Err(LobbyRepositoryError::LobbyNotFound);
        }

        if store.player_lobbies.get(&player_id) != Some(&lobby_id) {
            return Err(LobbyRepositoryError::Lobby(
                crate::domain::lobby::LobbyError::MemberNotFound,
            ));
        }

        let lobby = store
            .lobbies
            .get_mut(&lobby_id)
            .ok_or(LobbyRepositoryError::LobbyNotFound)?;
        lobby.set_ready(player_id, ready)?;

        Ok(lobby.clone())
    }
}
