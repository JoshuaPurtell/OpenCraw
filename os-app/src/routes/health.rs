use crate::discovery_runtime::{discovery_health_ready, discovery_health_status_label};
use crate::server::OsState;
use axum::routing::get;
use axum::{Extension, Json};
use chrono::Utc;
use std::sync::Arc;

pub fn router() -> axum::Router {
    axum::Router::new().route("/api/v1/os/health", get(get_health))
}

#[tracing::instrument(level = "debug", skip_all)]
async fn get_health(Extension(state): Extension<Arc<OsState>>) -> Json<serde_json::Value> {
    let discovery = state.discovery.status_snapshot().await;
    let discovery_health = discovery.health;

    Json(serde_json::json!({
        "status": discovery_health_status_label(discovery_health),
        "ready": discovery_health_ready(discovery_health),
        "checked_at": Utc::now(),
        "checks": {
            "discovery": {
                "health": discovery_health,
                "active": discovery.active,
                "consecutive_failures": discovery.consecutive_failures,
                "last_heartbeat_at": discovery.last_heartbeat_at,
                "last_success_at": discovery.last_success_at,
                "last_error_at": discovery.last_error_at,
                "last_error": discovery.last_error,
            }
        }
    }))
}
