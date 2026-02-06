use crate::server::OsState;
use axum::routing::get;
use axum::Extension;
use axum::Json;
use std::sync::Arc;

pub fn router() -> axum::Router {
    axum::Router::new().route("/api/v1/os/channels", get(list_channels))
}

#[tracing::instrument(level = "debug", skip_all)]
async fn list_channels(Extension(state): Extension<Arc<OsState>>) -> Json<serde_json::Value> {
    let mut channels: Vec<String> = state.channels.keys().cloned().collect();
    channels.sort();
    Json(serde_json::json!({ "channels": channels }))
}
