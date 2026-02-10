//! Chat command parser for OpenShell.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use crate::config::OpenShellConfig;
use crate::session::Session;
use chrono::Utc;
use std::time::Duration;

pub fn handle_command(
    cfg: &OpenShellConfig,
    session: &mut Session,
    input: &str,
    uptime: Duration,
    active_channels: &[String],
) -> Option<String> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let command = parts.first().copied().unwrap_or("/");
    let args = &parts[1..];

    let response = if command.eq_ignore_ascii_case("/model") {
        Some(handle_model_command(cfg, session, args))
    } else if command.eq_ignore_ascii_case("/help") {
        ensure_no_args("/help", args).or_else(|| Some(help_text().to_string()))
    } else if command.eq_ignore_ascii_case("/new") {
        if let Some(usage) = ensure_no_args("/new", args) {
            Some(usage)
        } else {
            session.reset();
            Some("Session reset.".to_string())
        }
    } else if command.eq_ignore_ascii_case("/think") {
        if let Some(usage) = ensure_no_args("/think", args) {
            Some(usage)
        } else {
            session.show_thinking = !session.show_thinking;
            Some(format!("show_thinking = {}", session.show_thinking))
        }
    } else if command.eq_ignore_ascii_case("/verbose") {
        if let Some(usage) = ensure_no_args("/verbose", args) {
            Some(usage)
        } else {
            session.show_tool_calls = !session.show_tool_calls;
            Some(format!("show_tool_calls = {}", session.show_tool_calls))
        }
    } else if command.eq_ignore_ascii_case("/usage") {
        ensure_no_args("/usage", args).or_else(|| {
            Some(format!(
                "prompt_tokens={} completion_tokens={}",
                session.usage_totals.prompt_tokens, session.usage_totals.completion_tokens
            ))
        })
    } else if command.eq_ignore_ascii_case("/status") {
        if let Some(usage) = ensure_no_args("/status", args) {
            Some(usage)
        } else {
            let channels = if active_channels.is_empty() {
                "none".to_string()
            } else {
                active_channels.join(",")
            };
            Some(format!(
                "model={}\ndefault_model={}\nchannels={}\nuptime_seconds={}",
                session
                    .model_override
                    .as_deref()
                    .unwrap_or(cfg.general.model.as_str()),
                cfg.general.model,
                channels,
                uptime.as_secs()
            ))
        }
    } else {
        Some(format!("Unknown command {command:?}. {}", help_text()))
    };

    if response.is_some() {
        session.last_active = Utc::now();
    }

    response
}

fn handle_model_command(cfg: &OpenShellConfig, session: &mut Session, args: &[&str]) -> String {
    let available = available_models(cfg);
    match args {
        [] => model_summary(cfg, session, &available),
        [action] if is_clear_token(action) => {
            session.model_override = None;
            format!(
                "model override cleared; using default model {}",
                cfg.general.model
            )
        }
        [action] if action.eq_ignore_ascii_case("list") => model_summary(cfg, session, &available),
        [action] if action.eq_ignore_ascii_case("use") || action.eq_ignore_ascii_case("set") => {
            "Usage: /model use <model_name>".to_string()
        }
        [action] => set_model_override(session, &available, action),
        [action, rest @ ..]
            if action.eq_ignore_ascii_case("use") || action.eq_ignore_ascii_case("set") =>
        {
            set_model_override(session, &available, &rest.join(" "))
        }
        _ => "Usage: /model | /model use <model_name> | /model clear".to_string(),
    }
}

fn available_models(cfg: &OpenShellConfig) -> Vec<String> {
    let mut models = Vec::new();
    push_unique_model(&mut models, &cfg.general.model);
    for model in &cfg.general.fallback_models {
        push_unique_model(&mut models, model);
    }
    models
}

fn set_model_override(session: &mut Session, available: &[String], requested: &str) -> String {
    let normalized = requested.trim();
    if normalized.is_empty() {
        return "Usage: /model use <model_name>".to_string();
    }
    if let Some(canonical) = resolve_model_name(available, normalized) {
        session.model_override = Some(canonical.to_string());
        return format!("model override set to {}", canonical);
    }
    format!(
        "unknown model {:?}. available_models={}",
        normalized,
        available.join(",")
    )
}

