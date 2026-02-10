use crate::config::AutomationConfig;
use anyhow::Result;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use cron::Schedule;
use horizons_core::events::models::{Event, EventDirection};
use horizons_core::events::traits::EventBus;
use horizons_core::models::{OrgId, ProjectDbHandle, ProjectId};
use horizons_core::onboard::traits::{ProjectDb, ProjectDbParam, ProjectDbValue};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationTrigger {
    Heartbeat,
    Interval {
        interval_seconds: u64,
    },
    Cron {
        expression: String,
    },
    Poll {
        source: String,
        interval_seconds: u64,
    },
    Hook {
        source: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationAction {
    LogMessage { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationJob {
    pub job_id: String,
    pub name: String,
    pub trigger: AutomationTrigger,
    pub action: AutomationAction,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub run_count: u64,
    pub failure_count: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateAutomationJobInput {
    pub name: String,
    pub trigger: AutomationTrigger,
    #[serde(default = "default_create_enabled")]
    pub enabled: bool,
    pub action: AutomationAction,
}

fn default_create_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateAutomationJobInput {
    pub name: Option<String>,
    pub trigger: Option<AutomationTrigger>,
    pub enabled: Option<bool>,
    pub action: Option<AutomationAction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationJobRunReceipt {
    pub job_id: String,
    pub triggered_by: String,
    pub run_at: DateTime<Utc>,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct AutomationState {
    heartbeat_ticks: u64,
    last_heartbeat_at: Option<DateTime<Utc>>,
    scheduler_ticks: u64,
    scheduler_runs: u64,
    scheduler_failures: u64,
    webhook_events: u64,
    webhook_duplicate_events: u64,
    last_webhook_source: Option<String>,
    last_webhook_at: Option<DateTime<Utc>>,
    last_webhook_payload_bytes: usize,
    poll_events: u64,
    poll_duplicate_events: u64,
    last_poll_source: Option<String>,
    last_poll_at: Option<DateTime<Utc>>,
    last_poll_payload_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationStatusSnapshot {
    pub enabled: bool,
    pub heartbeat_interval_seconds: u64,
    pub heartbeat_ticks: u64,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub scheduler_ticks: u64,
    pub scheduler_runs: u64,
    pub scheduler_failures: u64,
    pub webhook_events: u64,
    pub webhook_duplicate_events: u64,
    pub last_webhook_source: Option<String>,
    pub last_webhook_at: Option<DateTime<Utc>>,
    pub last_webhook_payload_bytes: usize,
    pub poll_events: u64,
    pub poll_duplicate_events: u64,
    pub last_poll_source: Option<String>,
    pub last_poll_at: Option<DateTime<Utc>>,
    pub last_poll_payload_bytes: usize,
    pub job_count: usize,
    pub enabled_job_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookReceipt {
    pub accepted: bool,
    pub source: String,
    pub event_id: Option<String>,
    pub duplicate_event: bool,
    pub occurred_at: Option<DateTime<Utc>>,
    pub metadata: Option<serde_json::Value>,
    pub received_at: DateTime<Utc>,
    pub payload_bytes: usize,
    pub matched_jobs: usize,
    pub executed_jobs: usize,
    pub run_receipts: Vec<AutomationJobRunReceipt>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PollReceipt {
    pub accepted: bool,
    pub source: String,
    pub event_id: Option<String>,
    pub duplicate_event: bool,
    pub occurred_at: Option<DateTime<Utc>>,
    pub metadata: Option<serde_json::Value>,
    pub received_at: DateTime<Utc>,
    pub payload_bytes: usize,
    pub matched_jobs: usize,
    pub due_jobs: usize,
    pub executed_jobs: usize,
    pub run_receipts: Vec<AutomationJobRunReceipt>,
}

pub struct AutomationRuntime {
    cfg: AutomationConfig,
    state: Arc<RwLock<AutomationState>>,
    jobs: Arc<RwLock<HashMap<String, AutomationJob>>>,
    project_db: Arc<dyn ProjectDb>,
    event_bus: Arc<dyn EventBus>,
    org_id: OrgId,
    project_id: ProjectId,
    project_db_handle: ProjectDbHandle,
    shutdown: CancellationToken,
    background_tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

#[derive(Clone)]
struct AutomationEventContext {
    event_bus: Arc<dyn EventBus>,
    org_id: OrgId,
    project_id: ProjectId,
}

#[derive(Debug, Clone)]
struct IngestRegistration {
    event_id: Option<String>,
    duplicate_event: bool,
    payload_sha256: String,
}

impl AutomationRuntime {
    pub async fn load_or_new(
        cfg: AutomationConfig,
        project_db: Arc<dyn ProjectDb>,
        event_bus: Arc<dyn EventBus>,
        org_id: OrgId,
        project_id: ProjectId,
        project_db_handle: ProjectDbHandle,
    ) -> Result<Self> {
        let runtime = Self {
            cfg,
            state: Arc::new(RwLock::new(AutomationState::default())),
            jobs: Arc::new(RwLock::new(HashMap::new())),
            project_db,
            event_bus,
            org_id,
            project_id,
            project_db_handle,
            shutdown: CancellationToken::new(),
            background_tasks: Arc::new(Mutex::new(Vec::new())),
        };
        runtime.ensure_schema().await?;
        runtime.load_jobs().await?;
        runtime.spawn_heartbeat_loop().await;
        runtime.spawn_scheduler_loop().await;
        Ok(runtime)
    }

    fn event_context(&self) -> AutomationEventContext {
        AutomationEventContext {
            event_bus: self.event_bus.clone(),
            org_id: self.org_id,
            project_id: self.project_id,
        }
    }

    pub async fn shutdown(&self) {
        self.shutdown.cancel();
        let handles = {
            let mut guard = self.background_tasks.lock().await;
            std::mem::take(&mut *guard)
        };
        for handle in handles {
            match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "automation background task join failed");
                }
                Err(_) => {
                    tracing::warn!("timed out waiting for automation background task shutdown");
                }
            }
        }
    }

    pub async fn status_snapshot(&self) -> AutomationStatusSnapshot {
        let state = self.state.read().await;
        let jobs = self.jobs.read().await;
        let job_count = jobs.len();
        let enabled_job_count = jobs.values().filter(|j| j.enabled).count();
        AutomationStatusSnapshot {
            enabled: self.cfg.enabled,
            heartbeat_interval_seconds: self.cfg.heartbeat_interval_seconds,
            heartbeat_ticks: state.heartbeat_ticks,
            last_heartbeat_at: state.last_heartbeat_at,
            scheduler_ticks: state.scheduler_ticks,
            scheduler_runs: state.scheduler_runs,
            scheduler_failures: state.scheduler_failures,
            webhook_events: state.webhook_events,
            webhook_duplicate_events: state.webhook_duplicate_events,
            last_webhook_source: state.last_webhook_source.clone(),
            last_webhook_at: state.last_webhook_at,
            last_webhook_payload_bytes: state.last_webhook_payload_bytes,
            poll_events: state.poll_events,
            poll_duplicate_events: state.poll_duplicate_events,
            last_poll_source: state.last_poll_source.clone(),
            last_poll_at: state.last_poll_at,
            last_poll_payload_bytes: state.last_poll_payload_bytes,
            job_count,
            enabled_job_count,
        }
    }

    pub async fn list_jobs(&self) -> Vec<AutomationJob> {
        let jobs = self.jobs.read().await;
        let mut out = jobs.values().cloned().collect::<Vec<_>>();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub async fn create_job(&self, input: CreateAutomationJobInput) -> Result<AutomationJob> {
        if !self.cfg.enabled {
            return Err(anyhow::anyhow!(
                "automation runtime is disabled; enable [automation].enabled to manage jobs"
            ));
        }
        validate_job_fields(&input.name, &input.trigger, &input.action)?;
        let now = Utc::now();
        let next_run_at = if input.enabled {
            compute_next_run_at(&input.trigger, now, self.cfg.heartbeat_interval_seconds)?
        } else {
            None
        };
        let job = AutomationJob {
            job_id: Uuid::new_v4().to_string(),
            name: input.name.trim().to_string(),
            trigger: input.trigger,
            action: input.action,
            enabled: input.enabled,
            created_at: now,
            updated_at: now,
            last_run_at: None,
            next_run_at,
            run_count: 0,
            failure_count: 0,
            last_error: None,
        };

        {
            let mut jobs = self.jobs.write().await;
            jobs.insert(job.job_id.clone(), job.clone());
        }
        self.persist_job(&job).await?;
        Ok(job)
    }

    pub async fn update_job(
        &self,
        job_id: &str,
        input: UpdateAutomationJobInput,
    ) -> Result<AutomationJob> {
        if !self.cfg.enabled {
            return Err(anyhow::anyhow!(
                "automation runtime is disabled; enable [automation].enabled to manage jobs"
            ));
        }

        let now = Utc::now();
        let updated = {
            let mut jobs = self.jobs.write().await;
            let job = jobs
                .get_mut(job_id)
                .ok_or_else(|| anyhow::anyhow!("automation job not found: {job_id}"))?;

            if let Some(name) = input.name.as_ref() {
                let trimmed = name.trim();
                if trimmed.is_empty() {
                    return Err(anyhow::anyhow!("job name must not be empty"));
                }
                job.name = trimmed.to_string();
            }
            if let Some(trigger) = input.trigger.as_ref() {
                job.trigger = trigger.clone();
            }
            if let Some(action) = input.action.as_ref() {
                job.action = action.clone();
            }
            if let Some(enabled) = input.enabled {
                job.enabled = enabled;
            }

            validate_job_fields(&job.name, &job.trigger, &job.action)?;
            job.next_run_at = if job.enabled {
                compute_next_run_at(&job.trigger, now, self.cfg.heartbeat_interval_seconds)?
            } else {
                None
            };
            job.updated_at = now;
            job.clone()
        };

        self.persist_job(&updated).await?;
        Ok(updated)
    }

    pub async fn get_job(&self, job_id: &str) -> Option<AutomationJob> {
        self.jobs.read().await.get(job_id).cloned()
    }

    pub async fn delete_job(&self, job_id: &str) -> Result<bool> {
        let removed = {
            let mut jobs = self.jobs.write().await;
            jobs.remove(job_id).is_some()
        };
        if !removed {
            return Ok(false);
        }
        self.project_db
            .execute(
                self.org_id,
                &self.project_db_handle,
                "DELETE FROM opencraw_automation_jobs WHERE job_id = ?1",
                &[ProjectDbParam::String(job_id.to_string())],
            )
            .await?;
        Ok(true)
    }

    pub async fn run_job_now(
        &self,
        job_id: &str,
        triggered_by: &str,
        payload: Option<&serde_json::Value>,
    ) -> Result<AutomationJobRunReceipt> {
        let now = Utc::now();
        let mut success = true;
        let mut error = None;

        let updated_job = {
            let mut jobs = self.jobs.write().await;
            let job = jobs
                .get_mut(job_id)
                .ok_or_else(|| anyhow::anyhow!("automation job not found: {job_id}"))?;
            if let Err(e) = execute_job_action(job, triggered_by, payload) {
                success = false;
                error = Some(e.to_string());
                job.failure_count = job.failure_count.saturating_add(1);
                job.last_error = Some(e.to_string());
            } else {
                job.run_count = job.run_count.saturating_add(1);
                job.last_error = None;
            }
            job.last_run_at = Some(now);
            job.updated_at = now;
            job.next_run_at =
                compute_next_run_at(&job.trigger, now, self.cfg.heartbeat_interval_seconds)?;
            job.clone()
        };

        self.persist_job(&updated_job).await?;

        {
            let mut state = self.state.write().await;
            if success {
                state.scheduler_runs = state.scheduler_runs.saturating_add(1);
            } else {
                state.scheduler_failures = state.scheduler_failures.saturating_add(1);
            }
        }
        let event_context = self.event_context();
        publish_automation_event(
            &event_context,
            EventDirection::Outbound,
            "os.automation.job.executed",
            "opencraw.automation.runtime",
            serde_json::json!({
                "job_id": job_id,
                "triggered_by": triggered_by,
                "success": success,
                "error": error.clone(),
            }),
            format!("job:{}:{}:{}", job_id, triggered_by, now.timestamp_millis()),
        )
        .await;

        Ok(AutomationJobRunReceipt {
            job_id: job_id.to_string(),
            triggered_by: triggered_by.to_string(),
            run_at: now,
            success,
            error,
        })
    }

    pub async fn ingest_webhook(
        &self,
        source: &str,
        payload: &serde_json::Value,
        provided_secret: Option<&str>,
        provided_event_id: Option<&str>,
        metadata: Option<&serde_json::Value>,
        occurred_at: Option<DateTime<Utc>>,
    ) -> Result<WebhookReceipt> {
        if !self.cfg.enabled {
            return Err(anyhow::anyhow!(
                "automation runtime is disabled; enable [automation].enabled to ingest webhooks"
            ));
        }
        let trimmed_source = source.trim();
        if trimmed_source.is_empty() {
            return Err(anyhow::anyhow!("webhook source must not be empty"));
        }
        self.validate_ingest_secret(provided_secret)?;

        let received_at = Utc::now();
        let payload_encoded = serde_json::to_vec(payload)
            .map_err(|e| anyhow::anyhow!("failed to serialize webhook payload: {e}"))?;
        let payload_bytes = payload_encoded.len();
        let metadata_owned = metadata.cloned();
        let ingest_registration = self
            .register_ingest_event(
                "webhook",
                trimmed_source,
                provided_event_id,
                &payload_encoded,
                received_at,
            )
            .await?;

        let matching_job_ids = {
            let jobs = self.jobs.read().await;
            jobs.values()
                .filter(|job| {
                    job.enabled
                        && matches!(
                            &job.trigger,
                            AutomationTrigger::Hook { source } if source.eq_ignore_ascii_case(trimmed_source)
                        )
                })
                .map(|job| job.job_id.clone())
                .collect::<Vec<_>>()
        };

        let mut run_receipts = Vec::new();
        if !ingest_registration.duplicate_event {
            for job_id in &matching_job_ids {
                match self
                    .run_job_now(job_id, &format!("webhook:{trimmed_source}"), Some(payload))
                    .await
                {
                    Ok(receipt) => run_receipts.push(receipt),
                    Err(e) => {
                        run_receipts.push(AutomationJobRunReceipt {
                            job_id: job_id.clone(),
                            triggered_by: format!("webhook:{trimmed_source}"),
                            run_at: Utc::now(),
                            success: false,
                            error: Some(e.to_string()),
                        });
                    }
                }
            }
        }

        {
            let mut state = self.state.write().await;
            state.webhook_events = state.webhook_events.saturating_add(1);
            if ingest_registration.duplicate_event {
                state.webhook_duplicate_events = state.webhook_duplicate_events.saturating_add(1);
            }
            state.last_webhook_source = Some(trimmed_source.to_string());
            state.last_webhook_at = Some(received_at);
            state.last_webhook_payload_bytes = payload_bytes;
        }
        let receipt_event_id = ingest_registration.event_id.clone();
        let payload_sha256 = ingest_registration.payload_sha256.clone();
        let event_context = self.event_context();
        publish_automation_event(
            &event_context,
            EventDirection::Inbound,
            "os.automation.webhook.received",
            "opencraw.automation.webhook",
            serde_json::json!({
                "source": trimmed_source,
                "event_id": receipt_event_id.clone(),
                "duplicate_event": ingest_registration.duplicate_event,
                "occurred_at": occurred_at.clone(),
                "metadata": metadata_owned.clone(),
                "payload_bytes": payload_bytes,
                "payload_sha256": payload_sha256.clone(),
                "matched_jobs": matching_job_ids.len(),
                "executed_jobs": run_receipts.iter().filter(|r| r.success).count(),
            }),
            format!(
                "webhook:{}:{}:{}",
                trimmed_source,
                receipt_event_id.as_deref().unwrap_or("no-event-id"),
                payload_sha256
            ),
        )
        .await;

        let executed_jobs = run_receipts.iter().filter(|r| r.success).count();
        Ok(WebhookReceipt {
            accepted: true,
            source: trimmed_source.to_string(),
            event_id: receipt_event_id,
            duplicate_event: ingest_registration.duplicate_event,
            occurred_at,
            metadata: metadata_owned,
            received_at,
            payload_bytes,
            matched_jobs: matching_job_ids.len(),
            executed_jobs,
            run_receipts,
        })
    }

    pub async fn ingest_poll(
        &self,
        source: &str,
        payload: &serde_json::Value,
        provided_secret: Option<&str>,
        provided_event_id: Option<&str>,
        metadata: Option<&serde_json::Value>,
        occurred_at: Option<DateTime<Utc>>,
    ) -> Result<PollReceipt> {
        if !self.cfg.enabled {
            return Err(anyhow::anyhow!(
                "automation runtime is disabled; enable [automation].enabled to ingest poll events"
            ));
        }
        let trimmed_source = source.trim();
        if trimmed_source.is_empty() {
            return Err(anyhow::anyhow!("poll source must not be empty"));
        }
        self.validate_ingest_secret(provided_secret)?;

        let received_at = Utc::now();
        let payload_encoded = serde_json::to_vec(payload)
            .map_err(|e| anyhow::anyhow!("failed to serialize poll payload: {e}"))?;
        let payload_bytes = payload_encoded.len();
        let metadata_owned = metadata.cloned();
        let ingest_registration = self
            .register_ingest_event(
                "poll",
                trimmed_source,
                provided_event_id,
                &payload_encoded,
                received_at,
            )
            .await?;

        let (matching_job_ids, due_job_ids) = {
            let jobs = self.jobs.read().await;
            let mut matching_job_ids = Vec::new();
            let mut due_job_ids = Vec::new();
            for job in jobs.values() {
                if !job.enabled {
                    continue;
                }
                let AutomationTrigger::Poll {
                    source,
                    interval_seconds,
                } = &job.trigger
                else {
                    continue;
                };
                if !source.eq_ignore_ascii_case(trimmed_source) {
                    continue;
                }

                matching_job_ids.push(job.job_id.clone());
                let min_interval = (*interval_seconds).max(1) as i64;
                let due = match job.last_run_at {
                    None => true,
                    Some(last_run_at) => {
                        let next_due = last_run_at + ChronoDuration::seconds(min_interval);
                        next_due <= received_at
                    }
                };
                if due {
                    due_job_ids.push(job.job_id.clone());
                }
            }
            (matching_job_ids, due_job_ids)
        };

        let mut run_receipts = Vec::new();
        if !ingest_registration.duplicate_event {
            for job_id in &due_job_ids {
                match self
                    .run_job_now(job_id, &format!("poll:{trimmed_source}"), Some(payload))
                    .await
                {
                    Ok(receipt) => run_receipts.push(receipt),
                    Err(e) => {
                        run_receipts.push(AutomationJobRunReceipt {
                            job_id: job_id.clone(),
                            triggered_by: format!("poll:{trimmed_source}"),
                            run_at: Utc::now(),
                            success: false,
                            error: Some(e.to_string()),
                        });
                    }
                }
            }
        }

        {
            let mut state = self.state.write().await;
            state.poll_events = state.poll_events.saturating_add(1);
            if ingest_registration.duplicate_event {
                state.poll_duplicate_events = state.poll_duplicate_events.saturating_add(1);
            }
            state.last_poll_source = Some(trimmed_source.to_string());
            state.last_poll_at = Some(received_at);
            state.last_poll_payload_bytes = payload_bytes;
        }
        let receipt_event_id = ingest_registration.event_id.clone();
        let payload_sha256 = ingest_registration.payload_sha256.clone();
        let event_context = self.event_context();
        publish_automation_event(
            &event_context,
            EventDirection::Inbound,
            "os.automation.poll.received",
            "opencraw.automation.poll",
            serde_json::json!({
                "source": trimmed_source,
                "event_id": receipt_event_id.clone(),
                "duplicate_event": ingest_registration.duplicate_event,
                "occurred_at": occurred_at.clone(),
                "metadata": metadata_owned.clone(),
                "payload_bytes": payload_bytes,
                "payload_sha256": payload_sha256.clone(),
                "matched_jobs": matching_job_ids.len(),
                "due_jobs": due_job_ids.len(),
                "executed_jobs": run_receipts.iter().filter(|r| r.success).count(),
            }),
            format!(
                "poll:{}:{}:{}",
                trimmed_source,
                receipt_event_id.as_deref().unwrap_or("no-event-id"),
                payload_sha256
            ),
        )
        .await;

        let executed_jobs = run_receipts.iter().filter(|r| r.success).count();
        Ok(PollReceipt {
            accepted: true,
            source: trimmed_source.to_string(),
            event_id: receipt_event_id,
            duplicate_event: ingest_registration.duplicate_event,
            occurred_at,
            metadata: metadata_owned,
            received_at,
            payload_bytes,
            matched_jobs: matching_job_ids.len(),
            due_jobs: due_job_ids.len(),
            executed_jobs,
            run_receipts,
        })
    }

    async fn register_ingest_event(
        &self,
        ingest_kind: &str,
        source: &str,
        provided_event_id: Option<&str>,
        payload_bytes: &[u8],
        received_at: DateTime<Utc>,
    ) -> Result<IngestRegistration> {
        let payload_sha256 = bytes_to_hex(&Sha256::digest(payload_bytes));
        let event_id = provided_event_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string());
        let Some(event_id_value) = event_id.clone() else {
            return Ok(IngestRegistration {
                event_id: None,
                duplicate_event: false,
                payload_sha256,
            });
        };

        let normalized_source = source.trim().to_ascii_lowercase();
        let existing = self
            .project_db
            .query(
                self.org_id,
                &self.project_db_handle,
                r#"
SELECT ingest_id
FROM opencraw_automation_ingest_events
WHERE ingest_kind = ?1 AND source = ?2 AND event_id = ?3
LIMIT 1
"#,
                &[
                    ProjectDbParam::String(ingest_kind.to_string()),
                    ProjectDbParam::String(normalized_source.clone()),
                    ProjectDbParam::String(event_id_value.clone()),
                ],
            )
            .await?;
        if !existing.is_empty() {
            return Ok(IngestRegistration {
                event_id: Some(event_id_value),
                duplicate_event: true,
                payload_sha256,
            });
        }

        let ingest_id = Uuid::new_v4().to_string();
        let insert_result = self
            .project_db
            .execute(
                self.org_id,
                &self.project_db_handle,
                r#"
INSERT INTO opencraw_automation_ingest_events (
    ingest_id,
    ingest_kind,
    source,
    event_id,
    payload_sha256,
    received_at
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6)
"#,
                &[
                    ProjectDbParam::String(ingest_id),
                    ProjectDbParam::String(ingest_kind.to_string()),
                    ProjectDbParam::String(normalized_source),
                    ProjectDbParam::String(event_id_value.clone()),
                    ProjectDbParam::String(payload_sha256.clone()),
                    ProjectDbParam::String(received_at.to_rfc3339()),
                ],
            )
            .await;

        match insert_result {
            Ok(_) => Ok(IngestRegistration {
                event_id: Some(event_id_value),
                duplicate_event: false,
                payload_sha256,
            }),
            Err(e) => {
                let message = format!("{e:#}");
                if message.contains("UNIQUE constraint failed")
                    || message.contains("unique constraint failed")
                {
                    Ok(IngestRegistration {
                        event_id: Some(event_id_value),
                        duplicate_event: true,
                        payload_sha256,
                    })
                } else {
                    Err(e.into())
                }
            }
        }
    }

    async fn ensure_schema(&self) -> Result<()> {
        self.project_db
            .execute(
                self.org_id,
                &self.project_db_handle,
                r#"
CREATE TABLE IF NOT EXISTS opencraw_automation_jobs (
    job_id TEXT PRIMARY KEY,
    job_json TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
)
"#,
                &[],
            )
            .await?;
        self.project_db
            .execute(
                self.org_id,
                &self.project_db_handle,
                r#"
CREATE TABLE IF NOT EXISTS opencraw_automation_ingest_events (
    ingest_id TEXT PRIMARY KEY,
    ingest_kind TEXT NOT NULL,
    source TEXT NOT NULL,
    event_id TEXT NOT NULL,
    payload_sha256 TEXT NOT NULL,
    received_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
)
"#,
                &[],
            )
            .await?;
        self.project_db
            .execute(
                self.org_id,
                &self.project_db_handle,
                r#"
CREATE UNIQUE INDEX IF NOT EXISTS idx_opencraw_automation_ingest_events_kind_source_event
ON opencraw_automation_ingest_events (ingest_kind, source, event_id)
"#,
                &[],
            )
            .await?;
        Ok(())
    }

    async fn load_jobs(&self) -> Result<()> {
        let rows = self
            .project_db
            .query(
                self.org_id,
                &self.project_db_handle,
                "SELECT job_id, job_json FROM opencraw_automation_jobs",
                &[],
            )
            .await?;
        let mut jobs_map = HashMap::new();
        for row in rows {
            let job_id = row_required_string(&row, "job_id")?;
            let job_json = row_required_string(&row, "job_json")?;
            let job: AutomationJob = serde_json::from_str(&job_json)?;
            jobs_map.insert(job_id, job);
        }
        *self.jobs.write().await = jobs_map;
        Ok(())
    }

    async fn persist_job(&self, job: &AutomationJob) -> Result<()> {
        let job_json = serde_json::to_string(job)?;
        self.project_db
            .execute(
                self.org_id,
                &self.project_db_handle,
                r#"
INSERT INTO opencraw_automation_jobs (job_id, job_json, updated_at)
VALUES (?1, ?2, CURRENT_TIMESTAMP)
ON CONFLICT(job_id) DO UPDATE
SET job_json = excluded.job_json,
    updated_at = CURRENT_TIMESTAMP
"#,
                &[
                    ProjectDbParam::String(job.job_id.clone()),
                    ProjectDbParam::String(job_json),
                ],
            )
            .await?;
        Ok(())
    }

    async fn spawn_heartbeat_loop(&self) {
        if !self.cfg.enabled {
            return;
        }
        let interval_seconds = self.cfg.heartbeat_interval_seconds.max(1);
        let state = self.state.clone();
        let shutdown = self.shutdown.child_token();
        let handle = tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(interval_seconds));
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        tracing::info!("automation heartbeat loop received shutdown signal");
                        break;
                    }
                    _ = interval.tick() => {
                        let now = Utc::now();
                        {
                            let mut guard = state.write().await;
                            guard.heartbeat_ticks = guard.heartbeat_ticks.saturating_add(1);
                            guard.last_heartbeat_at = Some(now);
                        }
                        tracing::info!(
                            heartbeat_interval_seconds = interval_seconds,
                            heartbeat_at = %now,
                            "automation heartbeat tick"
                        );
                    }
                }
            }
        });
        self.background_tasks.lock().await.push(handle);
    }

    async fn spawn_scheduler_loop(&self) {
        if !self.cfg.enabled {
            return;
        }
        let cfg = self.cfg.clone();
        let state = self.state.clone();
        let jobs = self.jobs.clone();
        let project_db = self.project_db.clone();
        let event_context = self.event_context();
        let org_id = self.org_id;
        let handle = self.project_db_handle.clone();
        let shutdown = self.shutdown.child_token();

        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        tracing::info!("automation scheduler loop received shutdown signal");
                        break;
                    }
                    _ = interval.tick() => {}
                }
                let now = Utc::now();
                let mut jobs_to_persist = Vec::new();
                let mut scheduler_failures = 0_u64;
                {
                    let mut jobs_guard = jobs.write().await;
                    for job in jobs_guard.values_mut() {
                        if !job.enabled {
                            continue;
                        }
                        if !is_time_based_trigger(&job.trigger) {
                            continue;
                        }
                        let Some(next_run) = job.next_run_at else {
                            continue;
                        };
                        if next_run > now {
                            continue;
                        }

                        let run_result = execute_job_action(job, "scheduler", None);
                        match run_result {
                            Ok(()) => {
                                job.run_count = job.run_count.saturating_add(1);
                                job.last_error = None;
                            }
                            Err(e) => {
                                job.failure_count = job.failure_count.saturating_add(1);
                                job.last_error = Some(e.to_string());
                                scheduler_failures = scheduler_failures.saturating_add(1);
                            }
                        }
                        job.last_run_at = Some(now);
                        job.updated_at = now;
                        match compute_next_run_at(&job.trigger, now, cfg.heartbeat_interval_seconds)
                        {
                            Ok(next) => {
                                job.next_run_at = next;
                            }
                            Err(e) => {
                                job.failure_count = job.failure_count.saturating_add(1);
                                job.last_error = Some(e.to_string());
                                job.next_run_at = None;
                                scheduler_failures = scheduler_failures.saturating_add(1);
                            }
                        }
                        jobs_to_persist.push(job.clone());
                    }
                }

                let executed_count = jobs_to_persist.len() as u64;
                for job in jobs_to_persist {
                    if let Err(e) =
                        persist_job_shared(project_db.clone(), org_id, handle.clone(), job.clone())
                            .await
                    {
                        tracing::error!(job_id = %job.job_id, error = %e, "failed to persist automation job update");
                    }
                    publish_automation_event(
                        &event_context,
                        EventDirection::Outbound,
                        "os.automation.job.scheduler_tick",
                        "opencraw.automation.scheduler",
                        serde_json::json!({
                            "job_id": job.job_id.clone(),
                            "run_count": job.run_count,
                            "failure_count": job.failure_count,
                            "last_error": job.last_error.clone(),
                        }),
                        format!("scheduler:{}:{}", job.job_id, now.timestamp_millis()),
                    )
                    .await;
                }

                {
                    let mut s = state.write().await;
                    s.scheduler_ticks = s.scheduler_ticks.saturating_add(1);
                    s.scheduler_runs = s.scheduler_runs.saturating_add(executed_count);
                    s.scheduler_failures = s.scheduler_failures.saturating_add(scheduler_failures);
                }
            }
        });
        self.background_tasks.lock().await.push(task);
    }
}

