//! OpenShell server.
//!
//! Builds a Horizons `AppState` (dev backends) and mounts OpenShell routes on top.
//! See: specifications/openshell/implementation_v0_1_0.md

use crate::assistant::AssistantAgent;
use crate::automation_runtime::AutomationRuntime;
use crate::channel_plugins;
use crate::config::{
    OpenShellConfig, ShellExecutionMode as ConfigShellExecutionMode,
    ShellSandboxBackend as ConfigShellSandboxBackend,
};
use crate::config_control::ConfigControl;
use crate::dev_backends;
use crate::discovery_runtime::DiscoveryRuntime;
use crate::gateway::Gateway;
use crate::http_auth;
use crate::routes;
use crate::session::SessionManager;
use crate::skills_runtime::SkillsRuntime;
use anyhow::Result;
use axum::Extension;
use axum::http::HeaderMap;
use axum::http::Request;
use axum::http::StatusCode;
use axum::response::Response;
use os_channels::{
    ChannelAdapter, DiscordAdapter, EmailAdapter, HttpPluginAdapter, ImessageAdapter,
    LinearAdapter, MatrixAdapter, SignalAdapter, SlackAdapter, TelegramAdapter,
    WhatsAppCloudAdapter,
};
use os_llm::validate_tool_name_all_providers;
use os_tools::{
    ApplyPatchTool, BrowserTool, ClipboardTool, EmailActionToggles, EmailTool, FilesystemTool,
    ImessageActionToggles, ImessageTool, LinearActionToggles, LinearLimits, LinearTool,
    LinearToolConfig, ShellExecutionMode as ToolShellExecutionMode, ShellPolicy,
    ShellSandboxBackend as ToolShellSandboxBackend, ShellTool, Tool,
};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use tower::limit::GlobalConcurrencyLimitLayer;
use tower_http::classify::ServerErrorsFailureClass;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

pub struct OsState {
    pub config_control: Arc<ConfigControl>,
    pub org_id: horizons_core::OrgId,
    pub channels: HashMap<String, Arc<dyn ChannelAdapter>>,
    pub channel_capability_matrix: HashMap<String, crate::channel_plugins::ChannelCapabilitySchema>,
    pub automation: Arc<AutomationRuntime>,
    pub discovery: Arc<DiscoveryRuntime>,
    pub skills: Arc<SkillsRuntime>,
    pub sessions: Arc<SessionManager>,
    pub memory: Option<Arc<dyn horizons_core::memory::traits::HorizonsMemory>>,
}

pub async fn doctor(config_path: Option<PathBuf>) -> Result<()> {
    let (cfg, path) = OpenShellConfig::load_with_path(config_path).await?;
    let model = cfg.default_model()?;
    tracing::info!(
        model = %model,
        llm_active_profile = %cfg.llm.active_profile,
        runtime_mode = ?cfg.runtime.mode,
        runtime_data_dir = %cfg.runtime.data_dir,
        config_path = %path.display(),
        "config ok"
    );
    Ok(())
}

pub async fn status(config_path: Option<PathBuf>) -> Result<()> {
    let (cfg, path) = OpenShellConfig::load_with_path(config_path).await?;
    let model = cfg.default_model()?;
    tracing::info!(
        model = %model,
        llm_active_profile = %cfg.llm.active_profile,
        runtime_mode = ?cfg.runtime.mode,
        runtime_data_dir = %cfg.runtime.data_dir,
        config_path = %path.display(),
        "status ok"
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
        "slack" => Arc::new(SlackAdapter::new(&cfg.channels.slack.bot_token)?),
        "matrix" => Arc::new(MatrixAdapter::new(
            &cfg.channels.matrix.homeserver_url,
            &cfg.channels.matrix.access_token,
            &cfg.channels.matrix.user_id,
        )?),
        "signal" => Arc::new(
            SignalAdapter::new(
                &cfg.channels.signal.api_base_url,
                &cfg.channels.signal.account,
            )?
            .with_api_token(cfg.channels.signal.api_token.clone())
            .with_poll_interval(Duration::from_millis(cfg.channels.signal.poll_interval_ms))
            .with_start_from_latest(cfg.channels.signal.start_from_latest)
            .with_receive_timeout_seconds(cfg.channels.signal.receive_timeout_seconds),
        ),
        "whatsapp" => Arc::new(WhatsAppCloudAdapter::new(
            &cfg.channels.whatsapp.access_token,
            &cfg.channels.whatsapp.phone_number_id,
        )?),
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
        other => {
            let Some(plugin_cfg) = cfg
                .channels
                .external_plugins
                .iter()
                .find(|plugin| plugin.enabled && plugin.id.trim().eq_ignore_ascii_case(other))
            else {
                return Err(anyhow::anyhow!("unknown channel: {other}"));
            };
            Arc::new(
                HttpPluginAdapter::new(plugin_cfg.id.trim(), &plugin_cfg.send_url)?
                    .with_poll_url(plugin_cfg.poll_url.clone())?
                    .with_auth_token(plugin_cfg.auth_token.clone())
                    .with_poll_interval(Duration::from_millis(plugin_cfg.poll_interval_ms))
                    .with_start_from_latest(plugin_cfg.start_from_latest)
                    .with_capabilities(
                        plugin_cfg.supports_streaming_deltas,
                        plugin_cfg.supports_typing_events,
                        plugin_cfg.supports_reactions,
                    ),
            )
        }
    };
    adapter
        .send(
            recipient,
            os_channels::OutboundMessage {
                content: message.to_string(),
                reply_to_message_id: None,
                attachments: vec![],
                metadata: serde_json::Value::Null,
            },
        )
        .await?;
    Ok(())
}

