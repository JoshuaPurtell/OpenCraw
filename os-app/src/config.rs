//! OpenShell configuration loader.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use serde::Deserialize;
use serde::Serialize;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenShellConfig {
    pub llm: LlmConfig,
    pub general: GeneralConfig,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub keys: KeysConfig,
    pub channels: ChannelsConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub queue: QueueConfig,
    #[serde(default)]
    pub context: ContextConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub optimization: OptimizationConfig,
    #[serde(default)]
    pub automation: AutomationConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralConfig {
    pub system_prompt: String,
}

fn default_failover_cooldown_base_seconds() -> u64 {
    5
}

fn default_failover_cooldown_max_seconds() -> u64 {
    300
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    Openai,
    Anthropic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmProfileConfig {
    pub provider: LlmProvider,
    pub model: String,
    #[serde(default)]
    pub fallback_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmConfig {
    pub active_profile: String,
    #[serde(default)]
    pub fallback_profiles: Vec<String>,
    #[serde(default = "default_failover_cooldown_base_seconds")]
    pub failover_cooldown_base_seconds: u64,
    #[serde(default = "default_failover_cooldown_max_seconds")]
    pub failover_cooldown_max_seconds: u64,
    pub profiles: std::collections::BTreeMap<String, LlmProfileConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeMode {
    Dev,
    Prod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BindMode {
    Loopback,
    Lan,
    Tailnet,
    Auto,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMode {
    Disabled,
    Mdns,
    TailnetServe,
    TailnetFunnel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExposure {
    Loopback,
    Lan,
    TailnetProxy,
    CustomLoopback,
    CustomPublic,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeNetworkPolicy {
    pub bind_mode: BindMode,
    pub bind_addr: SocketAddr,
    pub discovery_mode: DiscoveryMode,
    pub exposure: RuntimeExposure,
    pub public_ingress: bool,
    pub control_api_auth_configured: bool,
    pub allow_public_bind_without_auth: bool,
    pub advertised_base_url: Option<String>,
}

fn default_runtime_mode() -> RuntimeMode {
    RuntimeMode::Dev
}

fn default_runtime_data_dir() -> String {
    "data".to_string()
}

fn default_runtime_bind_mode() -> BindMode {
    BindMode::Loopback
}

fn default_runtime_discovery_mode() -> DiscoveryMode {
    DiscoveryMode::Disabled
}

fn default_runtime_allow_public_bind_without_auth() -> bool {
    false
}

fn default_runtime_http_timeout_seconds() -> u64 {
    30
}

fn default_runtime_http_max_in_flight() -> usize {
    256
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    #[serde(default = "default_runtime_mode")]
    pub mode: RuntimeMode,
    #[serde(default = "default_runtime_data_dir")]
    pub data_dir: String,
    #[serde(default = "default_runtime_bind_mode")]
    pub bind_mode: BindMode,
    #[serde(default)]
    pub bind_addr: Option<String>,
    #[serde(default = "default_runtime_discovery_mode")]
    pub discovery_mode: DiscoveryMode,
    #[serde(default)]
    pub advertised_base_url: Option<String>,
    #[serde(default = "default_runtime_allow_public_bind_without_auth")]
    pub allow_public_bind_without_auth: bool,
    #[serde(default = "default_runtime_http_timeout_seconds")]
    pub http_timeout_seconds: u64,
    #[serde(default = "default_runtime_http_max_in_flight")]
    pub http_max_in_flight: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            mode: default_runtime_mode(),
            data_dir: default_runtime_data_dir(),
            bind_mode: default_runtime_bind_mode(),
            bind_addr: None,
            discovery_mode: default_runtime_discovery_mode(),
            advertised_base_url: None,
            allow_public_bind_without_auth: default_runtime_allow_public_bind_without_auth(),
            http_timeout_seconds: default_runtime_http_timeout_seconds(),
            http_max_in_flight: default_runtime_http_max_in_flight(),
        }
    }
}

impl RuntimeConfig {
    pub fn data_dir_path(&self) -> anyhow::Result<PathBuf> {
        expand_home(&self.data_dir)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeysConfig {
    pub openai_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
    #[serde(default)]
    pub openai_api_keys: Vec<String>,
    #[serde(default)]
    pub anthropic_api_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelsConfig {
    pub webchat: WebChatConfig,
    #[serde(default)]
    pub telegram: TelegramConfig,
    #[serde(default)]
    pub discord: DiscordConfig,
    #[serde(default)]
    pub slack: SlackConfig,
    #[serde(default)]
    pub matrix: MatrixConfig,
    #[serde(default)]
    pub signal: SignalConfig,
    #[serde(default)]
    pub whatsapp: WhatsAppConfig,
    #[serde(default)]
    pub imessage: ImessageConfig,
    #[serde(default)]
    pub email: EmailConfig,
    #[serde(default)]
    pub linear: LinearConfig,
    #[serde(default)]
    pub external_plugins: Vec<ExternalChannelPluginConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebChatConfig {
    pub enabled: bool,
    pub port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SenderAccessMode {
    Pairing,
    Allowlist,
    Open,
}

fn default_sender_access_mode() -> SenderAccessMode {
    SenderAccessMode::Pairing
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelAccessConfig {
    #[serde(default = "default_sender_access_mode")]
    pub mode: SenderAccessMode,
    #[serde(default)]
    pub allowed_senders: Vec<String>,
}

impl Default for ChannelAccessConfig {
    fn default() -> Self {
        Self {
            mode: default_sender_access_mode(),
            allowed_senders: Vec::new(),
        }
    }
}

fn default_telegram_long_poll_timeout_seconds() -> u64 {
    30
}

fn default_telegram_allowed_updates() -> Vec<String> {
    vec![
        "message".to_string(),
        "message_reaction".to_string(),
        "callback_query".to_string(),
    ]
}

fn default_telegram_retry_base_ms() -> u64 {
    250
}

fn default_telegram_retry_max_ms() -> u64 {
    30_000
}

fn default_telegram_non_transient_delay_seconds() -> u64 {
    10
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramParseMode {
    Plain,
    Markdown,
    MarkdownV2,
    Html,
}

fn default_telegram_parse_mode() -> TelegramParseMode {
    TelegramParseMode::Markdown
}

fn default_telegram_disable_link_previews() -> bool {
    true
}

fn default_telegram_max_message_chars() -> usize {
    4000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: String,
    #[serde(default = "default_telegram_long_poll_timeout_seconds")]
    pub long_poll_timeout_seconds: u64,
    #[serde(default = "default_telegram_allowed_updates")]
    pub allowed_updates: Vec<String>,
    #[serde(default = "default_telegram_retry_base_ms")]
    pub retry_base_ms: u64,
    #[serde(default = "default_telegram_retry_max_ms")]
    pub retry_max_ms: u64,
    #[serde(default = "default_telegram_non_transient_delay_seconds")]
    pub non_transient_delay_seconds: u64,
    #[serde(default = "default_telegram_parse_mode")]
    pub parse_mode: TelegramParseMode,
    #[serde(default = "default_telegram_disable_link_previews")]
    pub disable_link_previews: bool,
    #[serde(default = "default_telegram_max_message_chars")]
    pub max_message_chars: usize,
    #[serde(default)]
    pub access: ChannelAccessConfig,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token: String::new(),
            long_poll_timeout_seconds: default_telegram_long_poll_timeout_seconds(),
            allowed_updates: default_telegram_allowed_updates(),
            retry_base_ms: default_telegram_retry_base_ms(),
            retry_max_ms: default_telegram_retry_max_ms(),
            non_transient_delay_seconds: default_telegram_non_transient_delay_seconds(),
            parse_mode: default_telegram_parse_mode(),
            disable_link_previews: default_telegram_disable_link_previews(),
            max_message_chars: default_telegram_max_message_chars(),
            access: ChannelAccessConfig::default(),
        }
    }
}

fn default_discord_require_mention_in_group_chats() -> bool {
    true
}

fn default_discord_intent_guild_messages() -> bool {
    true
}

fn default_discord_intent_message_content() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscordConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: String,
    #[serde(default = "default_discord_require_mention_in_group_chats")]
    pub require_mention_in_group_chats: bool,
    #[serde(default = "default_discord_intent_guild_messages")]
    pub intent_guild_messages: bool,
    #[serde(default = "default_discord_intent_message_content")]
    pub intent_message_content: bool,
    #[serde(default)]
    pub access: ChannelAccessConfig,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token: String::new(),
            require_mention_in_group_chats: default_discord_require_mention_in_group_chats(),
            intent_guild_messages: default_discord_intent_guild_messages(),
            intent_message_content: default_discord_intent_message_content(),
            access: ChannelAccessConfig::default(),
        }
    }
}

fn default_imessage_max_per_poll() -> usize {
    200
}

fn default_imessage_group_prefixes() -> Vec<String> {
    vec!["@opencraw".to_string(), "opencraw".to_string()]
}

fn default_channel_action_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImessageActionsConfig {
    #[serde(default = "default_channel_action_enabled")]
    pub list_recent: bool,
    #[serde(default = "default_channel_action_enabled")]
    pub send: bool,
}

impl Default for ImessageActionsConfig {
    fn default() -> Self {
        Self {
            list_recent: default_channel_action_enabled(),
            send: default_channel_action_enabled(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImessageConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Path to the macOS Messages database (`chat.db`).
    /// Default: `~/Library/Messages/chat.db`
    #[serde(default)]
    pub source_db: Option<String>,
    /// Poll interval in milliseconds.
    #[serde(default = "default_imessage_poll_interval_ms")]
    pub poll_interval_ms: u64,
    /// Start from the latest message at startup (avoids backfilling old history).
    #[serde(default = "default_imessage_start_from_latest")]
    pub start_from_latest: bool,
    /// Max rows read from chat.db per poll cycle.
    #[serde(default = "default_imessage_max_per_poll")]
    pub max_per_poll: usize,
    /// In group chats, only respond if the message starts with one of these prefixes.
    /// Example: ["@openshell", "openshell"]
    #[serde(default = "default_imessage_group_prefixes")]
    pub group_prefixes: Vec<String>,
    #[serde(default)]
    pub actions: ImessageActionsConfig,
    #[serde(default)]
    pub access: ChannelAccessConfig,
}

fn default_imessage_poll_interval_ms() -> u64 {
    1500
}

fn default_imessage_start_from_latest() -> bool {
    true
}

impl Default for ImessageConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            source_db: None,
            poll_interval_ms: default_imessage_poll_interval_ms(),
            start_from_latest: default_imessage_start_from_latest(),
            max_per_poll: default_imessage_max_per_poll(),
            group_prefixes: default_imessage_group_prefixes(),
            actions: ImessageActionsConfig::default(),
            access: ChannelAccessConfig::default(),
        }
    }
}

fn default_email_max_results() -> usize {
    25
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmailActionsConfig {
    #[serde(default = "default_channel_action_enabled")]
    pub list_inbox: bool,
    #[serde(default = "default_channel_action_enabled")]
    pub search: bool,
    #[serde(default = "default_channel_action_enabled")]
    pub read: bool,
    #[serde(default = "default_channel_action_enabled")]
    pub send: bool,
}

impl Default for EmailActionsConfig {
    fn default() -> Self {
        Self {
            list_inbox: default_channel_action_enabled(),
            search: default_channel_action_enabled(),
            read: default_channel_action_enabled(),
            send: default_channel_action_enabled(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmailConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Supported values: "gmail".
    #[serde(default = "default_email_provider")]
    pub provider: String,
    #[serde(default)]
    pub gmail_access_token: String,
    #[serde(default = "default_email_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_email_query")]
    pub query: String,
    #[serde(default = "default_email_start_from_latest")]
    pub start_from_latest: bool,
    #[serde(default = "default_email_mark_processed_as_read")]
    pub mark_processed_as_read: bool,
    #[serde(default = "default_email_max_results")]
    pub max_results: usize,
    #[serde(default)]
    pub actions: EmailActionsConfig,
    #[serde(default)]
    pub access: ChannelAccessConfig,
}

fn default_email_provider() -> String {
    "gmail".to_string()
}

fn default_email_poll_interval_ms() -> u64 {
    2000
}

fn default_email_query() -> String {
    "in:inbox is:unread".to_string()
}

fn default_email_start_from_latest() -> bool {
    true
}

fn default_email_mark_processed_as_read() -> bool {
    true
}

impl Default for EmailConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_email_provider(),
            gmail_access_token: String::new(),
            poll_interval_ms: default_email_poll_interval_ms(),
            query: default_email_query(),
            start_from_latest: default_email_start_from_latest(),
            mark_processed_as_read: default_email_mark_processed_as_read(),
            max_results: default_email_max_results(),
            actions: EmailActionsConfig::default(),
            access: ChannelAccessConfig::default(),
        }
    }
}

fn default_linear_max_issues() -> usize {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinearConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub default_team_id: String,
    #[serde(default = "default_linear_poll_interval_ms")]
    pub poll_interval_ms: u64,
    /// Optional list of team IDs/keys/names to include.
    #[serde(default)]
    pub team_ids: Vec<String>,
    #[serde(default = "default_linear_start_from_latest")]
    pub start_from_latest: bool,
    #[serde(default = "default_linear_max_issues")]
    pub max_issues: usize,
    #[serde(default)]
    pub actions: LinearActionsConfig,
    #[serde(default)]
    pub access: ChannelAccessConfig,
}

fn default_linear_poll_interval_ms() -> u64 {
    3000
}

fn default_linear_start_from_latest() -> bool {
    true
}

impl Default for LinearConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            default_team_id: String::new(),
            poll_interval_ms: default_linear_poll_interval_ms(),
            team_ids: Vec::new(),
            start_from_latest: default_linear_start_from_latest(),
            max_issues: default_linear_max_issues(),
            actions: LinearActionsConfig::default(),
            access: ChannelAccessConfig::default(),
        }
    }
}

fn default_linear_action_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinearActionsConfig {
    #[serde(default = "default_linear_action_enabled")]
    pub whoami: bool,
    #[serde(default = "default_linear_action_enabled")]
    pub list_assigned: bool,
    #[serde(default = "default_linear_action_enabled")]
    pub list_users: bool,
    #[serde(default = "default_linear_action_enabled")]
    pub list_teams: bool,
    #[serde(default = "default_linear_action_enabled")]
    pub list_projects: bool,
    #[serde(default = "default_linear_action_enabled")]
    pub create_issue: bool,
    #[serde(default = "default_linear_action_enabled")]
    pub create_project: bool,
    #[serde(default = "default_linear_action_enabled")]
    pub update_issue: bool,
    #[serde(default = "default_linear_action_enabled")]
    pub assign_issue: bool,
    #[serde(default = "default_linear_action_enabled")]
    pub comment_issue: bool,
}

impl Default for LinearActionsConfig {
    fn default() -> Self {
        Self {
            whoami: default_linear_action_enabled(),
            list_assigned: default_linear_action_enabled(),
            list_users: default_linear_action_enabled(),
            list_teams: default_linear_action_enabled(),
            list_projects: default_linear_action_enabled(),
            create_issue: default_linear_action_enabled(),
            create_project: default_linear_action_enabled(),
            update_issue: default_linear_action_enabled(),
            assign_issue: default_linear_action_enabled(),
            comment_issue: default_linear_action_enabled(),
        }
    }
}

fn default_slack_history_limit() -> usize {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlackConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: String,
    #[serde(default = "default_slack_poll_interval_ms")]
    pub poll_interval_ms: u64,
    /// Required list of Slack channel IDs to poll (e.g. C123ABC).
    #[serde(default)]
    pub channel_ids: Vec<String>,
    #[serde(default = "default_slack_start_from_latest")]
    pub start_from_latest: bool,
    #[serde(default = "default_slack_history_limit")]
    pub history_limit: usize,
    #[serde(default)]
    pub access: ChannelAccessConfig,
}

fn default_slack_poll_interval_ms() -> u64 {
    3000
}

fn default_slack_start_from_latest() -> bool {
    true
}

impl Default for SlackConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token: String::new(),
            poll_interval_ms: default_slack_poll_interval_ms(),
            channel_ids: Vec::new(),
            start_from_latest: default_slack_start_from_latest(),
            history_limit: default_slack_history_limit(),
            access: ChannelAccessConfig::default(),
        }
    }
}

fn default_matrix_sync_timeout_ms() -> u64 {
    30_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_matrix_homeserver_url")]
    pub homeserver_url: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default = "default_matrix_poll_interval_ms")]
    pub poll_interval_ms: u64,
    /// Required list of Matrix room IDs to poll (e.g. "!abc123:matrix.org").
    #[serde(default)]
    pub room_ids: Vec<String>,
    #[serde(default = "default_matrix_start_from_latest")]
    pub start_from_latest: bool,
    #[serde(default = "default_matrix_sync_timeout_ms")]
    pub sync_timeout_ms: u64,
    #[serde(default)]
    pub access: ChannelAccessConfig,
}

fn default_matrix_homeserver_url() -> String {
    "https://matrix-client.matrix.org".to_string()
}

fn default_matrix_poll_interval_ms() -> u64 {
    3000
}

fn default_matrix_start_from_latest() -> bool {
    true
}

impl Default for MatrixConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            homeserver_url: default_matrix_homeserver_url(),
            access_token: String::new(),
            user_id: String::new(),
            poll_interval_ms: default_matrix_poll_interval_ms(),
            room_ids: Vec::new(),
            start_from_latest: default_matrix_start_from_latest(),
            sync_timeout_ms: default_matrix_sync_timeout_ms(),
            access: ChannelAccessConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignalConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_signal_api_base_url")]
    pub api_base_url: String,
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub api_token: Option<String>,
    #[serde(default = "default_signal_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_signal_start_from_latest")]
    pub start_from_latest: bool,
    #[serde(default = "default_signal_receive_timeout_seconds")]
    pub receive_timeout_seconds: u64,
    #[serde(default)]
    pub access: ChannelAccessConfig,
}

