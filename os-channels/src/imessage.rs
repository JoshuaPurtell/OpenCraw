use crate::traits::ChannelAdapter;
use crate::types::{InboundMessage, InboundMessageKind, OutboundMessage};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OpenFlags};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// iMessage adapter backed by the local macOS Messages database (`chat.db`) for reads,
/// and AppleScript (`osascript`) for sends.
///
/// Permissions required on macOS:
/// - Full Disk Access for the terminal running OpenShell (to read `~/Library/Messages/chat.db`).
/// - Automation permission to control the Messages app (for `osascript` sends).
#[derive(Clone)]
pub struct ImessageAdapter {
    source_db: PathBuf,
    poll_interval: Duration,
    start_from_latest: bool,
    max_per_poll: usize,
    group_prefixes: Vec<String>,
}

impl ImessageAdapter {
    pub fn new(source_db: impl AsRef<Path>) -> Self {
        Self {
            source_db: source_db.as_ref().to_path_buf(),
            poll_interval: Duration::from_millis(1500),
            start_from_latest: true,
            max_per_poll: 200,
            // Avoid replying to every group message by default; require an explicit prefix.
            group_prefixes: vec!["@openshell".to_string(), "openshell".to_string()],
        }
    }

    pub fn default_source_db() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        Path::new(&home)
            .join("Library")
            .join("Messages")
            .join("chat.db")
    }

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    pub fn with_start_from_latest(mut self, start_from_latest: bool) -> Self {
        self.start_from_latest = start_from_latest;
        self
    }

    pub fn with_max_per_poll(mut self, max_per_poll: usize) -> Self {
        self.max_per_poll = max_per_poll.max(1);
        self
    }

    pub fn with_group_prefixes(mut self, prefixes: Vec<String>) -> Self {
        self.group_prefixes = prefixes
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        self
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for ImessageAdapter {
    fn channel_id(&self) -> &str {
        "imessage"
    }

    async fn start(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let adapter = self.clone();
        tokio::spawn(async move {
            if let Err(e) = adapter.poll_loop(tx).await {
                tracing::error!(%e, "imessage poll loop exited");
            }
        });
        Ok(())
    }

    async fn send(&self, recipient_id: &str, message: OutboundMessage) -> Result<()> {
        let handle = recipient_id.trim();
        if handle.is_empty() {
            return Err(anyhow!("recipient_id is required"));
        }
        let body = message.content.trim().to_string();
        if body.is_empty() {
            return Err(anyhow!("message content is empty"));
        }

        // NOTE: OpenShell's `OutboundMessage.attachments` are URLs; this adapter doesn't
        // support sending file attachments in v0.1.0.
        let parsed = parse_imessage_handle(handle);
        let script = build_send_script(&parsed, &body);

        tokio::task::spawn_blocking(move || run_osascript(&script)).await??;
        Ok(())
    }

    fn supports_reactions(&self) -> bool {
        true
    }
}

impl ImessageAdapter {
    #[tracing::instrument(level = "info", skip_all)]
    async fn poll_loop(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let mut last_rowid: Option<i64> = None;
        let mut failed_attempts: usize = 0;

        loop {
            match self.poll_once(&tx, &mut last_rowid).await {
                Ok(()) => failed_attempts = 0,
                Err(e) => {
                    failed_attempts += 1;
                    let backoff = Duration::from_millis((failed_attempts.min(20) as u64) * 250);
                    tracing::warn!(
                        %e,
                        failed_attempts,
                        "imessage poll failed (grant Full Disk Access to read chat.db?)"
                    );
                    tokio::time::sleep(backoff).await;
                }
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }

    async fn poll_once(
        &self,
        tx: &mpsc::Sender<InboundMessage>,
        last_rowid: &mut Option<i64>,
    ) -> Result<()> {
        let source_db = self.source_db.clone();
        let start_from_latest = self.start_from_latest;
        let starting_empty = last_rowid.is_none();
        let last_seen = last_rowid.unwrap_or(0);
        let max_per_poll = self.max_per_poll;
        let group_prefixes = self.group_prefixes.clone();

        let poll = tokio::task::spawn_blocking(move || {
            let conn = open_chat_db_readonly(&source_db)?;
            let mut last = last_seen;
            if starting_empty && start_from_latest {
                last = current_max_rowid(&conn)?;
            }

            let mut out = Vec::new();
            let mut stmt = conn.prepare_cached(
                r#"
SELECT
  m.ROWID,
  m.guid,
  m.text,
  m.is_from_me,
  h.id AS handle_id,
  h.service AS handle_service,
  c.guid AS chat_guid,
  c.display_name AS chat_display_name,
  c.service_name AS chat_service_name
FROM message m
LEFT JOIN handle h ON h.ROWID = m.handle_id
LEFT JOIN chat_message_join cmj ON cmj.message_id = m.ROWID
LEFT JOIN chat c ON c.ROWID = cmj.chat_id
WHERE m.ROWID > ?1
ORDER BY m.ROWID ASC
LIMIT ?2
"#,
            )?;

            let iter = stmt.query_map(params![last, max_per_poll as i64], |row| {
                Ok(RawMessage {
                    rowid: row.get(0)?,
                    guid: row.get(1)?,
                    text: row.get::<_, Option<String>>(2)?,
                    is_from_me: row.get::<_, i64>(3)?,
                    handle_id: row.get::<_, Option<String>>(4)?,
                    handle_service: row.get::<_, Option<String>>(5)?,
                    chat_guid: row.get::<_, Option<String>>(6)?,
                    chat_display_name: row.get::<_, Option<String>>(7)?,
                    chat_service_name: row.get::<_, Option<String>>(8)?,
                })
            })?;

            for item in iter {
                out.push(item?);
            }
            Ok::<_, anyhow::Error>(PollResult {
                start_rowid: last,
                rows: out,
            })
        })
        .await??;
        let mut rows = poll.rows;

        // Update last_rowid to the newest row we've seen, even if we end up filtering.
        if let Some(max) = rows.iter().map(|r| r.rowid).max() {
            *last_rowid = Some(max);
        } else if starting_empty && start_from_latest {
            // No rows returned and we started from latest; record the starting point.
            *last_rowid = Some(poll.start_rowid);
        }

        // Emit inbound messages.
        for raw in rows.drain(..) {
            if raw.is_from_me != 0 {
                continue;
            }

            let Some(sender_id) = raw.handle_id.clone().filter(|s| !s.trim().is_empty()) else {
                continue;
            };

            let text = raw.text.unwrap_or_default();
            let mut content = text.trim().to_string();
            if content.is_empty() {
                continue;
            }

            let thread_id = raw.chat_guid.clone();
            let is_group = thread_id
                .as_deref()
                .map(is_chat_handle)
                .unwrap_or(false);

            if is_group && !group_prefixes.is_empty() {
                if let Some(stripped) = strip_any_prefix(&content, &group_prefixes) {
                    content = stripped;
                } else {
                    continue;
                }
            }

            let meta = serde_json::json!({
                "handle_id": raw.handle_id,
                "handle_service": raw.handle_service,
                "chat_guid": raw.chat_guid,
                "chat_display_name": raw.chat_display_name,
                "chat_service_name": raw.chat_service_name,
            });

            let inbound = InboundMessage {
                kind: InboundMessageKind::Message,
                message_id: raw.guid,
                channel_id: "imessage".to_string(),
                sender_id,
                thread_id,
                is_group,
                content,
                metadata: meta,
                received_at: Utc::now(),
            };

            // If the receiver is gone, just stop sending.
            if tx.send(inbound).await.is_err() {
                return Ok(());
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct RawMessage {
    rowid: i64,
    guid: String,
    text: Option<String>,
    is_from_me: i64,
    handle_id: Option<String>,
    handle_service: Option<String>,
    chat_guid: Option<String>,
    chat_display_name: Option<String>,
    chat_service_name: Option<String>,
}

#[derive(Debug)]
struct PollResult {
    start_rowid: i64,
    rows: Vec<RawMessage>,
}

fn open_chat_db_readonly(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
    .with_context(|| format!("open chat db: {}", path.display()))?;
    conn.busy_timeout(Duration::from_millis(1000))
        .context("set sqlite busy timeout")?;
    Ok(conn)
}

fn current_max_rowid(conn: &Connection) -> Result<i64> {
    let v = conn.query_row("SELECT IFNULL(MAX(ROWID), 0) FROM message", [], |row| row.get(0))?;
    Ok(v)
}

fn strip_any_prefix(input: &str, prefixes: &[String]) -> Option<String> {
    let trimmed = input.trim_start();
    for p in prefixes {
        if p.trim().is_empty() {
            continue;
        }
        let ptrim = p.trim();
        if trimmed.len() < ptrim.len() {
            continue;
        }
        if trimmed[..ptrim.len()].eq_ignore_ascii_case(ptrim) {
            let rest = trimmed[ptrim.len()..].trim_start();
            // Common separators after a "mention".
            let rest = rest.strip_prefix(':').unwrap_or(rest).trim_start();
            let rest = rest.strip_prefix(',').unwrap_or(rest).trim_start();
            return Some(rest.to_string());
        }
    }
    None
}

fn escape_applescript(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn is_chat_handle(handle: &str) -> bool {
    handle.contains(";chat") || handle.starts_with("chat")
}

#[derive(Debug, Clone, Deserialize)]
struct ParsedImessageHandle {
    service: Option<String>,
    address: String,
    chat_id: String,
    is_chat: bool,
}

fn parse_imessage_handle(handle: &str) -> ParsedImessageHandle {
    let mut service = None;
    let mut address = handle.to_string();
    let mut is_chat = handle.starts_with("chat");
    let mut chat_id = handle.to_string();

    let parts: Vec<&str> = handle.splitn(3, ';').collect();
    if parts.len() == 3 {
        let service_raw = parts[0];
        let candidate = parts[2];
        if matches!(service_raw, "iMessage" | "SMS") {
            service = Some(service_raw.to_string());
            address = candidate.to_string();
            if candidate.starts_with("chat") {
                is_chat = true;
                chat_id = handle.to_string();
            }
        }
    }

    if is_chat {
        ParsedImessageHandle {
            service,
            address,
            chat_id,
            is_chat,
        }
    } else {
        ParsedImessageHandle {
            service,
            address,
            chat_id: String::new(),
            is_chat,
        }
    }
}

fn build_send_script(target: &ParsedImessageHandle, body: &str) -> String {
    let body = escape_applescript(body);
    if target.is_chat {
        format!(
            r#"tell application "Messages"
    set targetChat to chat id "{chat_id}"
    send "{body}" to targetChat
end tell"#,
            chat_id = escape_applescript(&target.chat_id),
            body = body
        )
    } else {
        let service_type = match target.service.as_deref() {
            Some("SMS") => "SMS",
            _ => "iMessage",
        };
        format!(
            r#"tell application "Messages"
    set targetService to first service whose service type is {service_type}
    set targetBuddy to buddy "{address}" of targetService
    send "{body}" to targetBuddy
end tell"#,
            service_type = service_type,
            address = escape_applescript(&target.address),
            body = body
        )
    }
}

fn run_osascript(script: &str) -> Result<()> {
    use std::process::Command;

    let mut child = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .spawn()
        .context("spawn osascript")?;

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().context("wait on osascript")? {
            if !status.success() {
                return Err(anyhow!("osascript failed"));
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            return Err(anyhow!("osascript timed out"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_prefixes() {
        let prefixes = vec!["@openshell".to_string(), "openshell".to_string()];
        assert_eq!(
            strip_any_prefix("@openshell: hi", &prefixes).as_deref(),
            Some("hi")
        );
        assert_eq!(
            strip_any_prefix("OpenShell, hi", &prefixes).as_deref(),
            Some("hi")
        );
        assert!(strip_any_prefix("hi", &prefixes).is_none());
    }

    #[test]
    fn parse_chat_handle() {
        let p = parse_imessage_handle("iMessage;+;chat123");
        assert!(p.is_chat);
        assert_eq!(p.chat_id, "iMessage;+;chat123");
    }

    #[test]
    fn parse_buddy_handle() {
        let p = parse_imessage_handle("iMessage;-;+14155551212");
        assert!(!p.is_chat);
        assert_eq!(p.address, "+14155551212");
        assert_eq!(p.service.as_deref(), Some("iMessage"));
    }
}