impl AutomationRuntime {
    fn validate_ingest_secret(&self, provided_secret: Option<&str>) -> Result<()> {
        if let Some(expected_secret) = self.cfg.webhook_secret.as_deref() {
            let provided = provided_secret
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| anyhow::anyhow!("missing webhook secret"))?;
            if provided != expected_secret {
                return Err(anyhow::anyhow!("invalid webhook secret"));
            }
        }
        Ok(())
    }
}

fn validate_job_fields(
    name: &str,
    trigger: &AutomationTrigger,
    action: &AutomationAction,
) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow::anyhow!("job name must not be empty"));
    }
    match trigger {
        AutomationTrigger::Heartbeat => {}
        AutomationTrigger::Interval { interval_seconds } => {
            if *interval_seconds == 0 {
                return Err(anyhow::anyhow!("interval_seconds must be > 0"));
            }
        }
        AutomationTrigger::Cron { expression } => {
            if expression.trim().is_empty() {
                return Err(anyhow::anyhow!("cron expression must not be empty"));
            }
            Schedule::from_str(expression)
                .map_err(|e| anyhow::anyhow!("invalid cron expression: {e}"))?;
        }
        AutomationTrigger::Poll {
            source,
            interval_seconds,
        } => {
            if source.trim().is_empty() {
                return Err(anyhow::anyhow!("poll source must not be empty"));
            }
            if *interval_seconds == 0 {
                return Err(anyhow::anyhow!("poll interval_seconds must be > 0"));
            }
        }
        AutomationTrigger::Hook { source } => {
            if source.trim().is_empty() {
                return Err(anyhow::anyhow!("hook source must not be empty"));
            }
        }
    }
    match action {
        AutomationAction::LogMessage { message } => {
            if message.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "log_message action requires non-empty message"
                ));
            }
        }
    }
    Ok(())
}

