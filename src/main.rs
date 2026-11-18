use std::{error::Error, sync::Arc};

use arena_core::{
    app::build_app, config::Config, service::player_service::PlayerService,
    storage::memory::InMemoryPlayerRepository,
};
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();

    let config = Config::from_env()?;
    let address = config.socket_address();
    let repository = Arc::new(InMemoryPlayerRepository::new());
    let player_service = PlayerService::new(repository);
    let app = build_app(player_service);
    let listener = TcpListener::bind(address).await?;

    tracing::info!(%address, "ArenaCore server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
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
