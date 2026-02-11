use crate::server::OsState;
use axum::routing::post;
use axum::{Extension, Json};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SendRequest {
    channel: String,
    recipient: String,
    message: String,
}

pub fn router() -> axum::Router {
    axum::Router::new().route("/api/v1/os/messages/send", post(send_message))
}

#[tracing::instrument(level = "info", skip_all)]
async fn send_message(
    Extension(state): Extension<Arc<OsState>>,
    Json(req): Json<SendRequest>,
) -> Json<serde_json::Value> {
    let channel = req.channel.trim().to_ascii_lowercase();
    if channel.is_empty() {
        return Json(serde_json::json!({
            "status": "error",
            "error": "channel is required"
        }));
    }
    let recipient = req.recipient.trim();
    if recipient.is_empty() {
        return Json(serde_json::json!({
            "status": "error",
            "error": "recipient is required"
        }));
    }
    let message_empty = req.message.trim().is_empty();
    if message_empty {
        return Json(serde_json::json!({
            "status": "error",
            "error": "message is required"
        }));
    }

    let Some(adapter) = state.channels.get(channel.as_str()) else {
        return Json(serde_json::json!({ "status": "error", "error": "unknown channel" }));
    };

    if let Err(e) = adapter
        .send(
            recipient,
            os_channels::OutboundMessage {
                content: req.message,
                reply_to_message_id: None,
                attachments: vec![],
                metadata: serde_json::Value::Null,
            },
        )
        .await
    {
        return Json(serde_json::json!({ "status": "error", "error": e.to_string() }));
    }

    Json(serde_json::json!({ "status": "ok" }))
}
