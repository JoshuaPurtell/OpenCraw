use crate::config::OpenShellConfig;
use anyhow::Result;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use os_channels::{
    ChannelAdapter, DiscordAdapter, EmailAdapter, HttpPluginAdapter, ImessageAdapter,
    InboundMessage, InboundMessageKind, LinearAdapter, MatrixAdapter, SignalAdapter, SlackAdapter,
    TelegramAdapter, WebChatAdapter, WhatsAppCloudAdapter,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelPluginId {
    WebChat,
    Telegram,
    Discord,
    Slack,
    Matrix,
    Signal,
    Whatsapp,
    Imessage,
    Email,
    Linear,
}

impl ChannelPluginId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WebChat => "webchat",
            Self::Telegram => "telegram",
            Self::Discord => "discord",
            Self::Slack => "slack",
            Self::Matrix => "matrix",
            Self::Signal => "signal",
            Self::Whatsapp => "whatsapp",
            Self::Imessage => "imessage",
            Self::Email => "email",
            Self::Linear => "linear",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ChannelCapabilitySchema {
    pub supports_streaming_deltas: bool,
    pub supports_typing_events: bool,
    pub supports_reactions: bool,
}

#[derive(Clone)]
pub struct LoadedChannel {
    pub plugin_id: ChannelPluginId,
    pub adapter: Arc<dyn ChannelAdapter>,
    pub capability_schema: ChannelCapabilitySchema,
    pub router: Option<Router>,
}

pub struct ChannelLoadResult {
    pub channels: HashMap<String, Arc<dyn ChannelAdapter>>,
    pub routers: Vec<Router>,
    pub capability_matrix: HashMap<String, ChannelCapabilitySchema>,
}

pub async fn load_enabled_channels(
    cfg: &OpenShellConfig,
    inbound_tx: mpsc::Sender<os_channels::InboundMessage>,
) -> Result<ChannelLoadResult> {
    let mut channels = HashMap::new();
    let mut routers = Vec::new();
    let mut capability_matrix = HashMap::new();

    for plugin_id in [
        ChannelPluginId::WebChat,
        ChannelPluginId::Telegram,
        ChannelPluginId::Discord,
        ChannelPluginId::Slack,
        ChannelPluginId::Matrix,
        ChannelPluginId::Signal,
        ChannelPluginId::Whatsapp,
        ChannelPluginId::Imessage,
        ChannelPluginId::Email,
        ChannelPluginId::Linear,
    ] {
        if !plugin_enabled(cfg, plugin_id) {
            continue;
        }

        let loaded = build_plugin(plugin_id, cfg, inbound_tx.clone()).await?;
        let channel_id = loaded.plugin_id.as_str().to_string();
        channels.insert(channel_id.clone(), loaded.adapter);
        capability_matrix.insert(channel_id.clone(), loaded.capability_schema);
        if let Some(router) = loaded.router {
            routers.push(router);
        }
    }

    for plugin_cfg in cfg
        .channels
        .external_plugins
        .iter()
        .filter(|plugin| plugin.enabled)
    {
        let channel_id = plugin_cfg.id.trim().to_ascii_lowercase();
        if channels.contains_key(&channel_id) {
            return Err(anyhow::anyhow!(
                "external channel plugin id {channel_id:?} conflicts with loaded channel"
            ));
        }
        let adapter = Arc::new(
            HttpPluginAdapter::new(&channel_id, &plugin_cfg.send_url)?
                .with_poll_url(plugin_cfg.poll_url.clone())?
                .with_auth_token(plugin_cfg.auth_token.clone())
                .with_poll_interval(std::time::Duration::from_millis(
                    plugin_cfg.poll_interval_ms,
                ))
                .with_start_from_latest(plugin_cfg.start_from_latest)
                .with_capabilities(
                    plugin_cfg.supports_streaming_deltas,
                    plugin_cfg.supports_typing_events,
                    plugin_cfg.supports_reactions,
                ),
        );
        adapter.start(inbound_tx.clone()).await?;
        let capability_schema = capability_schema(adapter.as_ref());
        let adapter: Arc<dyn ChannelAdapter> = adapter;
        channels.insert(channel_id.clone(), adapter);
        capability_matrix.insert(channel_id, capability_schema);
    }

    Ok(ChannelLoadResult {
        channels,
        routers,
        capability_matrix,
    })
}

fn plugin_enabled(cfg: &OpenShellConfig, plugin_id: ChannelPluginId) -> bool {
    match plugin_id {
        ChannelPluginId::WebChat => cfg.channels.webchat.enabled,
        ChannelPluginId::Telegram => cfg.channels.telegram.enabled,
        ChannelPluginId::Discord => cfg.channels.discord.enabled,
        ChannelPluginId::Slack => cfg.channels.slack.enabled,
        ChannelPluginId::Matrix => cfg.channels.matrix.enabled,
        ChannelPluginId::Signal => cfg.channels.signal.enabled,
        ChannelPluginId::Whatsapp => cfg.channels.whatsapp.enabled,
        ChannelPluginId::Imessage => cfg.channels.imessage.enabled,
        ChannelPluginId::Email => cfg.channels.email.enabled,
        ChannelPluginId::Linear => cfg.channels.linear.enabled,
    }
}

async fn build_plugin(
    plugin_id: ChannelPluginId,
    cfg: &OpenShellConfig,
    inbound_tx: mpsc::Sender<os_channels::InboundMessage>,
) -> Result<LoadedChannel> {
    match plugin_id {
        ChannelPluginId::WebChat => {
            let webchat = Arc::new(WebChatAdapter::new());
            webchat.start(inbound_tx).await?;
            let capability_schema = capability_schema(webchat.as_ref());
            let router = Some(webchat.clone().router());
            let adapter: Arc<dyn ChannelAdapter> = webchat;
            Ok(LoadedChannel {
                plugin_id,
                adapter,
                capability_schema,
                router,
            })
        }
        ChannelPluginId::Telegram => {
            let parse_mode = match cfg.channels.telegram.parse_mode {
                crate::config::TelegramParseMode::Plain => None,
                crate::config::TelegramParseMode::Markdown => Some("Markdown"),
                crate::config::TelegramParseMode::MarkdownV2 => Some("MarkdownV2"),
                crate::config::TelegramParseMode::Html => Some("HTML"),
            };
            let adapter = Arc::new(
                TelegramAdapter::new(&cfg.channels.telegram.bot_token)?
                    .with_long_poll_timeout_seconds(cfg.channels.telegram.long_poll_timeout_seconds)
                    .with_allowed_updates(cfg.channels.telegram.allowed_updates.clone())
                    .with_retry_backoff(
                        cfg.channels.telegram.retry_base_ms,
                        cfg.channels.telegram.retry_max_ms,
                    )
                    .with_non_transient_delay(std::time::Duration::from_secs(
                        cfg.channels.telegram.non_transient_delay_seconds,
                    ))
                    .with_parse_mode(parse_mode)
                    .with_disable_link_previews(cfg.channels.telegram.disable_link_previews)
                    .with_bot_commands(crate::commands::telegram_bot_commands())
                    .with_max_message_chars(cfg.channels.telegram.max_message_chars),
            );
            adapter.start(inbound_tx).await?;
            let capability_schema = capability_schema(adapter.as_ref());
            let adapter: Arc<dyn ChannelAdapter> = adapter;
            Ok(LoadedChannel {
                plugin_id,
                adapter,
                capability_schema,
                router: None,
            })
        }
        ChannelPluginId::Discord => {
            let intents = (if cfg.channels.discord.intent_guild_messages {
                1_u64 << 9
            } else {
                0
            }) | (if cfg.channels.discord.intent_message_content {
                1_u64 << 15
            } else {
                0
            });
            let adapter = Arc::new(
                DiscordAdapter::new(&cfg.channels.discord.bot_token)?
                    .with_gateway_intents(intents)
                    .with_require_mention_in_group_chats(
                        cfg.channels.discord.require_mention_in_group_chats,
                    ),
            );
            adapter.start(inbound_tx).await?;
            let capability_schema = capability_schema(adapter.as_ref());
            let adapter: Arc<dyn ChannelAdapter> = adapter;
            Ok(LoadedChannel {
                plugin_id,
                adapter,
                capability_schema,
                router: None,
            })
        }
        ChannelPluginId::Slack => {
            let adapter = Arc::new(
                SlackAdapter::new(&cfg.channels.slack.bot_token)?
                    .with_poll_interval(std::time::Duration::from_millis(
                        cfg.channels.slack.poll_interval_ms,
                    ))
                    .with_channel_ids(cfg.channels.slack.channel_ids.clone())
                    .with_start_from_latest(cfg.channels.slack.start_from_latest)
                    .with_history_limit(cfg.channels.slack.history_limit),
            );
            adapter.start(inbound_tx).await?;
            let capability_schema = capability_schema(adapter.as_ref());
            let adapter: Arc<dyn ChannelAdapter> = adapter;
            Ok(LoadedChannel {
                plugin_id,
                adapter,
                capability_schema,
                router: None,
            })
        }
        ChannelPluginId::Matrix => {
            let adapter = Arc::new(
                MatrixAdapter::new(
                    &cfg.channels.matrix.homeserver_url,
                    &cfg.channels.matrix.access_token,
                    &cfg.channels.matrix.user_id,
                )?
                .with_poll_interval(std::time::Duration::from_millis(
                    cfg.channels.matrix.poll_interval_ms,
                ))
                .with_room_ids(cfg.channels.matrix.room_ids.clone())
                .with_start_from_latest(cfg.channels.matrix.start_from_latest)
                .with_sync_timeout_ms(cfg.channels.matrix.sync_timeout_ms),
            );
            adapter.start(inbound_tx).await?;
            let capability_schema = capability_schema(adapter.as_ref());
            let adapter: Arc<dyn ChannelAdapter> = adapter;
            Ok(LoadedChannel {
                plugin_id,
                adapter,
                capability_schema,
                router: None,
            })
        }
        ChannelPluginId::Signal => {
            let adapter = Arc::new(
                SignalAdapter::new(
                    &cfg.channels.signal.api_base_url,
                    &cfg.channels.signal.account,
                )?
                .with_api_token(cfg.channels.signal.api_token.clone())
                .with_poll_interval(std::time::Duration::from_millis(
                    cfg.channels.signal.poll_interval_ms,
                ))
                .with_start_from_latest(cfg.channels.signal.start_from_latest)
                .with_receive_timeout_seconds(cfg.channels.signal.receive_timeout_seconds),
            );
            adapter.start(inbound_tx).await?;
            let capability_schema = capability_schema(adapter.as_ref());
            let adapter: Arc<dyn ChannelAdapter> = adapter;
            Ok(LoadedChannel {
                plugin_id,
                adapter,
                capability_schema,
                router: None,
            })
        }
        ChannelPluginId::Whatsapp => {
            let adapter = Arc::new(WhatsAppCloudAdapter::new(
                &cfg.channels.whatsapp.access_token,
                &cfg.channels.whatsapp.phone_number_id,
            )?);
            adapter.start(inbound_tx.clone()).await?;
            let capability_schema = capability_schema(adapter.as_ref());
            let router = Some(build_whatsapp_webhook_router(
                inbound_tx,
                cfg.channels.whatsapp.webhook_verify_token.clone(),
                cfg.channels.whatsapp.app_secret.clone(),
            ));
            let adapter: Arc<dyn ChannelAdapter> = adapter;
            Ok(LoadedChannel {
                plugin_id,
                adapter,
                capability_schema,
                router,
            })
        }
        ChannelPluginId::Imessage => {
            let source_db = cfg
                .channels
                .imessage
                .source_db
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("channels.imessage.source_db is required"))?;
            let source_db = expand_home(source_db)?;
            let adapter = Arc::new(
                ImessageAdapter::new(source_db)
                    .with_poll_interval(std::time::Duration::from_millis(
                        cfg.channels.imessage.poll_interval_ms,
                    ))
                    .with_start_from_latest(cfg.channels.imessage.start_from_latest)
                    .with_max_per_poll(cfg.channels.imessage.max_per_poll)
                    .with_group_prefixes(cfg.channels.imessage.group_prefixes.clone()),
            );
            adapter.start(inbound_tx).await?;
            let capability_schema = capability_schema(adapter.as_ref());
            let adapter: Arc<dyn ChannelAdapter> = adapter;
            Ok(LoadedChannel {
                plugin_id,
                adapter,
                capability_schema,
                router: None,
            })
        }
        ChannelPluginId::Email => {
            let adapter = Arc::new(
                EmailAdapter::new(&cfg.channels.email.gmail_access_token)?
                    .with_poll_interval(std::time::Duration::from_millis(
                        cfg.channels.email.poll_interval_ms,
                    ))
                    .with_query(cfg.channels.email.query.clone())
                    .with_start_from_latest(cfg.channels.email.start_from_latest)
                    .with_mark_processed_as_read(cfg.channels.email.mark_processed_as_read)
                    .with_max_results(cfg.channels.email.max_results),
            );
            adapter.start(inbound_tx).await?;
            let capability_schema = capability_schema(adapter.as_ref());
            let adapter: Arc<dyn ChannelAdapter> = adapter;
            Ok(LoadedChannel {
                plugin_id,
                adapter,
                capability_schema,
                router: None,
            })
        }
        ChannelPluginId::Linear => {
            let adapter = Arc::new(
                LinearAdapter::new(&cfg.channels.linear.api_key)?
                    .with_poll_interval(std::time::Duration::from_millis(
                        cfg.channels.linear.poll_interval_ms,
                    ))
                    .with_team_ids(cfg.channels.linear.team_ids.clone())
                    .with_start_from_latest(cfg.channels.linear.start_from_latest)
                    .with_max_issues(cfg.channels.linear.max_issues),
            );
            adapter.start(inbound_tx).await?;
            let capability_schema = capability_schema(adapter.as_ref());
            let adapter: Arc<dyn ChannelAdapter> = adapter;
            Ok(LoadedChannel {
                plugin_id,
                adapter,
                capability_schema,
                router: None,
            })
        }
    }
}

