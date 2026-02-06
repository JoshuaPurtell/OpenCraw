use crate::server::OsState;
use axum::extract::Query;
use axum::routing::{get, post};
use axum::{Extension, Json};
use horizons_core::memory::traits::{MemoryItem, MemoryType, RetrievalQuery, Scope};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct InstallSkillRequest {
    name: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: String,
}

pub fn router() -> axum::Router {
    axum::Router::new()
        .route("/api/v1/os/skills/install", post(install_skill))
        .route("/api/v1/os/skills/search", get(search_skills))
}

#[tracing::instrument(level = "info", skip_all)]
async fn install_skill(
    Extension(state): Extension<Arc<OsState>>,
    Json(req): Json<InstallSkillRequest>,
) -> Json<serde_json::Value> {
    let Some(mem) = state.memory.as_ref() else {
        return Json(serde_json::json!({ "status": "error", "error": "memory disabled" }));
    };

    let scope = Scope::new(state.org_id.to_string(), "os.skills".to_string());
    let content = serde_json::json!({
        "name": req.name,
        "description": req.description,
    });

    let item = MemoryItem::new(&scope, MemoryType::skill(), content, chrono::Utc::now())
        .with_importance(1.0)
        .with_index_text("skill".to_string());
    let _ = mem.append_item(state.org_id, item).await;

    Json(serde_json::json!({ "status": "ok" }))
}

#[tracing::instrument(level = "debug", skip_all)]
async fn search_skills(
    Extension(state): Extension<Arc<OsState>>,
    Query(q): Query<SearchQuery>,
) -> Json<serde_json::Value> {
    let Some(mem) = state.memory.as_ref() else {
        return Json(serde_json::json!({ "status": "error", "error": "memory disabled" }));
    };

    let mut query = RetrievalQuery::new(q.q, 10);
    query.type_filter = Some(vec![MemoryType::skill()]);

    let items = mem
        .retrieve(state.org_id, "os.skills", query)
        .await
        .unwrap_or_default();

    Json(serde_json::json!({ "skills": items }))
}
