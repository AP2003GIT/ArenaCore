use std::{sync::Arc, time::Instant};

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    http::{header::AUTHORIZATION, HeaderMap},
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::time::{self, MissedTickBehavior};
use uuid::Uuid;

use crate::{
    app::AppState, config::WebSocketConfig, service::notification_service::NotificationHub,
};

use super::error::ApiError;

#[derive(Deserialize)]
pub struct WebSocketQuery {
    player_id: Uuid,
}

pub async fn connect(
    State(state): State<AppState>,
    Query(query): Query<WebSocketQuery>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let token = bearer_token(&headers).ok_or_else(ApiError::unauthorized)?;
    if !state
        .dev_session_service
        .authenticates(token, query.player_id)
    {
        return Err(ApiError::unauthorized());
    }

    let notifications = Arc::clone(&state.notification_hub);
    let websocket_config = state.websocket_config;
    Ok(upgrade.on_upgrade(move |socket| {
        serve_socket(socket, query.player_id, notifications, websocket_config)
    }))
}

fn bearer_token(headers: &HeaderMap) -> Option<Uuid> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?;
    Uuid::parse_str(token).ok()
}

async fn serve_socket(
    socket: WebSocket,
    player_id: Uuid,
    notifications: Arc<NotificationHub>,
    config: WebSocketConfig,
) {
    let subscription = notifications.subscribe(player_id, config.channel_capacity());
    let connection_id = subscription.connection_id;
    let mut events = subscription.receiver;
    let (mut sender, mut receiver) = socket.split();
    let mut heartbeat = time::interval(config.heartbeat_interval());
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last_client_activity = Instant::now();

    loop {
        tokio::select! {
            event = events.recv() => {
                let Some(event) = event else {
                    break;
                };
                let Ok(json) = serde_json::to_string(&event) else {
                    tracing::error!(%player_id, %connection_id, "failed to serialize WebSocket event");
                    break;
                };
                if sender.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
            message = receiver.next() => {
                match message {
                    Some(Ok(Message::Pong(_))) => last_client_activity = Instant::now(),
                    Some(Ok(Message::Ping(payload))) => {
                        last_client_activity = Instant::now();
                        if sender.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Text(_) | Message::Binary(_))) => {
                        last_client_activity = Instant::now();
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                }
            }
            _ = heartbeat.tick() => {
                if last_client_activity.elapsed() > config.client_timeout() {
                    let _ = sender.send(Message::Close(None)).await;
                    break;
                }
                if sender.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
        }
    }

    notifications.unregister(player_id, connection_id);
}
