use std::{collections::HashMap, sync::RwLock};

use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerActivity {
    Lobby(Uuid),
    Queued,
    Match(Uuid),
}

#[derive(Default)]
pub struct PlayerActivityRegistry {
    pub(crate) activities: RwLock<HashMap<Uuid, PlayerActivity>>,
}

impl PlayerActivityRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}
