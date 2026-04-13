use clap::Parser;
use snodus_core::cli::{Cli, Commands};
use snodus_core::{build_router, build_state, config::GatewayConfig};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("snodus=info,snodus_core=info")),
        )
        .init();

    let cli = Cli::parse();
    let config = GatewayConfig::from_env().map_err(|e| anyhow::anyhow!(e))?;
    let pool = snodus_core::state::create_pool(&config.database_url).await?;
    snodus_core::state::run_core_migrations(&pool).await?;

    match cli.command {
        Some(Commands::Serve) | None => {}
        Some(cmd) => {
            snodus_core::cli::run_cli(cmd, &pool).await?;
            return Ok(());
        }
    }

    tracing::info!(
        "Starting snodus-core v{} (open source)",
        env!("CARGO_PKG_VERSION")
    );

    let state = build_state(config.clone(), pool).await?;
    let app = build_router(state);

    let addr = config.listen_addr();
    tracing::info!("Listening on {addr}");
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    tracing::info!("Shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C handler");
    tracing::info!("Received shutdown signal");
}