fn default_signal_api_base_url() -> String {
    "http://127.0.0.1:8080".to_string()
}

fn default_signal_poll_interval_ms() -> u64 {
    3000
}

fn default_signal_start_from_latest() -> bool {
    true
}

fn default_signal_receive_timeout_seconds() -> u64 {
    5
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WhatsAppConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub phone_number_id: String,
    #[serde(default)]
    pub webhook_verify_token: String,
    #[serde(default)]
    pub app_secret: Option<String>,
    #[serde(default)]
    pub access: ChannelAccessConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalChannelPluginConfig {
    /// Channel ID surfaced to OpenCraw (must be unique and in [a-z0-9_-]+).
    pub id: String,
    #[serde(default)]
    pub enabled: bool,
    /// Required HTTP endpoint used for outbound sends.
    pub send_url: String,
    /// Optional HTTP endpoint used for inbound poll events.
    #[serde(default)]
    pub poll_url: Option<String>,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default = "default_external_plugin_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_external_plugin_start_from_latest")]
    pub start_from_latest: bool,
    #[serde(default)]
    pub supports_streaming_deltas: bool,
    #[serde(default)]
    pub supports_typing_events: bool,
    #[serde(default)]
    pub supports_reactions: bool,
    #[serde(default)]
    pub access: ChannelAccessConfig,
}

fn default_external_plugin_poll_interval_ms() -> u64 {
    3000
}

fn default_external_plugin_start_from_latest() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolProfile {
    Minimal,
    Coding,
    Messaging,
    Full,
}

