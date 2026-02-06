use crate::traits::ChannelAdapter;
use crate::types::{InboundMessage, InboundMessageKind, OutboundMessage};
use anyhow::Result;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use chrono::Utc;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Clone)]
struct WebChatState {
    inbound_tx: Arc<tokio::sync::RwLock<Option<mpsc::Sender<InboundMessage>>>>,
    connections: Arc<DashMap<String, mpsc::UnboundedSender<Message>>>,
}

#[derive(Clone)]
pub struct WebChatAdapter {
    state: WebChatState,
}

impl WebChatAdapter {
    pub fn new() -> Self {
        Self {
            state: WebChatState {
                inbound_tx: Arc::new(tokio::sync::RwLock::new(None)),
                connections: Arc::new(DashMap::new()),
            },
        }
    }

    /// Router that serves the WebChat WebSocket at `/ws`.
    pub fn router(self: Arc<Self>) -> Router {
        Router::new().route("/ws", get(ws_upgrade)).with_state(self)
    }
}

async fn ws_upgrade(
    State(adapter): State<Arc<WebChatAdapter>>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| handle_socket(adapter, socket))
}

#[tracing::instrument(level = "info", skip_all)]
async fn handle_socket(adapter: Arc<WebChatAdapter>, socket: WebSocket) {
    let sender_id = Uuid::new_v4().to_string();
    let (mut ws_sender, mut ws_receiver) = socket.split();

    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Message>();
    adapter
        .state
        .connections
        .insert(sender_id.clone(), outbound_tx);

    let hello = serde_json::json!({ "type": "hello", "sender_id": sender_id });
    let _ = ws_sender
        .send(Message::Text(hello.to_string().into()))
        .await;

    let adapter_out = adapter.clone();
    let sender_id_out = sender_id.clone();
    let outbound_task = tokio::spawn(async move {
        while let Some(msg) = outbound_rx.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
        adapter_out.state.connections.remove(&sender_id_out);
    });

    while let Some(Ok(msg)) = ws_receiver.next().await {
        let Message::Text(text) = msg else {
            continue;
        };

        let parsed: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let msg_type = parsed
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("message");

        let (kind, content) = match msg_type {
            "reaction" => (
                InboundMessageKind::Reaction,
                parsed
                    .get("emoji")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            ),
            _ => (
                InboundMessageKind::Message,
                parsed
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            ),
        };

        let inbound = InboundMessage {
            kind,
            message_id: Uuid::new_v4().to_string(),
            channel_id: "webchat".to_string(),
            sender_id: sender_id.clone(),
            thread_id: Some(sender_id.clone()),
            is_group: false,
            content,
            metadata: parsed,
            received_at: Utc::now(),
        };

        let tx = adapter.state.inbound_tx.read().await.clone();
        if let Some(tx) = tx {
            let _ = tx.send(inbound).await;
        }
    }

    outbound_task.abort();
    adapter.state.connections.remove(&sender_id);
}

#[async_trait::async_trait]
impl ChannelAdapter for WebChatAdapter {
    fn channel_id(&self) -> &str {
        "webchat"
    }

    async fn start(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        *self.state.inbound_tx.write().await = Some(tx);
        Ok(())
    }

    async fn send(&self, recipient_id: &str, message: OutboundMessage) -> Result<()> {
        let Some(conn) = self.state.connections.get(recipient_id) else {
            return Ok(());
        };
        let payload = serde_json::json!({
            "type": "message",
            "content": message.content,
        });
        let _ = conn.send(Message::Text(payload.to_string().into()));
        Ok(())
    }

    fn supports_reactions(&self) -> bool {
        true
    }
}
