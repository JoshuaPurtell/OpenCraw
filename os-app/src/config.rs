//! OpenShell configuration loader.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use serde::Deserialize;
use serde::Serialize;
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralConfig {
    pub model: String,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeMode {
    Dev,
    Prod,
}

fn default_runtime_mode() -> RuntimeMode {
    RuntimeMode::Dev
}

fn default_runtime_data_dir() -> String {
    "data".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    #[serde(default = "default_runtime_mode")]
    pub mode: RuntimeMode,
    #[serde(default = "default_runtime_data_dir")]
    pub data_dir: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            mode: default_runtime_mode(),
            data_dir: default_runtime_data_dir(),
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
    pub imessage: ImessageConfig,
    #[serde(default)]
    pub email: EmailConfig,
    #[serde(default)]
    pub linear: LinearConfig,
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
pub struct ToolsConfig {
    #[serde(default)]
    pub shell: bool,
    #[serde(default)]
    pub browser: bool,
    #[serde(default)]
    pub filesystem: bool,
    #[serde(default)]
    pub clipboard: bool,
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

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            shell_approval: default_shell_approval(),
            browser_approval: default_browser_approval(),
            filesystem_write_approval: default_filesystem_write_approval(),
            allowed_users: Vec::new(),
            allow_all_senders: false,
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
        if let Ok(v) = std::env::var("OPENSHELL_DATA_DIR") {
            if !v.trim().is_empty() {
                self.runtime.data_dir = v;
            }
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
        if let Ok(v) = std::env::var("OPENAI_API_KEY") {
            if !v.trim().is_empty() {
                self.keys.openai_api_key = Some(v);
            }
        }
        if let Ok(v) = std::env::var("ANTHROPIC_API_KEY") {
            if !v.trim().is_empty() {
                self.keys.anthropic_api_key = Some(v);
            }
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
        if self.runtime.data_dir.trim().is_empty() {
            return Err(anyhow::anyhow!("runtime.data_dir is required"));
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
            if self.channels.email.provider.trim().to_ascii_lowercase() != "gmail" {
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
        let _ = self.api_key_for_model()?;
        Ok(())
    }

    pub fn api_key_for_model(&self) -> anyhow::Result<String> {
        let model = self.general.model.to_ascii_lowercase();
        if model.starts_with("claude-") {
            return self
                .keys
                .anthropic_api_key
                .clone()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("keys.anthropic_api_key is required for claude models")
                });
        }
        if model.starts_with("gpt-")
            || model.starts_with("o1")
            || model.starts_with("o3")
            || model.starts_with("o4")
        {
            return self
                .keys
                .openai_api_key
                .clone()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("keys.openai_api_key is required for OpenAI models")
                });
        }
        Err(anyhow::anyhow!(
            "unsupported general.model={:?}: cannot determine provider",
            self.general.model
        ))
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