pub async fn serve(config_path: Option<PathBuf>) -> Result<()> {
    let (cfg, cfg_path) = OpenShellConfig::load_with_path(config_path).await?;
    let started_at = Instant::now();
    let network_policy = cfg.runtime_network_policy()?;
    let addr = network_policy.bind_addr;
    let model = cfg.default_model()?;
    tracing::info!(
        runtime_mode = ?cfg.runtime.mode,
        runtime_data_dir = %cfg.runtime.data_dir,
        runtime_bind_mode = ?cfg.runtime.bind_mode,
        runtime_bind_addr_override = ?cfg.runtime.bind_addr,
        runtime_discovery_mode = ?network_policy.discovery_mode,
        runtime_exposure = ?network_policy.exposure,
        runtime_public_ingress = network_policy.public_ingress,
        runtime_control_api_auth_configured = network_policy.control_api_auth_configured,
        runtime_allow_public_bind_without_auth = network_policy.allow_public_bind_without_auth,
        runtime_advertised_base_url = ?network_policy.advertised_base_url,
        runtime_http_timeout_seconds = cfg.runtime.http_timeout_seconds,
        runtime_http_max_in_flight = cfg.runtime.http_max_in_flight,
        bind_addr = %addr,
        model = %model,
        llm_active_profile = %cfg.llm.active_profile,
        webchat_enabled = cfg.channels.webchat.enabled,
        telegram_enabled = cfg.channels.telegram.enabled,
        discord_enabled = cfg.channels.discord.enabled,
        slack_enabled = cfg.channels.slack.enabled,
        matrix_enabled = cfg.channels.matrix.enabled,
        signal_enabled = cfg.channels.signal.enabled,
        whatsapp_enabled = cfg.channels.whatsapp.enabled,
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
        context_tool_loops_max = cfg.context.tool_loops_max,
        context_tool_max_runtime_seconds = cfg.context.tool_max_runtime_seconds,
        context_tool_no_progress_limit = cfg.context.tool_no_progress_limit,
        "server configuration loaded"
    );
    let listener = preflight_bind_listener(addr).await?;

    let data_dir = cfg.runtime.data_dir_path()?;
    let runtime = dev_backends::build_runtime(&cfg, &data_dir).await?;
    let config_control = Arc::new(ConfigControl::new(cfg_path, cfg.clone())?);

    // Tools.
    let workspace_root = std::env::current_dir()?;
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
    if cfg.tools.tool_enabled("shell_execute") {
        let shell_sandbox_root = cfg
            .tools
            .shell_policy
            .sandbox_root
            .as_deref()
            .map(expand_home)
            .transpose()?
            .unwrap_or_else(|| workspace_root.clone());
        let shell_policy = ShellPolicy {
            default_mode: match cfg.tools.shell_policy.default_mode {
                ConfigShellExecutionMode::Sandbox => ToolShellExecutionMode::Sandbox,
                ConfigShellExecutionMode::Elevated => ToolShellExecutionMode::Elevated,
            },
            allow_elevated: cfg.tools.shell_policy.allow_elevated,
            sandbox_root: shell_sandbox_root,
            sandbox_backend: match cfg.tools.shell_policy.sandbox_backend {
                ConfigShellSandboxBackend::HostConstrained => {
                    ToolShellSandboxBackend::HostConstrained
                }
                ConfigShellSandboxBackend::HorizonsDocker => {
                    ToolShellSandboxBackend::HorizonsDocker
                }
            },
            sandbox_image: cfg.tools.shell_policy.sandbox_image.clone(),
            max_background_processes: cfg.tools.shell_policy.max_background_processes,
        };
        tools.push(Arc::new(ShellTool::new(
            std::time::Duration::from_secs(30),
            shell_policy,
        )));
    }
    if cfg.tools.tool_enabled("filesystem") {
        tools.push(Arc::new(FilesystemTool::new(&workspace_root)?));
    }
    if cfg.tools.tool_enabled("apply_patch") {
        tools.push(Arc::new(ApplyPatchTool::new(&workspace_root)?));
    }
    if cfg.tools.tool_enabled("clipboard") {
        tools.push(Arc::new(ClipboardTool::new()));
    }
    if cfg.tools.tool_enabled("browser") {
        tools.push(Arc::new(BrowserTool::new()?));
    }
    if cfg.tools.tool_enabled("email") && cfg.channels.email.enabled {
        tools.push(Arc::new(EmailTool::new(
            &cfg.channels.email.gmail_access_token,
            cfg.channels.email.query.clone(),
            EmailActionToggles {
                list_labels: cfg.channels.email.actions.list_labels,
                list_inbox: cfg.channels.email.actions.list_inbox,
                search: cfg.channels.email.actions.search,
                read: cfg.channels.email.actions.read,
                send: cfg.channels.email.actions.send,
            },
        )?));
    }
    if cfg.tools.tool_enabled("imessage") && cfg.channels.imessage.enabled {
        let source_db = cfg
            .channels
            .imessage
            .source_db
            .clone()
            .map(|p| expand_home(&p))
            .transpose()?
            .ok_or_else(|| anyhow::anyhow!("channels.imessage.source_db is required"))?;
        tools.push(Arc::new(ImessageTool::new(
            source_db,
            ImessageActionToggles {
                list_recent: cfg.channels.imessage.actions.list_recent,
                send: cfg.channels.imessage.actions.send,
            },
        )?));
    }
    if cfg.tools.tool_enabled("linear") && cfg.channels.linear.enabled {
        tools.push(Arc::new(LinearTool::new(
            &cfg.channels.linear.api_key,
            LinearToolConfig {
                graphql_url: cfg.channels.linear.graphql_url.clone(),
                default_team_id: Some(cfg.channels.linear.default_team_id.clone()),
                action_toggles: LinearActionToggles {
                    whoami: cfg.channels.linear.actions.whoami,
                    list_assigned: cfg.channels.linear.actions.list_assigned,
                    list_users: cfg.channels.linear.actions.list_users,
                    list_teams: cfg.channels.linear.actions.list_teams,
                    list_projects: cfg.channels.linear.actions.list_projects,
                    get_project: cfg.channels.linear.actions.get_project,
                    create_issue: cfg.channels.linear.actions.create_issue,
                    create_project: cfg.channels.linear.actions.create_project,
                    update_project: cfg.channels.linear.actions.update_project,
                    update_issue: cfg.channels.linear.actions.update_issue,
                    assign_issue: cfg.channels.linear.actions.assign_issue,
                    comment_issue: cfg.channels.linear.actions.comment_issue,
                    graphql_query: cfg.channels.linear.actions.graphql_query,
                    graphql_mutation: cfg.channels.linear.actions.graphql_mutation,
                },
                limits: LinearLimits {
                    default_max_results: cfg.channels.linear.limits.default_max_results,
                    max_results_cap: cfg.channels.linear.limits.max_results_cap,
                    reference_lookup_max_results: cfg
                        .channels
                        .linear
                        .limits
                        .reference_lookup_max_results,
                    graphql_max_query_chars: cfg.channels.linear.limits.graphql_max_query_chars,
                    graphql_max_variables_bytes: cfg
                        .channels
                        .linear
                        .limits
                        .graphql_max_variables_bytes,
                },
            },
        )?));
    }
    preflight_validate_tool_names(&tools)?;

    // Channels (plugin-registry loaded).
    let (inbound_tx, inbound_rx) = tokio::sync::mpsc::channel(1024);
    let channel_plugins::ChannelLoadResult {
        channels,
        routers: channel_routers,
        capability_matrix,
    } = channel_plugins::load_enabled_channels(&cfg, inbound_tx.clone()).await?;
    let streaming_channel_count = capability_matrix
        .values()
        .filter(|c| c.supports_streaming_deltas)
        .count();
    let typing_channel_count = capability_matrix
        .values()
        .filter(|c| c.supports_typing_events)
        .count();
    let reaction_channel_count = capability_matrix
        .values()
        .filter(|c| c.supports_reactions)
        .count();
    tracing::info!(
        loaded_channels = channels.len(),
        streaming_channel_count,
        typing_channel_count,
        reaction_channel_count,
        capability_matrix = ?capability_matrix,
        "channel plugins loaded"
    );

    let llm_clients = build_llm_clients(&cfg)?;
    tracing::info!(
        llm_profiles = llm_clients.len(),
        "llm profile chain initialized"
    );

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
        llm_clients,
        tools,
        runtime.memory.clone(),
        runtime.project_db.clone(),
        runtime.core_agents.clone(),
        runtime.org_id,
        runtime.project_id,
        runtime.project_db_handle.clone(),
        runtime.evaluation.clone(),
        channels.clone(),
    ));

    let gateway = Arc::new(Gateway::new(
        cfg.clone(),
        started_at,
        sessions.clone(),
        assistant,
        channels.clone(),
        inbound_rx,
    ));
    let shutdown = CancellationToken::new();
    let gateway_handle = gateway.start(shutdown.child_token());
    tracing::info!(
        channel_count = channels.len(),
        channels = ?channels.keys().collect::<Vec<_>>(),
        "gateway started"
    );

    let automation = Arc::new(
        AutomationRuntime::load_or_new(
            cfg.automation.clone(),
            runtime.project_db.clone(),
            runtime.event_bus.clone(),
            runtime.org_id,
            runtime.project_id,
            runtime.project_db_handle.clone(),
        )
        .await?,
    );
    let discovery = Arc::new(DiscoveryRuntime::new(network_policy.clone()));
    discovery.start().await;
    let skills = Arc::new(
        SkillsRuntime::load_or_new(
            runtime.project_db.clone(),
            runtime.org_id,
            runtime.project_db_handle.clone(),
            cfg.skills.clone(),
        )
        .await?,
    );

    let os_state = Arc::new(OsState {
        config_control,
        org_id: runtime.org_id,
        channels: channels.clone(),
        channel_capability_matrix: capability_matrix.clone(),
        automation: automation.clone(),
        discovery: discovery.clone(),
        skills,
        sessions: sessions.clone(),
        memory: runtime.memory.clone(),
    });

    let os_auth_policy = http_auth::MutatingAuthPolicy::from_config(&cfg);

    let mut os_router = routes::router()
        .layer(axum::middleware::from_fn(http_auth::require_mutating_auth))
        .layer(Extension(http_auth::MutatingAuthPolicyExt(os_auth_policy)))
        .layer(Extension(os_state.clone()));
    for plugin_router in channel_routers {
        os_router = os_router.merge(plugin_router);
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
        .layer(GlobalConcurrencyLimitLayer::new(
            cfg.runtime.http_max_in_flight,
        ))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(cfg.runtime.http_timeout_seconds),
        ))
        .layer(trace_layer)
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid));

    tracing::info!(%addr, "opencraw serving");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown.clone()))
        .await?;
    tracing::info!("http server shutdown completed");

    shutdown.cancel();
    automation.shutdown().await;
    discovery.shutdown().await;
    match gateway_handle.await {
        Ok(()) => tracing::info!("gateway shutdown completed"),
        Err(e) => tracing::error!(error = %e, "gateway task join failed during shutdown"),
    }

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

