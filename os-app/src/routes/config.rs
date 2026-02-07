use crate::config::OpenShellConfig;
use crate::server::OsState;
use axum::routing::{get, post};
use axum::{Extension, Json};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchConfigRequest {
    #[serde(default)]
    base_hash: Option<String>,
    patch: serde_json::Value,
}

pub fn router() -> axum::Router {
    axum::Router::new()
        .route("/api/v1/os/config/get", get(get_config))
        .route("/api/v1/os/config/apply", post(apply_config))
        .route("/api/v1/os/config/patch", post(patch_config))
}

#[tracing::instrument(level = "debug", skip_all)]
async fn get_config(Extension(state): Extension<Arc<OsState>>) -> Json<serde_json::Value> {
    let snapshot = state.config_control.snapshot().await;
    let mut config = match serde_json::to_value(snapshot.config) {
        Ok(v) => v,
        Err(e) => {
            return Json(serde_json::json!({
                "status": "error",
                "error": format!("failed to serialize config snapshot: {e}")
            }))
        }
    };
    redact_keys(&mut config);
    Json(serde_json::json!({
        "status": "ok",
        "path": snapshot.path,
        "base_hash": snapshot.base_hash,
        "updated_at": snapshot.updated_at,
        "config": config
    }))
}

#[tracing::instrument(level = "info", skip_all)]
async fn apply_config(
    Extension(state): Extension<Arc<OsState>>,
    Json(next): Json<OpenShellConfig>,
) -> Json<serde_json::Value> {
    match state.config_control.apply(next).await {
        Ok(snapshot) => Json(serde_json::json!({
            "status": "ok",
            "path": snapshot.path,
            "base_hash": snapshot.base_hash,
            "updated_at": snapshot.updated_at,
            "restart_required": true
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "error": e.to_string()
        })),
    }
}

#[tracing::instrument(level = "info", skip_all)]
async fn patch_config(
    Extension(state): Extension<Arc<OsState>>,
    Json(req): Json<PatchConfigRequest>,
) -> Json<serde_json::Value> {
    match state
        .config_control
        .patch(req.base_hash.as_deref(), req.patch)
        .await
    {
        Ok(snapshot) => Json(serde_json::json!({
            "status": "ok",
            "path": snapshot.path,
            "base_hash": snapshot.base_hash,
            "updated_at": snapshot.updated_at,
            "restart_required": true
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "error": e.to_string()
        })),
    }
}

fn redact_keys(config: &mut serde_json::Value) {
    let Some(keys) = config.get_mut("keys") else {
        return;
    };
    let Some(obj) = keys.as_object_mut() else {
        return;
    };
    if let Some(v) = obj.get_mut("openai_api_key") {
        if !v.is_null() {
            *v = serde_json::Value::String("REDACTED".to_string());
        }
    }
    if let Some(v) = obj.get_mut("anthropic_api_key") {
        if !v.is_null() {
            *v = serde_json::Value::String("REDACTED".to_string());
        }
    }
}
