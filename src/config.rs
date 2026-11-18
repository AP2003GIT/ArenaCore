use std::{env, net::SocketAddr};

use thiserror::Error;

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 3000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    host: String,
    port: u16,
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

        Ok(Self { host, port })
    }

    pub fn socket_address(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port)
            .parse()
            .expect("configuration is validated when it is constructed")
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ConfigError {
    #[error("ARENA_HOST must be a valid IP address: {0}")]
    InvalidHost(String),
    #[error("ARENA_PORT must be an integer from 0 to 65535: {0}")]
    InvalidPort(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_address_is_localhost_port_3000() {
        let config = Config {
            host: DEFAULT_HOST.to_owned(),
            port: DEFAULT_PORT,
        };

        assert_eq!(config.socket_address(), "127.0.0.1:3000".parse().unwrap());
    }
}
