use std::{collections::HashMap, sync::RwLock};

use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    domain::player::Player,
    storage::{PlayerRepository, RepositoryError},
};

#[derive(Default)]
pub struct InMemoryPlayerRepository {
    players: RwLock<HashMap<Uuid, Player>>,
}

impl InMemoryPlayerRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl PlayerRepository for InMemoryPlayerRepository {
    async fn insert(&self, player: Player) -> Result<Player, RepositoryError> {
        let mut players = self
            .players
            .write()
            .map_err(|_| RepositoryError::Unavailable)?;

        if players.contains_key(&player.id()) {
            return Err(RepositoryError::AlreadyExists);
        }

        players.insert(player.id(), player.clone());
        Ok(player)
    }

    async fn find_by_id(&self, player_id: Uuid) -> Result<Option<Player>, RepositoryError> {
        let players = self
            .players
            .read()
            .map_err(|_| RepositoryError::Unavailable)?;

        Ok(players.get(&player_id).cloned())
    }
}