fn capability_schema(adapter: &dyn ChannelAdapter) -> ChannelCapabilitySchema {
    ChannelCapabilitySchema {
        supports_streaming_deltas: adapter.supports_streaming_deltas(),
        supports_typing_events: adapter.supports_typing_events(),
        supports_reactions: adapter.supports_reactions(),
    }
}

fn expand_home(path: &str) -> Result<PathBuf> {
    let trimmed = path.trim().to_string();
    if !trimmed.starts_with("~/") {
        return Ok(PathBuf::from(trimmed));
    }
    let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(trimmed.replacen("~", &home, 1)))
}

#[derive(Clone)]
struct WhatsAppWebhookState {
    inbound_tx: mpsc::Sender<InboundMessage>,
    verify_token: String,
    app_secret: Option<String>,
}

fn build_whatsapp_webhook_router(
    inbound_tx: mpsc::Sender<InboundMessage>,
    verify_token: String,
    app_secret: Option<String>,
) -> Router {
    let state = Arc::new(WhatsAppWebhookState {
        inbound_tx,
        verify_token,
        app_secret: app_secret
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    });
    Router::new()
        .route(
            "/api/v1/os/channels/whatsapp/webhook",
            get(whatsapp_webhook_verify).post(whatsapp_webhook_ingest),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct WhatsAppVerifyQuery {
    #[serde(rename = "hub.mode")]
    mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    challenge: Option<String>,
}

async fn whatsapp_webhook_verify(
    State(state): State<Arc<WhatsAppWebhookState>>,
    Query(query): Query<WhatsAppVerifyQuery>,
) -> impl IntoResponse {
    let mode = query.mode.as_deref().map(str::trim).unwrap_or_default();
    let token = query
        .verify_token
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if mode.eq_ignore_ascii_case("subscribe") && token == state.verify_token {
        return (StatusCode::OK, query.challenge.unwrap_or_default()).into_response();
    }
    (StatusCode::FORBIDDEN, "verification failed".to_string()).into_response()
}

async fn whatsapp_webhook_ingest(
    State(state): State<Arc<WhatsAppWebhookState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Some(secret) = state.app_secret.as_deref() {
        if !verify_whatsapp_signature(&headers, &body, secret) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "status": "error",
                    "error": "invalid x-hub-signature-256",
                })),
            )
                .into_response();
        }
    }

    let payload: WhatsAppWebhookPayload = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "status": "error",
                    "error": format!("invalid whatsapp webhook payload: {error}"),
                })),
            )
                .into_response();
        }
    };

    let mut accepted = 0usize;
    for entry in payload.entry {
        for change in entry.changes {
            let phone_number_id = change
                .value
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.phone_number_id.as_deref())
                .unwrap_or_default()
                .to_string();
            for message in change.value.messages {
                let Some(inbound) = convert_whatsapp_message(&message, &phone_number_id) else {
                    continue;
                };
                if let Err(error) = state.inbound_tx.send(inbound).await {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(serde_json::json!({
                            "status": "error",
                            "error": format!("whatsapp inbound queue closed: {error}"),
                        })),
                    )
                        .into_response();
                }
                accepted += 1;
            }
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "accepted": accepted,
        })),
    )
        .into_response()
}

