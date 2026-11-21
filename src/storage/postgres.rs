use std::time::Duration;

use async_trait::async_trait;
use sqlx::{postgres::PgPoolOptions, FromRow, PgConnection, PgPool, Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    domain::{
        game_match::{GameMatch, MatchMode, MatchPlayer, MatchStatus},
        lobby::{Lobby, LobbyError, LobbyMember, LobbyStatus},
        matchmaking::{MatchmakingConfig, MatchmakingState, PlayerMatchmakingStatus, QueueEntry},
        player::Player,
    },
    storage::{
        EnqueueResult, LobbyRepository, LobbyRepositoryError, MatchmakingRepository,
        MatchmakingRepositoryError, PlayerRepository, RepositoryError,
    },
};

const MATCHMAKING_LOCK_ID: i64 = 0x4152_454E_4143_4F52;

#[derive(Clone)]
pub struct PostgresRepository {
    pool: PgPool,
}

impl PostgresRepository {
    pub async fn connect(
        database_url: &str,
        max_connections: u32,
        acquire_timeout: Duration,
    ) -> Result<Self, PostgresInitializationError> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .acquire_timeout(acquire_timeout)
            .connect(database_url)
            .await
            .map_err(PostgresInitializationError::Connect)?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(PostgresInitializationError::Migrate)?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn fetch_lobby(
        connection: &mut PgConnection,
        lobby_id: Uuid,
        for_update: bool,
    ) -> Result<Option<Lobby>, LobbyRepositoryError> {
        let sql = if for_update {
            "SELECT id, owner_player_id, capacity, status FROM lobbies WHERE id = $1 FOR UPDATE"
        } else {
            "SELECT id, owner_player_id, capacity, status FROM lobbies WHERE id = $1"
        };
        let Some(row) = sqlx::query_as::<_, LobbyRow>(sql)
            .bind(lobby_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(LobbyRepositoryError::Database)?
        else {
            return Ok(None);
        };
        let members = sqlx::query_as::<_, LobbyMemberRow>(
            "SELECT player_id, ready FROM lobby_memberships WHERE lobby_id = $1 ORDER BY joined_order",
        )
        .bind(lobby_id)
        .fetch_all(&mut *connection)
        .await
        .map_err(LobbyRepositoryError::Database)?;

        let capacity =
            u8::try_from(row.capacity).map_err(|_| LobbyRepositoryError::InvalidStoredState)?;
        let status =
            LobbyStatus::parse(&row.status).ok_or(LobbyRepositoryError::InvalidStoredState)?;
        Ok(Some(Lobby::restore(
            row.id,
            row.owner_player_id,
            capacity,
            status,
            members
                .into_iter()
                .map(|member| LobbyMember::restore(member.player_id, member.ready))
                .collect(),
        )))
    }

    async fn lock_matchmaking(
        transaction: &mut Transaction<'_, Postgres>,
    ) -> Result<(), MatchmakingRepositoryError> {
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(MATCHMAKING_LOCK_ID)
            .execute(&mut **transaction)
            .await
            .map_err(MatchmakingRepositoryError::Database)?;
        Ok(())
    }

    async fn load_queue_state(
        transaction: &mut Transaction<'_, Postgres>,
    ) -> Result<MatchmakingState, MatchmakingRepositoryError> {
        let rows = sqlx::query_as::<_, QueueRow>(
            "SELECT player_id, rating, mode, enqueued_at_seconds FROM matchmaking_queue ORDER BY enqueued_at_seconds, player_id",
        )
        .fetch_all(&mut **transaction)
        .await
        .map_err(MatchmakingRepositoryError::Database)?;
        let mut state = MatchmakingState::default();
        for row in rows {
            let rating = u16::try_from(row.rating)
                .map_err(|_| MatchmakingRepositoryError::InvalidStoredState)?;
            let enqueued_at_seconds = u64::try_from(row.enqueued_at_seconds)
                .map_err(|_| MatchmakingRepositoryError::InvalidStoredState)?;
            let mode = MatchMode::parse(&row.mode)
                .ok_or(MatchmakingRepositoryError::InvalidStoredState)?;
            state
                .enqueue(QueueEntry::new(
                    row.player_id,
                    rating,
                    mode,
                    enqueued_at_seconds,
                ))
                .map_err(|_| MatchmakingRepositoryError::InvalidStoredState)?;
        }
        Ok(state)
    }

    async fn persist_matches(
        transaction: &mut Transaction<'_, Postgres>,
        matches: &[GameMatch],
    ) -> Result<(), MatchmakingRepositoryError> {
        for game_match in matches {
            sqlx::query("INSERT INTO matches (id, mode, status) VALUES ($1, $2, $3)")
                .bind(game_match.id())
                .bind(game_match.mode().as_str())
                .bind(game_match.status().as_str())
                .execute(&mut **transaction)
                .await
                .map_err(MatchmakingRepositoryError::Database)?;
            sqlx::query(
                "INSERT INTO match_history (match_id, from_status, to_status) VALUES ($1, NULL, $2)",
            )
            .bind(game_match.id())
            .bind(game_match.status().as_str())
            .execute(&mut **transaction)
            .await
            .map_err(MatchmakingRepositoryError::Database)?;

            for (team, players) in [(1_i16, game_match.team_a()), (2_i16, game_match.team_b())] {
                for player in players {
                    sqlx::query(
                        "INSERT INTO match_players (match_id, player_id, team, rating) VALUES ($1, $2, $3, $4)",
                    )
                    .bind(game_match.id())
                    .bind(player.player_id())
                    .bind(team)
                    .bind(i16::try_from(player.rating()).expect("valid player rating fits i16"))
                    .execute(&mut **transaction)
                    .await
                    .map_err(MatchmakingRepositoryError::Database)?;
                    let removed = sqlx::query("DELETE FROM matchmaking_queue WHERE player_id = $1")
                        .bind(player.player_id())
                        .execute(&mut **transaction)
                        .await
                        .map_err(MatchmakingRepositoryError::Database)?;
                    let updated = sqlx::query(
                        "UPDATE player_activities SET activity_type = 'match', activity_id = $2, updated_at = NOW() WHERE player_id = $1 AND activity_type = 'queued'",
                    )
                    .bind(player.player_id())
                    .bind(game_match.id())
                    .execute(&mut **transaction)
                    .await
                    .map_err(MatchmakingRepositoryError::Database)?;
                    if removed.rows_affected() != 1 || updated.rows_affected() != 1 {
                        return Err(MatchmakingRepositoryError::InconsistentState);
                    }
                }
            }
        }
        Ok(())
    }

    async fn fetch_match(
        connection: &mut PgConnection,
        match_id: Uuid,
    ) -> Result<Option<GameMatch>, MatchmakingRepositoryError> {
        let Some(row) =
            sqlx::query_as::<_, MatchRow>("SELECT id, mode, status FROM matches WHERE id = $1")
                .bind(match_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(MatchmakingRepositoryError::Database)?
        else {
            return Ok(None);
        };
        let players = sqlx::query_as::<_, MatchPlayerRow>(
            "SELECT player_id, team, rating FROM match_players WHERE match_id = $1 ORDER BY team, player_id",
        )
        .bind(match_id)
        .fetch_all(&mut *connection)
        .await
        .map_err(MatchmakingRepositoryError::Database)?;
        let mode =
            MatchMode::parse(&row.mode).ok_or(MatchmakingRepositoryError::InvalidStoredState)?;
        let status = MatchStatus::parse(&row.status)
            .ok_or(MatchmakingRepositoryError::InvalidStoredState)?;
        let mut team_a = Vec::new();
        let mut team_b = Vec::new();
        for player in players {
            let rating = u16::try_from(player.rating)
                .map_err(|_| MatchmakingRepositoryError::InvalidStoredState)?;
            let match_player = MatchPlayer::new(player.player_id, rating);
            match player.team {
                1 => team_a.push(match_player),
                2 => team_b.push(match_player),
                _ => return Err(MatchmakingRepositoryError::InvalidStoredState),
            }
        }
        Ok(Some(GameMatch::restore(
            row.id, mode, status, team_a, team_b,
        )))
    }
}

#[async_trait]
impl PlayerRepository for PostgresRepository {
    async fn insert(&self, player: Player) -> Result<Player, RepositoryError> {
        let result =
            sqlx::query("INSERT INTO players (id, display_name, rating) VALUES ($1, $2, $3)")
                .bind(player.id())
                .bind(player.display_name())
                .bind(i16::try_from(player.rating()).expect("valid player rating fits i16"))
                .execute(&self.pool)
                .await;
        match result {
            Ok(_) => Ok(player),
            Err(error) if is_unique_violation(&error) => Err(RepositoryError::AlreadyExists),
            Err(error) => Err(RepositoryError::Database(error)),
        }
    }

    async fn find_by_id(&self, player_id: Uuid) -> Result<Option<Player>, RepositoryError> {
        let row = sqlx::query_as::<_, PlayerRow>(
            "SELECT id, display_name, rating FROM players WHERE id = $1",
        )
        .bind(player_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(RepositoryError::Database)?;
        row.map(PlayerRow::into_domain).transpose()
    }
}

#[async_trait]
impl LobbyRepository for PostgresRepository {
    async fn create(&self, lobby: Lobby) -> Result<Lobby, LobbyRepositoryError> {
        let owner_player_id = lobby
            .owner_player_id()
            .expect("a new lobby always has an owner");
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(LobbyRepositoryError::Database)?;
        let insert = sqlx::query(
            "INSERT INTO lobbies (id, owner_player_id, capacity, status) VALUES ($1, $2, $3, $4)",
        )
        .bind(lobby.id())
        .bind(owner_player_id)
        .bind(i16::from(lobby.capacity()))
        .bind(lobby.status().as_str())
        .execute(&mut *transaction)
        .await;
        match insert {
            Ok(_) => {}
            Err(error) if is_unique_violation(&error) => {
                return Err(LobbyRepositoryError::AlreadyExists)
            }
            Err(error) => return Err(LobbyRepositoryError::Database(error)),
        }
        match claim_activity(&mut transaction, owner_player_id, "lobby", Some(lobby.id()))
            .await
            .map_err(LobbyRepositoryError::Database)?
        {
            None => {}
            Some(activity) => return Err(activity.as_lobby_error()),
        }
        sqlx::query(
            "INSERT INTO lobby_memberships (lobby_id, player_id, ready) VALUES ($1, $2, FALSE)",
        )
        .bind(lobby.id())
        .bind(owner_player_id)
        .execute(&mut *transaction)
        .await
        .map_err(LobbyRepositoryError::Database)?;
        transaction
            .commit()
            .await
            .map_err(LobbyRepositoryError::Database)?;
        Ok(lobby)
    }

    async fn find_by_id(&self, lobby_id: Uuid) -> Result<Option<Lobby>, LobbyRepositoryError> {
        let mut connection = self
            .pool
            .acquire()
            .await
            .map_err(LobbyRepositoryError::Database)?;
        Self::fetch_lobby(&mut connection, lobby_id, false).await
    }

    async fn join(&self, lobby_id: Uuid, player_id: Uuid) -> Result<Lobby, LobbyRepositoryError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(LobbyRepositoryError::Database)?;
        if let Some(activity) = claim_activity(&mut transaction, player_id, "lobby", Some(lobby_id))
            .await
            .map_err(LobbyRepositoryError::Database)?
        {
            if activity == StoredActivity::Lobby(lobby_id) {
                let lobby = Self::fetch_lobby(&mut transaction, lobby_id, false)
                    .await?
                    .ok_or(LobbyRepositoryError::LobbyNotFound)?;
                transaction
                    .commit()
                    .await
                    .map_err(LobbyRepositoryError::Database)?;
                return Ok(lobby);
            }
            return Err(activity.as_lobby_error());
        }
        let mut lobby = Self::fetch_lobby(&mut transaction, lobby_id, true)
            .await?
            .ok_or(LobbyRepositoryError::LobbyNotFound)?;
        lobby.join(player_id)?;
        sqlx::query(
            "INSERT INTO lobby_memberships (lobby_id, player_id, ready) VALUES ($1, $2, FALSE)",
        )
        .bind(lobby_id)
        .bind(player_id)
        .execute(&mut *transaction)
        .await
        .map_err(LobbyRepositoryError::Database)?;
        update_lobby(&mut transaction, &lobby).await?;
        transaction
            .commit()
            .await
            .map_err(LobbyRepositoryError::Database)?;
        Ok(lobby)
    }

    async fn leave(&self, lobby_id: Uuid, player_id: Uuid) -> Result<Lobby, LobbyRepositoryError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(LobbyRepositoryError::Database)?;
        let activity = lock_activity(&mut transaction, player_id)
            .await
            .map_err(LobbyRepositoryError::Database)?;
        if activity != Some(StoredActivity::Lobby(lobby_id)) {
            return Err(LobbyRepositoryError::Lobby(LobbyError::MemberNotFound));
        }
        let mut lobby = Self::fetch_lobby(&mut transaction, lobby_id, true)
            .await?
            .ok_or(LobbyRepositoryError::LobbyNotFound)?;
        lobby.leave(player_id)?;
        sqlx::query("DELETE FROM lobby_memberships WHERE lobby_id = $1 AND player_id = $2")
            .bind(lobby_id)
            .bind(player_id)
            .execute(&mut *transaction)
            .await
            .map_err(LobbyRepositoryError::Database)?;
        sqlx::query("DELETE FROM player_activities WHERE player_id = $1")
            .bind(player_id)
            .execute(&mut *transaction)
            .await
            .map_err(LobbyRepositoryError::Database)?;
        update_lobby(&mut transaction, &lobby).await?;
        transaction
            .commit()
            .await
            .map_err(LobbyRepositoryError::Database)?;
        Ok(lobby)
    }

    async fn set_ready(
        &self,
        lobby_id: Uuid,
        player_id: Uuid,
        ready: bool,
    ) -> Result<Lobby, LobbyRepositoryError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(LobbyRepositoryError::Database)?;
        let mut lobby = Self::fetch_lobby(&mut transaction, lobby_id, true)
            .await?
            .ok_or(LobbyRepositoryError::LobbyNotFound)?;
        lobby.set_ready(player_id, ready)?;
        let updated = sqlx::query(
            "UPDATE lobby_memberships SET ready = $3 WHERE lobby_id = $1 AND player_id = $2",
        )
        .bind(lobby_id)
        .bind(player_id)
        .bind(ready)
        .execute(&mut *transaction)
        .await
        .map_err(LobbyRepositoryError::Database)?;
        if updated.rows_affected() != 1 {
            return Err(LobbyRepositoryError::Lobby(LobbyError::MemberNotFound));
        }
        update_lobby(&mut transaction, &lobby).await?;
        transaction
            .commit()
            .await
            .map_err(LobbyRepositoryError::Database)?;
        Ok(lobby)
    }
}

