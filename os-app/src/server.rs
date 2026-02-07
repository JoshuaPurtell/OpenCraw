//! OpenShell server.
//!
//! Builds a Horizons `AppState` (dev backends) and mounts OpenShell routes on top.
//! See: specifications/openshell/implementation_v0_1_0.md

use crate::assistant::AssistantAgent;
use crate::config::OpenShellConfig;
use crate::config_control::ConfigControl;
use crate::dev_backends;
use crate::gateway::Gateway;
use crate::routes;
use crate::session::SessionManager;
use anyhow::Result;
use axum::http::HeaderMap;
use axum::http::Request;
use axum::response::Response;
use os_channels::{
    ChannelAdapter, DiscordAdapter, EmailAdapter, ImessageAdapter, LinearAdapter, TelegramAdapter,
    WebChatAdapter,
};
use os_llm::validate_tool_name_all_providers;
use os_tools::{
    BrowserTool, ClipboardTool, EmailTool, FilesystemTool, ImessageTool, ShellTool, Tool,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_http::classify::ServerErrorsFailureClass;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

pub struct OsState {
    pub config_control: Arc<ConfigControl>,
    pub org_id: horizons_core::OrgId,
    pub channels: HashMap<String, Arc<dyn ChannelAdapter>>,
    pub sessions: Arc<SessionManager>,
    pub memory: Option<Arc<dyn horizons_core::memory::traits::HorizonsMemory>>,
}

pub async fn doctor(config_path: Option<PathBuf>) -> Result<()> {
    let (cfg, path) = OpenShellConfig::load_with_path(config_path).await?;
    tracing::info!(
        model = %cfg.general.model,
        runtime_mode = ?cfg.runtime.mode,
        runtime_data_dir = %cfg.runtime.data_dir,
        config_path = %path.display(),
        "config ok"
    );
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
        "telegram" => Arc::new(TelegramAdapter::new(&cfg.channels.telegram.bot_token)?),
        "discord" => Arc::new(DiscordAdapter::new(&cfg.channels.discord.bot_token)?),
        "imessage" => {
            let source_db = cfg
                .channels
                .imessage
                .source_db
                .clone()
                .map(|p| expand_home(&p))
                .transpose()?
                .ok_or_else(|| anyhow::anyhow!("channels.imessage.source_db is required"))?;
            Arc::new(ImessageAdapter::new(source_db))
        }
        "email" => Arc::new(EmailAdapter::new(&cfg.channels.email.gmail_access_token)?),
        "linear" => Arc::new(LinearAdapter::new(&cfg.channels.linear.api_key)?),
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
    let (cfg, cfg_path) = OpenShellConfig::load_with_path(config_path).await?;
    let started_at = Instant::now();
    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.channels.webchat.port));
    tracing::info!(
        runtime_mode = ?cfg.runtime.mode,
        runtime_data_dir = %cfg.runtime.data_dir,
        bind_addr = %addr,
        model = %cfg.general.model,
        webchat_enabled = cfg.channels.webchat.enabled,
        telegram_enabled = cfg.channels.telegram.enabled,
        discord_enabled = cfg.channels.discord.enabled,
        imessage_enabled = cfg.channels.imessage.enabled,
        email_enabled = cfg.channels.email.enabled,
        linear_enabled = cfg.channels.linear.enabled,
        queue_mode = ?cfg.queue.mode,
        queue_max_concurrency = cfg.queue.max_concurrency,
        queue_lane_buffer = cfg.queue.lane_buffer,
        queue_debounce_ms = cfg.queue.debounce_ms,
        context_max_prompt_tokens = cfg.context.max_prompt_tokens,
        context_min_recent_messages = cfg.context.min_recent_messages,
        context_max_tool_chars = cfg.context.max_tool_chars,
        "server configuration loaded"
    );
    let listener = preflight_bind_listener(addr).await?;

    let data_dir = cfg.runtime.data_dir_path()?;
    let runtime = dev_backends::build_runtime(&cfg, &data_dir).await?;
    let config_control = Arc::new(ConfigControl::new(cfg_path, cfg.clone()));

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
    if cfg.channels.email.enabled {
        tools.push(Arc::new(EmailTool::new(
            &cfg.channels.email.gmail_access_token,
            cfg.channels.email.query.clone(),
        )?));
    }
    if cfg.channels.imessage.enabled {
        let source_db = cfg
            .channels
            .imessage
            .source_db
            .clone()
            .map(|p| expand_home(&p))
            .transpose()?
            .ok_or_else(|| anyhow::anyhow!("channels.imessage.source_db is required"))?;
        tools.push(Arc::new(ImessageTool::new(source_db)?));
    }
    preflight_validate_tool_names(&tools)?;

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

    if cfg.channels.telegram.enabled {
        let tg = Arc::new(TelegramAdapter::new(&cfg.channels.telegram.bot_token)?);
        tg.start(inbound_tx.clone()).await?;
        channels.insert("telegram".to_string(), tg);
    }

    if cfg.channels.discord.enabled {
        let dc = Arc::new(DiscordAdapter::new(&cfg.channels.discord.bot_token)?);
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
            .ok_or_else(|| anyhow::anyhow!("channels.imessage.source_db is required"))?;

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

    if cfg.channels.email.enabled {
        let email = Arc::new(
            EmailAdapter::new(&cfg.channels.email.gmail_access_token)?
                .with_poll_interval(std::time::Duration::from_millis(
                    cfg.channels.email.poll_interval_ms,
                ))
                .with_query(cfg.channels.email.query.clone())
                .with_start_from_latest(cfg.channels.email.start_from_latest)
                .with_mark_processed_as_read(cfg.channels.email.mark_processed_as_read),
        );
        email.start(inbound_tx.clone()).await?;
        channels.insert("email".to_string(), email);
    }

    if cfg.channels.linear.enabled {
        let linear = Arc::new(
            LinearAdapter::new(&cfg.channels.linear.api_key)?
                .with_poll_interval(std::time::Duration::from_millis(
                    cfg.channels.linear.poll_interval_ms,
                ))
                .with_team_ids(cfg.channels.linear.team_ids.clone())
                .with_start_from_latest(cfg.channels.linear.start_from_latest),
        );
        linear.start(inbound_tx.clone()).await?;
        channels.insert("linear".to_string(), linear);
    }

    let llm = Some(os_llm::LlmClient::new(
        &cfg.api_key_for_model()?,
        &cfg.general.model,
    )?);

    let sessions = Arc::new(
        SessionManager::load_or_new(
            runtime.project_db.clone(),
            runtime.org_id,
            runtime.project_db_handle.clone(),
        )
        .await?,
    );
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
    tracing::info!(
        channel_count = channels.len(),
        channels = ?channels.keys().collect::<Vec<_>>(),
        "gateway started"
    );

    let os_state = Arc::new(OsState {
        config_control,
        org_id: runtime.org_id,
        channels: channels.clone(),
        sessions: sessions.clone(),
        memory: runtime.memory.clone(),
    });

    let mut os_router = routes::router().layer(axum::Extension(os_state.clone()));
    if let Some(webchat) = webchat_adapter {
        os_router = os_router.merge(webchat.clone().router());
    }

    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &Request<_>| {
            tracing::info_span!(
                "http.request",
                method = %request.method(),
                uri = %request.uri(),
                version = ?request.version(),
                request_id = %request_id_from_headers(request.headers())
            )
        })
        .on_request(|request: &Request<_>, _span: &tracing::Span| {
            tracing::info!(
                method = %request.method(),
                uri = %request.uri(),
                request_id = %request_id_from_headers(request.headers()),
                "http request started"
            );
        })
        .on_response(
            |response: &Response, latency: Duration, _span: &tracing::Span| {
                tracing::info!(
                    status = response.status().as_u16(),
                    latency_ms = latency.as_millis() as u64,
                    "http request completed"
                );
            },
        )
        .on_failure(
            |error: ServerErrorsFailureClass, latency: Duration, _span: &tracing::Span| {
                tracing::error!(
                    error_class = %error,
                    latency_ms = latency.as_millis() as u64,
                    "http request failed"
                );
            },
        );

    let app = horizons_rs::server::router(runtime.horizons_state.clone())
        .merge(os_router)
        .layer(trace_layer)
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid));

    tracing::info!(%addr, "opencraw serving");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn preflight_bind_listener(addr: SocketAddr) -> Result<tokio::net::TcpListener> {
    tracing::info!(%addr, "preflight bind check starting");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("preflight bind failed for {addr}: {e}"))?;
    tracing::info!(%addr, "preflight bind check passed");
    Ok(listener)
}

fn preflight_validate_tool_names(tools: &[Arc<dyn Tool>]) -> Result<()> {
    tracing::info!(
        tool_count = tools.len(),
        "preflight tool name validation starting"
    );
    for tool in tools {
        let spec = tool.spec();
        validate_tool_name_all_providers(&spec.name).map_err(|e| {
            anyhow::anyhow!(
                "preflight tool name validation failed for '{}': {e}",
                spec.name
            )
        })?;
    }
    tracing::info!(
        tool_count = tools.len(),
        "preflight tool name validation passed"
    );
    Ok(())
}

fn expand_home(path: &str) -> Result<std::path::PathBuf> {
    let trimmed = path.trim().to_string();
    if !trimmed.starts_with("~/") {
        return Ok(std::path::PathBuf::from(trimmed));
    }
    let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?;
    Ok(std::path::PathBuf::from(trimmed.replacen("~", &home, 1)))
}

fn request_id_from_headers(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string())
        .unwrap_or_else(|| "missing".to_string())
}
