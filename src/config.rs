use std::{env, net::SocketAddr, time::Duration};

use thiserror::Error;

use crate::domain::matchmaking::{MatchmakingConfig, MatchmakingConfigError};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 3000;
const DEFAULT_MATCH_BASE_GAP: u16 = 100;
const DEFAULT_MATCH_GAP_GROWTH_PER_SECOND: u16 = 10;
const DEFAULT_MATCH_MAX_GAP: u16 = 1000;
const DEFAULT_WS_CHANNEL_CAPACITY: usize = 32;
const DEFAULT_WS_HEARTBEAT_SECONDS: u64 = 15;
const DEFAULT_WS_CLIENT_TIMEOUT_SECONDS: u64 = 45;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebSocketConfig {
    channel_capacity: usize,
    heartbeat_interval: Duration,
    client_timeout: Duration,
}

impl WebSocketConfig {
    pub fn new(
        channel_capacity: usize,
        heartbeat_interval: Duration,
        client_timeout: Duration,
    ) -> Result<Self, WebSocketConfigError> {
        if channel_capacity == 0 {
            return Err(WebSocketConfigError::ZeroChannelCapacity);
        }
        if heartbeat_interval.is_zero() {
            return Err(WebSocketConfigError::ZeroHeartbeatInterval);
        }
        if client_timeout <= heartbeat_interval {
            return Err(WebSocketConfigError::TimeoutNotLongerThanHeartbeat);
        }

        Ok(Self {
            channel_capacity,
            heartbeat_interval,
            client_timeout,
        })
    }

    pub fn channel_capacity(self) -> usize {
        self.channel_capacity
    }

    pub fn heartbeat_interval(self) -> Duration {
        self.heartbeat_interval
    }

    pub fn client_timeout(self) -> Duration {
        self.client_timeout
    }
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            channel_capacity: DEFAULT_WS_CHANNEL_CAPACITY,
            heartbeat_interval: Duration::from_secs(DEFAULT_WS_HEARTBEAT_SECONDS),
            client_timeout: Duration::from_secs(DEFAULT_WS_CLIENT_TIMEOUT_SECONDS),
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum WebSocketConfigError {
    #[error("WebSocket channel capacity must be greater than zero")]
    ZeroChannelCapacity,
    #[error("WebSocket heartbeat interval must be greater than zero")]
    ZeroHeartbeatInterval,
    #[error("WebSocket client timeout must be longer than the heartbeat interval")]
    TimeoutNotLongerThanHeartbeat,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    host: String,
    port: u16,
    matchmaking: MatchmakingConfig,
    websocket: WebSocketConfig,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let host = env::var("ARENA_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_owned());
        let port = match env::var("ARENA_PORT") {
            Ok(value) => value
                .parse::<u16>()
                .map_err(|_| ConfigError::InvalidPort(value))?,
            Err(_) => DEFAULT_PORT,
        };

        let socket_address = format!("{host}:{port}");
        socket_address
            .parse::<SocketAddr>()
            .map_err(|_| ConfigError::InvalidHost(host.clone()))?;
        let matchmaking = MatchmakingConfig::new(
            env_u16("ARENA_MATCH_BASE_GAP", DEFAULT_MATCH_BASE_GAP)?,
            env_u16(
                "ARENA_MATCH_GAP_GROWTH_PER_SECOND",
                DEFAULT_MATCH_GAP_GROWTH_PER_SECOND,
            )?,
            env_u16("ARENA_MATCH_MAX_GAP", DEFAULT_MATCH_MAX_GAP)?,
        )?;
        let websocket = WebSocketConfig::new(
            env_usize("ARENA_WS_CHANNEL_CAPACITY", DEFAULT_WS_CHANNEL_CAPACITY)?,
            Duration::from_secs(env_u64(
                "ARENA_WS_HEARTBEAT_SECONDS",
                DEFAULT_WS_HEARTBEAT_SECONDS,
            )?),
            Duration::from_secs(env_u64(
                "ARENA_WS_CLIENT_TIMEOUT_SECONDS",
                DEFAULT_WS_CLIENT_TIMEOUT_SECONDS,
            )?),
        )?;

        Ok(Self {
            host,
            port,
            matchmaking,
            websocket,
        })
    }

    pub fn socket_address(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port)
            .parse()
            .expect("configuration is validated when it is constructed")
    }

    pub fn matchmaking_config(&self) -> MatchmakingConfig {
        self.matchmaking
    }

    pub fn websocket_config(&self) -> WebSocketConfig {
        self.websocket
    }
}

fn env_u16(name: &'static str, default: u16) -> Result<u16, ConfigError> {
    match env::var(name) {
        Ok(value) => value
            .parse::<u16>()
            .map_err(|_| ConfigError::InvalidMatchmakingValue { name, value }),
        Err(_) => Ok(default),
    }
}

fn env_usize(name: &'static str, default: usize) -> Result<usize, ConfigError> {
    match env::var(name) {
        Ok(value) => value
            .parse::<usize>()
            .map_err(|_| ConfigError::InvalidWebSocketValue { name, value }),
        Err(_) => Ok(default),
    }
}

fn env_u64(name: &'static str, default: u64) -> Result<u64, ConfigError> {
    match env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|_| ConfigError::InvalidWebSocketValue { name, value }),
        Err(_) => Ok(default),
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ConfigError {
    #[error("ARENA_HOST must be a valid IP address: {0}")]
    InvalidHost(String),
    #[error("ARENA_PORT must be an integer from 0 to 65535: {0}")]
    InvalidPort(String),
    #[error("{name} must be an integer from 0 to 65535: {value}")]
    InvalidMatchmakingValue { name: &'static str, value: String },
    #[error("{name} must be a non-negative integer: {value}")]
    InvalidWebSocketValue { name: &'static str, value: String },
    #[error(transparent)]
    Matchmaking(#[from] MatchmakingConfigError),
    #[error(transparent)]
    WebSocket(#[from] WebSocketConfigError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_address_is_localhost_port_3000() {
        let config = Config {
            host: DEFAULT_HOST.to_owned(),
            port: DEFAULT_PORT,
            matchmaking: MatchmakingConfig::default(),
            websocket: WebSocketConfig::default(),
        };

        assert_eq!(config.socket_address(), "127.0.0.1:3000".parse().unwrap());
    }
}
