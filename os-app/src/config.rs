//! OpenShell configuration loader.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct OpenShellConfig {
    pub general: GeneralConfig,
    #[serde(default)]
    pub keys: KeysConfig,
    pub channels: ChannelsConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub optimization: OptimizationConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeneralConfig {
    pub model: String,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct KeysConfig {
    pub openai_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelsConfig {
    pub webchat: WebChatConfig,
    #[serde(default)]
    pub telegram: TelegramConfig,
    #[serde(default)]
    pub discord: DiscordConfig,
    #[serde(default)]
    pub imessage: ImessageConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebChatConfig {
    pub enabled: bool,
    pub port: u16,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DiscordConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
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

#[derive(Debug, Clone, Default, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalMode {
    Human,
    Ai,
    Auto,
}

#[derive(Debug, Clone, Deserialize)]
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
    /// Default is false for safety: external channels (iMessage/Telegram/Discord) require an
    /// explicit allowlist in `security.allowed_users`.
    #[serde(default)]
    pub allow_all_senders: bool,
}

fn default_shell_approval() -> ApprovalMode {
    ApprovalMode::Human
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

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MemoryConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
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
        let path = path.unwrap_or_else(default_config_path);
        let contents = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| anyhow::anyhow!("read config {}: {e}", path.display()))?;

        let mut cfg: OpenShellConfig = toml::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("parse config {}: {e}", path.display()))?;

        cfg.apply_env_overrides();
        cfg.validate()?;
        Ok(cfg)
    }

    fn apply_env_overrides(&mut self) {
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
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.general.model.trim().is_empty() {
            return Err(anyhow::anyhow!("general.model is required"));
        }
        if self.channels.webchat.enabled && self.channels.webchat.port == 0 {
            return Err(anyhow::anyhow!("channels.webchat.port must be > 0"));
        }
        if self.channels.imessage.enabled && self.channels.imessage.poll_interval_ms == 0 {
            return Err(anyhow::anyhow!(
                "channels.imessage.poll_interval_ms must be > 0"
            ));
        }
        Ok(())
    }

    pub fn api_key_for_model(&self) -> Option<String> {
        let model = self.general.model.to_ascii_lowercase();
        if model.starts_with("claude-") {
            return self
                .keys
                .anthropic_api_key
                .clone()
                .filter(|s| !s.is_empty());
        }
        self.keys.openai_api_key.clone().filter(|s| !s.is_empty())
    }
}

pub fn default_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Path::new(&home).join(".opencraw").join("config.toml")
}

pub fn default_data_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Path::new(&home).join(".opencraw").join("data")
}