fn default_tool_profile() -> ToolProfile {
    ToolProfile::Minimal
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellExecutionMode {
    Sandbox,
    Elevated,
}

fn default_shell_execution_mode() -> ShellExecutionMode {
    ShellExecutionMode::Sandbox
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellSandboxBackend {
    HostConstrained,
    HorizonsDocker,
}

fn default_shell_sandbox_backend() -> ShellSandboxBackend {
    ShellSandboxBackend::HostConstrained
}

fn default_shell_max_background_processes() -> usize {
    8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellPolicyConfig {
    #[serde(default = "default_shell_execution_mode")]
    pub default_mode: ShellExecutionMode,
    #[serde(default)]
    pub allow_elevated: bool,
    #[serde(default = "default_shell_sandbox_backend")]
    pub sandbox_backend: ShellSandboxBackend,
    #[serde(default)]
    pub sandbox_root: Option<String>,
    #[serde(default)]
    pub sandbox_image: Option<String>,
    #[serde(default = "default_shell_max_background_processes")]
    pub max_background_processes: usize,
}

impl Default for ShellPolicyConfig {
    fn default() -> Self {
        Self {
            default_mode: default_shell_execution_mode(),
            allow_elevated: false,
            sandbox_backend: default_shell_sandbox_backend(),
            sandbox_root: None,
            sandbox_image: None,
            max_background_processes: default_shell_max_background_processes(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolsConfig {
    #[serde(default = "default_tool_profile")]
    pub profile: ToolProfile,
    #[serde(default)]
    pub shell: bool,
    #[serde(default)]
    pub browser: bool,
    #[serde(default)]
    pub filesystem: bool,
    #[serde(default)]
    pub clipboard: bool,
    #[serde(default)]
    pub apply_patch: bool,
    #[serde(default)]
    pub shell_policy: ShellPolicyConfig,
    /// Optional explicit allowlist of tool names.
    ///
    /// Supported names (case-insensitive): `shell_execute` (`shell`), `filesystem`,
    /// `browser`, `clipboard`, `apply_patch`, `email`, `imessage`, `linear`.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Optional explicit denylist of tool names. Deny always wins over allow.
    #[serde(default)]
    pub deny: Vec<String>,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            profile: default_tool_profile(),
            shell: false,
            browser: false,
            filesystem: false,
            clipboard: false,
            apply_patch: false,
            shell_policy: ShellPolicyConfig::default(),
            allow: Vec::new(),
            deny: Vec::new(),
        }
    }
}

impl ToolsConfig {
    pub fn is_tool_enabled(&self, tool_name: &str, default_enabled: bool) -> bool {
        let Some(canonical) = canonical_tool_name(tool_name) else {
            return false;
        };
        if list_contains_tool(&self.deny, canonical) {
            return false;
        }
        if !self.allow.is_empty() {
            return list_contains_tool(&self.allow, canonical);
        }
        default_enabled
    }

    pub fn default_enabled_for(&self, tool_name: &str) -> bool {
        let Some(canonical) = canonical_tool_name(tool_name) else {
            return false;
        };
        self.explicit_toggle(canonical) || profile_default_enabled(self.profile, canonical)
    }

    pub fn tool_enabled(&self, tool_name: &str) -> bool {
        self.is_tool_enabled(tool_name, self.default_enabled_for(tool_name))
    }

    fn explicit_toggle(&self, canonical: &str) -> bool {
        match canonical {
            "shell_execute" => self.shell,
            "filesystem" => self.filesystem,
            "browser" => self.browser,
            "clipboard" => self.clipboard,
            "apply_patch" => self.apply_patch,
            _ => false,
        }
    }
}

fn profile_default_enabled(profile: ToolProfile, canonical: &str) -> bool {
    match profile {
        ToolProfile::Minimal => matches!(canonical, "filesystem"),
        ToolProfile::Coding => {
            matches!(
                canonical,
                "shell_execute" | "filesystem" | "browser" | "apply_patch"
            )
        }
        ToolProfile::Messaging => matches!(canonical, "email" | "imessage" | "linear"),
        ToolProfile::Full => true,
    }
}

fn canonical_tool_name(name: &str) -> Option<&'static str> {
    let lowered = name.trim().to_ascii_lowercase();
    match lowered.as_str() {
        "shell" | "shell_execute" => Some("shell_execute"),
        "filesystem" => Some("filesystem"),
        "browser" => Some("browser"),
        "clipboard" => Some("clipboard"),
        "apply_patch" | "patch" => Some("apply_patch"),
        "email" => Some("email"),
        "imessage" => Some("imessage"),
        "linear" => Some("linear"),
        _ => None,
    }
}

fn list_contains_tool(entries: &[String], tool_name: &str) -> bool {
    entries
        .iter()
        .filter_map(|v| canonical_tool_name(v))
        .any(|v| v == tool_name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalMode {
    Human,
    Ai,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlApiKeyConfig {
    pub token: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityConfig {
    #[serde(default = "default_shell_approval")]
    pub shell_approval: ApprovalMode,
    #[serde(default = "default_browser_approval")]
    pub browser_approval: ApprovalMode,
    #[serde(default = "default_filesystem_write_approval")]
    pub filesystem_write_approval: ApprovalMode,
    /// Max time to wait for human approval of a proposed action before timing out.
    ///
    /// Set to `0` to wait indefinitely.
    #[serde(default = "default_human_approval_timeout_seconds")]
    pub human_approval_timeout_seconds: u64,
    /// Bearer token required for mutating `/api/v1/os/*` control-plane requests when set.
    ///
    /// In `runtime.mode = "prod"`, this token is required.
    #[serde(default)]
    pub control_api_key: Option<String>,
    /// Rotating bearer tokens for mutating control-plane access.
    ///
    /// Each token can be scope-limited. Empty scopes imply full mutating control-plane access.
    #[serde(default)]
    pub control_api_keys: Vec<ControlApiKeyConfig>,
    /// Prefixes that bypass mutating bearer-token middleware for machine-ingest routes.
    ///
    /// These routes still enforce their own route-specific shared-secret contracts.
    #[serde(default = "default_mutating_auth_exempt_prefixes")]
    pub mutating_auth_exempt_prefixes: Vec<String>,
}

fn default_shell_approval() -> ApprovalMode {
    ApprovalMode::Auto
}

fn default_browser_approval() -> ApprovalMode {
    ApprovalMode::Ai
}

fn default_filesystem_write_approval() -> ApprovalMode {
    ApprovalMode::Ai
}

fn default_human_approval_timeout_seconds() -> u64 {
    300
}

fn default_mutating_auth_exempt_prefixes() -> Vec<String> {
    vec![
        "/api/v1/os/automation/webhook/".to_string(),
        "/api/v1/os/automation/poll/".to_string(),
    ]
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            shell_approval: default_shell_approval(),
            browser_approval: default_browser_approval(),
            filesystem_write_approval: default_filesystem_write_approval(),
            human_approval_timeout_seconds: default_human_approval_timeout_seconds(),
            control_api_key: None,
            control_api_keys: Vec::new(),
            mutating_auth_exempt_prefixes: default_mutating_auth_exempt_prefixes(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueConfig {
    #[serde(default = "default_queue_mode")]
    pub mode: QueueMode,
    #[serde(default = "default_queue_max_concurrency")]
    pub max_concurrency: usize,
    #[serde(default = "default_queue_lane_buffer")]
    pub lane_buffer: usize,
    #[serde(default = "default_queue_debounce_ms")]
    pub debounce_ms: u64,
    #[serde(default = "default_queue_overflow_policy")]
    pub overflow_policy: QueueOverflowPolicy,
    #[serde(default = "default_queue_overflow_summary_max_messages")]
    pub overflow_summary_max_messages: usize,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            mode: default_queue_mode(),
            max_concurrency: default_queue_max_concurrency(),
            lane_buffer: default_queue_lane_buffer(),
            debounce_ms: default_queue_debounce_ms(),
            overflow_policy: default_queue_overflow_policy(),
            overflow_summary_max_messages: default_queue_overflow_summary_max_messages(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueMode {
    Collect,
    Followup,
    Steer,
    Interrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueOverflowPolicy {
    Block,
    Drop,
    Summarize,
}

fn default_queue_mode() -> QueueMode {
    QueueMode::Followup
}

fn default_queue_max_concurrency() -> usize {
    8
}

fn default_queue_lane_buffer() -> usize {
    64
}

fn default_queue_debounce_ms() -> u64 {
    250
}

fn default_queue_overflow_policy() -> QueueOverflowPolicy {
    QueueOverflowPolicy::Block
}

fn default_queue_overflow_summary_max_messages() -> usize {
    8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextConfig {
    #[serde(default = "default_context_max_prompt_tokens")]
    pub max_prompt_tokens: usize,
    #[serde(default = "default_context_min_recent_messages")]
    pub min_recent_messages: usize,
    #[serde(default = "default_context_max_tool_chars")]
    pub max_tool_chars: usize,
    #[serde(default = "default_context_tool_loops_max")]
    pub tool_loops_max: usize,
    #[serde(default = "default_context_tool_max_runtime_seconds")]
    pub tool_max_runtime_seconds: u64,
    #[serde(default = "default_context_tool_no_progress_limit")]
    pub tool_no_progress_limit: usize,
    #[serde(default)]
    pub compaction_enabled: bool,
    #[serde(default = "default_context_compaction_trigger_tokens")]
    pub compaction_trigger_tokens: usize,
    #[serde(default = "default_context_compaction_retain_messages")]
    pub compaction_retain_messages: usize,
    #[serde(default = "default_context_compaction_horizon")]
    pub compaction_horizon: String,
    #[serde(default = "default_context_compaction_flush_max_chars")]
    pub compaction_flush_max_chars: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_prompt_tokens: default_context_max_prompt_tokens(),
            min_recent_messages: default_context_min_recent_messages(),
            max_tool_chars: default_context_max_tool_chars(),
            tool_loops_max: default_context_tool_loops_max(),
            tool_max_runtime_seconds: default_context_tool_max_runtime_seconds(),
            tool_no_progress_limit: default_context_tool_no_progress_limit(),
            compaction_enabled: false,
            compaction_trigger_tokens: default_context_compaction_trigger_tokens(),
            compaction_retain_messages: default_context_compaction_retain_messages(),
            compaction_horizon: default_context_compaction_horizon(),
            compaction_flush_max_chars: default_context_compaction_flush_max_chars(),
        }
    }
}

fn default_context_max_prompt_tokens() -> usize {
    8000
}

fn default_context_min_recent_messages() -> usize {
    8
}

fn default_context_max_tool_chars() -> usize {
    4000
}

fn default_context_tool_loops_max() -> usize {
    12
}

fn default_context_tool_max_runtime_seconds() -> u64 {
    600
}

fn default_context_tool_no_progress_limit() -> usize {
    3
}

fn default_context_compaction_trigger_tokens() -> usize {
    6000
}

fn default_context_compaction_retain_messages() -> usize {
    12
}

fn default_context_compaction_horizon() -> String {
    "30d".to_string()
}

fn default_context_compaction_flush_max_chars() -> usize {
    16000
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptimizationConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_optimization_schedule")]
    pub schedule: String,
}

fn default_optimization_schedule() -> String {
    "0 0 * * 0".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_automation_heartbeat_interval_seconds")]
    pub heartbeat_interval_seconds: u64,
    #[serde(default)]
    pub webhook_secret: Option<String>,
}

fn default_automation_heartbeat_interval_seconds() -> u64 {
    300
}

impl Default for AutomationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            heartbeat_interval_seconds: default_automation_heartbeat_interval_seconds(),
            webhook_secret: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillsConfig {
    #[serde(default)]
    pub require_source_provenance: bool,
    #[serde(default = "default_skills_require_https_source")]
    pub require_https_source: bool,
    #[serde(default)]
    pub require_trusted_source: bool,
    #[serde(default)]
    pub trusted_source_prefixes: Vec<String>,
    #[serde(default)]
    pub require_sha256_signature: bool,
}

fn default_skills_require_https_source() -> bool {
    true
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            require_source_provenance: false,
            require_https_source: default_skills_require_https_source(),
            require_trusted_source: false,
            trusted_source_prefixes: Vec::new(),
            require_sha256_signature: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ConfigFileKind {
    Llm,
    General,
    Runtime,
    Keys,
    Tools,
    Security,
    Queue,
    Context,
    Memory,
    Optimization,
    Automation,
    Skills,
    ChannelWebchat,
    ChannelTelegram,
    ChannelDiscord,
    ChannelSlack,
    ChannelMatrix,
    ChannelSignal,
    ChannelWhatsapp,
    ChannelImessage,
    ChannelEmail,
    ChannelLinear,
    ChannelExternalPlugins,
}

impl ConfigFileKind {
    fn from_file_name(file_name: &str) -> Option<Self> {
        match file_name {
            "llm.toml" => Some(Self::Llm),
            "general.toml" => Some(Self::General),
            "runtime.toml" => Some(Self::Runtime),
            "keys.toml" => Some(Self::Keys),
            "tools.toml" => Some(Self::Tools),
            "security.toml" => Some(Self::Security),
            "queue.toml" => Some(Self::Queue),
            "context.toml" => Some(Self::Context),
            "memory.toml" => Some(Self::Memory),
            "optimization.toml" => Some(Self::Optimization),
            "automation.toml" => Some(Self::Automation),
            "skills.toml" => Some(Self::Skills),
            "channel-webchat.toml" => Some(Self::ChannelWebchat),
            "channel-telegram.toml" => Some(Self::ChannelTelegram),
            "channel-discord.toml" => Some(Self::ChannelDiscord),
            "channel-slack.toml" => Some(Self::ChannelSlack),
            "channel-matrix.toml" => Some(Self::ChannelMatrix),
            "channel-signal.toml" => Some(Self::ChannelSignal),
            "channel-whatsapp.toml" => Some(Self::ChannelWhatsapp),
            "channel-imessage.toml" => Some(Self::ChannelImessage),
            "channel-email.toml" | "channel-email-gmail.toml" => Some(Self::ChannelEmail),
            "channel-linear.toml" => Some(Self::ChannelLinear),
            "channel-external-plugins.toml" | "channel-external-plugin.toml" => {
                Some(Self::ChannelExternalPlugins)
            }
            _ => None,
        }
    }

    fn canonical_file_name(self) -> &'static str {
        match self {
            Self::Llm => "llm.toml",
            Self::General => "general.toml",
            Self::Runtime => "runtime.toml",
            Self::Keys => "keys.toml",
            Self::Tools => "tools.toml",
            Self::Security => "security.toml",
            Self::Queue => "queue.toml",
            Self::Context => "context.toml",
            Self::Memory => "memory.toml",
            Self::Optimization => "optimization.toml",
            Self::Automation => "automation.toml",
            Self::Skills => "skills.toml",
            Self::ChannelWebchat => "channel-webchat.toml",
            Self::ChannelTelegram => "channel-telegram.toml",
            Self::ChannelDiscord => "channel-discord.toml",
            Self::ChannelSlack => "channel-slack.toml",
            Self::ChannelMatrix => "channel-matrix.toml",
            Self::ChannelSignal => "channel-signal.toml",
            Self::ChannelWhatsapp => "channel-whatsapp.toml",
            Self::ChannelImessage => "channel-imessage.toml",
            Self::ChannelEmail => "channel-email.toml",
            Self::ChannelLinear => "channel-linear.toml",
            Self::ChannelExternalPlugins => "channel-external-plugins.toml",
        }
    }

    fn canonical_file_names() -> &'static [&'static str] {
        &[
            "llm.toml",
            "general.toml",
            "runtime.toml",
            "keys.toml",
            "tools.toml",
            "security.toml",
            "queue.toml",
            "context.toml",
            "memory.toml",
            "optimization.toml",
            "automation.toml",
            "skills.toml",
            "channel-webchat.toml",
            "channel-telegram.toml",
            "channel-discord.toml",
            "channel-slack.toml",
            "channel-matrix.toml",
            "channel-signal.toml",
            "channel-whatsapp.toml",
            "channel-imessage.toml",
            "channel-email.toml",
            "channel-linear.toml",
            "channel-external-plugins.toml",
        ]
    }

    fn canonical_file_names_csv() -> String {
        Self::canonical_file_names().join(", ")
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LlmFragment {
    llm: LlmConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneralFragment {
    general: GeneralConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeFragment {
    runtime: RuntimeConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeysFragment {
    keys: KeysConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolsFragment {
    tools: ToolsConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecurityFragment {
    security: SecurityConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueFragment {
    queue: QueueConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextFragment {
    context: ContextConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryFragment {
    memory: MemoryConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OptimizationFragment {
    optimization: OptimizationConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AutomationFragment {
    automation: AutomationConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillsFragment {
    skills: SkillsConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelWebchatFragment {
    channels: ChannelWebchatOnly,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelWebchatOnly {
    webchat: WebChatConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelTelegramFragment {
    channels: ChannelTelegramOnly,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelTelegramOnly {
    telegram: TelegramConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelDiscordFragment {
    channels: ChannelDiscordOnly,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelDiscordOnly {
    discord: DiscordConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelSlackFragment {
    channels: ChannelSlackOnly,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelSlackOnly {
    slack: SlackConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelMatrixFragment {
    channels: ChannelMatrixOnly,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelMatrixOnly {
    matrix: MatrixConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelSignalFragment {
    channels: ChannelSignalOnly,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelSignalOnly {
    signal: SignalConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelWhatsappFragment {
    channels: ChannelWhatsappOnly,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelWhatsappOnly {
    whatsapp: WhatsAppConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelImessageFragment {
    channels: ChannelImessageOnly,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelImessageOnly {
    imessage: ImessageConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelEmailFragment {
    channels: ChannelEmailOnly,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelEmailOnly {
    email: EmailConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelLinearFragment {
    channels: ChannelLinearOnly,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelLinearOnly {
    linear: LinearConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelExternalPluginsFragment {
    channels: ChannelExternalPluginsOnly,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelExternalPluginsOnly {
    external_plugins: Vec<ExternalChannelPluginConfig>,
}

fn parse_fragment_file<T>(contents: &str, path: &Path) -> anyhow::Result<T>
where
    T: for<'de> serde::Deserialize<'de>,
{
    toml::from_str(contents)
        .map_err(|e| anyhow::anyhow!("parse config fragment {}: {e}", path.display()))
}

impl OpenShellConfig {
    pub async fn load(path: Option<PathBuf>) -> anyhow::Result<Self> {
        let (cfg, _) = Self::load_with_path(path).await?;
        Ok(cfg)
    }

    pub async fn load_with_path(path: Option<PathBuf>) -> anyhow::Result<(Self, PathBuf)> {
        let path = match path {
            Some(path) => path,
            None => default_config_path()?,
        };
        let contents = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| anyhow::anyhow!("read config {}: {e}", path.display()))?;

        let mut cfg: OpenShellConfig = toml::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("parse config {}: {e}", path.display()))?;

        cfg.apply_config_fragments(&path).await?;
        cfg.validate()?;
        Ok((cfg, path))
    }

    async fn apply_config_fragments(&mut self, base_config_path: &Path) -> anyhow::Result<()> {
        let fragments_dir = config_fragments_dir(base_config_path);
        let fragment_entries = read_config_fragment_entries(&fragments_dir).await?;

        for (kind, path) in fragment_entries {
            let contents = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| anyhow::anyhow!("read config fragment {}: {e}", path.display()))?;
            self.apply_config_fragment(kind, &path, &contents)?;
        }

        Ok(())
    }

    fn apply_config_fragment(
        &mut self,
        kind: ConfigFileKind,
        path: &Path,
        contents: &str,
    ) -> anyhow::Result<()> {
        match kind {
            ConfigFileKind::Llm => {
                let fragment: LlmFragment = parse_fragment_file(contents, path)?;
                self.llm = fragment.llm;
            }
            ConfigFileKind::General => {
                let fragment: GeneralFragment = parse_fragment_file(contents, path)?;
                self.general = fragment.general;
            }
            ConfigFileKind::Runtime => {
                let fragment: RuntimeFragment = parse_fragment_file(contents, path)?;
                self.runtime = fragment.runtime;
            }
            ConfigFileKind::Keys => {
                let fragment: KeysFragment = parse_fragment_file(contents, path)?;
                self.keys = fragment.keys;
            }
            ConfigFileKind::Tools => {
                let fragment: ToolsFragment = parse_fragment_file(contents, path)?;
                self.tools = fragment.tools;
            }
            ConfigFileKind::Security => {
                let fragment: SecurityFragment = parse_fragment_file(contents, path)?;
                self.security = fragment.security;
            }
            ConfigFileKind::Queue => {
                let fragment: QueueFragment = parse_fragment_file(contents, path)?;
                self.queue = fragment.queue;
            }
            ConfigFileKind::Context => {
                let fragment: ContextFragment = parse_fragment_file(contents, path)?;
                self.context = fragment.context;
            }
            ConfigFileKind::Memory => {
                let fragment: MemoryFragment = parse_fragment_file(contents, path)?;
                self.memory = fragment.memory;
            }
            ConfigFileKind::Optimization => {
                let fragment: OptimizationFragment = parse_fragment_file(contents, path)?;
                self.optimization = fragment.optimization;
            }
            ConfigFileKind::Automation => {
                let fragment: AutomationFragment = parse_fragment_file(contents, path)?;
                self.automation = fragment.automation;
            }
            ConfigFileKind::Skills => {
                let fragment: SkillsFragment = parse_fragment_file(contents, path)?;
                self.skills = fragment.skills;
            }
            ConfigFileKind::ChannelWebchat => {
                let fragment: ChannelWebchatFragment = parse_fragment_file(contents, path)?;
                self.channels.webchat = fragment.channels.webchat;
            }
            ConfigFileKind::ChannelTelegram => {
                let fragment: ChannelTelegramFragment = parse_fragment_file(contents, path)?;
                self.channels.telegram = fragment.channels.telegram;
            }
            ConfigFileKind::ChannelDiscord => {
                let fragment: ChannelDiscordFragment = parse_fragment_file(contents, path)?;
                self.channels.discord = fragment.channels.discord;
            }
            ConfigFileKind::ChannelSlack => {
                let fragment: ChannelSlackFragment = parse_fragment_file(contents, path)?;
                self.channels.slack = fragment.channels.slack;
            }
            ConfigFileKind::ChannelMatrix => {
                let fragment: ChannelMatrixFragment = parse_fragment_file(contents, path)?;
                self.channels.matrix = fragment.channels.matrix;
            }
            ConfigFileKind::ChannelSignal => {
                let fragment: ChannelSignalFragment = parse_fragment_file(contents, path)?;
                self.channels.signal = fragment.channels.signal;
            }
            ConfigFileKind::ChannelWhatsapp => {
                let fragment: ChannelWhatsappFragment = parse_fragment_file(contents, path)?;
                self.channels.whatsapp = fragment.channels.whatsapp;
            }
            ConfigFileKind::ChannelImessage => {
                let fragment: ChannelImessageFragment = parse_fragment_file(contents, path)?;
                self.channels.imessage = fragment.channels.imessage;
            }
            ConfigFileKind::ChannelEmail => {
                let fragment: ChannelEmailFragment = parse_fragment_file(contents, path)?;
                self.channels.email = fragment.channels.email;
            }
            ConfigFileKind::ChannelLinear => {
                let fragment: ChannelLinearFragment = parse_fragment_file(contents, path)?;
                self.channels.linear = fragment.channels.linear;
            }
            ConfigFileKind::ChannelExternalPlugins => {
                let fragment: ChannelExternalPluginsFragment = parse_fragment_file(contents, path)?;
                self.channels.external_plugins = fragment.channels.external_plugins;
            }
        }

        Ok(())
    }

    pub fn runtime_network_policy(&self) -> anyhow::Result<RuntimeNetworkPolicy> {
        let bind_addr = resolve_bind_addr(
            self.runtime.bind_mode,
            self.runtime.bind_addr.as_deref(),
            self.channels.webchat.port,
        )?;
        let advertised_base_url = self
            .runtime
            .advertised_base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        if let Some(url) = advertised_base_url.as_deref() {
            let lowered = url.to_ascii_lowercase();
            if !(lowered.starts_with("http://") || lowered.starts_with("https://")) {
                return Err(anyhow::anyhow!(
                    "runtime.advertised_base_url must start with http:// or https://"
                ));
            }
        }

        let exposure = resolve_runtime_exposure(
            self.runtime.bind_mode,
            bind_addr,
            self.runtime.discovery_mode,
        );
        let public_ingress = matches!(
            exposure,
            RuntimeExposure::Lan | RuntimeExposure::CustomPublic
        ) || self.runtime.discovery_mode == DiscoveryMode::TailnetFunnel;
        let control_api_auth_configured = self.control_api_auth_configured();

        if matches!(
            self.runtime.discovery_mode,
            DiscoveryMode::TailnetServe | DiscoveryMode::TailnetFunnel
        ) && !matches!(self.runtime.bind_mode, BindMode::Tailnet | BindMode::Auto)
        {
            return Err(anyhow::anyhow!(
                "runtime.discovery_mode={:?} requires runtime.bind_mode to be 'tailnet' or 'auto'",
                self.runtime.discovery_mode
            ));
        }

        if self.runtime.discovery_mode == DiscoveryMode::Mdns
            && !matches!(
                exposure,
                RuntimeExposure::Lan | RuntimeExposure::CustomPublic
            )
        {
            return Err(anyhow::anyhow!(
                "runtime.discovery_mode=mdns requires a public bind target (runtime.bind_mode=lan or runtime.bind_mode=custom with non-loopback address)"
            ));
        }

        if self.runtime.discovery_mode == DiscoveryMode::TailnetFunnel
            && !control_api_auth_configured
        {
            return Err(anyhow::anyhow!(
                "runtime.discovery_mode=tailnet_funnel requires security.control_api_key or security.control_api_keys"
            ));
        }

        if matches!(
            exposure,
            RuntimeExposure::Lan | RuntimeExposure::CustomPublic
        ) && !control_api_auth_configured
            && !self.runtime.allow_public_bind_without_auth
        {
            return Err(anyhow::anyhow!(
                "public bind target requires security.control_api_key or security.control_api_keys unless runtime.allow_public_bind_without_auth=true"
            ));
        }

        Ok(RuntimeNetworkPolicy {
            bind_mode: self.runtime.bind_mode,
            bind_addr,
            discovery_mode: self.runtime.discovery_mode,
            exposure,
            public_ingress,
            control_api_auth_configured,
            allow_public_bind_without_auth: self.runtime.allow_public_bind_without_auth,
            advertised_base_url,
        })
    }

    pub fn control_api_key_pool(&self) -> Vec<ControlApiKeyConfig> {
        let mut tokens = Vec::new();
        let mut seen_tokens = std::collections::HashSet::new();

        if let Some(token) = self
            .security
            .control_api_key
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            if seen_tokens.insert(token.to_string()) {
                tokens.push(ControlApiKeyConfig {
                    token: token.to_string(),
                    scopes: Vec::new(),
                    description: None,
                });
            }
        }

        for configured in &self.security.control_api_keys {
            let token = configured.token.trim();
            if token.is_empty() || !seen_tokens.insert(token.to_string()) {
                continue;
            }
            let mut scopes = Vec::new();
            for scope in &configured.scopes {
                let Some(normalized) = normalize_control_api_scope(scope) else {
                    continue;
                };
                if !scopes.iter().any(|existing| existing == &normalized) {
                    scopes.push(normalized);
                }
            }
            tokens.push(ControlApiKeyConfig {
                token: token.to_string(),
                scopes,
                description: configured
                    .description
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(ToOwned::to_owned),
            });
        }

        tokens
    }

    fn control_api_auth_configured(&self) -> bool {
        !self.control_api_key_pool().is_empty()
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.general.system_prompt.trim().is_empty() {
            return Err(anyhow::anyhow!("general.system_prompt is required"));
        }
        if self.llm.active_profile.trim().is_empty() {
            return Err(anyhow::anyhow!("llm.active_profile is required"));
        }
        if self.llm.profiles.is_empty() {
            return Err(anyhow::anyhow!(
                "llm.profiles must contain at least one profile"
            ));
        }
        if self.llm.failover_cooldown_base_seconds == 0 {
            return Err(anyhow::anyhow!(
                "llm.failover_cooldown_base_seconds must be > 0"
            ));
        }
        if self.llm.failover_cooldown_max_seconds == 0 {
            return Err(anyhow::anyhow!(
                "llm.failover_cooldown_max_seconds must be > 0"
            ));
        }
        if self.llm.failover_cooldown_base_seconds > self.llm.failover_cooldown_max_seconds {
            return Err(anyhow::anyhow!(
                "llm.failover_cooldown_base_seconds must be <= llm.failover_cooldown_max_seconds"
            ));
        }
        if self
            .llm
            .fallback_profiles
            .iter()
            .any(|profile| profile.trim().is_empty())
        {
            return Err(anyhow::anyhow!(
                "llm.fallback_profiles cannot contain empty values"
            ));
        }
        for (profile_name, profile) in &self.llm.profiles {
            if profile_name.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "llm.profiles keys must not be empty or whitespace"
                ));
            }
            if profile.model.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "llm.profiles.{profile_name}.model is required"
                ));
            }
            if profile
                .fallback_models
                .iter()
                .any(|model| model.trim().is_empty())
            {
                return Err(anyhow::anyhow!(
                    "llm.profiles.{profile_name}.fallback_models cannot contain empty values"
                ));
            }
        }
        let _ = self.llm_profile_chain_names()?;
        if self
            .keys
            .openai_api_keys
            .iter()
            .any(|key| key.trim().is_empty())
        {
            return Err(anyhow::anyhow!(
                "keys.openai_api_keys cannot contain empty values"
            ));
        }
        if self
            .keys
            .anthropic_api_keys
            .iter()
            .any(|key| key.trim().is_empty())
        {
            return Err(anyhow::anyhow!(
                "keys.anthropic_api_keys cannot contain empty values"
            ));
        }
        if self.runtime.data_dir.trim().is_empty() {
            return Err(anyhow::anyhow!("runtime.data_dir is required"));
        }
        if self.runtime.http_timeout_seconds == 0 {
            return Err(anyhow::anyhow!("runtime.http_timeout_seconds must be > 0"));
        }
        if self.runtime.http_max_in_flight == 0 {
            return Err(anyhow::anyhow!("runtime.http_max_in_flight must be > 0"));
        }
        if self.queue.max_concurrency == 0 {
            return Err(anyhow::anyhow!("queue.max_concurrency must be > 0"));
        }
        if self.queue.lane_buffer == 0 {
            return Err(anyhow::anyhow!("queue.lane_buffer must be > 0"));
        }
        if self.queue.overflow_summary_max_messages == 0 {
            return Err(anyhow::anyhow!(
                "queue.overflow_summary_max_messages must be > 0"
            ));
        }
        if self.context.max_prompt_tokens == 0 {
            return Err(anyhow::anyhow!("context.max_prompt_tokens must be > 0"));
        }
        if self.context.max_tool_chars == 0 {
            return Err(anyhow::anyhow!("context.max_tool_chars must be > 0"));
        }
        if self.context.tool_loops_max == 0 {
            return Err(anyhow::anyhow!("context.tool_loops_max must be > 0"));
        }
        if self.context.tool_max_runtime_seconds == 0 {
            return Err(anyhow::anyhow!(
                "context.tool_max_runtime_seconds must be > 0"
            ));
        }
        if self.context.tool_no_progress_limit == 0 {
            return Err(anyhow::anyhow!(
                "context.tool_no_progress_limit must be > 0"
            ));
        }
        if self.context.compaction_enabled && !self.memory.enabled {
            return Err(anyhow::anyhow!(
                "context.compaction_enabled=true requires memory.enabled=true"
            ));
        }
        if self.context.compaction_trigger_tokens == 0 {
            return Err(anyhow::anyhow!(
                "context.compaction_trigger_tokens must be > 0"
            ));
        }
        if self.context.compaction_retain_messages == 0 {
            return Err(anyhow::anyhow!(
                "context.compaction_retain_messages must be > 0"
            ));
        }
        if self.context.compaction_horizon.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "context.compaction_horizon must not be empty"
            ));
        }
        if self.context.compaction_flush_max_chars == 0 {
            return Err(anyhow::anyhow!(
                "context.compaction_flush_max_chars must be > 0"
            ));
        }
        if self.automation.heartbeat_interval_seconds == 0 {
            return Err(anyhow::anyhow!(
                "automation.heartbeat_interval_seconds must be > 0"
            ));
        }
        if let Some(secret) = self.automation.webhook_secret.as_deref() {
            if secret.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "automation.webhook_secret must not be empty when provided"
                ));
            }
        }
        if self.skills.require_trusted_source && self.skills.trusted_source_prefixes.is_empty() {
            return Err(anyhow::anyhow!(
                "skills.trusted_source_prefixes must contain at least one value when skills.require_trusted_source=true"
            ));
        }
        for prefix in &self.skills.trusted_source_prefixes {
            let trimmed = prefix.trim();
            if trimmed.is_empty() {
                return Err(anyhow::anyhow!(
                    "skills.trusted_source_prefixes cannot contain empty values"
                ));
            }
            if self.skills.require_https_source
                && !trimmed.to_ascii_lowercase().starts_with("https://")
            {
                return Err(anyhow::anyhow!(
                    "skills.trusted_source_prefixes entry must use https:// when skills.require_https_source=true: {prefix:?}"
                ));
            }
        }
        if self.tools.shell_policy.max_background_processes == 0 {
            return Err(anyhow::anyhow!(
                "tools.shell_policy.max_background_processes must be > 0"
            ));
        }
        if self.tools.shell_policy.default_mode == ShellExecutionMode::Elevated
            && !self.tools.shell_policy.allow_elevated
        {
            return Err(anyhow::anyhow!(
                "tools.shell_policy.default_mode=elevated requires tools.shell_policy.allow_elevated=true"
            ));
        }
        if let Some(root) = self.tools.shell_policy.sandbox_root.as_deref() {
            if root.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "tools.shell_policy.sandbox_root must not be empty when provided"
                ));
            }
        }
        if let Some(image) = self.tools.shell_policy.sandbox_image.as_deref() {
            if image.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "tools.shell_policy.sandbox_image must not be empty when provided"
                ));
            }
        }
        for entry in &self.tools.allow {
            if canonical_tool_name(entry).is_none() {
                return Err(anyhow::anyhow!(
                    "tools.allow contains unsupported tool name: {entry:?}"
                ));
            }
        }
        for entry in &self.tools.deny {
            if canonical_tool_name(entry).is_none() {
                return Err(anyhow::anyhow!(
                    "tools.deny contains unsupported tool name: {entry:?}"
                ));
            }
        }
        if self.channels.webchat.enabled && self.channels.webchat.port == 0 {
            return Err(anyhow::anyhow!("channels.webchat.port must be > 0"));
        }
        validate_channel_access(
            "channels.telegram",
            self.channels.telegram.enabled,
            &self.channels.telegram.access,
        )?;
        if self.channels.telegram.enabled && self.channels.telegram.bot_token.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "channels.telegram.bot_token is required when channels.telegram.enabled=true"
            ));
        }
        if self.channels.telegram.enabled {
            if self.channels.telegram.long_poll_timeout_seconds == 0 {
                return Err(anyhow::anyhow!(
                    "channels.telegram.long_poll_timeout_seconds must be > 0"
                ));
            }
            if self.channels.telegram.allowed_updates.is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.telegram.allowed_updates must contain at least one update type when channels.telegram.enabled=true"
                ));
            }
            if self
                .channels
                .telegram
                .allowed_updates
                .iter()
                .any(|update| update.trim().is_empty())
            {
                return Err(anyhow::anyhow!(
                    "channels.telegram.allowed_updates cannot contain empty values"
                ));
            }
            if self.channels.telegram.retry_base_ms == 0 {
                return Err(anyhow::anyhow!(
                    "channels.telegram.retry_base_ms must be > 0"
                ));
            }
            if self.channels.telegram.retry_max_ms == 0 {
                return Err(anyhow::anyhow!(
                    "channels.telegram.retry_max_ms must be > 0"
                ));
            }
            if self.channels.telegram.retry_max_ms < self.channels.telegram.retry_base_ms {
                return Err(anyhow::anyhow!(
                    "channels.telegram.retry_max_ms must be >= channels.telegram.retry_base_ms"
                ));
            }
            if self.channels.telegram.non_transient_delay_seconds == 0 {
                return Err(anyhow::anyhow!(
                    "channels.telegram.non_transient_delay_seconds must be > 0"
                ));
            }
            if self.channels.telegram.max_message_chars == 0 {
                return Err(anyhow::anyhow!(
                    "channels.telegram.max_message_chars must be > 0"
                ));
            }
            if self.channels.telegram.max_message_chars > 4096 {
                return Err(anyhow::anyhow!(
                    "channels.telegram.max_message_chars must be <= 4096"
                ));
            }
        }
        validate_channel_access(
            "channels.discord",
            self.channels.discord.enabled,
            &self.channels.discord.access,
        )?;
        if self.channels.discord.enabled && self.channels.discord.bot_token.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "channels.discord.bot_token is required when channels.discord.enabled=true"
            ));
        }
        if self.channels.discord.enabled
            && !self.channels.discord.intent_guild_messages
            && !self.channels.discord.intent_message_content
        {
            return Err(anyhow::anyhow!(
                "channels.discord must enable at least one gateway intent when channels.discord.enabled=true"
            ));
        }
        validate_channel_access(
            "channels.slack",
            self.channels.slack.enabled,
            &self.channels.slack.access,
        )?;
        if self.channels.slack.enabled {
            if self.channels.slack.bot_token.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.slack.bot_token is required when channels.slack.enabled=true"
                ));
            }
            if self.channels.slack.poll_interval_ms == 0 {
                return Err(anyhow::anyhow!(
                    "channels.slack.poll_interval_ms must be > 0"
                ));
            }
            if !(1..=200).contains(&self.channels.slack.history_limit) {
                return Err(anyhow::anyhow!(
                    "channels.slack.history_limit must be between 1 and 200"
                ));
            }
            if self.channels.slack.channel_ids.is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.slack.channel_ids must contain at least one channel id when channels.slack.enabled=true"
                ));
            }
            if self
                .channels
                .slack
                .channel_ids
                .iter()
                .any(|channel_id| channel_id.trim().is_empty())
            {
                return Err(anyhow::anyhow!(
                    "channels.slack.channel_ids cannot contain empty values"
                ));
            }
        }
        validate_channel_access(
            "channels.matrix",
            self.channels.matrix.enabled,
            &self.channels.matrix.access,
        )?;
        if self.channels.matrix.enabled {
            if self.channels.matrix.homeserver_url.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.matrix.homeserver_url is required when channels.matrix.enabled=true"
                ));
            }
            let parsed =
                reqwest::Url::parse(self.channels.matrix.homeserver_url.trim()).map_err(|e| {
                    anyhow::anyhow!("channels.matrix.homeserver_url must be a valid URL: {e}")
                })?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(anyhow::anyhow!(
                    "channels.matrix.homeserver_url must use http or https scheme"
                ));
            }
            if self.channels.matrix.access_token.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.matrix.access_token is required when channels.matrix.enabled=true"
                ));
            }
            if self.channels.matrix.user_id.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.matrix.user_id is required when channels.matrix.enabled=true"
                ));
            }
            if self.channels.matrix.poll_interval_ms == 0 {
                return Err(anyhow::anyhow!(
                    "channels.matrix.poll_interval_ms must be > 0"
                ));
            }
            if self.channels.matrix.sync_timeout_ms == 0 {
                return Err(anyhow::anyhow!(
                    "channels.matrix.sync_timeout_ms must be > 0"
                ));
            }
            if self.channels.matrix.room_ids.is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.matrix.room_ids must contain at least one room id when channels.matrix.enabled=true"
                ));
            }
            if self
                .channels
                .matrix
                .room_ids
                .iter()
                .any(|room_id| room_id.trim().is_empty())
            {
                return Err(anyhow::anyhow!(
                    "channels.matrix.room_ids cannot contain empty values"
                ));
            }
        }
        validate_channel_access(
            "channels.signal",
            self.channels.signal.enabled,
            &self.channels.signal.access,
        )?;
        if self.channels.signal.enabled {
            let api_base_url = self.channels.signal.api_base_url.trim();
            if api_base_url.is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.signal.api_base_url is required when channels.signal.enabled=true"
                ));
            }
            let parsed = reqwest::Url::parse(api_base_url).map_err(|e| {
                anyhow::anyhow!("channels.signal.api_base_url must be a valid URL: {e}")
            })?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(anyhow::anyhow!(
                    "channels.signal.api_base_url must use http or https scheme"
                ));
            }
            if self.channels.signal.account.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.signal.account is required when channels.signal.enabled=true"
                ));
            }
            if self.channels.signal.poll_interval_ms == 0 {
                return Err(anyhow::anyhow!(
                    "channels.signal.poll_interval_ms must be > 0"
                ));
            }
            if self.channels.signal.receive_timeout_seconds == 0 {
                return Err(anyhow::anyhow!(
                    "channels.signal.receive_timeout_seconds must be > 0"
                ));
            }
            if let Some(api_token) = self.channels.signal.api_token.as_deref() {
                if api_token.trim().is_empty() {
                    return Err(anyhow::anyhow!(
                        "channels.signal.api_token must not be empty when provided"
                    ));
                }
            }
        }
        validate_channel_access(
            "channels.whatsapp",
            self.channels.whatsapp.enabled,
            &self.channels.whatsapp.access,
        )?;
        if self.channels.whatsapp.enabled {
            if self.channels.whatsapp.access_token.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.whatsapp.access_token is required when channels.whatsapp.enabled=true"
                ));
            }
            if self.channels.whatsapp.phone_number_id.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.whatsapp.phone_number_id is required when channels.whatsapp.enabled=true"
                ));
            }
            if self
                .channels
                .whatsapp
                .webhook_verify_token
                .trim()
                .is_empty()
            {
                return Err(anyhow::anyhow!(
                    "channels.whatsapp.webhook_verify_token is required when channels.whatsapp.enabled=true"
                ));
            }
            if let Some(secret) = self.channels.whatsapp.app_secret.as_deref() {
                if secret.trim().is_empty() {
                    return Err(anyhow::anyhow!(
                        "channels.whatsapp.app_secret must not be empty when provided"
                    ));
                }
            }
        }
        validate_channel_access(
            "channels.imessage",
            self.channels.imessage.enabled,
            &self.channels.imessage.access,
        )?;
        if self.channels.imessage.enabled && self.channels.imessage.poll_interval_ms == 0 {
            return Err(anyhow::anyhow!(
                "channels.imessage.poll_interval_ms must be > 0"
            ));
        }
        if self.channels.imessage.enabled && self.channels.imessage.max_per_poll == 0 {
            return Err(anyhow::anyhow!(
                "channels.imessage.max_per_poll must be > 0"
            ));
        }
        if self.channels.imessage.enabled
            && match self.channels.imessage.source_db.as_deref() {
                Some(v) => v.trim().is_empty(),
                None => true,
            }
        {
            return Err(anyhow::anyhow!(
                "channels.imessage.source_db is required when channels.imessage.enabled=true"
            ));
        }
        validate_channel_access(
            "channels.email",
            self.channels.email.enabled,
            &self.channels.email.access,
        )?;
        if self.channels.email.enabled {
            if !self
                .channels
                .email
                .provider
                .trim()
                .eq_ignore_ascii_case("gmail")
            {
                return Err(anyhow::anyhow!(
                    "channels.email.provider must be 'gmail' when channels.email.enabled=true"
                ));
            }
            if self.channels.email.gmail_access_token.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.email.gmail_access_token is required when channels.email.enabled=true"
                ));
            }
            if self.channels.email.poll_interval_ms == 0 {
                return Err(anyhow::anyhow!(
                    "channels.email.poll_interval_ms must be > 0"
                ));
            }
            if self.channels.email.max_results == 0 {
                return Err(anyhow::anyhow!("channels.email.max_results must be > 0"));
            }
            if self.channels.email.query.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.email.query must not be empty when channels.email.enabled=true"
                ));
            }
        }
        validate_channel_access(
            "channels.linear",
            self.channels.linear.enabled,
            &self.channels.linear.access,
        )?;
        if self.channels.linear.enabled {
            if self.channels.linear.api_key.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.linear.api_key is required when channels.linear.enabled=true"
                ));
            }
            if self.channels.linear.poll_interval_ms == 0 {
                return Err(anyhow::anyhow!(
                    "channels.linear.poll_interval_ms must be > 0"
                ));
            }
            if self.channels.linear.max_issues == 0 {
                return Err(anyhow::anyhow!("channels.linear.max_issues must be > 0"));
            }
            if self
                .channels
                .linear
                .team_ids
                .iter()
                .any(|team| team.trim().is_empty())
            {
                return Err(anyhow::anyhow!(
                    "channels.linear.team_ids cannot contain empty values"
                ));
            }
        }
        let reserved_channel_ids = std::collections::HashSet::from([
            "webchat", "telegram", "discord", "slack", "matrix", "signal", "whatsapp", "imessage",
            "email", "linear",
        ]);
        let mut seen_external_channel_ids = std::collections::HashSet::new();
        for plugin in &self.channels.external_plugins {
            let plugin_id = plugin.id.trim().to_ascii_lowercase();
            if plugin_id.is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.external_plugins entries must include non-empty id"
                ));
            }
            if !plugin_id
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
            {
                return Err(anyhow::anyhow!(
                    "channels.external_plugins id {:?} is invalid: expected [a-z0-9_-]+",
                    plugin.id
                ));
            }
            if reserved_channel_ids.contains(plugin_id.as_str()) {
                return Err(anyhow::anyhow!(
                    "channels.external_plugins id {:?} conflicts with built-in channel id",
                    plugin.id
                ));
            }
            if !seen_external_channel_ids.insert(plugin_id.clone()) {
                return Err(anyhow::anyhow!(
                    "channels.external_plugins contains duplicate id {:?}",
                    plugin.id
                ));
            }
            validate_channel_access(
                &format!("channels.external_plugins[{plugin_id}]"),
                plugin.enabled,
                &plugin.access,
            )?;
            if !plugin.enabled {
                continue;
            }
            let send_url = plugin.send_url.trim();
            if send_url.is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.external_plugins[{plugin_id}].send_url is required when enabled=true"
                ));
            }
            let send_parsed = reqwest::Url::parse(send_url).map_err(|e| {
                anyhow::anyhow!(
                    "channels.external_plugins[{plugin_id}].send_url must be a valid URL: {e}"
                )
            })?;
            if !matches!(send_parsed.scheme(), "http" | "https") {
                return Err(anyhow::anyhow!(
                    "channels.external_plugins[{plugin_id}].send_url must use http or https scheme"
                ));
            }
            if plugin.poll_interval_ms == 0 {
                return Err(anyhow::anyhow!(
                    "channels.external_plugins[{plugin_id}].poll_interval_ms must be > 0"
                ));
            }
            if let Some(poll_url) = plugin.poll_url.as_deref() {
                let poll_url = poll_url.trim();
                if poll_url.is_empty() {
                    return Err(anyhow::anyhow!(
                        "channels.external_plugins[{plugin_id}].poll_url must not be empty when provided"
                    ));
                }
                let poll_parsed = reqwest::Url::parse(poll_url).map_err(|e| {
                    anyhow::anyhow!(
                        "channels.external_plugins[{plugin_id}].poll_url must be a valid URL: {e}"
                    )
                })?;
                if !matches!(poll_parsed.scheme(), "http" | "https") {
                    return Err(anyhow::anyhow!(
                        "channels.external_plugins[{plugin_id}].poll_url must use http or https scheme"
                    ));
                }
            }
            if let Some(auth_token) = plugin.auth_token.as_deref() {
                if auth_token.trim().is_empty() {
                    return Err(anyhow::anyhow!(
                        "channels.external_plugins[{plugin_id}].auth_token must not be empty when provided"
                    ));
                }
            }
        }
        let mut seen_control_tokens = std::collections::HashSet::new();
        if let Some(raw) = self.security.control_api_key.as_deref() {
            let token = raw.trim();
            if token.is_empty() {
                return Err(anyhow::anyhow!(
                    "security.control_api_key must not be empty when provided"
                ));
            }
            seen_control_tokens.insert(token.to_string());
        }
        for token in &self.security.control_api_keys {
            let token_value = token.token.trim();
            if token_value.is_empty() {
                return Err(anyhow::anyhow!(
                    "security.control_api_keys entries must include non-empty token"
                ));
            }
            if !seen_control_tokens.insert(token_value.to_string()) {
                return Err(anyhow::anyhow!(
                    "security.control_api_keys contains duplicate token values"
                ));
            }
            for scope in &token.scopes {
                if !is_supported_control_api_scope(scope) {
                    return Err(anyhow::anyhow!(
                        "security.control_api_keys contains unsupported scope {:?}; supported scopes: {:?}",
                        scope,
                        supported_control_api_scopes()
                    ));
                }
            }
            if let Some(description) = token.description.as_deref() {
                if description.trim().is_empty() {
                    return Err(anyhow::anyhow!(
                        "security.control_api_keys descriptions must not be empty when provided"
                    ));
                }
            }
        }
        if self.runtime.mode == RuntimeMode::Prod && !self.control_api_auth_configured() {
            return Err(anyhow::anyhow!(
                "security.control_api_key or security.control_api_keys is required when runtime.mode=prod"
            ));
        }
        for prefix in &self.security.mutating_auth_exempt_prefixes {
            let trimmed = prefix.trim();
            if trimmed.is_empty() {
                return Err(anyhow::anyhow!(
                    "security.mutating_auth_exempt_prefixes cannot contain empty values"
                ));
            }
            if !trimmed.starts_with('/') {
                return Err(anyhow::anyhow!(
                    "security.mutating_auth_exempt_prefixes entries must start with '/': {prefix:?}"
                ));
            }
        }
        let _ = self.runtime_network_policy()?;
        for profile_name in self.llm_profile_chain_names()? {
            let profile = self.llm_profile(&profile_name)?;
            let _ = self.api_keys_for_provider(profile.provider)?;
        }
        Ok(())
    }

    fn channel_access_config(&self, channel_id: &str) -> Option<&ChannelAccessConfig> {
        match channel_id {
            "telegram" => Some(&self.channels.telegram.access),
            "discord" => Some(&self.channels.discord.access),
            "slack" => Some(&self.channels.slack.access),
            "matrix" => Some(&self.channels.matrix.access),
            "signal" => Some(&self.channels.signal.access),
            "whatsapp" => Some(&self.channels.whatsapp.access),
            "imessage" => Some(&self.channels.imessage.access),
            "email" => Some(&self.channels.email.access),
            "linear" => Some(&self.channels.linear.access),
            _ => self
                .channels
                .external_plugins
                .iter()
                .find(|plugin| plugin.id.eq_ignore_ascii_case(channel_id))
                .map(|plugin| &plugin.access),
        }
    }

    pub fn channel_access_mode(&self, channel_id: &str) -> SenderAccessMode {
        self.channel_access_config(channel_id)
            .map(|access| access.mode)
            .unwrap_or(SenderAccessMode::Pairing)
    }

    pub fn channel_is_sender_allowlisted(&self, channel_id: &str, sender_id: &str) -> bool {
        self.channel_access_config(channel_id)
            .map(|access| {
                access
                    .allowed_senders
                    .iter()
                    .map(|entry| entry.trim())
                    .any(|entry| entry == sender_id)
            })
            .unwrap_or(false)
    }

    pub fn llm_profile(&self, name: &str) -> anyhow::Result<&LlmProfileConfig> {
        self.llm
            .profiles
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("llm profile {:?} not found", name))
    }

    pub fn llm_profile_chain_names(&self) -> anyhow::Result<Vec<String>> {
        let active = self.llm.active_profile.trim();
        if active.is_empty() {
            return Err(anyhow::anyhow!("llm.active_profile is required"));
        }

        let mut names = Vec::with_capacity(1 + self.llm.fallback_profiles.len());
        names.push(active.to_string());
        for fallback in &self.llm.fallback_profiles {
            let fallback = fallback.trim();
            if fallback.is_empty() {
                return Err(anyhow::anyhow!(
                    "llm.fallback_profiles cannot contain empty values"
                ));
            }
            if !names.iter().any(|existing| existing == fallback) {
                names.push(fallback.to_string());
            }
        }

        for name in &names {
            if !self.llm.profiles.contains_key(name) {
                return Err(anyhow::anyhow!(
                    "llm profile {:?} referenced by active/fallback chain is not defined; available profiles: {:?}",
                    name,
                    self.llm.profiles.keys().collect::<Vec<_>>()
                ));
            }
        }
        Ok(names)
    }

    pub fn configured_models(&self) -> anyhow::Result<Vec<String>> {
        let mut models = Vec::new();
        for profile_name in self.llm_profile_chain_names()? {
            let profile = self.llm_profile(&profile_name)?;
            push_unique_model_name(&mut models, profile.model.as_str());
            for fallback in &profile.fallback_models {
                push_unique_model_name(&mut models, fallback.as_str());
            }
        }
        Ok(models)
    }

    pub fn default_model(&self) -> anyhow::Result<&str> {
        let profile_name = self.llm.active_profile.trim();
        let profile = self.llm_profile(profile_name)?;
        Ok(profile.model.trim())
    }

    pub fn api_key_for_active_profile_model(&self) -> anyhow::Result<String> {
        let profile_name = self.llm.active_profile.trim();
        let profile = self.llm_profile(profile_name)?;
        self.api_keys_for_provider(profile.provider)?
            .into_iter()
            .next()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no usable API keys found for llm.active_profile={:?}",
                    self.llm.active_profile
                )
            })
    }

    pub fn api_keys_for_provider(&self, provider: LlmProvider) -> anyhow::Result<Vec<String>> {
        let mut keys = Vec::new();
        match provider {
            LlmProvider::Anthropic => {
                push_unique_trimmed(&mut keys, self.keys.anthropic_api_key.as_deref());
                for key in &self.keys.anthropic_api_keys {
                    push_unique_trimmed(&mut keys, Some(key.as_str()));
                }
            }
            LlmProvider::Openai => {
                push_unique_trimmed(&mut keys, self.keys.openai_api_key.as_deref());
                for key in &self.keys.openai_api_keys {
                    push_unique_trimmed(&mut keys, Some(key.as_str()));
                }
            }
        }
        if keys.is_empty() {
            let provider_name = match provider {
                LlmProvider::Anthropic => "Anthropic",
                LlmProvider::Openai => "OpenAI",
            };
            return Err(anyhow::anyhow!(
                "at least one {provider_name} API key is required for configured llm profiles"
            ));
        }
        Ok(keys)
    }
}

