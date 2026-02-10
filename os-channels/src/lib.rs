//! Channel adapters for OpenShell.
//!
//! Adapters are pure I/O: they convert platform messages to/from OpenShell
//! `InboundMessage` / `OutboundMessage`.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

mod discord;
mod email;
mod http_plugin;
mod imessage;
mod linear;
mod matrix;
mod signal;
mod slack;
mod telegram;
mod traits;
mod types;
mod webchat;
mod whatsapp;

pub use discord::DiscordAdapter;
pub use email::EmailAdapter;
pub use http_plugin::HttpPluginAdapter;
pub use imessage::ImessageAdapter;
pub use linear::LinearAdapter;
pub use matrix::MatrixAdapter;
pub use signal::SignalAdapter;
pub use slack::SlackAdapter;
pub use telegram::TelegramAdapter;
pub use traits::ChannelAdapter;
pub use types::{
    Attachment, ChannelId, InboundMessage, InboundMessageKind, MessageId, OutboundMessage,
    SenderId, ThreadId,
};
pub use webchat::WebChatAdapter;
pub use whatsapp::WhatsAppCloudAdapter;
