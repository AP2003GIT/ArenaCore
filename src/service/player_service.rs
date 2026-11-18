use std::sync::Arc;

use thiserror::Error;
use uuid::Uuid;

use crate::{
    domain::player::Player,
    storage::{PlayerRepository, RepositoryError},
};

const DEFAULT_RATING: i32 = 1200;
const MIN_RATING: i32 = 0;
const MAX_RATING: i32 = 3000;
const MIN_DISPLAY_NAME_LENGTH: usize = 3;
const MAX_DISPLAY_NAME_LENGTH: usize = 24;

#[derive(Clone)]
pub struct PlayerService {
    repository: Arc<dyn PlayerRepository>,
}

impl PlayerService {
    pub fn new(repository: Arc<dyn PlayerRepository>) -> Self {
        Self { repository }
    }

    pub async fn create_player(
        &self,
        display_name: String,
        rating: Option<i32>,
    ) -> Result<Player, PlayerServiceError> {
        let display_name = normalize_display_name(display_name)?;
        let rating = validate_rating(rating.unwrap_or(DEFAULT_RATING))?;
        let player = Player::new(display_name, rating);

        self.repository
            .insert(player)
            .await
            .map_err(PlayerServiceError::from)
    }

    pub async fn get_player(&self, player_id: Uuid) -> Result<Player, PlayerServiceError> {
        self.repository
            .find_by_id(player_id)
            .await?
            .ok_or(PlayerServiceError::PlayerNotFound)
    }
}

fn normalize_display_name(display_name: String) -> Result<String, PlayerServiceError> {
    let display_name = display_name.trim();
    let character_count = display_name.chars().count();

    if !(MIN_DISPLAY_NAME_LENGTH..=MAX_DISPLAY_NAME_LENGTH).contains(&character_count) {
        return Err(PlayerServiceError::InvalidDisplayName);
    }

    Ok(display_name.to_owned())
}

fn validate_rating(rating: i32) -> Result<u16, PlayerServiceError> {
    if !(MIN_RATING..=MAX_RATING).contains(&rating) {
        return Err(PlayerServiceError::InvalidRating);
    }

    Ok(rating as u16)
}

#[derive(Debug, Error)]
pub enum PlayerServiceError {
    #[error("display name is invalid")]
    InvalidDisplayName,
    #[error("rating is invalid")]
    InvalidRating,
    #[error("player was not found")]
    PlayerNotFound,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::memory::InMemoryPlayerRepository;

    fn service() -> PlayerService {
        PlayerService::new(Arc::new(InMemoryPlayerRepository::new()))
    }

    #[tokio::test]
    async fn trims_name_and_applies_default_rating() {
        let player = service()
            .create_player("  Ada  ".to_owned(), None)
            .await
            .unwrap();

        assert_eq!(player.display_name(), "Ada");
        assert_eq!(player.rating(), 1200);
    }

    #[tokio::test]
    async fn counts_unicode_characters_instead_of_bytes() {
        let player = service()
            .create_player("李雷韩".to_owned(), Some(1500))
            .await
            .unwrap();

        assert_eq!(player.display_name(), "李雷韩");
    }
}