fn convert_whatsapp_message(
    message: &WhatsAppMessage,
    phone_number_id: &str,
) -> Option<InboundMessage> {
    let sender = message.from.trim();
    if sender.is_empty() {
        return None;
    }
    let message_id = if message.id.trim().is_empty() {
        Cow::Owned(ulid::Ulid::new().to_string())
    } else {
        Cow::Borrowed(message.id.as_str())
    };
    let thread_id = format!("wa:{}:{}", phone_number_id, sender);

    if message.message_type == "text" {
        let content = message
            .text
            .as_ref()
            .map(|text| text.body.trim())
            .filter(|value| !value.is_empty())?
            .to_string();
        return Some(InboundMessage {
            kind: InboundMessageKind::Message,
            message_id: message_id.as_ref().to_string().into(),
            channel_id: "whatsapp".into(),
            sender_id: sender.to_string().into(),
            thread_id: Some(thread_id.clone().into()),
            is_group: false,
            content,
            metadata: serde_json::json!({
                "provider": "whatsapp_cloud",
                "phone_number_id": phone_number_id,
                "message": message,
            }),
            received_at: Utc::now(),
        });
    }

    if message.message_type == "reaction" {
        let content = message
            .reaction
            .as_ref()
            .map(|reaction| reaction.emoji.trim())
            .filter(|value| !value.is_empty())?
            .to_string();
        return Some(InboundMessage {
            kind: InboundMessageKind::Reaction,
            message_id: message_id.as_ref().to_string().into(),
            channel_id: "whatsapp".into(),
            sender_id: sender.to_string().into(),
            thread_id: Some(thread_id.into()),
            is_group: false,
            content,
            metadata: serde_json::json!({
                "provider": "whatsapp_cloud",
                "phone_number_id": phone_number_id,
                "message": message,
            }),
            received_at: Utc::now(),
        });
    }

    None
}