#[async_trait]
impl MatchmakingRepository for PostgresRepository {
    async fn enqueue(
        &self,
        entry: QueueEntry,
        now_seconds: u64,
        config: MatchmakingConfig,
    ) -> Result<EnqueueResult, MatchmakingRepositoryError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(MatchmakingRepositoryError::Database)?;
        Self::lock_matchmaking(&mut transaction).await?;
        if let Some(activity) = claim_activity(&mut transaction, entry.player_id(), "queued", None)
            .await
            .map_err(MatchmakingRepositoryError::Database)?
        {
            return Err(activity.as_matchmaking_error());
        }
        sqlx::query(
            "INSERT INTO matchmaking_queue (player_id, mode, rating, enqueued_at_seconds) VALUES ($1, $2, $3, $4)",
        )
        .bind(entry.player_id())
        .bind(entry.mode().as_str())
        .bind(i16::try_from(entry.rating()).expect("valid player rating fits i16"))
        .bind(i64::try_from(entry.enqueued_at_seconds()).unwrap_or(i64::MAX))
        .execute(&mut *transaction)
        .await
        .map_err(MatchmakingRepositoryError::Database)?;
        let player_id = entry.player_id();
        let mut state = Self::load_queue_state(&mut transaction).await?;
        let matches = state.form_available_matches(now_seconds, config);
        Self::persist_matches(&mut transaction, &matches).await?;
        let status = state.status(player_id);
        transaction
            .commit()
            .await
            .map_err(MatchmakingRepositoryError::Database)?;
        Ok(EnqueueResult {
            status,
            formed_matches: matches,
        })
    }

    async fn cancel(&self, player_id: Uuid) -> Result<bool, MatchmakingRepositoryError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(MatchmakingRepositoryError::Database)?;
        Self::lock_matchmaking(&mut transaction).await?;
        if lock_activity(&mut transaction, player_id)
            .await
            .map_err(MatchmakingRepositoryError::Database)?
            != Some(StoredActivity::Queued)
        {
            transaction
                .commit()
                .await
                .map_err(MatchmakingRepositoryError::Database)?;
            return Ok(false);
        }
        let removed = sqlx::query("DELETE FROM matchmaking_queue WHERE player_id = $1")
            .bind(player_id)
            .execute(&mut *transaction)
            .await
            .map_err(MatchmakingRepositoryError::Database)?;
        sqlx::query("DELETE FROM player_activities WHERE player_id = $1")
            .bind(player_id)
            .execute(&mut *transaction)
            .await
            .map_err(MatchmakingRepositoryError::Database)?;
        if removed.rows_affected() != 1 {
            return Err(MatchmakingRepositoryError::InconsistentState);
        }
        transaction
            .commit()
            .await
            .map_err(MatchmakingRepositoryError::Database)?;
        Ok(true)
    }

    async fn status(
        &self,
        player_id: Uuid,
    ) -> Result<PlayerMatchmakingStatus, MatchmakingRepositoryError> {
        let activity = sqlx::query_as::<_, ActivityRow>(
            "SELECT activity_type, activity_id FROM player_activities WHERE player_id = $1",
        )
        .bind(player_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(MatchmakingRepositoryError::Database)?;
        let Some(activity) = activity else {
            return Ok(PlayerMatchmakingStatus::Idle);
        };
        match StoredActivity::try_from(activity)? {
            StoredActivity::Queued => {
                let mode: Option<String> =
                    sqlx::query_scalar("SELECT mode FROM matchmaking_queue WHERE player_id = $1")
                        .bind(player_id)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(MatchmakingRepositoryError::Database)?;
                let mode = mode
                    .as_deref()
                    .and_then(MatchMode::parse)
                    .ok_or(MatchmakingRepositoryError::InconsistentState)?;
                Ok(PlayerMatchmakingStatus::Queued { mode })
            }
            StoredActivity::Match(match_id) => {
                let mode: Option<String> =
                    sqlx::query_scalar("SELECT mode FROM matches WHERE id = $1")
                        .bind(match_id)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(MatchmakingRepositoryError::Database)?;
                let mode = mode
                    .as_deref()
                    .and_then(MatchMode::parse)
                    .ok_or(MatchmakingRepositoryError::InconsistentState)?;
                Ok(PlayerMatchmakingStatus::Matched { match_id, mode })
            }
            StoredActivity::Lobby(_) => Ok(PlayerMatchmakingStatus::Idle),
        }
    }

    async fn find_match(
        &self,
        match_id: Uuid,
    ) -> Result<Option<GameMatch>, MatchmakingRepositoryError> {
        let mut connection = self
            .pool
            .acquire()
            .await
            .map_err(MatchmakingRepositoryError::Database)?;
        Self::fetch_match(&mut connection, match_id).await
    }

    async fn run_pending(
        &self,
        now_seconds: u64,
        config: MatchmakingConfig,
    ) -> Result<Vec<GameMatch>, MatchmakingRepositoryError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(MatchmakingRepositoryError::Database)?;
        Self::lock_matchmaking(&mut transaction).await?;
        let mut state = Self::load_queue_state(&mut transaction).await?;
        let matches = state.form_available_matches(now_seconds, config);
        Self::persist_matches(&mut transaction, &matches).await?;
        transaction
            .commit()
            .await
            .map_err(MatchmakingRepositoryError::Database)?;
        Ok(matches)
    }
}