fn build_llm_clients(cfg: &OpenShellConfig) -> Result<Vec<os_llm::LlmClient>> {
    let mut clients = Vec::new();
    let mut seen_profiles: HashSet<(String, String, String)> = HashSet::new();

    for profile_name in cfg.llm_profile_chain_names()? {
        let profile = cfg.llm_profile(&profile_name)?;
        let keys = cfg.api_keys_for_provider(profile.provider)?;
        let mut model_chain = Vec::with_capacity(1 + profile.fallback_models.len());
        model_chain.push(profile.model.clone());
        model_chain.extend(profile.fallback_models.iter().cloned());

        for model in model_chain {
            for api_key in &keys {
                let profile_key = (profile_name.clone(), model.clone(), api_key.clone());
                if !seen_profiles.insert(profile_key) {
                    continue;
                }
                clients.push(os_llm::LlmClient::new(api_key, &model)?);
            }
        }
    }

    if clients.is_empty() {
        return Err(anyhow::anyhow!(
            "no llm clients could be built from configured llm profile chain"
        ));
    }
    Ok(clients)
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

async fn shutdown_signal(shutdown: CancellationToken) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(sig) => sig,
            Err(e) => {
                tracing::error!(error = %e, "failed to install SIGTERM handler; falling back to ctrl_c only");
                if let Err(ctrlc_err) = tokio::signal::ctrl_c().await {
                    tracing::error!(error = %ctrlc_err, "failed to await ctrl-c signal");
                }
                shutdown.cancel();
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::warn!("received ctrl-c; beginning graceful shutdown");
            }
            _ = terminate.recv() => {
                tracing::warn!("received SIGTERM; beginning graceful shutdown");
            }
        }
    }
    #[cfg(not(unix))]
    {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %e, "failed to await ctrl-c signal");
        } else {
            tracing::warn!("received ctrl-c; beginning graceful shutdown");
        }
    }
    shutdown.cancel();
}
