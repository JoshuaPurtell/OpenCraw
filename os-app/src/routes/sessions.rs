use crate::server::OsState;
use axum::extract::Path;
use axum::routing::{delete, get};
use axum::{Extension, Json};
use std::sync::Arc;
use uuid::Uuid;

pub fn router() -> axum::Router {
    axum::Router::new()
        .route("/api/v1/os/sessions", get(list_sessions))
        .route("/api/v1/os/sessions/{id}", delete(delete_session))
}

#[tracing::instrument(level = "debug", skip_all)]
async fn list_sessions(Extension(state): Extension<Arc<OsState>>) -> Json<serde_json::Value> {
    let sessions = state.sessions.list();
    Json(serde_json::json!({ "sessions": sessions }))
}

#[tracing::instrument(level = "info", skip_all)]
async fn delete_session(
    Extension(state): Extension<Arc<OsState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let Ok(id) = Uuid::parse_str(&id) else {
        return Json(serde_json::json!({ "status": "error", "error": "invalid id" }));
    };
    match state.sessions.delete_by_id(id).await {
        Ok(ok) => Json(serde_json::json!({ "status": if ok { "ok" } else { "not_found" } })),
        Err(e) => Json(serde_json::json!({ "status": "error", "error": e.to_string() })),
    }
}
