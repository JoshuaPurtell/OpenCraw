use crate::types::{InboundMessage, OutboundMessage};
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

#[async_trait]
pub trait ChannelAdapter: Send + Sync {
    /// Unique channel identifier: "webchat", "telegram", "discord".
    fn channel_id(&self) -> &str;

    /// Start receiving messages. Push to tx for each inbound message.
    async fn start(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()>;

    /// Send a message to a specific user/thread on this platform.
    async fn send(&self, recipient_id: &str, message: OutboundMessage) -> Result<()>;

    fn supports_reactions(&self) -> bool {
        false
    }
}
