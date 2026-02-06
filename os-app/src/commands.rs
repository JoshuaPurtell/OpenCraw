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
            "model={}\nchannels={}\nuptime_seconds={}",
            cfg.general.model,
            active_channels.join(","),
            uptime.as_secs()
        )),
        _ => Some("Unknown command. Supported: /new /status /think /verbose /usage".to_string()),
    }
}
