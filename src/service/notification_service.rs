use std::{collections::HashMap, sync::RwLock};

use serde::Serialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::domain::{game_match::GameMatch, lobby::Lobby, matchmaking::PlayerMatchmakingStatus};

#[derive(Clone, Debug, Serialize)]
pub struct PlayerEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    event_id: Uuid,
    occurred_at: String,
    payload: Value,
}

impl PlayerEvent {
    pub fn lobby_updated(lobby: &Lobby) -> Self {
        Self::new(
            "lobby_updated",
            json!({
                "lobby_id": lobby.id(),
                "owner_player_id": lobby.owner_player_id(),
                "capacity": lobby.capacity(),
                "status": lobby.status().as_str(),
                "members": lobby.members().iter().map(|member| json!({
                    "player_id": member.player_id(),
                    "ready": member.is_ready(),
                })).collect::<Vec<_>>(),
            }),
        )
    }

    pub fn queue_updated(player_id: Uuid, status: PlayerMatchmakingStatus) -> Self {
        let (state, mode, match_id) = match status {
            PlayerMatchmakingStatus::Idle => ("idle", None, None),
            PlayerMatchmakingStatus::Queued { mode } => ("queued", Some(mode.as_str()), None),
            PlayerMatchmakingStatus::Matched { match_id, mode } => {
                ("matched", Some(mode.as_str()), Some(match_id))
            }
        };

        Self::new(
            "queue_updated",
            json!({
                "player_id": player_id,
                "status": state,
                "mode": mode,
                "match_id": match_id,
            }),
        )
    }

    pub fn match_found(game_match: &GameMatch) -> Self {
        Self::new(
            "match_found",
            json!({
                "match_id": game_match.id(),
                "mode": game_match.mode().as_str(),
                "status": game_match.status().as_str(),
                "team_a": game_match.team_a().iter().map(|player| json!({
                    "player_id": player.player_id(),
                    "rating": player.rating(),
                })).collect::<Vec<_>>(),
                "team_b": game_match.team_b().iter().map(|player| json!({
                    "player_id": player.player_id(),
                    "rating": player.rating(),
                })).collect::<Vec<_>>(),
            }),
        )
    }

    pub fn match_started(game_match: &GameMatch) -> Self {
        Self::new("match_started", match_payload(game_match))
    }

    pub fn match_completed(game_match: &GameMatch) -> Self {
        Self::new("match_completed", match_payload(game_match))
    }

    pub fn match_cancelled(game_match: &GameMatch) -> Self {
        Self::new("match_cancelled", match_payload(game_match))
    }

    fn new(event_type: &'static str, payload: Value) -> Self {
        let occurred_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned());

        Self {
            event_type,
            event_id: Uuid::new_v4(),
            occurred_at,
            payload,
        }
    }
}

fn match_payload(game_match: &GameMatch) -> Value {
    json!({
        "match_id": game_match.id(),
        "mode": game_match.mode().as_str(),
        "status": game_match.status().as_str(),
        "winning_team": game_match.result().map(|result| result.winning_team().as_str()),
        "team_a": game_match.team_a().iter().map(|player| json!({
            "player_id": player.player_id(),
            "rating": player.rating(),
        })).collect::<Vec<_>>(),
        "team_b": game_match.team_b().iter().map(|player| json!({
            "player_id": player.player_id(),
            "rating": player.rating(),
        })).collect::<Vec<_>>(),
        "rating_changes": game_match.result().map(|result| result.rating_changes().iter().map(|change| json!({
            "player_id": change.player_id(),
            "rating_before": change.rating_before(),
            "rating_after": change.rating_after(),
            "delta": change.delta(),
        })).collect::<Vec<_>>()),
    })
}

pub struct PlayerSubscription {
    pub connection_id: Uuid,
    pub receiver: mpsc::Receiver<PlayerEvent>,
}

#[derive(Default)]
pub struct NotificationHub {
    connections: RwLock<HashMap<Uuid, HashMap<Uuid, mpsc::Sender<PlayerEvent>>>>,
}

impl NotificationHub {
    pub fn subscribe(&self, player_id: Uuid, capacity: usize) -> PlayerSubscription {
        let connection_id = Uuid::new_v4();
        let (sender, receiver) = mpsc::channel(capacity);
        let mut connections = self
            .connections
            .write()
            .unwrap_or_else(|error| error.into_inner());
        connections
            .entry(player_id)
            .or_default()
            .insert(connection_id, sender);

        PlayerSubscription {
            connection_id,
            receiver,
        }
    }

    pub fn publish(&self, player_id: Uuid, event: PlayerEvent) {
        let mut connections = self
            .connections
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let Some(player_connections) = connections.get_mut(&player_id) else {
            return;
        };

        player_connections.retain(|connection_id, sender| match sender.try_send(event.clone()) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%player_id, %connection_id, %error, "dropping slow or closed WebSocket subscriber");
                false
            }
        });
        if player_connections.is_empty() {
            connections.remove(&player_id);
        }
    }

    pub fn unregister(&self, player_id: Uuid, connection_id: Uuid) {
        let mut connections = self
            .connections
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let Some(player_connections) = connections.get_mut(&player_id) else {
            return;
        };
        player_connections.remove(&connection_id);
        if player_connections.is_empty() {
            connections.remove(&player_id);
        }
    }

    pub fn connection_count(&self, player_id: Uuid) -> usize {
        self.connections
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .get(&player_id)
            .map_or(0, HashMap::len)
    }

    pub fn total_connection_count(&self) -> usize {
        self.connections
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .map(HashMap::len)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_channel_is_removed_instead_of_growing_without_bound() {
        let hub = NotificationHub::default();
        let player_id = Uuid::new_v4();
        let mut subscription = hub.subscribe(player_id, 1);

        hub.publish(
            player_id,
            PlayerEvent::queue_updated(player_id, PlayerMatchmakingStatus::Idle),
        );
        assert_eq!(hub.connection_count(player_id), 1);

        hub.publish(
            player_id,
            PlayerEvent::queue_updated(player_id, PlayerMatchmakingStatus::Idle),
        );
        assert_eq!(hub.connection_count(player_id), 0);
        assert!(subscription.receiver.try_recv().is_ok());
        assert!(matches!(
            subscription.receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
    }
}