fn resolve_model_name<'a>(available: &'a [String], requested: &str) -> Option<&'a str> {
    available
        .iter()
        .find(|model| model.eq_ignore_ascii_case(requested))
        .map(String::as_str)
}

fn model_summary(cfg: &OpenShellConfig, session: &Session, available: &[String]) -> String {
    let active = session
        .model_override
        .as_deref()
        .unwrap_or(cfg.general.model.as_str());
    format!(
        "active_model={active}\ndefault_model={}\navailable_models={}",
        cfg.general.model,
        available.join(",")
    )
}

fn push_unique_model(models: &mut Vec<String>, candidate: &str) {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return;
    }
    if !models
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(trimmed))
    {
        models.push(trimmed.to_string());
    }
}

fn is_clear_token(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "clear" | "reset" | "unset"
    )
}

fn ensure_no_args(usage: &str, args: &[&str]) -> Option<String> {
    if args.is_empty() {
        None
    } else {
        Some(format!("Usage: {usage}"))
    }
}

fn help_text() -> &'static str {
    "Supported: /help /new /status /think /verbose /usage /model"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OpenShellConfig;
    use chrono::{Duration as ChronoDuration, Utc};
    use os_llm::Usage;
    use uuid::Uuid;

    fn base_cfg() -> OpenShellConfig {
        serde_json::from_value(serde_json::json!({
            "general": {
                "model": "gpt-4o-mini",
                "fallback_models": ["gpt-4.1-mini", " GPT-4O-MINI ", "  "],
                "system_prompt": "You are OpenShell."
            },
            "channels": {
                "webchat": { "enabled": true, "port": 3000 }
            }
        }))
        .expect("parse base config")
    }

    fn new_session() -> Session {
        let now = Utc::now();
        Session {
            id: Uuid::new_v4(),
            history: Vec::new(),
            created_at: now,
            last_active: now,
            show_thinking: false,
            show_tool_calls: false,
            usage_totals: Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
            },
            last_assistant_message_id: None,
            last_user_message_id: None,
            model_override: None,
            model_pinning: Default::default(),
        }
    }

    #[test]
    fn returns_none_for_non_command_text() {
        let cfg = base_cfg();
        let mut session = new_session();
        let response = handle_command(
            &cfg,
            &mut session,
            "hello there",
            Duration::from_secs(7),
            &["webchat".to_string()],
        );
        assert!(response.is_none());
    }

    #[test]
    fn unknown_command_is_not_treated_as_model_command() {
        let cfg = base_cfg();
        let mut session = new_session();
        let response = handle_command(
            &cfg,
            &mut session,
            "/modelx",
            Duration::from_secs(1),
            &["webchat".to_string()],
        )
        .expect("unknown command response");
        assert!(response.contains("Unknown command"));
    }

    #[test]
    fn status_command_rejects_unexpected_args() {
        let cfg = base_cfg();
        let mut session = new_session();
        let response = handle_command(
            &cfg,
            &mut session,
            "/status extra",
            Duration::from_secs(10),
            &[],
        )
        .expect("status usage response");
        assert_eq!(response, "Usage: /status");
    }

    #[test]
    fn model_command_supports_direct_model_selection() {
        let cfg = base_cfg();
        let mut session = new_session();
        let response = handle_command(
            &cfg,
            &mut session,
            "/model GPT-4.1-MINI",
            Duration::from_secs(10),
            &["webchat".to_string()],
        )
        .expect("model set response");
        assert!(response.contains("model override set to gpt-4.1-mini"));
        assert_eq!(session.model_override.as_deref(), Some("gpt-4.1-mini"));
    }

    #[test]
    fn commands_update_last_active() {
        let cfg = base_cfg();
        let mut session = new_session();
        session.last_active = Utc::now() - ChronoDuration::minutes(5);
        let before = session.last_active;
        let _ = handle_command(
            &cfg,
            &mut session,
            "/usage",
            Duration::from_secs(10),
            &["webchat".to_string()],
        );
        assert!(session.last_active > before);
    }
}