fn validate_channel_access(
    channel_field: &str,
    enabled: bool,
    access: &ChannelAccessConfig,
) -> anyhow::Result<()> {
    let mut seen = std::collections::HashSet::new();
    for sender in &access.allowed_senders {
        let trimmed = sender.trim();
        if trimmed.is_empty() {
            return Err(anyhow::anyhow!(
                "{channel_field}.access.allowed_senders cannot contain empty values"
            ));
        }
        if trimmed.chars().any(char::is_control) {
            return Err(anyhow::anyhow!(
                "{channel_field}.access.allowed_senders cannot contain control characters"
            ));
        }
        if !seen.insert(trimmed.to_string()) {
            return Err(anyhow::anyhow!(
                "{channel_field}.access.allowed_senders contains duplicate sender id {:?}",
                trimmed
            ));
        }
    }

    if enabled && access.mode == SenderAccessMode::Allowlist && access.allowed_senders.is_empty() {
        return Err(anyhow::anyhow!(
            "{channel_field}.access.allowed_senders must contain at least one sender when access.mode=allowlist and the channel is enabled"
        ));
    }

    Ok(())
}

fn resolve_bind_addr(
    bind_mode: BindMode,
    bind_addr: Option<&str>,
    webchat_port: u16,
) -> anyhow::Result<SocketAddr> {
    let addr = match bind_mode {
        BindMode::Loopback => SocketAddr::from(([127, 0, 0, 1], webchat_port)),
        BindMode::Lan => SocketAddr::from(([0, 0, 0, 0], webchat_port)),
        BindMode::Tailnet => SocketAddr::from(([127, 0, 0, 1], webchat_port)),
        BindMode::Auto => SocketAddr::from(([127, 0, 0, 1], webchat_port)),
        BindMode::Custom => {
            let raw = bind_addr
                .ok_or_else(|| {
                    anyhow::anyhow!("runtime.bind_addr is required when runtime.bind_mode=custom")
                })?
                .trim()
                .to_string();
            if raw.is_empty() {
                return Err(anyhow::anyhow!(
                    "runtime.bind_addr must not be empty when runtime.bind_mode=custom"
                ));
            }
            raw.parse::<SocketAddr>().map_err(|e| {
                anyhow::anyhow!("runtime.bind_addr must be a valid socket address: {e}")
            })?
        }
    };
    Ok(addr)
}

