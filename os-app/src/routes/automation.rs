use crate::automation_runtime::{CreateAutomationJobInput, UpdateAutomationJobInput};
use crate::server::OsState;
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::sync::Arc;

pub fn router() -> axum::Router {
    axum::Router::new()
        .route("/api/v1/os/automation/status", get(status))
        .route(
            "/api/v1/os/automation/jobs",
            get(list_jobs).post(create_job),
        )
        .route(
            "/api/v1/os/automation/jobs/{job_id}",
            get(get_job).patch(update_job).delete(delete_job),
        )
        .route("/api/v1/os/automation/jobs/{job_id}/run", post(run_job))
        .route(
            "/api/v1/os/automation/webhook/{source}",
            post(webhook_ingest),
        )
        .route("/api/v1/os/automation/poll/{source}", post(poll_ingest))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunAutomationJobRequest {
    #[serde(default)]
    triggered_by: Option<String>,
    #[serde(default)]
    payload: Option<serde_json::Value>,
}

const INGEST_ENVELOPE_SCHEMA: &str = "opencraw_ingest_envelope_v1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IngestEnvelopeRequest {
    schema: String,
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    occurred_at: Option<DateTime<Utc>>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum IngestRequestBody {
    Envelope(IngestEnvelopeRequest),
    Raw(serde_json::Value),
}

#[tracing::instrument(level = "debug", skip_all)]
async fn status(Extension(state): Extension<Arc<OsState>>) -> Json<serde_json::Value> {
    let snapshot = state.automation.status_snapshot().await;
    Json(serde_json::json!({
        "status": "ok",
        "automation": snapshot
    }))
}

#[tracing::instrument(level = "debug", skip_all)]
async fn list_jobs(Extension(state): Extension<Arc<OsState>>) -> Json<serde_json::Value> {
    let jobs = state.automation.list_jobs().await;
    Json(serde_json::json!({
        "status": "ok",
        "jobs": jobs,
    }))
}

#[tracing::instrument(level = "info", skip_all)]
async fn create_job(
    Extension(state): Extension<Arc<OsState>>,
    Json(input): Json<CreateAutomationJobInput>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.automation.create_job(input).await {
        Ok(job) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "status": "created",
                "job": job
            })),
        ),
        Err(e) => (
            automation_error_status(&e.to_string()),
            Json(serde_json::json!({
                "status": "error",
                "error": e.to_string()
            })),
        ),
    }
}

#[tracing::instrument(level = "debug", skip_all)]
async fn get_job(
    Path(job_id): Path<String>,
    Extension(state): Extension<Arc<OsState>>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.automation.get_job(&job_id).await {
        Some(job) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ok",
                "job": job
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "not_found",
                "error": format!("automation job not found: {job_id}")
            })),
        ),
    }
}

#[tracing::instrument(level = "info", skip_all)]
async fn update_job(
    Path(job_id): Path<String>,
    Extension(state): Extension<Arc<OsState>>,
    Json(input): Json<UpdateAutomationJobInput>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.automation.update_job(&job_id, input).await {
        Ok(job) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "updated",
                "job": job
            })),
        ),
        Err(e) => (
            automation_error_status(&e.to_string()),
            Json(serde_json::json!({
                "status": "error",
                "error": e.to_string()
            })),
        ),
    }
}

#[tracing::instrument(level = "info", skip_all)]
async fn delete_job(
    Path(job_id): Path<String>,
    Extension(state): Extension<Arc<OsState>>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.automation.delete_job(&job_id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "deleted",
                "job_id": job_id
            })),
        ),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "not_found",
                "error": format!("automation job not found: {job_id}")
            })),
        ),
        Err(e) => (
            automation_error_status(&e.to_string()),
            Json(serde_json::json!({
                "status": "error",
                "error": e.to_string()
            })),
        ),
    }
}

#[tracing::instrument(level = "info", skip_all)]
async fn run_job(
    Path(job_id): Path<String>,
    Extension(state): Extension<Arc<OsState>>,
    Json(req): Json<RunAutomationJobRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let triggered_by = req
        .triggered_by
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("api");
    match state
        .automation
        .run_job_now(&job_id, triggered_by, req.payload.as_ref())
        .await
    {
        Ok(receipt) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "executed",
                "receipt": receipt
            })),
        ),
        Err(e) => (
            automation_error_status(&e.to_string()),
            Json(serde_json::json!({
                "status": "error",
                "error": e.to_string()
            })),
        ),
    }
}

