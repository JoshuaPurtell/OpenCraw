//! Allowlist enforcement.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use crate::config::OpenShellConfig;

pub fn is_allowed(cfg: &OpenShellConfig, channel_id: &str, sender_id: &str) -> bool {
    // WebChat is a local/dev channel; allow by default.
    if channel_id == "webchat" {
        return true;
    }

    // For external channels (iMessage/Telegram/Discord), require explicit allowlisting by
    // default to avoid accidental data exfiltration and unintended auto-replies.
    if cfg.security.allowed_users.is_empty() {
        return cfg.security.allow_all_senders;
    }

    let composite = format!("{channel_id}:{sender_id}");
    cfg.security
        .allowed_users
        .iter()
        .any(|u| u == sender_id || u == &composite)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ApprovalMode, AutomationConfig, ChannelsConfig, ContextConfig, DiscordConfig, EmailConfig,
        GeneralConfig, ImessageConfig, KeysConfig, LinearConfig, MatrixConfig, MemoryConfig,
        OpenShellConfig, OptimizationConfig, QueueConfig, RuntimeConfig, SecurityConfig,
        SignalConfig, SkillsConfig, SlackConfig, TelegramConfig, ToolsConfig, WebChatConfig,
        WhatsAppConfig,
    };

    fn base_cfg() -> OpenShellConfig {
        OpenShellConfig {
            general: GeneralConfig {
                model: "gpt-4o-mini".to_string(),
                fallback_models: Vec::new(),
                failover_cooldown_base_seconds: 5,
                failover_cooldown_max_seconds: 300,
                system_prompt: "x".to_string(),
            },
            keys: KeysConfig::default(),
            channels: ChannelsConfig {
                webchat: WebChatConfig {
                    enabled: true,
                    port: 3000,
                },
                telegram: TelegramConfig::default(),
                discord: DiscordConfig::default(),
                slack: SlackConfig::default(),
                matrix: MatrixConfig::default(),
                signal: SignalConfig::default(),
                whatsapp: WhatsAppConfig::default(),
                imessage: ImessageConfig::default(),
                email: EmailConfig::default(),
                linear: LinearConfig::default(),
                external_plugins: Vec::new(),
            },
            tools: ToolsConfig::default(),
            security: SecurityConfig {
                shell_approval: ApprovalMode::Human,
                browser_approval: ApprovalMode::Ai,
                filesystem_write_approval: ApprovalMode::Ai,
                allowed_users: vec![],
                allow_all_senders: false,
                control_api_key: None,
                control_api_keys: vec![],
                mutating_auth_exempt_prefixes: vec![
                    "/api/v1/os/automation/webhook/".to_string(),
                    "/api/v1/os/automation/poll/".to_string(),
                ],
            },
            runtime: RuntimeConfig::default(),
            queue: QueueConfig::default(),
            context: ContextConfig::default(),
            memory: MemoryConfig::default(),
            optimization: OptimizationConfig::default(),
            automation: AutomationConfig::default(),
            skills: SkillsConfig::default(),
        }
    }

    #[test]
    fn webchat_is_allowed_by_default() {
        let cfg = base_cfg();
        assert!(is_allowed(&cfg, "webchat", "any"));
    }

    #[test]
    fn external_channels_denied_by_default() {
        let cfg = base_cfg();
        assert!(!is_allowed(&cfg, "imessage", "+14155551212"));
        assert!(!is_allowed(&cfg, "telegram", "123"));
        assert!(!is_allowed(&cfg, "discord", "456"));
    }

    #[test]
    fn allow_all_senders_allows_external_channels_when_allowlist_empty() {
        let mut cfg = base_cfg();
        cfg.security.allow_all_senders = true;
        assert!(is_allowed(&cfg, "imessage", "+14155551212"));
    }

    #[test]
    fn allowlist_matches_raw_sender_or_composite() {
        let mut cfg = base_cfg();
        cfg.security.allowed_users = vec!["+14155551212".to_string()];
        assert!(is_allowed(&cfg, "imessage", "+14155551212"));

        let mut cfg = base_cfg();
        cfg.security.allowed_users = vec!["imessage:+14155551212".to_string()];
        assert!(is_allowed(&cfg, "imessage", "+14155551212"));
    }
}
