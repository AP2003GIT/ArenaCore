use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Player {
    id: Uuid,
    display_name: String,
    rating: u16,
}

impl Player {
    pub fn new(display_name: String, rating: u16) -> Self {
        Self {
            id: Uuid::new_v4(),
            display_name,
            rating,
        }
    }

    pub(crate) fn restore(id: Uuid, display_name: String, rating: u16) -> Self {
        Self {
            id,
            display_name,
            rating,
        }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn rating(&self) -> u16 {
        self.rating
    }

    pub(crate) fn set_rating(&mut self, rating: u16) {
        self.rating = rating;
    }
}