async fn update_lobby(
    transaction: &mut Transaction<'_, Postgres>,
    lobby: &Lobby,
) -> Result<(), LobbyRepositoryError> {
    sqlx::query(
        "UPDATE lobbies SET owner_player_id = $2, status = $3, updated_at = NOW() WHERE id = $1",
    )
    .bind(lobby.id())
    .bind(lobby.owner_player_id())
    .bind(lobby.status().as_str())
    .execute(&mut **transaction)
    .await
    .map_err(LobbyRepositoryError::Database)?;
    Ok(())
}

async fn claim_activity(
    transaction: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    activity_type: &str,
    activity_id: Option<Uuid>,
) -> Result<Option<StoredActivity>, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO player_activities (player_id, activity_type, activity_id) VALUES ($1, $2, $3) ON CONFLICT (player_id) DO NOTHING",
    )
    .bind(player_id)
    .bind(activity_type)
    .bind(activity_id)
    .execute(&mut **transaction)
    .await?;
    if result.rows_affected() == 1 {
        return Ok(None);
    }
    let row = sqlx::query_as::<_, ActivityRow>(
        "SELECT activity_type, activity_id FROM player_activities WHERE player_id = $1 FOR UPDATE",
    )
    .bind(player_id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(Some(StoredActivity::try_from_row(row)?))
}

