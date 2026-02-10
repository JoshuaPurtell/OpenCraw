use axum::Json;
use axum::routing::get;

pub fn router() -> axum::Router {
    axum::Router::new().route("/api/v1/os/health", get(get_health))
}

#[tracing::instrument(level = "debug", skip_all)]
async fn get_health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}
