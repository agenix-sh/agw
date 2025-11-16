use anyhow::Result;
use clap::Parser;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

mod config;
mod error;
mod executor;
mod plan;
mod resp;
mod worker;

use config::Config;
use worker::Worker;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing subscriber
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // Parse CLI arguments
    let config = Config::parse();

    info!("AGW v{} starting...", env!("CARGO_PKG_VERSION"));

    // Create and run worker
    let worker = Worker::new(config).await?;
    worker.run().await?;

    Ok(())
}
