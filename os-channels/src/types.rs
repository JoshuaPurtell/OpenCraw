use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub content_type: String,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboundMessageKind {
    Message,
    Reaction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub kind: InboundMessageKind,
    pub message_id: String,
    pub channel_id: String,
    pub sender_id: String,
    pub thread_id: Option<String>,
    pub is_group: bool,
    pub content: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub content: String,
    #[serde(default)]
    pub reply_to_message_id: Option<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}
