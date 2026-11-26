use std::{error::Error, net::SocketAddr, sync::Arc, time::Duration};

use arena_core::{
    app::{build_app, AppState},
    config::{Config, StorageBackend},
    operations::{OperationalMetrics, RateLimiter},
    service::{
        dev_session_service::DevSessionService,
        lobby_service::LobbyService,
        match_lifecycle_service::MatchLifecycleService,
        matchmaking_service::{MatchmakingService, SystemClock},
        notification_service::NotificationHub,
        player_service::PlayerService,
    },
    storage::{
        activity::PlayerActivityRegistry,
        lobby_memory::InMemoryLobbyRepository,
        match_lifecycle_memory::InMemoryMatchLifecycleRepository,
        matchmaking_memory::InMemoryMatchmakingRepository,
        memory::InMemoryPlayerRepository,
        postgres::PostgresRepository,
        readiness::{InMemoryReadiness, PostgresReadiness, ReadinessProbe},
        LobbyRepository, MatchLifecycleRepository, MatchmakingRepository, PlayerRepository,
    },
};
use tokio::{
    net::TcpListener,
    task::JoinHandle,
    time::{self, MissedTickBehavior},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();

    let config = Config::from_env()?;
    let address = config.socket_address();
    let repositories = match config.database_config().backend() {
        StorageBackend::Memory => {
            let activity_registry = Arc::new(PlayerActivityRegistry::new());
            let players = Arc::new(InMemoryPlayerRepository::new());
            let matchmaking = Arc::new(InMemoryMatchmakingRepository::new(Arc::clone(
                &activity_registry,
            )));
            let match_lifecycle = Arc::new(InMemoryMatchLifecycleRepository::new(
                Arc::clone(&players),
                Arc::clone(&matchmaking),
                Arc::clone(&activity_registry),
            ));
            RepositorySet {
                players,
                lobbies: Arc::new(InMemoryLobbyRepository::new(Arc::clone(&activity_registry))),
                matchmaking,
                match_lifecycle,
                readiness: Arc::new(InMemoryReadiness),
            }
        }
        StorageBackend::Postgres => {
            let database = config.database_config();
            let repository = Arc::new(
                PostgresRepository::connect(
                    database
                        .url()
                        .expect("PostgreSQL URL is validated during configuration"),
                    database.max_connections(),
                    database.acquire_timeout(),
                )
                .await?,
            );
            RepositorySet {
                players: repository.clone(),
                lobbies: repository.clone(),
                matchmaking: repository.clone(),
                match_lifecycle: repository.clone(),
                readiness: Arc::new(PostgresReadiness::new(repository.pool().clone())),
            }
        }
    };
    let player_service = PlayerService::new(Arc::clone(&repositories.players));
    let notification_hub = Arc::new(NotificationHub::default());
    let lobby_service = LobbyService::new(
        Arc::clone(&repositories.players),
        repositories.lobbies,
        Arc::clone(&notification_hub),
    );
    let matchmaking_service = MatchmakingService::new(
        Arc::clone(&repositories.players),
        repositories.matchmaking,
        Arc::new(SystemClock),
        config.matchmaking_config(),
        Arc::clone(&notification_hub),
    );
    let match_lifecycle_service = MatchLifecycleService::new(
        repositories.match_lifecycle,
        config.rating_config(),
        Arc::clone(&notification_hub),
    );
    let dev_session_service = DevSessionService::new(repositories.players);
    let metrics = Arc::new(OperationalMetrics::default());
    let rate_limiter = Arc::new(RateLimiter::new(
        config.rate_limit_config(),
        Arc::clone(&metrics),
    ));
    let app = build_app(AppState {
        player_service,
        lobby_service,
        matchmaking_service: matchmaking_service.clone(),
        match_lifecycle_service,
        dev_session_service,
        notification_hub,
        websocket_config: config.websocket_config(),
        readiness_probe: repositories.readiness,
        metrics,
        rate_limiter,
    });
    let listener = TcpListener::bind(address).await?;
    let matchmaker_task = spawn_matchmaker(matchmaking_service);

    tracing::info!(%address, "ArenaCore server listening");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    matchmaker_task.abort();

    Ok(())
}

struct RepositorySet {
    players: Arc<dyn PlayerRepository>,
    lobbies: Arc<dyn LobbyRepository>,
    matchmaking: Arc<dyn MatchmakingRepository>,
    match_lifecycle: Arc<dyn MatchLifecycleRepository>,
    readiness: Arc<dyn ReadinessProbe>,
}

fn spawn_matchmaker(service: MatchmakingService) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            match service.run_pending().await {
                Ok(0) => {}
                Ok(count) => tracing::info!(count, "background matchmaking formed matches"),
                Err(error) => tracing::error!(%error, "background matchmaking failed"),
            }
        }
    })
}

fn init_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("arena_core=info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

async fn shutdown_signal() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => tracing::info!("shutdown signal received"),
        Err(error) => tracing::error!(%error, "failed to listen for shutdown signal"),
    }
}
