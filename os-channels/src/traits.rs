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

    /// Send a streaming delta chunk to a specific user/thread.
    /// Adapters that do not support chunked updates should keep the default.
    async fn send_delta(&self, _recipient_id: &str, _delta: &str) -> Result<()> {
        Err(anyhow::anyhow!(
            "send_delta is not supported by this channel"
        ))
    }

    /// Send typing state updates where supported.
    async fn send_typing(&self, _recipient_id: &str, _active: bool) -> Result<()> {
        Err(anyhow::anyhow!(
            "send_typing is not supported by this channel"
        ))
    }

    fn supports_streaming_deltas(&self) -> bool {
        false
    }

    fn supports_typing_events(&self) -> bool {
        false
    }

    fn supports_reactions(&self) -> bool {
        false
    }
}
