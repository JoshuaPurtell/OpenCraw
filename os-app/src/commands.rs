//! Chat command parser for OpenShell.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use crate::config::OpenShellConfig;
use crate::session::Session;
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

    if let Some(model_response) = handle_model_command(cfg, session, trimmed) {
        return Some(model_response);
    }

    match trimmed {
        "/new" => {
            session.reset();
            Some("Session reset.".to_string())
        }
        "/think" => {
            session.show_thinking = !session.show_thinking;
            Some(format!("show_thinking = {}", session.show_thinking))
        }
        "/verbose" => {
            session.show_tool_calls = !session.show_tool_calls;
            Some(format!("show_tool_calls = {}", session.show_tool_calls))
        }
        "/usage" => Some(format!(
            "prompt_tokens={} completion_tokens={}",
            session.usage_totals.prompt_tokens, session.usage_totals.completion_tokens
        )),
        "/status" => Some(format!(
            "model={}\ndefault_model={}\nchannels={}\nuptime_seconds={}",
            session
                .model_override
                .as_deref()
                .unwrap_or(cfg.general.model.as_str()),
            cfg.general.model,
            active_channels.join(","),
            uptime.as_secs()
        )),
        _ => Some(
            "Unknown command. Supported: /new /status /think /verbose /usage /model".to_string(),
        ),
    }
}

fn handle_model_command(
    cfg: &OpenShellConfig,
    session: &mut Session,
    trimmed_command: &str,
) -> Option<String> {
    if !trimmed_command.starts_with("/model") {
        return None;
    }

    let parts: Vec<&str> = trimmed_command.split_whitespace().collect();
    let available = available_models(cfg);
    if parts.len() == 1 {
        let active = session
            .model_override
            .as_deref()
            .unwrap_or(cfg.general.model.as_str());
        return Some(format!(
            "active_model={active}\ndefault_model={}\navailable_models={}",
            cfg.general.model,
            available.join(",")
        ));
    }

    if parts.len() == 2
        && matches!(
            parts[1].to_ascii_lowercase().as_str(),
            "clear" | "reset" | "unset"
        )
    {
        session.model_override = None;
        return Some(format!(
            "model override cleared; using default model {}",
            cfg.general.model
        ));
    }

    if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("use") {
        let requested = parts[2..].join(" ");
        let normalized = requested.trim();
        if normalized.is_empty() {
            return Some("Usage: /model use <model_name>".to_string());
        }
        if let Some(canonical) = resolve_model_name(&available, normalized) {
            session.model_override = Some(canonical.to_string());
            return Some(format!("model override set to {}", canonical));
        }
        return Some(format!(
            "unknown model {:?}. available_models={}",
            normalized,
            available.join(",")
        ));
    }

    Some("Usage: /model | /model use <model_name> | /model clear".to_string())
}

fn available_models(cfg: &OpenShellConfig) -> Vec<String> {
    let mut models = Vec::new();
    models.push(cfg.general.model.clone());
    for model in &cfg.general.fallback_models {
        if !models.iter().any(|m| m.eq_ignore_ascii_case(model)) {
            models.push(model.clone());
        }
    }
    models
}

fn resolve_model_name<'a>(available: &'a [String], requested: &str) -> Option<&'a str> {
    available
        .iter()
        .find(|model| model.eq_ignore_ascii_case(requested))
        .map(String::as_str)
}
