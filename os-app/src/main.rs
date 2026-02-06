//! OpenCraw main binary.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

mod assistant;
mod commands;
mod config;
mod dev_backends;
mod gateway;
mod pairing;
mod routes;
mod server;
mod session;
mod setup;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "opencraw", version, about = "OpenCraw personal AI assistant")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the OpenCraw server (default).
    Serve {
        /// Path to config file. Defaults to ~/.opencraw/config.toml
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Validate config and perform basic health checks.
    Doctor {
        /// Path to config file. Defaults to ~/.opencraw/config.toml
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// One-shot send to a recipient via a configured channel.
    Send {
        channel: String,
        recipient: String,
        message: String,
        /// Path to config file. Defaults to ~/.opencraw/config.toml
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Serve { config: None }) {
        Command::Serve { config } => server::serve(config).await,
        Command::Doctor { config } => server::doctor(config).await,
        Command::Send {
            channel,
            recipient,
            message,
            config,
        } => server::send_one_shot(config, &channel, &recipient, &message).await,
    }
}
