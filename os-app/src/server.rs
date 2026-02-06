//! OpenShell server.
//!
//! Builds a Horizons `AppState` (dev backends) and mounts OpenShell routes on top.
//! See: specifications/openshell/implementation_v0_1_0.md

use crate::assistant::AssistantAgent;
use crate::config::OpenShellConfig;
use crate::dev_backends;
use crate::gateway::Gateway;
use crate::routes;
use crate::session::SessionManager;
use anyhow::Result;
use os_channels::{ChannelAdapter, DiscordAdapter, ImessageAdapter, TelegramAdapter, WebChatAdapter};
use os_tools::{BrowserTool, ClipboardTool, FilesystemTool, ShellTool, Tool};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub struct OsState {
    pub cfg: OpenShellConfig,
    pub org_id: horizons_core::OrgId,
    pub project_id: horizons_core::ProjectId,
    pub project_db_handle: horizons_core::ProjectDbHandle,
    pub channels: HashMap<String, Arc<dyn ChannelAdapter>>,
    pub sessions: Arc<SessionManager>,
    pub memory: Option<Arc<dyn horizons_core::memory::traits::HorizonsMemory>>,
}

pub async fn doctor(config_path: Option<PathBuf>) -> Result<()> {
    let cfg = OpenShellConfig::load(config_path).await?;
    tracing::info!(model = %cfg.general.model, "config ok");
    Ok(())
}

pub async fn send_one_shot(
    config_path: Option<PathBuf>,
    channel: &str,
    recipient: &str,
    message: &str,
) -> Result<()> {
    let cfg = OpenShellConfig::load(config_path).await?;
    let adapter: Arc<dyn ChannelAdapter> = match channel {
        "telegram" => Arc::new(TelegramAdapter::new(&cfg.channels.telegram.bot_token)),
        "discord" => Arc::new(DiscordAdapter::new(&cfg.channels.discord.bot_token)),
        "imessage" => Arc::new(ImessageAdapter::new(ImessageAdapter::default_source_db())),
        other => return Err(anyhow::anyhow!("unknown channel: {other}")),
    };
    adapter
        .send(
            recipient,
            os_channels::OutboundMessage {
                content: message.to_string(),
                reply_to_message_id: None,
                attachments: vec![],
            },
        )
        .await?;
    Ok(())
}

pub async fn serve(config_path: Option<PathBuf>) -> Result<()> {
    let cfg = OpenShellConfig::load(config_path).await?;
    let started_at = Instant::now();

    let data_dir = PathBuf::from("data");
    let runtime = dev_backends::build_dev_runtime(&cfg, &data_dir).await?;

    // Tools.
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
    if cfg.tools.shell {
        tools.push(Arc::new(ShellTool::new(std::time::Duration::from_secs(30))));
    }
    if cfg.tools.filesystem {
        tools.push(Arc::new(FilesystemTool::new(std::env::current_dir()?)?));
    }
    if cfg.tools.clipboard {
        tools.push(Arc::new(ClipboardTool::new()));
    }
    if cfg.tools.browser {
        tools.push(Arc::new(BrowserTool::new()));
    }

    // Channels.
    let (inbound_tx, inbound_rx) = tokio::sync::mpsc::channel(1024);
    let mut channels: HashMap<String, Arc<dyn ChannelAdapter>> = HashMap::new();

    let mut webchat_adapter: Option<Arc<WebChatAdapter>> = None;
    if cfg.channels.webchat.enabled {
        let webchat = Arc::new(WebChatAdapter::new());
        webchat.start(inbound_tx.clone()).await?;
        channels.insert("webchat".to_string(), webchat.clone());
        webchat_adapter = Some(webchat);
    }

    if cfg.channels.telegram.enabled && !cfg.channels.telegram.bot_token.trim().is_empty() {
        let tg = Arc::new(TelegramAdapter::new(&cfg.channels.telegram.bot_token));
        tg.start(inbound_tx.clone()).await?;
        channels.insert("telegram".to_string(), tg);
    }

    if cfg.channels.discord.enabled && !cfg.channels.discord.bot_token.trim().is_empty() {
        let dc = Arc::new(DiscordAdapter::new(&cfg.channels.discord.bot_token));
        dc.start(inbound_tx.clone()).await?;
        channels.insert("discord".to_string(), dc);
    }

    if cfg.channels.imessage.enabled {
        let source_db = cfg
            .channels
            .imessage
            .source_db
            .clone()
            .map(|p| expand_home(&p))
            .transpose()?
            .unwrap_or_else(ImessageAdapter::default_source_db);

        let im = Arc::new(
            ImessageAdapter::new(source_db)
                .with_poll_interval(std::time::Duration::from_millis(
                    cfg.channels.imessage.poll_interval_ms,
                ))
                .with_start_from_latest(cfg.channels.imessage.start_from_latest)
                .with_group_prefixes(cfg.channels.imessage.group_prefixes.clone()),
        );
        im.start(inbound_tx.clone()).await?;
        channels.insert("imessage".to_string(), im);
    }

    let llm = cfg
        .api_key_for_model()
        .map(|key| os_llm::LlmClient::new(&key, &cfg.general.model));

    let sessions = Arc::new(SessionManager::new());
    let assistant = Arc::new(AssistantAgent::new(
        cfg.clone(),
        llm,
        tools,
        runtime.memory.clone(),
        runtime.project_db.clone(),
        runtime.core_agents.clone(),
        runtime.org_id,
        runtime.project_id,
        runtime.project_db_handle.clone(),
        runtime.evaluation.clone(),
    ));

    let gateway = Arc::new(Gateway::new(
        cfg.clone(),
        started_at,
        sessions.clone(),
        assistant,
        channels.clone(),
        inbound_rx,
    ));
    gateway.start();

    let os_state = Arc::new(OsState {
        cfg: cfg.clone(),
        org_id: runtime.org_id,
        project_id: runtime.project_id,
        project_db_handle: runtime.project_db_handle.clone(),
        channels: channels.clone(),
        sessions: sessions.clone(),
        memory: runtime.memory.clone(),
    });

    let mut os_router = routes::router().layer(axum::Extension(os_state.clone()));
    if let Some(webchat) = webchat_adapter {
        os_router = os_router.merge(webchat.clone().router());
    }

    let app = horizons_rs::server::router(runtime.horizons_state.clone()).merge(os_router);

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.channels.webchat.port));
    tracing::info!(%addr, "opencraw serving");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn expand_home(path: &str) -> Result<std::path::PathBuf> {
    let trimmed = path.trim().to_string();
    if !trimmed.starts_with("~/") {
        return Ok(std::path::PathBuf::from(trimmed));
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Ok(std::path::PathBuf::from(trimmed.replacen("~", &home, 1)))
}