fn is_time_based_trigger(trigger: &AutomationTrigger) -> bool {
    matches!(
        trigger,
        AutomationTrigger::Heartbeat
            | AutomationTrigger::Interval { .. }
            | AutomationTrigger::Cron { .. }
    )
}

fn compute_next_run_at(
    trigger: &AutomationTrigger,
    from: DateTime<Utc>,
    heartbeat_interval_seconds: u64,
) -> Result<Option<DateTime<Utc>>> {
    match trigger {
        AutomationTrigger::Heartbeat => Ok(Some(
            from + ChronoDuration::seconds(heartbeat_interval_seconds.max(1) as i64),
        )),
        AutomationTrigger::Interval { interval_seconds } => Ok(Some(
            from + ChronoDuration::seconds((*interval_seconds).max(1) as i64),
        )),
        AutomationTrigger::Cron { expression } => {
            let schedule = Schedule::from_str(expression)
                .map_err(|e| anyhow::anyhow!("invalid cron expression: {e}"))?;
            Ok(schedule.after(&from).next())
        }
        AutomationTrigger::Poll {
            interval_seconds, ..
        } => Ok(Some(
            from + ChronoDuration::seconds((*interval_seconds).max(1) as i64),
        )),
        AutomationTrigger::Hook { .. } => Ok(None),
    }
}

