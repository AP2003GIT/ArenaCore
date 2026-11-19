use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;
use uuid::Uuid;

use crate::storage::{PlayerRepository, RepositoryError};

#[derive(Clone)]
pub struct DevSessionService {
    player_repository: Arc<dyn PlayerRepository>,
    sessions: Arc<RwLock<HashMap<Uuid, Uuid>>>,
}

impl DevSessionService {
    pub fn new(player_repository: Arc<dyn PlayerRepository>) -> Self {
        Self {
            player_repository,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn issue(&self, player_id: Uuid) -> Result<Uuid, DevSessionError> {
        let player = self
            .player_repository
            .find_by_id(player_id)
            .await
            .map_err(DevSessionError::PlayerRepository)?;
        if player.is_none() {
            return Err(DevSessionError::PlayerNotFound);
        }

        let token = Uuid::new_v4();
        self.sessions
            .write()
            .map_err(|_| DevSessionError::Unavailable)?
            .insert(token, player_id);
        Ok(token)
    }

    pub fn authenticates(&self, token: Uuid, player_id: Uuid) -> bool {
        self.sessions
            .read()
            .map(|sessions| sessions.get(&token) == Some(&player_id))
            .unwrap_or(false)
    }
}

#[derive(Debug, Error)]
pub enum DevSessionError {
    #[error("player was not found")]
    PlayerNotFound,
    #[error("development session store is unavailable")]
    Unavailable,
    #[error("player repository operation failed: {0}")]
    PlayerRepository(RepositoryError),
}
