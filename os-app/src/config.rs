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
    pub model: String,
    #[serde(default)]
    pub fallback_models: Vec<String>,
    #[serde(default = "default_failover_cooldown_base_seconds")]
    pub failover_cooldown_base_seconds: u64,
    #[serde(default = "default_failover_cooldown_max_seconds")]
    pub failover_cooldown_max_seconds: u64,
    pub system_prompt: String,
}

fn default_failover_cooldown_base_seconds() -> u64 {
    5
}

fn default_failover_cooldown_max_seconds() -> u64 {
    300
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscordConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// In group chats, only respond if the message starts with one of these prefixes.
    /// Example: ["@openshell", "openshell"]
    #[serde(default)]
    pub group_prefixes: Vec<String>,
}

fn default_imessage_poll_interval_ms() -> u64 {
    1500
}

fn default_imessage_start_from_latest() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinearConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_linear_poll_interval_ms")]
    pub poll_interval_ms: u64,
    /// Optional list of team IDs/keys/names to include.
    #[serde(default)]
    pub team_ids: Vec<String>,
    #[serde(default = "default_linear_start_from_latest")]
    pub start_from_latest: bool,
}

fn default_linear_poll_interval_ms() -> u64 {
    3000
}

fn default_linear_start_from_latest() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
}

fn default_slack_poll_interval_ms() -> u64 {
    3000
}