async fn lock_activity(
    transaction: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<Option<StoredActivity>, sqlx::Error> {
    let row = sqlx::query_as::<_, ActivityRow>(
        "SELECT activity_type, activity_id FROM player_activities WHERE player_id = $1 FOR UPDATE",
    )
    .bind(player_id)
    .fetch_optional(&mut **transaction)
    .await?;
    row.map(StoredActivity::try_from_row).transpose()
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StoredActivity {
    Lobby(Uuid),
    Queued,
    Match(Uuid),
}

impl StoredActivity {
    fn try_from_row(row: ActivityRow) -> Result<Self, sqlx::Error> {
        match (row.activity_type.as_str(), row.activity_id) {
            ("lobby", Some(id)) => Ok(Self::Lobby(id)),
            ("queued", None) => Ok(Self::Queued),
            ("match", Some(id)) => Ok(Self::Match(id)),
            _ => Err(sqlx::Error::Protocol(
                "invalid stored player activity".to_owned(),
            )),
        }
    }

    fn as_lobby_error(self) -> LobbyRepositoryError {
        match self {
            Self::Lobby(_) => LobbyRepositoryError::PlayerAlreadyInLobby,
            Self::Queued => LobbyRepositoryError::PlayerQueued,
            Self::Match(_) => LobbyRepositoryError::PlayerInMatch,
        }
    }

    fn as_matchmaking_error(self) -> MatchmakingRepositoryError {
        match self {
            Self::Lobby(_) => MatchmakingRepositoryError::PlayerInLobby,
            Self::Queued => MatchmakingRepositoryError::PlayerAlreadyQueued,
            Self::Match(_) => MatchmakingRepositoryError::PlayerInMatch,
        }
    }
}

impl TryFrom<ActivityRow> for StoredActivity {
    type Error = MatchmakingRepositoryError;

    fn try_from(row: ActivityRow) -> Result<Self, Self::Error> {
        Self::try_from_row(row).map_err(MatchmakingRepositoryError::Database)
    }
}

#[derive(FromRow)]
struct PlayerRow {
    id: Uuid,
    display_name: String,
    rating: i16,
}

impl PlayerRow {
    fn into_domain(self) -> Result<Player, RepositoryError> {
        let rating = u16::try_from(self.rating).map_err(|_| {
            RepositoryError::Database(sqlx::Error::Protocol(
                "invalid stored player rating".to_owned(),
            ))
        })?;
        Ok(Player::restore(self.id, self.display_name, rating))
    }
}

#[derive(FromRow)]
struct LobbyRow {
    id: Uuid,
    owner_player_id: Option<Uuid>,
    capacity: i16,
    status: String,
}

#[derive(FromRow)]
struct LobbyMemberRow {
    player_id: Uuid,
    ready: bool,
}

#[derive(FromRow)]
struct QueueRow {
    player_id: Uuid,
    rating: i16,
    mode: String,
    enqueued_at_seconds: i64,
}

#[derive(FromRow)]
struct MatchRow {
    id: Uuid,
    mode: String,
    status: String,
}

#[derive(FromRow)]
struct MatchPlayerRow {
    player_id: Uuid,
    team: i16,
    rating: i16,
}

#[derive(FromRow)]
struct ActivityRow {
    activity_type: String,
    activity_id: Option<Uuid>,
}

#[derive(Debug, Error)]
pub enum PostgresInitializationError {
    #[error("failed to connect to PostgreSQL")]
    Connect(#[source] sqlx::Error),
    #[error("failed to apply PostgreSQL migrations")]
    Migrate(#[source] sqlx::migrate::MigrateError),
}
