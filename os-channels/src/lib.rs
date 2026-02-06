//! Channel adapters for OpenShell.
//!
//! Adapters are pure I/O: they convert platform messages to/from OpenShell
//! `InboundMessage` / `OutboundMessage`.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

mod discord;
mod imessage;
mod telegram;
mod traits;
mod types;
mod webchat;

pub use discord::DiscordAdapter;
pub use imessage::ImessageAdapter;
pub use telegram::TelegramAdapter;
pub use traits::ChannelAdapter;
pub use types::{Attachment, InboundMessage, InboundMessageKind, OutboundMessage};
pub use webchat::WebChatAdapter;