fn execute_job_action(
    job: &AutomationJob,
    triggered_by: &str,
    payload: Option<&serde_json::Value>,
) -> Result<()> {
    match &job.action {
        AutomationAction::LogMessage { message } => {
            tracing::info!(
                job_id = %job.job_id,
                job_name = %job.name,
                triggered_by = triggered_by,
                payload_present = payload.is_some(),
                message = %message,
                "automation job action executed"
            );
        }
    }
    Ok(())
}

async fn persist_job_shared(
    project_db: Arc<dyn ProjectDb>,
    org_id: OrgId,
    project_db_handle: ProjectDbHandle,
    job: AutomationJob,
) -> Result<()> {
    let job_json = serde_json::to_string(&job)?;
    project_db
        .execute(
            org_id,
            &project_db_handle,
            r#"
INSERT INTO opencraw_automation_jobs (job_id, job_json, updated_at)
VALUES (?1, ?2, CURRENT_TIMESTAMP)
ON CONFLICT(job_id) DO UPDATE
SET job_json = excluded.job_json,
    updated_at = CURRENT_TIMESTAMP
"#,
            &[
                ProjectDbParam::String(job.job_id.clone()),
                ProjectDbParam::String(job_json),
            ],
        )
        .await?;
    Ok(())
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(nibble_to_hex(byte >> 4));
        out.push(nibble_to_hex(byte & 0x0f));
    }
    out
}

