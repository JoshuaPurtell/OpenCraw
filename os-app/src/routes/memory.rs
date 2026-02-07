use crate::server::OsState;
use axum::routing::post;
use axum::{Extension, Json};
use horizons_core::memory::traits::RetrievalQuery;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemorySearchRequest {
    channel_id: String,
    sender_id: String,
    query: String,
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemorySummarizeRequest {
    channel_id: String,
    sender_id: String,
    horizon: String,
}

pub fn router() -> axum::Router {
    axum::Router::new()
        .route("/api/v1/os/memory/search", post(search_memory))
        .route("/api/v1/os/memory/summarize", post(summarize_memory))
}

#[tracing::instrument(level = "info", skip_all)]
async fn search_memory(
    Extension(state): Extension<Arc<OsState>>,
    Json(req): Json<MemorySearchRequest>,
) -> Json<serde_json::Value> {
    let Some(memory) = state.memory.as_ref() else {
        return Json(serde_json::json!({ "status": "error", "error": "memory disabled" }));
    };

    let (channel_id, sender_id) = match validate_scope_identifiers(&req.channel_id, &req.sender_id)
    {
        Ok(v) => v,
        Err(e) => return Json(serde_json::json!({ "status": "error", "error": e.to_string() })),
    };
    let query = match validate_non_empty("query", &req.query) {
        Ok(v) => v,
        Err(e) => return Json(serde_json::json!({ "status": "error", "error": e.to_string() })),
    };
    if !(1..=50).contains(&req.limit) {
        return Json(serde_json::json!({
            "status": "error",
            "error": "limit must be between 1 and 50"
        }));
    }

    let agent_id = agent_scope_id(&channel_id, &sender_id);
    let items = match memory
        .retrieve(
            state.org_id,
            &agent_id,
            RetrievalQuery::new(query.clone(), req.limit),
        )
        .await
    {
        Ok(items) => items,
        Err(e) => {
            return Json(serde_json::json!({ "status": "error", "error": e.to_string() }));
        }
    };

    Json(serde_json::json!({
        "status": "ok",
        "agent_id": agent_id,
        "query": query,
        "limit": req.limit,
        "items": items
    }))
}

#[tracing::instrument(level = "info", skip_all)]
async fn summarize_memory(
    Extension(state): Extension<Arc<OsState>>,
    Json(req): Json<MemorySummarizeRequest>,
) -> Json<serde_json::Value> {
    let Some(memory) = state.memory.as_ref() else {
        return Json(serde_json::json!({ "status": "error", "error": "memory disabled" }));
    };

    let (channel_id, sender_id) = match validate_scope_identifiers(&req.channel_id, &req.sender_id)
    {
        Ok(v) => v,
        Err(e) => return Json(serde_json::json!({ "status": "error", "error": e.to_string() })),
    };
    let horizon = match validate_non_empty("horizon", &req.horizon) {
        Ok(v) => v,
        Err(e) => return Json(serde_json::json!({ "status": "error", "error": e.to_string() })),
    };

    let agent_id = agent_scope_id(&channel_id, &sender_id);
    let summary = match memory.summarize(state.org_id, &agent_id, &horizon).await {
        Ok(summary) => summary,
        Err(e) => {
            return Json(serde_json::json!({ "status": "error", "error": e.to_string() }));
        }
    };

    Json(serde_json::json!({
        "status": "ok",
        "agent_id": agent_id,
        "horizon": horizon,
        "summary": summary
    }))
}

fn agent_scope_id(channel_id: &str, sender_id: &str) -> String {
    format!("os.assistant.{channel_id}.{sender_id}")
}

fn validate_scope_identifiers(
    channel_id: &str,
    sender_id: &str,
) -> anyhow::Result<(String, String)> {
    let channel_id = validate_non_empty("channel_id", channel_id)?;
    let sender_id = validate_non_empty("sender_id", sender_id)?;
    Ok((channel_id, sender_id))
}

fn validate_non_empty(field: &str, value: &str) -> anyhow::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow::anyhow!("{field} must not be empty"));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::{agent_scope_id, validate_non_empty, validate_scope_identifiers};

    #[test]
    fn scope_is_deterministic() {
        assert_eq!(
            agent_scope_id("webchat", "user-1"),
            "os.assistant.webchat.user-1"
        );
    }

    #[test]
    fn validate_non_empty_rejects_blank() {
        let err = validate_non_empty("query", "  ").expect_err("blank must fail");
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn validate_scope_identifiers_accepts_trimmed_values() {
        let (channel_id, sender_id) =
            validate_scope_identifiers(" webchat ", " user-1 ").expect("scope should validate");
        assert_eq!(channel_id, "webchat");
        assert_eq!(sender_id, "user-1");
    }
}
