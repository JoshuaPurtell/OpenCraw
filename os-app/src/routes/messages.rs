use crate::server::OsState;
use axum::routing::post;
use axum::{Extension, Json};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
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
    let Some(adapter) = state.channels.get(&req.channel) else {
        return Json(serde_json::json!({ "status": "error", "error": "unknown channel" }));
    };

    if let Err(e) = adapter
        .send(
            &req.recipient,
            os_channels::OutboundMessage {
                content: req.message,
                reply_to_message_id: None,
                attachments: vec![],
            },
        )
        .await
    {
        return Json(serde_json::json!({ "status": "error", "error": e.to_string() }));
    }

    Json(serde_json::json!({ "status": "ok" }))
}