fn nibble_to_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => '0',
    }
}

fn row_required_string(
    row: &std::collections::BTreeMap<String, ProjectDbValue>,
    key: &str,
) -> Result<String> {
    let value = row
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("automation row missing required key: {key}"))?;
    match value {
        ProjectDbValue::String(v) => Ok(v.clone()),
        other => Err(anyhow::anyhow!(
            "automation row key {key} expected string but received {other:?}"
        )),
    }
}

async fn publish_automation_event(
    context: &AutomationEventContext,
    direction: EventDirection,
    topic: &str,
    source: &str,
    payload: serde_json::Value,
    dedupe_key: String,
) {
    let event = match Event::new(
        context.org_id.to_string(),
        Some(context.project_id.to_string()),
        direction,
        topic,
        source,
        payload,
        dedupe_key,
        serde_json::json!({}),
        None,
    ) {
        Ok(event) => event,
        Err(e) => {
            tracing::warn!(error = %e, topic, source, "failed to build automation event");
            return;
        }
    };
    if let Err(e) = context.event_bus.publish(event).await {
        tracing::warn!(error = %e, topic, source, "failed to publish automation event");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizons_core::models::ProjectId;
    use horizons_rs::dev_backends::{DevEventBus, DevProjectDb};
    use serde_json::json;
    use std::sync::Arc;

    async fn new_runtime(cfg: AutomationConfig) -> AutomationRuntime {
        let root = std::env::temp_dir().join(format!("opencraw-automation-{}", Uuid::new_v4()));
        let project_db = Arc::new(
            DevProjectDb::new(root.join("project_dbs"))
                .await
                .expect("new dev project db"),
        );
        let org_id = OrgId(Uuid::new_v4());
        let project_id = ProjectId(Uuid::new_v4());
        let event_bus = Arc::new(DevEventBus::new());
        let handle = project_db
            .provision(org_id, project_id)
            .await
            .expect("provision project db");
        AutomationRuntime::load_or_new(cfg, project_db, event_bus, org_id, project_id, handle)
            .await
            .expect("load automation runtime")
    }

    #[tokio::test]
    async fn poll_ingest_respects_interval_gate() {
        let runtime = new_runtime(AutomationConfig {
            enabled: true,
            heartbeat_interval_seconds: 300,
            webhook_secret: None,
        })
        .await;
        let job = runtime
            .create_job(CreateAutomationJobInput {
                name: "poll-github".to_string(),
                trigger: AutomationTrigger::Poll {
                    source: "github".to_string(),
                    interval_seconds: 3600,
                },
                enabled: true,
                action: AutomationAction::LogMessage {
                    message: "poll run".to_string(),
                },
            })
            .await
            .expect("create poll job");

        let first = runtime
            .ingest_poll("github", &json!({ "items": [1] }), None, None, None, None)
            .await
            .expect("first poll ingest");
        assert_eq!(first.matched_jobs, 1);
        assert_eq!(first.due_jobs, 1);
        assert_eq!(first.executed_jobs, 1);

        let second = runtime
            .ingest_poll("github", &json!({ "items": [2] }), None, None, None, None)
            .await
            .expect("second poll ingest");
        assert_eq!(second.matched_jobs, 1);
        assert_eq!(second.due_jobs, 0);
        assert_eq!(second.executed_jobs, 0);

        let updated = runtime.get_job(&job.job_id).await.expect("job exists");
        assert_eq!(updated.run_count, 1);
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn poll_ingest_requires_secret_when_configured() {
        let runtime = new_runtime(AutomationConfig {
            enabled: true,
            heartbeat_interval_seconds: 300,
            webhook_secret: Some("topsecret".to_string()),
        })
        .await;
        let err = runtime
            .ingest_poll("github", &json!({"ok": true}), None, None, None, None)
            .await
            .expect_err("poll ingest without secret should fail");
        assert!(err.to_string().contains("missing webhook secret"));
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn webhook_ingest_dedupes_replayed_event_id() {
        let runtime = new_runtime(AutomationConfig {
            enabled: true,
            heartbeat_interval_seconds: 300,
            webhook_secret: None,
        })
        .await;
        let job = runtime
            .create_job(CreateAutomationJobInput {
                name: "hook-github".to_string(),
                trigger: AutomationTrigger::Hook {
                    source: "github".to_string(),
                },
                enabled: true,
                action: AutomationAction::LogMessage {
                    message: "hook run".to_string(),
                },
            })
            .await
            .expect("create hook job");

        let first = runtime
            .ingest_webhook(
                "github",
                &json!({ "event": "push", "id": 1 }),
                None,
                Some("evt-123"),
                None,
                None,
            )
            .await
            .expect("first webhook ingest");
        assert!(!first.duplicate_event);
        assert_eq!(first.executed_jobs, 1);

        let second = runtime
            .ingest_webhook(
                "github",
                &json!({ "event": "push", "id": 1 }),
                None,
                Some("evt-123"),
                None,
                None,
            )
            .await
            .expect("second webhook ingest");
        assert!(second.duplicate_event);
        assert_eq!(second.executed_jobs, 0);

        let updated = runtime.get_job(&job.job_id).await.expect("job exists");
        assert_eq!(updated.run_count, 1);
        let status = runtime.status_snapshot().await;
        assert_eq!(status.webhook_duplicate_events, 1);
        runtime.shutdown().await;
    }
}
