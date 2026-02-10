use crate::server::OsState;
use crate::skills_runtime::{InstallSkillInput, SkillPolicyDecision};
use axum::extract::Path;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Extension, Json};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApproveSkillRequest {
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RevokeSkillRequest {
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScansQuery {
    #[serde(default = "default_scans_limit")]
    limit: usize,
}

fn default_scans_limit() -> usize {
    50
}

pub fn router() -> axum::Router {
    axum::Router::new()
        .route("/api/v1/os/skills", get(list_skills))
        .route("/api/v1/os/skills/{skill_id}", get(get_skill))
        .route("/api/v1/os/skills/install", post(install_skill))
        .route("/api/v1/os/skills/{skill_id}/approve", post(approve_skill))
        .route("/api/v1/os/skills/{skill_id}/revoke", post(revoke_skill))
        .route("/api/v1/os/skills/{skill_id}/rescan", post(rescan_skill))
        .route("/api/v1/os/skills/{skill_id}/scans", get(list_skill_scans))
        .route("/api/v1/os/skills/search", get(search_skills))
}

#[tracing::instrument(level = "debug", skip_all)]
async fn list_skills(Extension(state): Extension<Arc<OsState>>) -> Json<serde_json::Value> {
    let skills = state.skills.list().await;
    Json(serde_json::json!({
        "status": "ok",
        "skills": skills
    }))
}

#[tracing::instrument(level = "info", skip_all)]
async fn install_skill(
    Extension(state): Extension<Arc<OsState>>,
    Json(req): Json<InstallSkillInput>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.skills.install(req).await {
        Ok(record) => {
            let (status, label) = match record.decision {
                SkillPolicyDecision::Approve => (StatusCode::CREATED, "approved"),
                SkillPolicyDecision::Warn => (StatusCode::CREATED, "warn"),
                SkillPolicyDecision::Block => (StatusCode::FORBIDDEN, "blocked"),
            };
            (
                status,
                Json(serde_json::json!({
                    "status": label,
                    "skill": record
                })),
            )
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "status": "error", "error": e.to_string() })),
        ),
    }
}

#[tracing::instrument(level = "debug", skip_all)]
async fn get_skill(
    Path(skill_id): Path<String>,
    Extension(state): Extension<Arc<OsState>>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.skills.get(&skill_id).await {
        Some(skill) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ok",
                "skill": skill
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "not_found",
                "error": format!("skill not found: {skill_id}")
            })),
        ),
    }
}

#[tracing::instrument(level = "info", skip_all)]
async fn approve_skill(
    Path(skill_id): Path<String>,
    Extension(state): Extension<Arc<OsState>>,
    Json(req): Json<ApproveSkillRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.skills.approve(&skill_id, req.note).await {
        Ok(skill) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "approved",
                "skill": skill
            })),
        ),
        Err(e) => (
            skills_error_status(&e.to_string()),
            Json(serde_json::json!({
                "status": "error",
                "error": e.to_string()
            })),
        ),
    }
}

#[tracing::instrument(level = "info", skip_all)]
async fn rescan_skill(
    Path(skill_id): Path<String>,
    Extension(state): Extension<Arc<OsState>>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.skills.rescan(&skill_id).await {
        Ok(skill) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": match skill.decision {
                    SkillPolicyDecision::Approve => "approved",
                    SkillPolicyDecision::Warn => "warn",
                    SkillPolicyDecision::Block => "blocked",
                },
                "skill": skill
            })),
        ),
        Err(e) => (
            skills_error_status(&e.to_string()),
            Json(serde_json::json!({
                "status": "error",
                "error": e.to_string()
            })),
        ),
    }
}

#[tracing::instrument(level = "info", skip_all)]
async fn revoke_skill(
    Path(skill_id): Path<String>,
    Extension(state): Extension<Arc<OsState>>,
    Json(req): Json<RevokeSkillRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.skills.revoke(&skill_id, req.note).await {
        Ok(skill) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "revoked",
                "skill": skill
            })),
        ),
        Err(e) => (
            skills_error_status(&e.to_string()),
            Json(serde_json::json!({
                "status": "error",
                "error": e.to_string()
            })),
        ),
    }
}

#[tracing::instrument(level = "debug", skip_all)]
async fn list_skill_scans(
    Path(skill_id): Path<String>,
    Query(q): Query<ScansQuery>,
    Extension(state): Extension<Arc<OsState>>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.skills.list_scans(&skill_id, q.limit).await {
        Ok(scans) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ok",
                "scans": scans
            })),
        ),
        Err(e) => (
            skills_error_status(&e.to_string()),
            Json(serde_json::json!({
                "status": "error",
                "error": e.to_string()
            })),
        ),
    }
}

#[tracing::instrument(level = "debug", skip_all)]
async fn search_skills(
    Extension(state): Extension<Arc<OsState>>,
    Query(q): Query<SearchQuery>,
) -> Json<serde_json::Value> {
    let items = state.skills.search(&q.q).await;

    Json(serde_json::json!({
        "status": "ok",
        "skills": items
    }))
}

fn skills_error_status(error: &str) -> StatusCode {
    if error.contains("not found") {
        return StatusCode::NOT_FOUND;
    }
    if error.contains("blocked skill") {
        return StatusCode::FORBIDDEN;
    }
    if error.contains("blocked")
        || error.contains("missing")
        || error.contains("invalid")
        || error.contains("must")
    {
        return StatusCode::BAD_REQUEST;
    }
    StatusCode::INTERNAL_SERVER_ERROR
}
