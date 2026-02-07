use crate::error::{Result, ToolError};
use crate::traits::{optional_string, require_string, Tool, ToolSpec};
use async_trait::async_trait;
use horizons_core::core_agents::models::RiskLevel;
use os_channels::{ChannelAdapter, ImessageAdapter, OutboundMessage};
use rusqlite::{params, Connection, OpenFlags};
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

#[derive(Clone)]
pub struct ImessageTool {
    source_db: PathBuf,
}

impl ImessageTool {
    pub fn new(source_db: impl AsRef<Path>) -> Result<Self> {
        let source_db = source_db.as_ref().to_path_buf();
        if source_db.as_os_str().is_empty() {
            return Err(ToolError::InvalidArguments(
                "imessage source_db is required".to_string(),
            ));
        }
        if !source_db.exists() {
            return Err(ToolError::InvalidArguments(format!(
                "imessage source_db does not exist: {}",
                source_db.display()
            )));
        }
        Ok(Self { source_db })
    }

    async fn list_recent(
        &self,
        limit: usize,
        chat_guid: Option<String>,
        handle_id: Option<String>,
    ) -> Result<Vec<ImessageMessage>> {
        let source_db = self.source_db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = open_chat_db_readonly(&source_db)?;
            let mut stmt = conn
                .prepare_cached(
                    r#"
SELECT
  m.ROWID,
  m.guid,
  m.text,
  m.is_from_me,
  h.id AS handle_id,
  c.guid AS chat_guid,
  c.display_name AS chat_display_name
FROM message m
LEFT JOIN handle h ON h.ROWID = m.handle_id
LEFT JOIN chat_message_join cmj ON cmj.message_id = m.ROWID
LEFT JOIN chat c ON c.ROWID = cmj.chat_id
WHERE (?1 IS NULL OR c.guid = ?1)
  AND (?2 IS NULL OR h.id = ?2)
ORDER BY m.ROWID DESC
LIMIT ?3
"#,
                )
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

            let rows = stmt
                .query_map(
                    params![chat_guid.as_deref(), handle_id.as_deref(), limit as i64],
                    |row| {
                        Ok(ImessageMessage {
                            rowid: row.get(0)?,
                            guid: row.get(1)?,
                            text: row.get(2)?,
                            is_from_me: row.get::<_, i64>(3)? != 0,
                            handle_id: row.get(4)?,
                            chat_guid: row.get(5)?,
                            chat_display_name: row.get(6)?,
                        })
                    },
                )
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(|e| ToolError::ExecutionFailed(e.to_string()))?);
            }
            Ok(out)
        })
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("imessage read task join error: {e}")))?
    }

    async fn send_message(&self, recipient: &str, content: &str) -> Result<serde_json::Value> {
        let recipient = recipient.trim();
        if recipient.is_empty() {
            return Err(ToolError::InvalidArguments(
                "recipient must not be empty".to_string(),
            ));
        }
        let content = content.trim();
        if content.is_empty() {
            return Err(ToolError::InvalidArguments(
                "content must not be empty".to_string(),
            ));
        }

        let adapter = ImessageAdapter::new(self.source_db.clone());
        adapter
            .send(
                recipient,
                OutboundMessage {
                    content: content.to_string(),
                    reply_to_message_id: None,
                    attachments: vec![],
                },
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(json!({
            "status": "sent",
            "recipient": recipient,
        }))
    }
}

#[async_trait]
impl Tool for ImessageTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "imessage".to_string(),
            description: "Read recent iMessage/SMS messages and send iMessage replies.".to_string(),
            parameters_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "action": { "type": "string", "enum": ["list_recent", "send"] },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
                    "chat_guid": { "type": "string" },
                    "handle_id": { "type": "string" },
                    "recipient": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["action"]
            }),
            risk_level: RiskLevel::High,
        }
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn execute(&self, arguments: serde_json::Value) -> Result<serde_json::Value> {
        let action = require_string(&arguments, "action")?;
        match action.as_str() {
            "list_recent" => {
                let limit = parse_limit(&arguments)?;
                let chat_guid = optional_string(&arguments, "chat_guid")?;
                let handle_id = optional_string(&arguments, "handle_id")?;
                let messages = self.list_recent(limit, chat_guid, handle_id).await?;
                Ok(json!({
                    "count": messages.len(),
                    "messages": messages
                }))
            }
            "send" => {
                let recipient = require_string(&arguments, "recipient")?;
                let content = require_string(&arguments, "content")?;
                self.send_message(&recipient, &content).await
            }
            other => Err(ToolError::InvalidArguments(format!(
                "unknown action: {other}"
            ))),
        }
    }
}

fn open_chat_db_readonly(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| ToolError::ExecutionFailed(format!("open chat db {}: {e}", path.display())))?;
    conn.busy_timeout(Duration::from_millis(1000))
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    Ok(conn)
}

fn parse_limit(arguments: &serde_json::Value) -> Result<usize> {
    match arguments.get("limit") {
        None => Ok(DEFAULT_LIMIT),
        Some(v) => {
            let n = v.as_u64().ok_or_else(|| {
                ToolError::InvalidArguments("limit must be an integer".to_string())
            })?;
            let n = usize::try_from(n)
                .map_err(|_| ToolError::InvalidArguments("limit is out of range".to_string()))?;
            if !(1..=MAX_LIMIT).contains(&n) {
                return Err(ToolError::InvalidArguments(format!(
                    "limit must be between 1 and {MAX_LIMIT}"
                )));
            }
            Ok(n)
        }
    }
}

#[derive(Debug, Serialize)]
struct ImessageMessage {
    rowid: i64,
    guid: String,
    text: Option<String>,
    is_from_me: bool,
    handle_id: Option<String>,
    chat_guid: Option<String>,
    chat_display_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::parse_limit;

    #[test]
    fn parse_limit_defaults_and_bounds() {
        assert_eq!(parse_limit(&serde_json::json!({})).unwrap(), 20);
        assert!(parse_limit(&serde_json::json!({"limit": 0})).is_err());
        assert!(parse_limit(&serde_json::json!({"limit": 101})).is_err());
    }
}
