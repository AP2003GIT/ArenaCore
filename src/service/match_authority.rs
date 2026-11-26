use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ring::signature::{UnparsedPublicKey, ED25519};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::domain::game_match::{MatchActor, MatchTeam};

const TOKEN_PREFIX: &str = "arenacore-match-v1";
const MAX_TOKEN_LIFETIME_SECONDS: i64 = 300;

#[derive(Clone, Copy)]
pub enum MatchCommand {
    Start,
    Complete(MatchTeam),
    Cancel,
}

impl MatchCommand {
    fn scope(self) -> (&'static str, &'static str) {
        match self {
            Self::Start => ("start", "-"),
            Self::Complete(team) => ("complete", team.as_str()),
            Self::Cancel => ("cancel", "-"),
        }
    }
}

#[derive(Clone, Default, Eq, PartialEq)]
pub struct MatchAuthority {
    game_server_key: Option<[u8; 32]>,
    administrator_key: Option<[u8; 32]>,
    allow_dev_participants: bool,
}

impl MatchAuthority {
    pub fn new(
        game_server_key: Option<&str>,
        administrator_key: Option<&str>,
        allow_dev_participants: bool,
    ) -> Result<Self, MatchAuthorityConfigError> {
        let game_server_key = parse_key(game_server_key)
            .map_err(|()| MatchAuthorityConfigError::InvalidGameServerKey)?;
        let administrator_key = parse_key(administrator_key)
            .map_err(|()| MatchAuthorityConfigError::InvalidAdministratorKey)?;
        if game_server_key.is_some() && game_server_key == administrator_key {
            return Err(MatchAuthorityConfigError::ReusedKey);
        }
        Ok(Self {
            game_server_key,
            administrator_key,
            allow_dev_participants,
        })
    }

    pub fn allows_dev_participants(&self) -> bool {
        self.allow_dev_participants
    }

    pub fn verify(
        &self,
        token: &str,
        match_id: Uuid,
        command: MatchCommand,
    ) -> Result<MatchActor, MatchAuthorizationError> {
        use MatchAuthorizationError::{Forbidden, InvalidCredential};

        if token.len() > 256 {
            return Err(InvalidCredential);
        }
        let (message, signature) = token.rsplit_once('.').ok_or(InvalidCredential)?;
        let (_, expires) = message.rsplit_once('.').ok_or(InvalidCredential)?;
        let expires: i64 = expires.parse().map_err(|_| InvalidCredential)?;
        let now = OffsetDateTime::now_utc().unix_timestamp();
        if expires <= now || expires > now + MAX_TOKEN_LIFETIME_SECONDS {
            return Err(InvalidCredential);
        }

        let role = message.split('.').nth(1).ok_or(InvalidCredential)?;
        let (actor, key) = match role {
            "game_server" => (MatchActor::GameServer, self.game_server_key.as_ref()),
            "administrator" => (MatchActor::Administrator, self.administrator_key.as_ref()),
            _ => return Err(InvalidCredential),
        };
        let key = key.ok_or(InvalidCredential)?;
        let (action, winner) = command.scope();
        if message != format!("{TOKEN_PREFIX}.{role}.{match_id}.{action}.{winner}.{expires}") {
            return Err(InvalidCredential);
        }
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| InvalidCredential)?;
        UnparsedPublicKey::new(&ED25519, key)
            .verify(message.as_bytes(), &signature)
            .map_err(|_| InvalidCredential)?;

        if actor == MatchActor::GameServer && matches!(command, MatchCommand::Cancel) {
            return Err(Forbidden);
        }
        Ok(actor)
    }
}

fn parse_key(configured: Option<&str>) -> Result<Option<[u8; 32]>, ()> {
    configured
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.len() != 43 {
                return Err(());
            }
            URL_SAFE_NO_PAD
                .decode(value)
                .map_err(|_| ())?
                .try_into()
                .map_err(|_| ())
        })
        .transpose()
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum MatchAuthorityConfigError {
    #[error("ARENA_MATCH_GAME_SERVER_PUBLIC_KEY must be an unpadded base64url Ed25519 public key")]
    InvalidGameServerKey,
    #[error("ARENA_MATCH_ADMIN_PUBLIC_KEY must be an unpadded base64url Ed25519 public key")]
    InvalidAdministratorKey,
    #[error("the game-server and match administrator must use different public keys")]
    ReusedKey,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum MatchAuthorizationError {
    #[error("a valid bearer credential for this match command is required")]
    InvalidCredential,
    #[error("the credential role cannot perform this match command")]
    Forbidden,
}
