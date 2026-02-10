use crate::server::OsState;
use axum::Extension;
use axum::Json;
use axum::routing::get;
use std::sync::Arc;

pub fn router() -> axum::Router {
    axum::Router::new().route("/api/v1/os/channels", get(list_channels))
}

#[tracing::instrument(level = "debug", skip_all)]
async fn list_channels(Extension(state): Extension<Arc<OsState>>) -> Json<serde_json::Value> {
    let mut channels: Vec<String> = state.channels.keys().cloned().collect();
    channels.sort();
    let mut capabilities: Vec<serde_json::Value> = state
        .channel_capability_matrix
        .iter()
        .map(|(channel_id, c)| {
            serde_json::json!({
                "channel_id": channel_id,
                "supports_streaming_deltas": c.supports_streaming_deltas,
                "supports_typing_events": c.supports_typing_events,
                "supports_reactions": c.supports_reactions,
            })
        })
        .collect();
    capabilities.sort_by(|a, b| {
        let a_id = a
            .get("channel_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let b_id = b
            .get("channel_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        a_id.cmp(b_id)
    });
    Json(serde_json::json!({
        "channels": channels,
        "capabilities": capabilities
    }))
}
