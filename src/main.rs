mod cli;
mod config;
mod core;
mod registry;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    let log_level = if cli.verbose { "debug" } else { "info" };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    fmt().with_env_filter(filter).with_target(false).init();

    cli::dispatch(cli).await
}