fn resolve_runtime_exposure(
    bind_mode: BindMode,
    bind_addr: SocketAddr,
    discovery_mode: DiscoveryMode,
) -> RuntimeExposure {
    match bind_mode {
        BindMode::Loopback => RuntimeExposure::Loopback,
        BindMode::Lan => RuntimeExposure::Lan,
        BindMode::Tailnet => RuntimeExposure::TailnetProxy,
        BindMode::Auto => {
            if matches!(
                discovery_mode,
                DiscoveryMode::TailnetServe | DiscoveryMode::TailnetFunnel
            ) {
                RuntimeExposure::TailnetProxy
            } else {
                RuntimeExposure::Loopback
            }
        }
        BindMode::Custom => {
            if bind_addr.ip().is_loopback() {
                RuntimeExposure::CustomLoopback
            } else {
                RuntimeExposure::CustomPublic
            }
        }
    }
}

fn push_unique_trimmed(target: &mut Vec<String>, candidate: Option<&str>) {
    let Some(raw) = candidate else { return };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }
    if !target.iter().any(|existing| existing == trimmed) {
        target.push(trimmed.to_string());
    }
}

fn push_unique_model_name(target: &mut Vec<String>, candidate: &str) {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return;
    }
    if !target
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(trimmed))
    {
        target.push(trimmed.to_string());
    }
}

