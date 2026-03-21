use anyhow::Result;
use tracing::info;

mod app;
mod ble;
mod config;
mod detector;
mod input;
mod layer;
mod llm;
mod menu;
mod preview;
mod renderer;
mod session;
mod shell;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("Surfterm v{} starting", env!("CARGO_PKG_VERSION"));

    app::run()?;

    Ok(())
}
