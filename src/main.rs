use anyhow::Result;
use tracing::info;

mod config;
mod detector;
mod layer;
mod renderer;
mod session;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("Surfterm v{} starting", env!("CARGO_PKG_VERSION"));

    // Phase 1: winit event loop will be initialized here
    info!("Surfterm initialized successfully");

    Ok(())
}
