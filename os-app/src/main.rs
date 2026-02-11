//! OpenCraw main binary.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

mod assistant;
mod automation_runtime;
mod channel_plugins;
mod commands;
mod config;
mod config_control;
mod dev_backends;
mod discovery_runtime;
mod gateway;
mod http_auth;
mod init;
mod pairing;
mod routes;
mod server;
mod session;
mod setup;
mod skills_runtime;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;

#[derive(Debug, Parser)]
#[command(name = "opencraw", version, about = "OpenCraw personal AI assistant")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the OpenCraw server (default).
    Serve,
    /// Initialize ~/.opencraw with local config templates (idempotent).
    Init,
    /// Validate config and perform basic health checks.
    Doctor,
    /// Show current runtime status and health summary.
    Status,
    /// One-shot send to a recipient via a configured channel.
    Send {
        channel: String,
        recipient: String,
        message: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing()?;
    install_panic_hook();

    let cli = Cli::parse();

    let command = if let Some(command) = cli.command {
        command
    } else {
        Command::Serve
    };

    match command {
        Command::Serve => server::serve(None).await,
        Command::Init => {
            let report = init::initialize_default().await?;
            if report.created.is_empty() {
                println!(
                    "opencraw init: already initialized at {}",
                    report.root.display()
                );
            } else {
                println!("opencraw init: initialized {}", report.root.display());
                for path in &report.created {
                    println!("created {}", path.display());
                }
                if !report.skipped.is_empty() {
                    println!("kept {} existing file(s) unchanged", report.skipped.len());
                }
            }
            println!("next: edit local configs under {}", report.root.display());
            Ok(())
        }
        Command::Doctor => server::doctor(None).await,
        Command::Status => server::status(None).await,
        Command::Send {
            channel,
            recipient,
            message,
        } => server::send_one_shot(None, &channel, &recipient, &message).await,
    }
}

fn init_tracing() -> anyhow::Result<()> {
    let env_filter = match EnvFilter::try_from_default_env() {
        Ok(v) => v,
        Err(_) => EnvFilter::new(
            "info,opencraw=debug,os_app=debug,os_channels=debug,os_llm=debug,tower_http=info",
        ),
    };
    let log_format = std::env::var("OPENCRAW_LOG_FORMAT")
        .unwrap_or_else(|_| "json".to_string())
        .to_ascii_lowercase();

    match log_format.as_str() {
        "json" => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                .with_target(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_file(true)
                .with_line_number(true)
                .json()
                .flatten_event(true)
                .with_current_span(true)
                .with_span_list(true)
                .init();
        }
        "pretty" => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                .with_target(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_file(true)
                .with_line_number(true)
                .pretty()
                .init();
        }
        "compact" => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                .with_target(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_file(true)
                .with_line_number(true)
                .compact()
                .init();
        }
        other => {
            return Err(anyhow::anyhow!(
                "unsupported OPENCRAW_LOG_FORMAT={other:?}; expected one of: json, pretty, compact"
            ));
        }
    }

    tracing::info!(
        log_format = %log_format,
        env_filter = ?std::env::var("RUST_LOG").ok(),
        "tracing initialized"
    );
    Ok(())
}

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let location = panic_info
            .location()
            .map(|loc| format!("{}:{}", loc.file(), loc.line()))
            .unwrap_or_else(|| "unknown".to_string());
        let payload = panic_payload_to_string(panic_info.payload());
        tracing::error!(
            panic_location = %location,
            panic_payload = %payload,
            "panic captured"
        );
        default_hook(panic_info);
    }));
}

fn panic_payload_to_string(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        return msg.to_string();
    }
    if let Some(msg) = payload.downcast_ref::<String>() {
        return msg.clone();
    }
    "non-string panic payload".to_string()
}