#[tracing::instrument(level = "info", skip_all)]
async fn webhook_ingest(
    Path(source): Path<String>,
    headers: HeaderMap,
    Extension(state): Extension<Arc<OsState>>,
    Json(body): Json<IngestRequestBody>,
) -> (StatusCode, Json<serde_json::Value>) {
    let provided_secret = ingest_secret_from_headers(&headers);
    let header_event_id = ingest_event_id_from_headers(&headers);
    let ingest_contract = match parse_ingest_contract(body, header_event_id) {
        Ok(contract) => contract,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "status": "error",
                    "error": e
                })),
            );
        }
    };

    match state
        .automation
        .ingest_webhook(
            &source,
            &ingest_contract.payload,
            provided_secret,
            ingest_contract.event_id.as_deref(),
            ingest_contract.metadata.as_ref(),
            ingest_contract.occurred_at,
        )
        .await
    {
        Ok(receipt) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "accepted",
                "receipt": receipt
            })),
        ),
        Err(e) => {
            let status = if e.to_string().contains("secret") {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::BAD_REQUEST
            };
            (
                status,
                Json(serde_json::json!({
                    "status": "error",
                    "error": e.to_string()
                })),
            )
        }
    }
}

#[tracing::instrument(level = "info", skip_all)]
async fn poll_ingest(
    Path(source): Path<String>,
    headers: HeaderMap,
    Extension(state): Extension<Arc<OsState>>,
    Json(body): Json<IngestRequestBody>,
) -> (StatusCode, Json<serde_json::Value>) {
    let provided_secret = ingest_secret_from_headers(&headers);
    let header_event_id = ingest_event_id_from_headers(&headers);
    let ingest_contract = match parse_ingest_contract(body, header_event_id) {
        Ok(contract) => contract,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "status": "error",
                    "error": e
                })),
            );
        }
    };
    match state
        .automation
        .ingest_poll(
            &source,
            &ingest_contract.payload,
            provided_secret,
            ingest_contract.event_id.as_deref(),
            ingest_contract.metadata.as_ref(),
            ingest_contract.occurred_at,
        )
        .await
    {
        Ok(receipt) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "accepted",
                "receipt": receipt
            })),
        ),
        Err(e) => {
            let status = if e.to_string().contains("secret") {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::BAD_REQUEST
            };
            (
                status,
                Json(serde_json::json!({
                    "status": "error",
                    "error": e.to_string()
                })),
            )
        }
    }
}

fn ingest_secret_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-opencraw-webhook-secret")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn ingest_event_id_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-opencraw-event-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

#[derive(Debug)]
struct IngestContract {
    event_id: Option<String>,
    occurred_at: Option<DateTime<Utc>>,
    metadata: Option<serde_json::Value>,
    payload: serde_json::Value,
}

fn parse_ingest_contract(
    body: IngestRequestBody,
    header_event_id: Option<&str>,
) -> Result<IngestContract, String> {
    match body {
        IngestRequestBody::Raw(payload) => Ok(IngestContract {
            event_id: header_event_id.map(|v| v.to_string()),
            occurred_at: None,
            metadata: None,
            payload,
        }),
        IngestRequestBody::Envelope(envelope) => {
            let schema = envelope.schema.trim();
            if schema != INGEST_ENVELOPE_SCHEMA {
                return Err(format!(
                    "invalid ingest envelope schema {:?}; expected {:?}",
                    schema, INGEST_ENVELOPE_SCHEMA
                ));
            }
            let body_event_id = envelope
                .event_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToOwned::to_owned);
            if let (Some(header_id), Some(body_id)) = (header_event_id, body_event_id.as_deref()) {
                if header_id != body_id {
                    return Err("ingest event_id mismatch between header and envelope".to_string());
                }
            }
            Ok(IngestContract {
                event_id: header_event_id.map(|v| v.to_string()).or(body_event_id),
                occurred_at: envelope.occurred_at,
                metadata: envelope.metadata,
                payload: envelope.payload,
            })
        }
    }
}

fn automation_error_status(error: &str) -> StatusCode {
    if error.contains("missing webhook secret")
        || error.contains("invalid webhook secret")
        || error.contains("missing bearer token")
        || error.contains("invalid bearer token")
    {
        return StatusCode::UNAUTHORIZED;
    }
    if error.contains("not found") {
        return StatusCode::NOT_FOUND;
    }
    if error.contains("disabled")
        || error.contains("invalid")
        || error.contains("must")
        || error.contains("missing")
    {
        return StatusCode::BAD_REQUEST;
    }
    StatusCode::INTERNAL_SERVER_ERROR
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_raw_ingest_body_uses_header_event_id() {
        let contract = parse_ingest_contract(
            IngestRequestBody::Raw(json!({"hello": "world"})),
            Some("evt-123"),
        )
        .expect("parse raw ingest");
        assert_eq!(contract.event_id.as_deref(), Some("evt-123"));
        assert_eq!(contract.payload, json!({"hello": "world"}));
        assert!(contract.metadata.is_none());
    }

    #[test]
    fn parse_envelope_rejects_event_id_mismatch() {
        let body = IngestRequestBody::Envelope(IngestEnvelopeRequest {
            schema: INGEST_ENVELOPE_SCHEMA.to_string(),
            event_id: Some("evt-body".to_string()),
            occurred_at: None,
            metadata: None,
            payload: json!({"ok": true}),
        });
        let err =
            parse_ingest_contract(body, Some("evt-header")).expect_err("mismatch should fail");
        assert!(err.contains("event_id mismatch"));
    }
}
