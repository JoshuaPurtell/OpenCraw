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
        match snapshot.config.configured_models() {
            Ok(models) => models,
            Err(error) => {
                return Json(serde_json::json!({
                    "status": "error",
                    "error": format!("model configuration invalid: {error}"),
                }));
            }
        }
    };

    let normalized_override =
        match normalize_model_override(req.model_override.as_deref(), &configured_models) {
            Ok(override_value) => override_value,
            Err(error) => {
                return Json(serde_json::json!({
                    "status": "error",
                    "error": error,
                    "available_models": configured_models,
                }));
            }
        };

    if let Err(error) = validate_model_pinning(req.model_pinning, normalized_override.as_deref()) {
        return Json(serde_json::json!({
            "status": "error",
            "error": error,
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

fn normalize_model_override(
    model_override: Option<&str>,
    configured_models: &[String],
) -> Result<Option<String>, String> {
    let Some(raw) = model_override else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if let Some(model) = configured_models
        .iter()
        .find(|model| model.eq_ignore_ascii_case(trimmed))
    {
        return Ok(Some(model.clone()));
    }
    Err(format!("unknown model override {trimmed:?}"))
}

fn validate_model_pinning(
    model_pinning: Option<ModelPinningMode>,
    normalized_override: Option<&str>,
) -> Result<(), String> {
    if matches!(model_pinning, Some(ModelPinningMode::Strict)) && normalized_override.is_none() {
        return Err("model_pinning='strict' requires non-empty model_override".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{normalize_model_override, validate_model_pinning};
    use crate::session::ModelPinningMode;

    #[test]
    fn normalize_model_override_accepts_case_insensitive_match() {
        let models = vec!["gpt-4o-mini".to_string(), "gpt-4.1-mini".to_string()];
        let normalized =
            normalize_model_override(Some(" GPT-4.1-MINI "), &models).expect("normalize model");
        assert_eq!(normalized.as_deref(), Some("gpt-4.1-mini"));
    }

    #[test]
    fn normalize_model_override_rejects_unknown_model() {
        let models = vec!["gpt-4o-mini".to_string()];
        let error =
            normalize_model_override(Some("claude-3"), &models).expect_err("unknown should fail");
        assert!(error.contains("unknown model override"));
    }

    #[test]
    fn strict_pinning_requires_override() {
        let error =
            validate_model_pinning(Some(ModelPinningMode::Strict), None).expect_err("must fail");
        assert!(error.contains("requires non-empty model_override"));
    }

    #[test]
    fn strict_pinning_allows_override() {
        validate_model_pinning(Some(ModelPinningMode::Strict), Some("gpt-4o-mini"))
            .expect("strict pinning should pass when override is set");
    }
}