pub(crate) fn supported_control_api_scopes() -> &'static [&'static str] {
    &[
        "*",
        "control:write",
        "config:write",
        "sessions:write",
        "automation:write",
        "skills:write",
        "messages:write",
        "channels:write",
    ]
}

pub(crate) fn normalize_control_api_scope(scope: &str) -> Option<String> {
    let normalized = scope.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    Some(normalized)
}

pub(crate) fn is_supported_control_api_scope(scope: &str) -> bool {
    let Some(normalized) = normalize_control_api_scope(scope) else {
        return false;
    };
    supported_control_api_scopes()
        .iter()
        .any(|supported| normalized == *supported)
}

pub fn default_config_path() -> anyhow::Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?;
    Ok(Path::new(&home).join(".opencraw").join("config.toml"))
}

fn config_fragments_dir(base_config_path: &Path) -> PathBuf {
    let parent = base_config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    parent.join("configs")
}

async fn read_config_fragment_entries(
    fragments_dir: &Path,
) -> anyhow::Result<Vec<(ConfigFileKind, PathBuf)>> {
    let metadata = match tokio::fs::metadata(fragments_dir).await {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(anyhow::anyhow!(
                "read configs directory {}: {err}",
                fragments_dir.display()
            ));
        }
    };
    if !metadata.is_dir() {
        return Err(anyhow::anyhow!(
            "configs path {} exists but is not a directory",
            fragments_dir.display()
        ));
    }

    let mut read_dir = tokio::fs::read_dir(fragments_dir).await.map_err(|err| {
        anyhow::anyhow!("open configs directory {}: {err}", fragments_dir.display())
    })?;

    let mut entries: Vec<(String, ConfigFileKind, PathBuf)> = Vec::new();
    let mut seen_kinds: std::collections::HashMap<ConfigFileKind, PathBuf> =
        std::collections::HashMap::new();

    while let Some(entry) = read_dir.next_entry().await.map_err(|err| {
        anyhow::anyhow!(
            "read configs directory entry {}: {err}",
            fragments_dir.display()
        )
    })? {
        let path = entry.path();
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid config filename: {}", path.display()))?
            .to_string();

        if file_name.starts_with('.') {
            continue;
        }

        let file_type = entry.file_type().await.map_err(|err| {
            anyhow::anyhow!("read config fragment metadata {}: {err}", path.display())
        })?;
        if !file_type.is_file() {
            tracing::debug!(
                path = %path.display(),
                "ignoring non-file entry in configs directory"
            );
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            tracing::debug!(
                path = %path.display(),
                "ignoring non-toml entry in configs directory"
            );
            continue;
        }

        let Some(kind) = ConfigFileKind::from_file_name(&file_name) else {
            tracing::debug!(
                file_name = %file_name,
                expected = %ConfigFileKind::canonical_file_names_csv(),
                "ignoring unknown config fragment filename"
            );
            continue;
        };

        if let Some(existing_path) = seen_kinds.insert(kind, path.clone()) {
            return Err(anyhow::anyhow!(
                "duplicate config fragment kind {:?}: {} and {} (use only one of aliases for {})",
                kind,
                existing_path.display(),
                path.display(),
                kind.canonical_file_name()
            ));
        }

        entries.push((file_name, kind, path));
    }

    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries
        .into_iter()
        .map(|(_, kind, path)| (kind, path))
        .collect())
}

