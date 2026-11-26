use std::{env, net::SocketAddr, time::Duration};

use thiserror::Error;

use crate::domain::matchmaking::{MatchmakingConfig, MatchmakingConfigError};
use crate::domain::rating::{RatingConfig, RatingConfigError};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 3000;
const DEFAULT_MATCH_BASE_GAP: u16 = 100;
const DEFAULT_MATCH_GAP_GROWTH_PER_SECOND: u16 = 10;
const DEFAULT_MATCH_MAX_GAP: u16 = 1000;
const DEFAULT_WS_CHANNEL_CAPACITY: usize = 32;
const DEFAULT_WS_HEARTBEAT_SECONDS: u64 = 15;
const DEFAULT_WS_CLIENT_TIMEOUT_SECONDS: u64 = 45;
const DEFAULT_DB_MAX_CONNECTIONS: u32 = 10;
const DEFAULT_DB_ACQUIRE_TIMEOUT_SECONDS: u64 = 5;
const DEFAULT_RATE_LIMIT_REQUESTS: u32 = 120;
const DEFAULT_RATE_LIMIT_WINDOW_SECONDS: u64 = 60;
const DEFAULT_RATING_K_FACTOR: u16 = 32;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StorageBackend {
    #[default]
    Memory,
    Postgres,
}

#[derive(Clone, Eq, PartialEq)]
pub struct DatabaseConfig {
    backend: StorageBackend,
    url: Option<String>,
    max_connections: u32,
    acquire_timeout: Duration,
}

impl DatabaseConfig {
    pub fn backend(&self) -> StorageBackend {
        self.backend
    }

    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    pub fn max_connections(&self) -> u32 {
        self.max_connections
    }

    pub fn acquire_timeout(&self) -> Duration {
        self.acquire_timeout
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            backend: StorageBackend::Memory,
            url: None,
            max_connections: DEFAULT_DB_MAX_CONNECTIONS,
            acquire_timeout: Duration::from_secs(DEFAULT_DB_ACQUIRE_TIMEOUT_SECONDS),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateLimitConfig {
    requests: u32,
    window: Duration,
}

impl RateLimitConfig {
    pub fn new(requests: u32, window: Duration) -> Result<Self, RateLimitConfigError> {
        if requests == 0 {
            return Err(RateLimitConfigError::ZeroRequests);
        }
        if window.is_zero() {
            return Err(RateLimitConfigError::ZeroWindow);
        }
        Ok(Self { requests, window })
    }

    pub fn requests(self) -> u32 {
        self.requests
    }

    pub fn window(self) -> Duration {
        self.window
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests: DEFAULT_RATE_LIMIT_REQUESTS,
            window: Duration::from_secs(DEFAULT_RATE_LIMIT_WINDOW_SECONDS),
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RateLimitConfigError {
    #[error("rate limit request count must be greater than zero")]
    ZeroRequests,
    #[error("rate limit window must be greater than zero")]
    ZeroWindow,
}

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

#[derive(Clone, Eq, PartialEq)]
pub struct Config {
    host: String,
    port: u16,
    matchmaking: MatchmakingConfig,
    rating: RatingConfig,
    websocket: WebSocketConfig,
    database: DatabaseConfig,
    rate_limit: RateLimitConfig,
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
        let rating = RatingConfig::new(env_rating_u16(
            "ARENA_RATING_K_FACTOR",
            DEFAULT_RATING_K_FACTOR,
        )?)?;
        let backend = match env::var("ARENA_STORAGE").as_deref() {
            Ok("memory") | Err(_) => StorageBackend::Memory,
            Ok("postgres") => StorageBackend::Postgres,
            Ok(value) => return Err(ConfigError::InvalidStorageBackend(value.to_owned())),
        };
        let url = match (backend, env::var("ARENA_DATABASE_URL")) {
            (StorageBackend::Postgres, Ok(value)) if !value.trim().is_empty() => Some(value),
            (StorageBackend::Postgres, _) => return Err(ConfigError::MissingDatabaseUrl),
            (StorageBackend::Memory, value) => value.ok(),
        };
        let database = DatabaseConfig {
            backend,
            url,
            max_connections: env_u32("ARENA_DB_MAX_CONNECTIONS", DEFAULT_DB_MAX_CONNECTIONS)?,
            acquire_timeout: Duration::from_secs(env_operational_u64(
                "ARENA_DB_ACQUIRE_TIMEOUT_SECONDS",
                DEFAULT_DB_ACQUIRE_TIMEOUT_SECONDS,
            )?),
        };
        if database.max_connections == 0 || database.acquire_timeout.is_zero() {
            return Err(ConfigError::InvalidDatabasePool);
        }
        let rate_limit = RateLimitConfig::new(
            env_u32("ARENA_RATE_LIMIT_REQUESTS", DEFAULT_RATE_LIMIT_REQUESTS)?,
            Duration::from_secs(env_operational_u64(
                "ARENA_RATE_LIMIT_WINDOW_SECONDS",
                DEFAULT_RATE_LIMIT_WINDOW_SECONDS,
            )?),
        )?;

        Ok(Self {
            host,
            port,
            matchmaking,
            rating,
            websocket,
            database,
            rate_limit,
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

    pub fn rating_config(&self) -> RatingConfig {
        self.rating
    }

    pub fn database_config(&self) -> &DatabaseConfig {
        &self.database
    }

    pub fn rate_limit_config(&self) -> RateLimitConfig {
        self.rate_limit
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

fn env_rating_u16(name: &'static str, default: u16) -> Result<u16, ConfigError> {
    match env::var(name) {
        Ok(value) => value
            .parse::<u16>()
            .map_err(|_| ConfigError::InvalidRatingValue { name, value }),
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

fn env_u32(name: &'static str, default: u32) -> Result<u32, ConfigError> {
    match env::var(name) {
        Ok(value) => value
            .parse::<u32>()
            .map_err(|_| ConfigError::InvalidOperationalValue { name, value }),
        Err(_) => Ok(default),
    }
}

fn env_operational_u64(name: &'static str, default: u64) -> Result<u64, ConfigError> {
    match env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|_| ConfigError::InvalidOperationalValue { name, value }),
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
    #[error("ARENA_STORAGE must be either memory or postgres: {0}")]
    InvalidStorageBackend(String),
    #[error("ARENA_DATABASE_URL is required when ARENA_STORAGE=postgres")]
    MissingDatabaseUrl,
    #[error("database pool size and acquire timeout must be greater than zero")]
    InvalidDatabasePool,
    #[error("{name} must be a non-negative integer: {value}")]
    InvalidOperationalValue { name: &'static str, value: String },
    #[error("{name} must be an integer from 1 to 128: {value}")]
    InvalidRatingValue { name: &'static str, value: String },
    #[error(transparent)]
    Matchmaking(#[from] MatchmakingConfigError),
    #[error(transparent)]
    WebSocket(#[from] WebSocketConfigError),
    #[error(transparent)]
    RateLimit(#[from] RateLimitConfigError),
    #[error(transparent)]
    Rating(#[from] RatingConfigError),
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
            rating: RatingConfig::default(),
            websocket: WebSocketConfig::default(),
            database: DatabaseConfig::default(),
            rate_limit: RateLimitConfig::default(),
        };

        assert_eq!(config.socket_address(), "127.0.0.1:3000".parse().unwrap());
    }
}