fn default_slack_start_from_latest() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// `browser`, `clipboard`, `apply_patch`, `email`, `imessage`.
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
        ToolProfile::Messaging => matches!(canonical, "email" | "imessage"),
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
    #[serde(default)]
    pub allowed_users: Vec<String>,
    /// If true, OpenShell will respond to any sender on non-webchat channels.
    ///
    /// Default is false for safety: external channels require an explicit allowlist in
    /// `security.allowed_users`.
    #[serde(default)]
    pub allow_all_senders: bool,
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
            allowed_users: Vec::new(),
            allow_all_senders: false,
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

        cfg.apply_env_overrides()?;
        cfg.validate()?;
        Ok((cfg, path))
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

    fn apply_env_overrides(&mut self) -> anyhow::Result<()> {
        if let Ok(v) = std::env::var("OPENSHELL_RUNTIME_MODE") {
            let mode = v.trim().to_ascii_lowercase();
            self.runtime.mode = match mode.as_str() {
                "dev" => RuntimeMode::Dev,
                "prod" => RuntimeMode::Prod,
                _ => {
                    return Err(anyhow::anyhow!(
                        "invalid OPENSHELL_RUNTIME_MODE={v:?}: expected 'dev' or 'prod'"
                    ));
                }
            };
        }
        if let Ok(v) = std::env::var("OPENSHELL_BIND_MODE") {
            let mode = v.trim().to_ascii_lowercase();
            self.runtime.bind_mode = match mode.as_str() {
                "loopback" => BindMode::Loopback,
                "lan" => BindMode::Lan,
                "tailnet" => BindMode::Tailnet,
                "auto" => BindMode::Auto,
                "custom" => BindMode::Custom,
                _ => {
                    return Err(anyhow::anyhow!(
                        "invalid OPENSHELL_BIND_MODE={v:?}: expected 'loopback', 'lan', 'tailnet', 'auto', or 'custom'"
                    ));
                }
            };
        }
        if let Ok(v) = std::env::var("OPENSHELL_DISCOVERY_MODE") {
            let mode = v.trim().to_ascii_lowercase();
            self.runtime.discovery_mode = match mode.as_str() {
                "disabled" | "off" => DiscoveryMode::Disabled,
                "mdns" | "bonjour" => DiscoveryMode::Mdns,
                "tailnet_serve" | "serve" => DiscoveryMode::TailnetServe,
                "tailnet_funnel" | "funnel" => DiscoveryMode::TailnetFunnel,
                _ => {
                    return Err(anyhow::anyhow!(
                        "invalid OPENSHELL_DISCOVERY_MODE={v:?}: expected 'disabled', 'mdns', 'tailnet_serve', or 'tailnet_funnel'"
                    ));
                }
            };
        }
        if let Ok(v) = std::env::var("OPENSHELL_DATA_DIR") {
            if !v.trim().is_empty() {
                self.runtime.data_dir = v;
            }
        }
        if let Ok(v) = std::env::var("OPENSHELL_BIND_ADDR") {
            self.runtime.bind_addr = if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = std::env::var("OPENSHELL_ADVERTISED_BASE_URL") {
            self.runtime.advertised_base_url = if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = std::env::var("OPENSHELL_ALLOW_PUBLIC_BIND_WITHOUT_AUTH") {
            self.runtime.allow_public_bind_without_auth =
                parse_env_bool("OPENSHELL_ALLOW_PUBLIC_BIND_WITHOUT_AUTH", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_HTTP_TIMEOUT_SECONDS") {
            self.runtime.http_timeout_seconds =
                parse_env_u64("OPENSHELL_HTTP_TIMEOUT_SECONDS", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_HTTP_MAX_IN_FLIGHT") {
            self.runtime.http_max_in_flight = parse_env_usize("OPENSHELL_HTTP_MAX_IN_FLIGHT", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_QUEUE_MAX_CONCURRENCY") {
            self.queue.max_concurrency = parse_env_usize("OPENSHELL_QUEUE_MAX_CONCURRENCY", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_QUEUE_MODE") {
            let mode = v.trim().to_ascii_lowercase();
            self.queue.mode = match mode.as_str() {
                "collect" => QueueMode::Collect,
                "followup" => QueueMode::Followup,
                "steer" => QueueMode::Steer,
                "interrupt" => QueueMode::Interrupt,
                _ => {
                    return Err(anyhow::anyhow!(
                        "invalid OPENSHELL_QUEUE_MODE={v:?}: expected one of 'collect', 'followup', 'steer', 'interrupt'"
                    ));
                }
            };
        }
        if let Ok(v) = std::env::var("OPENSHELL_QUEUE_LANE_BUFFER") {
            self.queue.lane_buffer = parse_env_usize("OPENSHELL_QUEUE_LANE_BUFFER", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_QUEUE_DEBOUNCE_MS") {
            self.queue.debounce_ms = parse_env_u64("OPENSHELL_QUEUE_DEBOUNCE_MS", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_QUEUE_OVERFLOW_POLICY") {
            let policy = v.trim().to_ascii_lowercase();
            self.queue.overflow_policy = match policy.as_str() {
                "block" => QueueOverflowPolicy::Block,
                "drop" => QueueOverflowPolicy::Drop,
                "summarize" => QueueOverflowPolicy::Summarize,
                _ => {
                    return Err(anyhow::anyhow!(
                        "invalid OPENSHELL_QUEUE_OVERFLOW_POLICY={v:?}: expected one of 'block', 'drop', 'summarize'"
                    ));
                }
            };
        }
        if let Ok(v) = std::env::var("OPENSHELL_QUEUE_OVERFLOW_SUMMARY_MAX_MESSAGES") {
            self.queue.overflow_summary_max_messages =
                parse_env_usize("OPENSHELL_QUEUE_OVERFLOW_SUMMARY_MAX_MESSAGES", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_CONTEXT_MAX_PROMPT_TOKENS") {
            self.context.max_prompt_tokens =
                parse_env_usize("OPENSHELL_CONTEXT_MAX_PROMPT_TOKENS", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_CONTEXT_MIN_RECENT_MESSAGES") {
            self.context.min_recent_messages =
                parse_env_usize("OPENSHELL_CONTEXT_MIN_RECENT_MESSAGES", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_CONTEXT_MAX_TOOL_CHARS") {
            self.context.max_tool_chars = parse_env_usize("OPENSHELL_CONTEXT_MAX_TOOL_CHARS", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_CONTEXT_COMPACTION_ENABLED") {
            self.context.compaction_enabled =
                parse_env_bool("OPENSHELL_CONTEXT_COMPACTION_ENABLED", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_CONTEXT_COMPACTION_TRIGGER_TOKENS") {
            self.context.compaction_trigger_tokens =
                parse_env_usize("OPENSHELL_CONTEXT_COMPACTION_TRIGGER_TOKENS", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_CONTEXT_COMPACTION_RETAIN_MESSAGES") {
            self.context.compaction_retain_messages =
                parse_env_usize("OPENSHELL_CONTEXT_COMPACTION_RETAIN_MESSAGES", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_CONTEXT_COMPACTION_HORIZON") {
            if !v.trim().is_empty() {
                self.context.compaction_horizon = v;
            }
        }
        if let Ok(v) = std::env::var("OPENSHELL_CONTEXT_COMPACTION_FLUSH_MAX_CHARS") {
            self.context.compaction_flush_max_chars =
                parse_env_usize("OPENSHELL_CONTEXT_COMPACTION_FLUSH_MAX_CHARS", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_MODEL") {
            if !v.trim().is_empty() {
                self.general.model = v;
            }
        }
        if let Ok(v) = std::env::var("OPENSHELL_FAILOVER_COOLDOWN_BASE_SECONDS") {
            self.general.failover_cooldown_base_seconds =
                parse_env_u64("OPENSHELL_FAILOVER_COOLDOWN_BASE_SECONDS", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_FAILOVER_COOLDOWN_MAX_SECONDS") {
            self.general.failover_cooldown_max_seconds =
                parse_env_u64("OPENSHELL_FAILOVER_COOLDOWN_MAX_SECONDS", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_FALLBACK_MODELS") {
            self.general.fallback_models = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Ok(v) = std::env::var("OPENAI_API_KEY") {
            if !v.trim().is_empty() {
                self.keys.openai_api_key = Some(v);
            }
        }
        if let Ok(v) = std::env::var("OPENAI_API_KEYS") {
            self.keys.openai_api_keys = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Ok(v) = std::env::var("ANTHROPIC_API_KEY") {
            if !v.trim().is_empty() {
                self.keys.anthropic_api_key = Some(v);
            }
        }
        if let Ok(v) = std::env::var("ANTHROPIC_API_KEYS") {
            self.keys.anthropic_api_keys = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Ok(v) = std::env::var("OPENSHELL_CONTROL_API_KEY") {
            self.security.control_api_key = if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = std::env::var("OPENSHELL_CONTROL_API_KEYS") {
            self.security.control_api_keys = v
                .split(',')
                .map(|token| token.trim())
                .filter(|token| !token.is_empty())
                .map(|token| ControlApiKeyConfig {
                    token: token.to_string(),
                    scopes: Vec::new(),
                    description: None,
                })
                .collect();
        }
        if let Ok(v) = std::env::var("OPENSHELL_MUTATING_AUTH_EXEMPT_PREFIXES") {
            self.security.mutating_auth_exempt_prefixes = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Ok(v) = std::env::var("OPENSHELL_AUTOMATION_ENABLED") {
            self.automation.enabled = parse_env_bool("OPENSHELL_AUTOMATION_ENABLED", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_HEARTBEAT_INTERVAL_SECONDS") {
            self.automation.heartbeat_interval_seconds =
                parse_env_u64("OPENSHELL_HEARTBEAT_INTERVAL_SECONDS", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_WEBHOOK_SECRET") {
            self.automation.webhook_secret = if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = std::env::var("OPENSHELL_SKILLS_REQUIRE_SOURCE_PROVENANCE") {
            self.skills.require_source_provenance =
                parse_env_bool("OPENSHELL_SKILLS_REQUIRE_SOURCE_PROVENANCE", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_SKILLS_REQUIRE_HTTPS_SOURCE") {
            self.skills.require_https_source =
                parse_env_bool("OPENSHELL_SKILLS_REQUIRE_HTTPS_SOURCE", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_SKILLS_REQUIRE_TRUSTED_SOURCE") {
            self.skills.require_trusted_source =
                parse_env_bool("OPENSHELL_SKILLS_REQUIRE_TRUSTED_SOURCE", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_SKILLS_TRUSTED_SOURCE_PREFIXES") {
            self.skills.trusted_source_prefixes = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Ok(v) = std::env::var("OPENSHELL_SKILLS_REQUIRE_SHA256_SIGNATURE") {
            self.skills.require_sha256_signature =
                parse_env_bool("OPENSHELL_SKILLS_REQUIRE_SHA256_SIGNATURE", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_TOOL_PROFILE") {
            let profile = v.trim().to_ascii_lowercase();
            self.tools.profile = match profile.as_str() {
                "minimal" => ToolProfile::Minimal,
                "coding" => ToolProfile::Coding,
                "messaging" => ToolProfile::Messaging,
                "full" => ToolProfile::Full,
                _ => {
                    return Err(anyhow::anyhow!(
                        "invalid OPENSHELL_TOOL_PROFILE={v:?}: expected 'minimal', 'coding', 'messaging', or 'full'"
                    ));
                }
            };
        }
        if let Ok(v) = std::env::var("OPENSHELL_TOOLS_ALLOW") {
            self.tools.allow = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Ok(v) = std::env::var("OPENSHELL_TOOLS_DENY") {
            self.tools.deny = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Ok(v) = std::env::var("OPENSHELL_SHELL_DEFAULT_MODE") {
            let mode = v.trim().to_ascii_lowercase();
            self.tools.shell_policy.default_mode = match mode.as_str() {
                "sandbox" => ShellExecutionMode::Sandbox,
                "elevated" => ShellExecutionMode::Elevated,
                _ => {
                    return Err(anyhow::anyhow!(
                        "invalid OPENSHELL_SHELL_DEFAULT_MODE={v:?}: expected 'sandbox' or 'elevated'"
                    ));
                }
            };
        }
        if let Ok(v) = std::env::var("OPENSHELL_SHELL_SANDBOX_BACKEND") {
            let backend = v.trim().to_ascii_lowercase();
            self.tools.shell_policy.sandbox_backend = match backend.as_str() {
                "host_constrained" | "host" => ShellSandboxBackend::HostConstrained,
                "horizons_docker" | "docker" => ShellSandboxBackend::HorizonsDocker,
                _ => {
                    return Err(anyhow::anyhow!(
                        "invalid OPENSHELL_SHELL_SANDBOX_BACKEND={v:?}: expected 'host_constrained' or 'horizons_docker'"
                    ));
                }
            };
        }
        if let Ok(v) = std::env::var("OPENSHELL_SHELL_ALLOW_ELEVATED") {
            self.tools.shell_policy.allow_elevated =
                parse_env_bool("OPENSHELL_SHELL_ALLOW_ELEVATED", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_SHELL_SANDBOX_ROOT") {
            self.tools.shell_policy.sandbox_root = if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = std::env::var("OPENSHELL_SHELL_SANDBOX_IMAGE") {
            self.tools.shell_policy.sandbox_image =
                if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = std::env::var("OPENSHELL_SHELL_MAX_BACKGROUND_PROCESSES") {
            self.tools.shell_policy.max_background_processes =
                parse_env_usize("OPENSHELL_SHELL_MAX_BACKGROUND_PROCESSES", &v)?;
        }
        if let Ok(v) = std::env::var("TELEGRAM_BOT_TOKEN") {
            if !v.trim().is_empty() {
                self.channels.telegram.bot_token = v;
                self.channels.telegram.enabled = true;
            }
        }
        if let Ok(v) = std::env::var("DISCORD_BOT_TOKEN") {
            if !v.trim().is_empty() {
                self.channels.discord.bot_token = v;
                self.channels.discord.enabled = true;
            }
        }
        if let Ok(v) = std::env::var("OPENSHELL_SLACK_POLL_INTERVAL_MS") {
            self.channels.slack.poll_interval_ms =
                parse_env_u64("OPENSHELL_SLACK_POLL_INTERVAL_MS", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_SLACK_CHANNEL_IDS") {
            if !v.trim().is_empty() {
                self.channels.slack.channel_ids = v
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
        if let Ok(v) = std::env::var("OPENSHELL_SLACK_START_FROM_LATEST") {
            self.channels.slack.start_from_latest =
                parse_env_bool("OPENSHELL_SLACK_START_FROM_LATEST", &v)?;
        }
        if let Ok(v) = std::env::var("SLACK_BOT_TOKEN") {
            if !v.trim().is_empty() {
                self.channels.slack.bot_token = v;
                self.channels.slack.enabled = true;
            }
        }
        if let Ok(v) = std::env::var("MATRIX_HOMESERVER_URL") {
            if !v.trim().is_empty() {
                self.channels.matrix.homeserver_url = v;
            }
        }
        if let Ok(v) = std::env::var("OPENSHELL_MATRIX_POLL_INTERVAL_MS") {
            self.channels.matrix.poll_interval_ms =
                parse_env_u64("OPENSHELL_MATRIX_POLL_INTERVAL_MS", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_MATRIX_ROOM_IDS") {
            if !v.trim().is_empty() {
                self.channels.matrix.room_ids = v
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
        if let Ok(v) = std::env::var("OPENSHELL_MATRIX_START_FROM_LATEST") {
            self.channels.matrix.start_from_latest =
                parse_env_bool("OPENSHELL_MATRIX_START_FROM_LATEST", &v)?;
        }
        if let Ok(v) = std::env::var("MATRIX_USER_ID") {
            if !v.trim().is_empty() {
                self.channels.matrix.user_id = v;
            }
        }
        if let Ok(v) = std::env::var("MATRIX_ACCESS_TOKEN") {
            if !v.trim().is_empty() {
                self.channels.matrix.access_token = v;
                self.channels.matrix.enabled = true;
            }
        }
        if let Ok(v) = std::env::var("SIGNAL_API_BASE_URL") {
            if !v.trim().is_empty() {
                self.channels.signal.api_base_url = v;
            }
        }
        if let Ok(v) = std::env::var("OPENSHELL_SIGNAL_POLL_INTERVAL_MS") {
            self.channels.signal.poll_interval_ms =
                parse_env_u64("OPENSHELL_SIGNAL_POLL_INTERVAL_MS", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_SIGNAL_START_FROM_LATEST") {
            self.channels.signal.start_from_latest =
                parse_env_bool("OPENSHELL_SIGNAL_START_FROM_LATEST", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_SIGNAL_RECEIVE_TIMEOUT_SECONDS") {
            self.channels.signal.receive_timeout_seconds =
                parse_env_u64("OPENSHELL_SIGNAL_RECEIVE_TIMEOUT_SECONDS", &v)?;
        }
        if let Ok(v) = std::env::var("SIGNAL_ACCOUNT") {
            if !v.trim().is_empty() {
                self.channels.signal.account = v;
                self.channels.signal.enabled = true;
            }
        }
        if let Ok(v) = std::env::var("SIGNAL_API_TOKEN") {
            self.channels.signal.api_token = if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = std::env::var("WHATSAPP_ACCESS_TOKEN") {
            if !v.trim().is_empty() {
                self.channels.whatsapp.access_token = v;
                self.channels.whatsapp.enabled = true;
            }
        }
        if let Ok(v) = std::env::var("WHATSAPP_PHONE_NUMBER_ID") {
            if !v.trim().is_empty() {
                self.channels.whatsapp.phone_number_id = v;
            }
        }
        if let Ok(v) = std::env::var("WHATSAPP_WEBHOOK_VERIFY_TOKEN") {
            if !v.trim().is_empty() {
                self.channels.whatsapp.webhook_verify_token = v;
            }
        }
        if let Ok(v) = std::env::var("WHATSAPP_APP_SECRET") {
            self.channels.whatsapp.app_secret = if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = std::env::var("IMESSAGE_SOURCE_DB") {
            if !v.trim().is_empty() {
                self.channels.imessage.source_db = Some(v);
                self.channels.imessage.enabled = true;
            }
        }
        if let Ok(v) = std::env::var("OPENSHELL_EMAIL_PROVIDER") {
            if !v.trim().is_empty() {
                self.channels.email.provider = v;
            }
        }
        if let Ok(v) = std::env::var("OPENSHELL_EMAIL_QUERY") {
            if !v.trim().is_empty() {
                self.channels.email.query = v;
            }
        }
        if let Ok(v) = std::env::var("OPENSHELL_EMAIL_POLL_INTERVAL_MS") {
            self.channels.email.poll_interval_ms =
                parse_env_u64("OPENSHELL_EMAIL_POLL_INTERVAL_MS", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_EMAIL_START_FROM_LATEST") {
            self.channels.email.start_from_latest =
                parse_env_bool("OPENSHELL_EMAIL_START_FROM_LATEST", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_EMAIL_MARK_PROCESSED_AS_READ") {
            self.channels.email.mark_processed_as_read =
                parse_env_bool("OPENSHELL_EMAIL_MARK_PROCESSED_AS_READ", &v)?;
        }
        if let Ok(v) = std::env::var("GMAIL_ACCESS_TOKEN") {
            if !v.trim().is_empty() {
                self.channels.email.gmail_access_token = v;
                self.channels.email.enabled = true;
            }
        }
        if let Ok(v) = std::env::var("OPENSHELL_LINEAR_POLL_INTERVAL_MS") {
            self.channels.linear.poll_interval_ms =
                parse_env_u64("OPENSHELL_LINEAR_POLL_INTERVAL_MS", &v)?;
        }
        if let Ok(v) = std::env::var("OPENSHELL_LINEAR_TEAM_IDS") {
            if !v.trim().is_empty() {
                self.channels.linear.team_ids = v
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
        if let Ok(v) = std::env::var("OPENSHELL_LINEAR_START_FROM_LATEST") {
            self.channels.linear.start_from_latest =
                parse_env_bool("OPENSHELL_LINEAR_START_FROM_LATEST", &v)?;
        }
        if let Ok(v) = std::env::var("LINEAR_API_KEY") {
            if !v.trim().is_empty() {
                self.channels.linear.api_key = v;
                self.channels.linear.enabled = true;
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.general.model.trim().is_empty() {
            return Err(anyhow::anyhow!("general.model is required"));
        }
        if self.general.failover_cooldown_base_seconds == 0 {
            return Err(anyhow::anyhow!(
                "general.failover_cooldown_base_seconds must be > 0"
            ));
        }
        if self.general.failover_cooldown_max_seconds == 0 {
            return Err(anyhow::anyhow!(
                "general.failover_cooldown_max_seconds must be > 0"
            ));
        }
        if self.general.failover_cooldown_base_seconds > self.general.failover_cooldown_max_seconds
        {
            return Err(anyhow::anyhow!(
                "general.failover_cooldown_base_seconds must be <= general.failover_cooldown_max_seconds"
            ));
        }
        if self
            .general
            .fallback_models
            .iter()
            .any(|model| model.trim().is_empty())
        {
            return Err(anyhow::anyhow!(
                "general.fallback_models cannot contain empty values"
            ));
        }
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
        if self.channels.telegram.enabled && self.channels.telegram.bot_token.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "channels.telegram.bot_token is required when channels.telegram.enabled=true"
            ));
        }
        if self.channels.discord.enabled && self.channels.discord.bot_token.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "channels.discord.bot_token is required when channels.discord.enabled=true"
            ));
        }
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
        if self.channels.imessage.enabled && self.channels.imessage.poll_interval_ms == 0 {
            return Err(anyhow::anyhow!(
                "channels.imessage.poll_interval_ms must be > 0"
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
            if self.channels.email.query.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "channels.email.query must not be empty when channels.email.enabled=true"
                ));
            }
        }
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
        let _ = self.api_keys_for_model_name(&self.general.model)?;
        for fallback_model in &self.general.fallback_models {
            let _ = self.api_keys_for_model_name(fallback_model)?;
        }
        Ok(())
    }

    pub fn api_key_for_model(&self) -> anyhow::Result<String> {
        self.api_keys_for_model_name(&self.general.model)?
            .into_iter()
            .next()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no usable API keys found for general.model={:?}",
                    self.general.model
                )
            })
    }

    pub fn api_keys_for_model_name(&self, model_name: &str) -> anyhow::Result<Vec<String>> {
        let provider = detect_provider_for_model(model_name)?;
        let mut keys = Vec::new();
        match provider {
            ModelProvider::Anthropic => {
                push_unique_trimmed(&mut keys, self.keys.anthropic_api_key.as_deref());
                for key in &self.keys.anthropic_api_keys {
                    push_unique_trimmed(&mut keys, Some(key.as_str()));
                }
            }
            ModelProvider::OpenAI => {
                push_unique_trimmed(&mut keys, self.keys.openai_api_key.as_deref());
                for key in &self.keys.openai_api_keys {
                    push_unique_trimmed(&mut keys, Some(key.as_str()));
                }
            }
        }
        if keys.is_empty() {
            let provider_name = match provider {
                ModelProvider::Anthropic => "Anthropic",
                ModelProvider::OpenAI => "OpenAI",
            };
            return Err(anyhow::anyhow!(
                "at least one {provider_name} API key is required for model {model_name:?}"
            ));
        }
        Ok(keys)
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelProvider {
    OpenAI,
    Anthropic,
}

fn detect_provider_for_model(model_name: &str) -> anyhow::Result<ModelProvider> {
    let model = model_name.trim().to_ascii_lowercase();
    if model.starts_with("claude-") {
        return Ok(ModelProvider::Anthropic);
    }
    if model.starts_with("gpt-")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
    {
        return Ok(ModelProvider::OpenAI);
    }
    Err(anyhow::anyhow!(
        "unsupported model {model_name:?}: provider cannot be inferred"
    ))
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

fn parse_env_usize(name: &str, raw: &str) -> anyhow::Result<usize> {
    raw.trim()
        .parse::<usize>()
        .map_err(|e| anyhow::anyhow!("invalid {name}={raw:?}: {e}"))
}

fn parse_env_u64(name: &str, raw: &str) -> anyhow::Result<u64> {
    raw.trim()
        .parse::<u64>()
        .map_err(|e| anyhow::anyhow!("invalid {name}={raw:?}: {e}"))
}

fn parse_env_bool(name: &str, raw: &str) -> anyhow::Result<bool> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(anyhow::anyhow!(
            "invalid {name}={raw:?}: expected boolean (true/false)"
        )),
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
        BindMode, ControlApiKeyConfig, DiscoveryMode, ExternalChannelPluginConfig, OpenShellConfig,
        RuntimeExposure, ToolProfile, ToolsConfig, canonical_tool_name,
    };

    fn base_config() -> OpenShellConfig {
        toml::from_str(
            r#"
[general]
model = "gpt-4o-mini"
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
