use std::{env, net::SocketAddr};

use thiserror::Error;

use crate::domain::matchmaking::{MatchmakingConfig, MatchmakingConfigError};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 3000;
const DEFAULT_MATCH_BASE_GAP: u16 = 100;
const DEFAULT_MATCH_GAP_GROWTH_PER_SECOND: u16 = 10;
const DEFAULT_MATCH_MAX_GAP: u16 = 1000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    host: String,
    port: u16,
    matchmaking: MatchmakingConfig,
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

        Ok(Self {
            host,
            port,
            matchmaking,
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
}

fn env_u16(name: &'static str, default: u16) -> Result<u16, ConfigError> {
    match env::var(name) {
        Ok(value) => value
            .parse::<u16>()
            .map_err(|_| ConfigError::InvalidMatchmakingValue { name, value }),
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
    #[error(transparent)]
    Matchmaking(#[from] MatchmakingConfigError),
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
        };

        assert_eq!(config.socket_address(), "127.0.0.1:3000".parse().unwrap());
    }
}
