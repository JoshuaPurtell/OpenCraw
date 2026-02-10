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
        .route("/api/v1/os/config/network", get(get_network_policy))
        .route("/api/v1/os/config/discovery", get(get_discovery_status))
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
            }));
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

#[tracing::instrument(level = "debug", skip_all)]
async fn get_network_policy(Extension(state): Extension<Arc<OsState>>) -> Json<serde_json::Value> {
    let snapshot = state.config_control.snapshot().await;
    match snapshot.config.runtime_network_policy() {
        Ok(policy) => Json(serde_json::json!({
            "status": "ok",
            "path": snapshot.path,
            "base_hash": snapshot.base_hash,
            "updated_at": snapshot.updated_at,
            "network_policy": policy,
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "error": e.to_string(),
        })),
    }
}

#[tracing::instrument(level = "debug", skip_all)]
async fn get_discovery_status(
    Extension(state): Extension<Arc<OsState>>,
) -> Json<serde_json::Value> {
    let status = state.discovery.status_snapshot().await;
    Json(serde_json::json!({
        "status": "ok",
        "discovery": status,
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

    if let Some(security) = config.get_mut("security").and_then(|v| v.as_object_mut()) {
        if let Some(v) = security.get_mut("control_api_key") {
            if !v.is_null() {
                *v = serde_json::Value::String("REDACTED".to_string());
            }
        }
        if let Some(entries) = security
            .get_mut("control_api_keys")
            .and_then(|v| v.as_array_mut())
        {
            for entry in entries {
                if let Some(token) = entry.as_object_mut().and_then(|obj| obj.get_mut("token")) {
                    if !token.is_null() {
                        *token = serde_json::Value::String("REDACTED".to_string());
                    }
                }
            }
        }
    }

    if let Some(automation) = config.get_mut("automation").and_then(|v| v.as_object_mut()) {
        if let Some(v) = automation.get_mut("webhook_secret") {
            if !v.is_null() {
                *v = serde_json::Value::String("REDACTED".to_string());
            }
        }
    }

    if let Some(channels) = config.get_mut("channels").and_then(|v| v.as_object_mut()) {
        redact_channel_secret(channels, "telegram", "bot_token");
        redact_channel_secret(channels, "discord", "bot_token");
        redact_channel_secret(channels, "slack", "bot_token");
        redact_channel_secret(channels, "matrix", "access_token");
        redact_channel_secret(channels, "signal", "api_token");
        redact_channel_secret(channels, "whatsapp", "access_token");
        redact_channel_secret(channels, "whatsapp", "app_secret");
        redact_channel_secret(channels, "email", "gmail_access_token");
        redact_channel_secret(channels, "linear", "api_key");
        redact_external_plugin_secret(channels, "auth_token");
    }
}

fn redact_channel_secret(
    channels: &mut serde_json::Map<String, serde_json::Value>,
    channel_key: &str,
    secret_key: &str,
) {
    let Some(channel_obj) = channels
        .get_mut(channel_key)
        .and_then(|v| v.as_object_mut())
    else {
        return;
    };
    let Some(secret) = channel_obj.get_mut(secret_key) else {
        return;
    };
    if !secret.is_null() {
        *secret = serde_json::Value::String("REDACTED".to_string());
    }
}

fn redact_external_plugin_secret(
    channels: &mut serde_json::Map<String, serde_json::Value>,
    secret_key: &str,
) {
    let Some(entries) = channels
        .get_mut("external_plugins")
        .and_then(|value| value.as_array_mut())
    else {
        return;
    };
    for entry in entries {
        let Some(secret) = entry
            .as_object_mut()
            .and_then(|obj| obj.get_mut(secret_key))
        else {
            continue;
        };
        if !secret.is_null() {
            *secret = serde_json::Value::String("REDACTED".to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::redact_keys;

    #[test]
    fn redact_keys_masks_control_api_key_pool_tokens() {
        let mut config = serde_json::json!({
            "keys": {
                "openai_api_key": "openai-secret",
                "anthropic_api_key": "anthropic-secret",
            },
            "security": {
                "control_api_key": "legacy-secret",
                "control_api_keys": [
                    { "token": "rotate-1", "scopes": ["config:write"] },
                    { "token": "rotate-2", "scopes": [] }
                ]
            },
            "channels": {
                "telegram": { "bot_token": "telegram-secret" },
                "discord": { "bot_token": "discord-secret" },
                "slack": { "bot_token": "slack-secret" },
                "matrix": { "access_token": "matrix-secret" },
                "signal": { "api_token": "signal-secret" },
                "whatsapp": { "access_token": "wa-secret", "app_secret": "wa-app-secret" },
                "email": { "gmail_access_token": "gmail-secret" },
                "linear": { "api_key": "linear-secret" },
                "external_plugins": [
                    { "id": "custom_ops", "auth_token": "plugin-secret" }
                ]
            },
            "automation": {
                "webhook_secret": "webhook-secret"
            }
        });

        redact_keys(&mut config);

        assert_eq!(
            config
                .get("keys")
                .and_then(|v| v.get("openai_api_key"))
                .and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        assert_eq!(
            config
                .get("keys")
                .and_then(|v| v.get("anthropic_api_key"))
                .and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        assert_eq!(
            config
                .get("security")
                .and_then(|v| v.get("control_api_key"))
                .and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        let pool = config
            .get("security")
            .and_then(|v| v.get("control_api_keys"))
            .and_then(|v| v.as_array())
            .expect("control_api_keys array must exist");
        assert_eq!(
            pool[0].get("token").and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        assert_eq!(
            pool[1].get("token").and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        assert_eq!(
            config
                .get("automation")
                .and_then(|v| v.get("webhook_secret"))
                .and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        assert_eq!(
            config
                .get("channels")
                .and_then(|v| v.get("telegram"))
                .and_then(|v| v.get("bot_token"))
                .and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        assert_eq!(
            config
                .get("channels")
                .and_then(|v| v.get("discord"))
                .and_then(|v| v.get("bot_token"))
                .and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        assert_eq!(
            config
                .get("channels")
                .and_then(|v| v.get("slack"))
                .and_then(|v| v.get("bot_token"))
                .and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        assert_eq!(
            config
                .get("channels")
                .and_then(|v| v.get("matrix"))
                .and_then(|v| v.get("access_token"))
                .and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        assert_eq!(
            config
                .get("channels")
                .and_then(|v| v.get("signal"))
                .and_then(|v| v.get("api_token"))
                .and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        assert_eq!(
            config
                .get("channels")
                .and_then(|v| v.get("whatsapp"))
                .and_then(|v| v.get("access_token"))
                .and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        assert_eq!(
            config
                .get("channels")
                .and_then(|v| v.get("whatsapp"))
                .and_then(|v| v.get("app_secret"))
                .and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        assert_eq!(
            config
                .get("channels")
                .and_then(|v| v.get("email"))
                .and_then(|v| v.get("gmail_access_token"))
                .and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        assert_eq!(
            config
                .get("channels")
                .and_then(|v| v.get("linear"))
                .and_then(|v| v.get("api_key"))
                .and_then(|v| v.as_str()),
            Some("REDACTED")
        );
        assert_eq!(
            config
                .get("channels")
                .and_then(|v| v.get("external_plugins"))
                .and_then(|v| v.as_array())
                .and_then(|entries| entries.first())
                .and_then(|entry| entry.get("auth_token"))
                .and_then(|v| v.as_str()),
            Some("REDACTED")
        );
    }
}
