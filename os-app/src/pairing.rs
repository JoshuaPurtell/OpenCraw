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
        ApprovalMode, ChannelsConfig, DiscordConfig, GeneralConfig, ImessageConfig, KeysConfig,
        MemoryConfig, OpenShellConfig, OptimizationConfig, SecurityConfig, TelegramConfig,
        ToolsConfig, WebChatConfig,
    };

    fn base_cfg() -> OpenShellConfig {
        OpenShellConfig {
            general: GeneralConfig {
                model: "gpt-4o-mini".to_string(),
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
                imessage: ImessageConfig::default(),
            },
            tools: ToolsConfig::default(),
            security: SecurityConfig {
                shell_approval: ApprovalMode::Human,
                browser_approval: ApprovalMode::Ai,
                filesystem_write_approval: ApprovalMode::Ai,
                allowed_users: vec![],
                allow_all_senders: false,
            },
            memory: MemoryConfig::default(),
            optimization: OptimizationConfig::default(),
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
