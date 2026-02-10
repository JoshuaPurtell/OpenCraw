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

    fn enforce_invariants(&mut self) {
        self.model_override = normalize_optional_string(self.model_override.take());
        if self.model_override.is_none() {
            self.model_pinning = ModelPinningMode::Prefer;
        }
        if self.last_active < self.created_at {
            self.last_active = self.created_at;
        }
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
            let channel_id = match row_required_string(&row, "channel_id") {
                Ok(v) => v,
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "skipping persisted session row missing channel_id"
                    );
                    continue;
                }
            };
            let sender_id = match row_required_string(&row, "sender_id") {
                Ok(v) => v,
                Err(error) => {
                    tracing::warn!(
                        %channel_id,
                        error = %error,
                        "skipping persisted session row missing sender_id"
                    );
                    continue;
                }
            };
            let session_json = match row_required_string(&row, "session_json") {
                Ok(v) => v,
                Err(error) => {
                    tracing::warn!(
                        %channel_id,
                        %sender_id,
                        error = %error,
                        "skipping persisted session row missing session_json"
                    );
                    continue;
                }
            };
            match serde_json::from_str::<Session>(&session_json) {
                Ok(mut session) => {
                    session.enforce_invariants();
                    self.sessions
                        .insert(SessionScope::new(channel_id, sender_id), session);
                }
                Err(error) => {
                    tracing::warn!(
                        %channel_id,
                        %sender_id,
                        error = %error,
                        "skipping persisted session row with invalid session_json"
                    );
                }
            }
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
        let mut session = self
            .sessions
            .entry(SessionScope::new(channel_id, sender_id))
            .or_insert_with(Session::new);
        session.enforce_invariants();
        session
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
            self.sessions.remove(&key);
            return Ok(true);
        }
        Ok(false)
    }

    pub async fn persist(&self) -> Result<()> {
        let snapshots: Vec<(SessionScope, Session)> = self
            .sessions
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();
        for (scope, mut session) in snapshots {
            session.enforce_invariants();
            self.persist_scope(&scope, &session).await?;
        }
        Ok(())
    }

    async fn persist_scope(&self, scope: &SessionScope, session: &Session) -> Result<()> {
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

        let normalized_override = normalize_optional_string(model_override);

        let (previous, updated) = if let Some(mut session) = self.sessions.get_mut(&key) {
            let previous = session.clone();
            session.model_override = normalized_override;
            if let Some(mode) = model_pinning {
                session.model_pinning = mode;
            }
            session.enforce_invariants();
            session.last_active = Utc::now();
            (previous, session.clone())
        } else {
            return Ok(None);
        };

        if let Err(error) = self.persist_scope(&key, &updated).await {
            if let Some(mut session) = self.sessions.get_mut(&key) {
                *session = previous;
            }
            return Err(error);
        }

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

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
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
    use chrono::Duration;
    use horizons_core::models::ProjectId;
    use horizons_rs::dev_backends::DevProjectDb;
    use os_llm::Role;
    use std::sync::Arc;

    async fn new_manager() -> (Arc<DevProjectDb>, OrgId, ProjectDbHandle, SessionManager) {
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
        (project_db, org_id, handle, manager)
    }

    #[tokio::test]
    async fn persists_and_reloads_sessions() {
        let (project_db, org_id, handle, manager) = new_manager().await;
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

    #[tokio::test]
    async fn skips_invalid_rows_when_loading_from_store() {
        let (project_db, org_id, handle, manager) = new_manager().await;
        {
            let mut session = manager.get_or_create_mut("webchat", "user-good");
            session.history.push(ChatMessage {
                role: Role::User,
                content: "hello".to_string(),
                tool_calls: vec![],
                tool_call_id: None,
            });
        }
        manager.persist().await.expect("persist valid session");

        project_db
            .execute(
                org_id,
                &handle,
                r#"
INSERT INTO opencraw_sessions (channel_id, sender_id, session_json, updated_at)
VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
"#,
                &[
                    ProjectDbParam::String("webchat".to_string()),
                    ProjectDbParam::String("user-corrupt".to_string()),
                    ProjectDbParam::String("{\"id\":\"broken".to_string()),
                ],
            )
            .await
            .expect("insert corrupt row");

        let reloaded = SessionManager::load_or_new(project_db.clone(), org_id, handle.clone())
            .await
            .expect("reload manager while corrupt row exists");
        let sessions = reloaded.list();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].sender_id, "user-good");
    }

    #[tokio::test]
    async fn set_model_override_enforces_invariants() {
        let (project_db, org_id, handle, manager) = new_manager().await;
        let session_id = {
            let session = manager.get_or_create_mut("webchat", "operator");
            session.id
        };

        let updated = manager
            .set_model_override_by_id(
                session_id,
                Some(" gpt-4.1-mini ".to_string()),
                Some(ModelPinningMode::Strict),
            )
            .await
            .expect("set strict model override")
            .expect("session should exist");
        assert_eq!(updated.model_override.as_deref(), Some("gpt-4.1-mini"));
        assert_eq!(updated.model_pinning, ModelPinningMode::Strict);

        let updated = manager
            .set_model_override_by_id(
                session_id,
                Some("    ".to_string()),
                Some(ModelPinningMode::Strict),
            )
            .await
            .expect("clear model override by blank")
            .expect("session should exist");
        assert_eq!(updated.model_override, None);
        assert_eq!(updated.model_pinning, ModelPinningMode::Prefer);

        let reloaded = SessionManager::load_or_new(project_db.clone(), org_id, handle.clone())
            .await
            .expect("reload manager");
        let summary = reloaded
            .list()
            .into_iter()
            .find(|session| session.id == session_id)
            .expect("session should exist");
        assert_eq!(summary.model_override, None);
        assert_eq!(summary.model_pinning, ModelPinningMode::Prefer);
    }

    #[tokio::test]
    async fn persist_normalizes_timestamp_and_pinning() {
        let (project_db, org_id, handle, manager) = new_manager().await;
        let session_id = {
            let mut session = manager.get_or_create_mut("webchat", "invariant-user");
            session.model_override = None;
            session.model_pinning = ModelPinningMode::Strict;
            session.last_active = session.created_at - Duration::seconds(30);
            session.id
        };

        manager.persist().await.expect("persist normalized session");

        let reloaded = SessionManager::load_or_new(project_db.clone(), org_id, handle.clone())
            .await
            .expect("reload manager");
        let summary = reloaded
            .list()
            .into_iter()
            .find(|session| session.id == session_id)
            .expect("session should exist");
        assert_eq!(summary.model_pinning, ModelPinningMode::Prefer);
        assert!(summary.last_active >= summary.created_at);
    }
}
