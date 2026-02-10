use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl Deref for $name {
            type Target = str;

            fn deref(&self) -> &Self::Target {
                self.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

id_newtype!(MessageId);
id_newtype!(ChannelId);
id_newtype!(SenderId);
id_newtype!(ThreadId);

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
    pub message_id: MessageId,
    pub channel_id: ChannelId,
    pub sender_id: SenderId,
    pub thread_id: Option<ThreadId>,
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
    pub reply_to_message_id: Option<MessageId>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}