fn verify_whatsapp_signature(headers: &HeaderMap, body: &[u8], app_secret: &str) -> bool {
    let Some(signature_header) = headers
        .get("x-hub-signature-256")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
    else {
        return false;
    };
    let expected = format!("sha256={}", hmac_sha256_hex(app_secret.as_bytes(), body));
    constant_time_eq(&expected, signature_header)
}

fn hmac_sha256_hex(key: &[u8], payload: &[u8]) -> String {
    let mut key_block = [0_u8; 64];
    if key.len() > 64 {
        let mut hasher = Sha256::new();
        hasher.update(key);
        let digest = hasher.finalize();
        key_block[..32].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut inner_pad = [0_u8; 64];
    let mut outer_pad = [0_u8; 64];
    for index in 0..64 {
        inner_pad[index] = key_block[index] ^ 0x36;
        outer_pad[index] = key_block[index] ^ 0x5c;
    }

    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(payload);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_digest);
    let digest = outer.finalize();

    to_lower_hex(&digest)
}

fn to_lower_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap_or('0'));
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap_or('0'));
    }
    out
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    let mut diff = left_bytes.len() ^ right_bytes.len();
    let max_len = left_bytes.len().max(right_bytes.len());
    for index in 0..max_len {
        let l = left_bytes.get(index).copied().unwrap_or(0);
        let r = right_bytes.get(index).copied().unwrap_or(0);
        diff |= (l ^ r) as usize;
    }
    diff == 0
}

