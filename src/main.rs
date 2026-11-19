use std::{error::Error, sync::Arc, time::Duration};

use arena_core::{
    app::{build_app, AppState},
    config::Config,
    service::{
        dev_session_service::DevSessionService,
        lobby_service::LobbyService,
        matchmaking_service::{MatchmakingService, SystemClock},
        notification_service::NotificationHub,
        player_service::PlayerService,
    },
    storage::{
        activity::PlayerActivityRegistry, lobby_memory::InMemoryLobbyRepository,
        matchmaking_memory::InMemoryMatchmakingRepository, memory::InMemoryPlayerRepository,
        LobbyRepository, MatchmakingRepository, PlayerRepository,
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
    let activity_registry = Arc::new(PlayerActivityRegistry::new());
    let player_repository: Arc<dyn PlayerRepository> = Arc::new(InMemoryPlayerRepository::new());
    let lobby_repository: Arc<dyn LobbyRepository> =
        Arc::new(InMemoryLobbyRepository::new(Arc::clone(&activity_registry)));
    let matchmaking_repository: Arc<dyn MatchmakingRepository> =
        Arc::new(InMemoryMatchmakingRepository::new(activity_registry));
    let player_service = PlayerService::new(Arc::clone(&player_repository));
    let notification_hub = Arc::new(NotificationHub::default());
    let lobby_service = LobbyService::new(
        Arc::clone(&player_repository),
        lobby_repository,
        Arc::clone(&notification_hub),
    );
    let matchmaking_service = MatchmakingService::new(
        Arc::clone(&player_repository),
        matchmaking_repository,
        Arc::new(SystemClock),
        config.matchmaking_config(),
        Arc::clone(&notification_hub),
    );
    let dev_session_service = DevSessionService::new(player_repository);
    let app = build_app(AppState {
        player_service,
        lobby_service,
        matchmaking_service: matchmaking_service.clone(),
        dev_session_service,
        notification_hub,
        websocket_config: config.websocket_config(),
    });
    let listener = TcpListener::bind(address).await?;
    let matchmaker_task = spawn_matchmaker(matchmaking_service);

    tracing::info!(%address, "ArenaCore server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    matchmaker_task.abort();

    Ok(())
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
