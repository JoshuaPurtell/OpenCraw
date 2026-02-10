use crate::server::OsState;
use crate::session::ModelPinningMode;
use axum::extract::Path;
use axum::routing::{delete, get, post};
use axum::{Extension, Json};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SetSessionModelRequest {
    #[serde(default)]
    model_override: Option<String>,
    #[serde(default)]
    model_pinning: Option<ModelPinningMode>,
}

pub fn router() -> axum::Router {
    axum::Router::new()
        .route("/api/v1/os/sessions", get(list_sessions))
        .route("/api/v1/os/sessions/{id}", delete(delete_session))
        .route("/api/v1/os/sessions/{id}/model", post(set_session_model))
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

#[tracing::instrument(level = "info", skip_all)]
async fn set_session_model(
    Extension(state): Extension<Arc<OsState>>,
    Path(id): Path<String>,
    Json(req): Json<SetSessionModelRequest>,
) -> Json<serde_json::Value> {
    let Ok(id) = Uuid::parse_str(&id) else {
        return Json(serde_json::json!({ "status": "error", "error": "invalid id" }));
    };

    let configured_models = {
        let snapshot = state.config_control.snapshot().await;
        let mut models = vec![snapshot.config.general.model];
        for fallback in snapshot.config.general.fallback_models {
            if !models
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&fallback))
            {
                models.push(fallback);
            }
        }
        models
    };

    let normalized_override = match req.model_override.as_deref() {
        None => None,
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else if let Some(model) = configured_models
                .iter()
                .find(|model| model.eq_ignore_ascii_case(trimmed))
            {
                Some(model.clone())
            } else {
                return Json(serde_json::json!({
                    "status": "error",
                    "error": format!("unknown model override {trimmed:?}"),
                    "available_models": configured_models,
                }));
            }
        }
    };

    if matches!(req.model_pinning, Some(ModelPinningMode::Strict)) && normalized_override.is_none()
    {
        return Json(serde_json::json!({
            "status": "error",
            "error": "model_pinning='strict' requires non-empty model_override",
        }));
    }

    match state
        .sessions
        .set_model_override_by_id(id, normalized_override.clone(), req.model_pinning)
        .await
    {
        Ok(Some(updated)) => Json(serde_json::json!({
            "status": "ok",
            "model_override": updated.model_override,
            "model_pinning": updated.model_pinning,
        })),
        Ok(None) => Json(serde_json::json!({ "status": "not_found" })),
        Err(e) => Json(serde_json::json!({ "status": "error", "error": e.to_string() })),
    }
}
