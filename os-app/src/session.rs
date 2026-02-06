//! Session manager for (channel_id, sender_id) isolation.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use os_llm::{ChatMessage, Usage};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Session {
    pub id: Uuid,
    pub history: Vec<ChatMessage>,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub show_thinking: bool,
    pub show_tool_calls: bool,
    pub usage_totals: Usage,
    pub last_assistant_message_id: Option<String>,
    pub last_user_message_id: Option<String>,
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

#[derive(Clone)]
pub struct SessionManager {
    sessions: DashMap<(String, String), Session>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
        }
    }

    pub fn get_or_create_mut(
        &self,
        channel_id: &str,
        sender_id: &str,
    ) -> dashmap::mapref::one::RefMut<'_, (String, String), Session> {
        self.sessions
            .entry((channel_id.to_string(), sender_id.to_string()))
            .or_insert_with(Session::new)
    }

    pub fn list(&self) -> Vec<SessionSummary> {
        let mut out: Vec<SessionSummary> = self
            .sessions
            .iter()
            .map(|entry| {
                let ((channel_id, sender_id), s) = entry.pair();
                SessionSummary {
                    id: s.id,
                    channel_id: channel_id.clone(),
                    sender_id: sender_id.clone(),
                    created_at: s.created_at,
                    last_active: s.last_active,
                    messages: s.history.len(),
                }
            })
            .collect();
        out.sort_by_key(|s| s.last_active);
        out.reverse();
        out
    }

    pub fn delete_by_id(&self, id: Uuid) -> bool {
        let mut to_remove = None;
        for e in self.sessions.iter() {
            if e.value().id == id {
                to_remove = Some(e.key().clone());
                break;
            }
        }
        if let Some(key) = to_remove {
            self.sessions.remove(&key);
            return true;
        }
        false
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionSummary {
    pub id: Uuid,
    pub channel_id: String,
    pub sender_id: String,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub messages: usize,
}
