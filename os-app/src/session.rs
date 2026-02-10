//! Session manager for (channel_id, sender_id) isolation.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use anyhow::Result;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use horizons_core::models::{OrgId, ProjectDbHandle};
use horizons_core::onboard::traits::{ProjectDb, ProjectDbParam, ProjectDbValue};
use os_channels::{ChannelId, SenderId};
use os_llm::{ChatMessage, Usage};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelPinningMode {
    #[default]
    Prefer,
    Strict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub history: Vec<ChatMessage>,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    #[serde(default)]
    pub show_thinking: bool,
    #[serde(default)]
    pub show_tool_calls: bool,
    #[serde(default = "default_usage")]
    pub usage_totals: Usage,
    #[serde(default)]
    pub last_assistant_message_id: Option<String>,
    #[serde(default)]
    pub last_user_message_id: Option<String>,
    #[serde(default)]
    pub model_override: Option<String>,
    #[serde(default)]
    pub model_pinning: ModelPinningMode,
}

impl Session {
    fn new() -> Self {
        let now = Utc::now();
        Self {
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
            model_pinning: ModelPinningMode::Prefer,
        }
    }

    pub fn reset(&mut self) {
        self.history.clear();
        self.usage_totals.prompt_tokens = 0;
        self.usage_totals.completion_tokens = 0;
        self.last_assistant_message_id = None;
        self.last_user_message_id = None;
        self.last_active = Utc::now();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SessionScope {
    channel_id: ChannelId,
    sender_id: SenderId,
}

impl SessionScope {
    fn new(channel_id: impl Into<ChannelId>, sender_id: impl Into<SenderId>) -> Self {
        Self {
            channel_id: channel_id.into(),
            sender_id: sender_id.into(),
        }
    }

    fn channel_id(&self) -> &str {
        self.channel_id.as_str()
    }

    fn sender_id(&self) -> &str {
        self.sender_id.as_str()
    }
}

#[derive(Clone)]
pub struct SessionManager {
    sessions: DashMap<SessionScope, Session>,
    project_db: Arc<dyn ProjectDb>,
    org_id: OrgId,
    project_db_handle: ProjectDbHandle,
}

impl SessionManager {
    pub async fn load_or_new(
        project_db: Arc<dyn ProjectDb>,
        org_id: OrgId,
        project_db_handle: ProjectDbHandle,
    ) -> Result<Self> {
        let manager = Self {
            sessions: DashMap::new(),
            project_db,
            org_id,
            project_db_handle,
        };
        manager.ensure_schema().await?;
        manager.load_from_store().await?;
        Ok(manager)
    }

    async fn load_from_store(&self) -> Result<()> {
        let rows = self
            .project_db
            .query(
                self.org_id,
                &self.project_db_handle,
                r#"
SELECT channel_id, sender_id, session_json
  FROM opencraw_sessions
"#,
                &[],
            )
            .await?;
        for row in rows {
            let channel_id = row_required_string(&row, "channel_id")?;
            let sender_id = row_required_string(&row, "sender_id")?;
            let session_json = row_required_string(&row, "session_json")?;
            let session: Session = serde_json::from_str(&session_json)?;
            self.sessions
                .insert(SessionScope::new(channel_id, sender_id), session);
        }
        Ok(())
    }

    async fn ensure_schema(&self) -> Result<()> {
        self.project_db
            .execute(
                self.org_id,
                &self.project_db_handle,
                r#"
CREATE TABLE IF NOT EXISTS opencraw_sessions (
    channel_id TEXT NOT NULL,
    sender_id TEXT NOT NULL,
    session_json TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (channel_id, sender_id)
)
"#,
                &[],
            )
            .await?;
        Ok(())
    }

    pub fn get_or_create_mut(
        &self,
        channel_id: &str,
        sender_id: &str,
    ) -> dashmap::mapref::one::RefMut<'_, SessionScope, Session> {
        self.sessions
            .entry(SessionScope::new(channel_id, sender_id))
            .or_insert_with(Session::new)
    }

    pub fn list(&self) -> Vec<SessionSummary> {
        let mut out: Vec<SessionSummary> = self
            .sessions
            .iter()
            .map(|entry| {
                let (scope, s) = entry.pair();
                SessionSummary {
                    id: s.id,
                    channel_id: scope.channel_id().to_string(),
                    sender_id: scope.sender_id().to_string(),
                    created_at: s.created_at,
                    last_active: s.last_active,
                    messages: s.history.len(),
                    model_override: s.model_override.clone(),
                    model_pinning: s.model_pinning,
                }
            })
            .collect();
        out.sort_by_key(|s| s.last_active);
        out.reverse();
        out
    }

    pub async fn delete_by_id(&self, id: Uuid) -> Result<bool> {
        let mut to_remove = None;
        for e in self.sessions.iter() {
            if e.value().id == id {
                to_remove = Some(e.key().clone());
                break;
            }
        }
        if let Some(key) = to_remove {
            self.sessions.remove(&key);
            self.project_db
                .execute(
                    self.org_id,
                    &self.project_db_handle,
                    r#"
DELETE FROM opencraw_sessions
 WHERE channel_id = ?1
   AND sender_id = ?2
"#,
                    &[
                        ProjectDbParam::String(key.channel_id().to_string()),
                        ProjectDbParam::String(key.sender_id().to_string()),
                    ],
                )
                .await?;
            return Ok(true);
        }
        Ok(false)
    }

    pub async fn persist(&self) -> Result<()> {
        for entry in self.sessions.iter() {
            let (scope, session) = entry.pair();
            let session_json = serde_json::to_string(session)?;
            self.project_db
                .execute(
                    self.org_id,
                    &self.project_db_handle,
                    r#"
INSERT INTO opencraw_sessions (channel_id, sender_id, session_json, updated_at)
VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
ON CONFLICT(channel_id, sender_id) DO UPDATE
SET session_json = excluded.session_json,
    updated_at = CURRENT_TIMESTAMP
"#,
                    &[
                        ProjectDbParam::String(scope.channel_id().to_string()),
                        ProjectDbParam::String(scope.sender_id().to_string()),
                        ProjectDbParam::String(session_json),
                    ],
                )
                .await?;
        }
        Ok(())
    }

    pub async fn set_model_override_by_id(
        &self,
        id: Uuid,
        model_override: Option<String>,
        model_pinning: Option<ModelPinningMode>,
    ) -> Result<Option<Session>> {
        let mut key_to_update = None;
        for entry in self.sessions.iter() {
            if entry.value().id == id {
                key_to_update = Some(entry.key().clone());
                break;
            }
        }
        let Some(key) = key_to_update else {
            return Ok(None);
        };

        let normalized_override = model_override.and_then(|value| {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });

        let updated = if let Some(mut session) = self.sessions.get_mut(&key) {
            session.model_override = normalized_override;
            if session.model_override.is_none() {
                session.model_pinning = ModelPinningMode::Prefer;
            } else if let Some(mode) = model_pinning {
                session.model_pinning = mode;
            }
            session.last_active = Utc::now();
            session.clone()
        } else {
            return Ok(None);
        };

        self.persist().await?;
        Ok(Some(updated))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub id: Uuid,
    pub channel_id: String,
    pub sender_id: String,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub messages: usize,
    pub model_override: Option<String>,
    pub model_pinning: ModelPinningMode,
}

fn default_usage() -> Usage {
    Usage {
        prompt_tokens: 0,
        completion_tokens: 0,
    }
}

fn row_required_string(
    row: &std::collections::BTreeMap<String, ProjectDbValue>,
    key: &str,
) -> Result<String> {
    let value = row
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("session row missing required key: {key}"))?;
    match value {
        ProjectDbValue::String(v) => Ok(v.clone()),
        other => Err(anyhow::anyhow!(
            "session row key {key} expected string but received {other:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizons_core::models::ProjectId;
    use horizons_rs::dev_backends::DevProjectDb;
    use os_llm::Role;
    use std::sync::Arc;

    #[tokio::test]
    async fn persists_and_reloads_sessions() {
        let root = std::env::temp_dir().join(format!("opencraw-session-{}", Uuid::new_v4()));
        let project_db = Arc::new(
            DevProjectDb::new(root.join("project_dbs"))
                .await
                .expect("new dev project db"),
        );
        let org_id = OrgId(Uuid::new_v4());
        let project_id = ProjectId(Uuid::new_v4());
        let handle = project_db
            .provision(org_id, project_id)
            .await
            .expect("provision project db");
        let manager = SessionManager::load_or_new(project_db.clone(), org_id, handle.clone())
            .await
            .expect("load manager");
        {
            let mut session = manager.get_or_create_mut("webchat", "user-1");
            session.history.push(ChatMessage {
                role: Role::User,
                content: "hello".to_string(),
                tool_calls: vec![],
                tool_call_id: None,
            });
            session.last_active = Utc::now();
        }
        manager.persist().await.expect("persist sessions");

        let reloaded = SessionManager::load_or_new(project_db.clone(), org_id, handle.clone())
            .await
            .expect("reload manager");
        let sessions = reloaded.list();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].channel_id, "webchat");
        assert_eq!(sessions[0].sender_id, "user-1");
        assert_eq!(sessions[0].messages, 1);
    }
}