#[derive(Debug, Deserialize)]
struct WhatsAppWebhookPayload {
    #[serde(default)]
    entry: Vec<WhatsAppEntry>,
}

#[derive(Debug, Deserialize)]
struct WhatsAppEntry {
    #[serde(default)]
    changes: Vec<WhatsAppChange>,
}

#[derive(Debug, Deserialize)]
struct WhatsAppChange {
    #[serde(default)]
    value: WhatsAppChangeValue,
}

#[derive(Debug, Default, Deserialize)]
struct WhatsAppChangeValue {
    #[serde(default)]
    metadata: Option<WhatsAppMetadata>,
    #[serde(default)]
    messages: Vec<WhatsAppMessage>,
}

#[derive(Debug, Clone, Deserialize)]
struct WhatsAppMetadata {
    #[serde(default)]
    phone_number_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WhatsAppMessage {
    #[serde(default)]
    id: String,
    #[serde(default)]
    from: String,
    #[serde(rename = "type", default)]
    message_type: String,
    #[serde(default)]
    text: Option<WhatsAppText>,
    #[serde(default)]
    reaction: Option<WhatsAppReaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WhatsAppText {
    #[serde(default)]
    body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WhatsAppReaction {
    #[serde(default)]
    emoji: String,
}

#[cfg(test)]
mod tests {
    use super::{
        WhatsAppMessage, WhatsAppReaction, WhatsAppText, constant_time_eq,
        convert_whatsapp_message, hmac_sha256_hex, verify_whatsapp_signature,
    };
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn hmac_matches_known_sha256_vector() {
        let digest = hmac_sha256_hex(b"key", b"The quick brown fox jumps over the lazy dog");
        assert_eq!(
            digest,
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }

    #[test]
    fn signature_verification_accepts_valid_header() {
        let body = br#"{"hello":"world"}"#;
        let digest = hmac_sha256_hex(b"secret", body);
        let signature = format!("sha256={digest}");
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-hub-signature-256",
            HeaderValue::from_str(&signature).expect("signature header"),
        );
        assert!(verify_whatsapp_signature(&headers, body, "secret"));
        assert!(!verify_whatsapp_signature(&headers, body, "wrong"));
    }

    #[test]
    fn convert_whatsapp_text_message_to_inbound_event() {
        let message = WhatsAppMessage {
            id: "wamid.abc".to_string(),
            from: "15551234567".to_string(),
            message_type: "text".to_string(),
            text: Some(WhatsAppText {
                body: "hello".to_string(),
            }),
            reaction: None,
        };
        let inbound =
            convert_whatsapp_message(&message, "12345").expect("text message should convert");
        assert_eq!(inbound.channel_id.as_str(), "whatsapp");
        assert_eq!(inbound.sender_id.as_str(), "15551234567");
        assert_eq!(inbound.content, "hello");
    }

    #[test]
    fn convert_whatsapp_reaction_message_to_inbound_event() {
        let message = WhatsAppMessage {
            id: "wamid.def".to_string(),
            from: "15551234567".to_string(),
            message_type: "reaction".to_string(),
            text: None,
            reaction: Some(WhatsAppReaction {
                emoji: "🔥".to_string(),
            }),
        };
        let inbound = convert_whatsapp_message(&message, "12345").expect("reaction should convert");
        assert_eq!(inbound.content, "🔥");
        assert_eq!(inbound.kind, os_channels::InboundMessageKind::Reaction);
    }

    #[test]
    fn constant_time_eq_rejects_different_lengths_and_values() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(!constant_time_eq("abc", "abx"));
    }
}
