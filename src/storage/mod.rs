pub mod memory;

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::player::Player;

#[async_trait]
pub trait PlayerRepository: Send + Sync {
    async fn insert(&self, player: Player) -> Result<Player, RepositoryError>;
    async fn find_by_id(&self, player_id: Uuid) -> Result<Option<Player>, RepositoryError>;
}

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("player already exists")]
    AlreadyExists,
    #[error("repository lock is unavailable")]
    Unavailable,
}