fn expand_home(path: &str) -> anyhow::Result<PathBuf> {
    let trimmed = path.trim().to_string();
    if !trimmed.starts_with("~/") {
        return Ok(PathBuf::from(trimmed));
    }
    let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(trimmed.replacen("~", &home, 1)))
}

#[cfg(test)]
mod tests {
    use super::{
        BindMode, ChannelAccessConfig, ControlApiKeyConfig, DiscoveryMode,
        ExternalChannelPluginConfig, OpenShellConfig, RuntimeExposure, SenderAccessMode,
        ToolProfile, ToolsConfig, canonical_tool_name,
    };
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    fn base_config() -> OpenShellConfig {
        toml::from_str(
            r#"
[llm]
active_profile = "default"

[llm.profiles.default]
provider = "openai"
model = "gpt-4o-mini"

[general]
system_prompt = "test prompt"

[keys]
openai_api_key = "test-key"

[channels.webchat]
enabled = true
port = 3000
"#,
        )
        .expect("parse base config")
    }

    fn temp_config_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("opencraw-config-{name}-{}", Uuid::new_v4()))
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent directory");
        }
        std::fs::write(path, contents).expect("write file");
    }

    fn load_with_path_blocking(path: PathBuf) -> anyhow::Result<OpenShellConfig> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        runtime.block_on(async { OpenShellConfig::load(Some(path)).await })
    }

    #[test]
    fn loads_and_applies_configs_directory_fragments() {
        let root = temp_config_root("fragments-merge");
        let config_path = root.join("config.toml");
        let telegram_path = root.join("configs/channel-telegram.toml");
        write_file(
            &config_path,
            r#"
[llm]
active_profile = "default"

[llm.profiles.default]
provider = "openai"
model = "gpt-4o-mini"

[general]
system_prompt = "test"

[keys]
openai_api_key = "x"

[channels.webchat]
enabled = true
port = 3000
"#,
        );
        write_file(
            &telegram_path,
            r#"
[channels.telegram]
enabled = true
bot_token = "token"

[channels.telegram.access]
mode = "open"
"#,
        );

        let cfg = load_with_path_blocking(config_path.clone()).expect("load config");
        assert!(
            cfg.channels.telegram.enabled,
            "channel fragment should override base config"
        );
        assert_eq!(
            cfg.channels.telegram.access.mode,
            SenderAccessMode::Open,
            "channel access mode should come from channel fragment"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ignores_unknown_config_fragment_file_name() {
        let root = temp_config_root("fragments-unknown-file");
        let config_path = root.join("config.toml");
        write_file(
            &config_path,
            r#"
[llm]
active_profile = "default"

[llm.profiles.default]
provider = "openai"
model = "gpt-4o-mini"

[general]
system_prompt = "test"

[keys]
openai_api_key = "x"

[channels.webchat]
enabled = true
port = 3000

[channels.telegram]
enabled = false
bot_token = ""
"#,
        );
        write_file(
            &root.join("configs/provider-telegram.toml"),
            r#"
[channels.telegram]
enabled = true
bot_token = "token"
"#,
        );

        let cfg = load_with_path_blocking(config_path.clone())
            .expect("unknown fragment filename should be ignored");
        assert!(
            !cfg.channels.telegram.enabled,
            "unknown fragment file must not override known config"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ignores_non_toml_files_in_configs_dir() {
        let root = temp_config_root("fragments-ignore-non-toml");
        let config_path = root.join("config.toml");
        write_file(
            &config_path,
            r#"
[llm]
active_profile = "default"

[llm.profiles.default]
provider = "openai"
model = "gpt-4o-mini"

[general]
system_prompt = "test"

[keys]
openai_api_key = "x"

[channels.webchat]
enabled = true
port = 3000

[channels.email]
enabled = false
poll_interval_ms = 5000
"#,
        );
        write_file(
            &root.join("configs/channel-email.toml.bak.20260210T221132Z"),
            r#"
[channels.email]
enabled = true
poll_interval_ms = 1000
"#,
        );

        let cfg = load_with_path_blocking(config_path.clone())
            .expect("non-toml fragment files should be ignored");
        assert!(
            !cfg.channels.email.enabled,
            "non-toml backup fragment must not override channel config"
        );
        assert_eq!(
            cfg.channels.email.poll_interval_ms, 5000,
            "base config should remain unchanged when backup files are present"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_invalid_fragment_schema() {
        let root = temp_config_root("fragments-invalid-schema");
        let config_path = root.join("config.toml");
        write_file(
            &config_path,
            r#"
[llm]
active_profile = "default"

[llm.profiles.default]
provider = "openai"
model = "gpt-4o-mini"

[general]
system_prompt = "test"

[keys]
openai_api_key = "x"

[channels.webchat]
enabled = true
port = 3000
"#,
        );
        write_file(
            &root.join("configs/security.toml"),
            r#"
[security]
not_a_real_key = 1
"#,
        );

        let err =
            load_with_path_blocking(config_path.clone()).expect_err("invalid schema should fail");
        assert!(
            err.to_string().contains("parse config fragment"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn canonical_tool_name_accepts_shell_alias() {
        assert_eq!(canonical_tool_name("shell"), Some("shell_execute"));
        assert_eq!(canonical_tool_name("shell_execute"), Some("shell_execute"));
        assert_eq!(canonical_tool_name("patch"), Some("apply_patch"));
    }

    #[test]
    fn deny_takes_precedence_over_allow() {
        let cfg = ToolsConfig {
            allow: vec!["shell".to_string()],
            deny: vec!["shell_execute".to_string()],
            ..ToolsConfig::default()
        };
        assert!(!cfg.is_tool_enabled("shell_execute", true));
    }

    #[test]
    fn allowlist_overrides_default_toggle() {
        let cfg = ToolsConfig {
            allow: vec!["browser".to_string()],
            deny: vec![],
            ..ToolsConfig::default()
        };
        assert!(cfg.is_tool_enabled("browser", false));
        assert!(!cfg.is_tool_enabled("filesystem", true));
    }

    #[test]
    fn profile_defaults_enable_coding_tools() {
        let cfg = ToolsConfig {
            profile: ToolProfile::Coding,
            ..ToolsConfig::default()
        };
        assert!(cfg.tool_enabled("shell_execute"));
        assert!(cfg.tool_enabled("filesystem"));
        assert!(cfg.tool_enabled("apply_patch"));
        assert!(cfg.tool_enabled("browser"));
        assert!(!cfg.tool_enabled("imessage"));
    }

    #[test]
    fn profile_defaults_enable_messaging_tools() {
        let cfg = ToolsConfig {
            profile: ToolProfile::Messaging,
            ..ToolsConfig::default()
        };
        assert!(cfg.tool_enabled("email"));
        assert!(cfg.tool_enabled("imessage"));
        assert!(cfg.tool_enabled("linear"));
        assert!(!cfg.tool_enabled("shell_execute"));
    }

    #[test]
    fn explicit_toggle_enables_tool_outside_profile() {
        let cfg = ToolsConfig {
            profile: ToolProfile::Minimal,
            browser: true,
            ..ToolsConfig::default()
        };
        assert!(cfg.tool_enabled("browser"));
    }

    #[test]
    fn public_bind_requires_auth_or_explicit_override() {
        let mut cfg = base_config();
        cfg.runtime.bind_mode = BindMode::Lan;

        let err = cfg
            .runtime_network_policy()
            .expect_err("lan bind without auth should fail");
        assert!(err.to_string().contains(
            "public bind target requires security.control_api_key or security.control_api_keys"
        ));

        cfg.runtime.allow_public_bind_without_auth = true;
        let policy = cfg
            .runtime_network_policy()
            .expect("explicit override should allow lan bind");
        assert_eq!(policy.exposure, RuntimeExposure::Lan);
        assert!(policy.public_ingress);
    }

    #[test]
    fn mdns_discovery_requires_public_bind_target() {
        let mut cfg = base_config();
        cfg.runtime.discovery_mode = DiscoveryMode::Mdns;
        cfg.security.control_api_key = Some("token".to_string());

        let err = cfg
            .runtime_network_policy()
            .expect_err("mdns on loopback should fail");
        assert!(err.to_string().contains("runtime.discovery_mode=mdns"));

        cfg.runtime.bind_mode = BindMode::Lan;
        let policy = cfg
            .runtime_network_policy()
            .expect("mdns should be valid on lan bind");
        assert_eq!(policy.exposure, RuntimeExposure::Lan);
    }

    #[test]
    fn public_bind_allows_rotating_control_api_keys() {
        let mut cfg = base_config();
        cfg.runtime.bind_mode = BindMode::Lan;
        cfg.security.control_api_keys = vec![ControlApiKeyConfig {
            token: "token-1".to_string(),
            scopes: vec!["config:write".to_string()],
            description: Some("ops".to_string()),
        }];

        let policy = cfg
            .runtime_network_policy()
            .expect("scoped rotating keys should satisfy auth requirement");
        assert!(policy.control_api_auth_configured);
        assert_eq!(policy.exposure, RuntimeExposure::Lan);
    }

    #[test]
    fn tailnet_funnel_requires_control_api_key() {
        let mut cfg = base_config();
        cfg.runtime.bind_mode = BindMode::Tailnet;
        cfg.runtime.discovery_mode = DiscoveryMode::TailnetFunnel;

        let err = cfg
            .runtime_network_policy()
            .expect_err("tailnet funnel without auth should fail");
        assert!(err.to_string().contains(
            "tailnet_funnel requires security.control_api_key or security.control_api_keys"
        ));

        cfg.security.control_api_key = Some("token".to_string());
        let policy = cfg
            .runtime_network_policy()
            .expect("tailnet funnel should pass with auth");
        assert_eq!(policy.exposure, RuntimeExposure::TailnetProxy);
    }

    #[test]
    fn duplicate_legacy_and_rotating_control_tokens_rejected() {
        let mut cfg = base_config();
        cfg.security.control_api_key = Some("dup-token".to_string());
        cfg.security.control_api_keys = vec![ControlApiKeyConfig {
            token: "dup-token".to_string(),
            scopes: vec!["config:write".to_string()],
            description: None,
        }];

        let err = cfg
            .validate()
            .expect_err("duplicate legacy+rotating token must fail");
        assert!(
            err.to_string()
                .contains("security.control_api_keys contains duplicate token values")
        );
    }

    #[test]
    fn enabled_allowlist_mode_requires_sender_ids() {
        let mut cfg = base_config();
        cfg.channels.telegram.enabled = true;
        cfg.channels.telegram.bot_token = "token".to_string();
        cfg.channels.telegram.access.mode = SenderAccessMode::Allowlist;
        cfg.channels.telegram.access.allowed_senders.clear();

        let err = cfg
            .validate()
            .expect_err("enabled allowlist mode without senders should fail");
        assert!(
            err.to_string().contains(
                "channels.telegram.access.allowed_senders must contain at least one sender"
            )
        );
    }

    #[test]
    fn channel_access_lookup_is_provider_local() {
        let mut cfg = base_config();
        cfg.channels.telegram.access.mode = SenderAccessMode::Open;
        cfg.channels.telegram.access.allowed_senders = vec!["12345".to_string()];

        assert_eq!(cfg.channel_access_mode("telegram"), SenderAccessMode::Open);
        assert!(cfg.channel_is_sender_allowlisted("telegram", "12345"));
        assert!(!cfg.channel_is_sender_allowlisted("telegram", "99999"));
    }

    #[test]
    fn external_plugin_access_lookup_uses_plugin_config() {
        let mut cfg = base_config();
        cfg.channels
            .external_plugins
            .push(ExternalChannelPluginConfig {
                id: "custom_ops".to_string(),
                enabled: true,
                send_url: "https://plugins.example.com/send".to_string(),
                poll_url: None,
                auth_token: None,
                poll_interval_ms: 3000,
                start_from_latest: true,
                supports_streaming_deltas: false,
                supports_typing_events: false,
                supports_reactions: false,
                access: ChannelAccessConfig {
                    mode: SenderAccessMode::Allowlist,
                    allowed_senders: vec!["ops-user".to_string()],
                },
            });

        assert_eq!(
            cfg.channel_access_mode("custom_ops"),
            SenderAccessMode::Allowlist
        );
        assert!(cfg.channel_is_sender_allowlisted("custom_ops", "ops-user"));
        assert!(!cfg.channel_is_sender_allowlisted("custom_ops", "random-user"));
    }

    #[test]
    fn external_plugin_id_conflicting_with_builtin_is_rejected() {
        let mut cfg = base_config();
        cfg.channels
            .external_plugins
            .push(ExternalChannelPluginConfig {
                id: "slack".to_string(),
                enabled: true,
                send_url: "https://plugins.example.com/send".to_string(),
                poll_url: None,
                auth_token: None,
                poll_interval_ms: 3000,
                start_from_latest: true,
                supports_streaming_deltas: false,
                supports_typing_events: false,
                supports_reactions: false,
                access: ChannelAccessConfig::default(),
            });

        let err = cfg
            .validate()
            .expect_err("builtin id conflict should fail validation");
        assert!(
            err.to_string()
                .contains("conflicts with built-in channel id")
        );
    }

    #[test]
    fn enabled_external_plugin_requires_valid_send_url() {
        let mut cfg = base_config();
        cfg.channels
            .external_plugins
            .push(ExternalChannelPluginConfig {
                id: "custom_ops".to_string(),
                enabled: true,
                send_url: "ftp://plugins.example.com/send".to_string(),
                poll_url: Some("https://plugins.example.com/poll".to_string()),
                auth_token: Some("secret".to_string()),
                poll_interval_ms: 3000,
                start_from_latest: true,
                supports_streaming_deltas: false,
                supports_typing_events: false,
                supports_reactions: false,
                access: ChannelAccessConfig::default(),
            });

        let err = cfg
            .validate()
            .expect_err("invalid send_url scheme should fail validation");
        assert!(
            err.to_string()
                .contains("send_url must use http or https scheme")
        );
    }
}
