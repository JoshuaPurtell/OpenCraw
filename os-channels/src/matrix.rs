use crate::traits::ChannelAdapter;
use crate::types::{InboundMessage, InboundMessageKind, OutboundMessage};
use anyhow::{Result, anyhow};
use chrono::Utc;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Clone)]
pub struct MatrixAdapter {
    http: reqwest::Client,
    homeserver_url: String,
    access_token: String,
    user_id: String,
    poll_interval: Duration,
    room_ids: Vec<String>,
    start_from_latest: bool,
    sync_timeout_ms: u64,
}

impl MatrixAdapter {
    pub fn new(homeserver_url: &str, access_token: &str, user_id: &str) -> Result<Self> {
        let homeserver_url = normalize_homeserver_url(homeserver_url)?;
        let access_token = access_token.trim();
        if access_token.is_empty() {
            return Err(anyhow!("matrix access token is required"));
        }
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return Err(anyhow!("matrix user id is required"));
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            homeserver_url,
            access_token: access_token.to_string(),
            user_id: user_id.to_string(),
            poll_interval: Duration::from_millis(3000),
            room_ids: Vec::new(),
            start_from_latest: true,
            sync_timeout_ms: 30_000,
        })
    }

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    pub fn with_room_ids(mut self, room_ids: Vec<String>) -> Self {
        let mut deduped = Vec::new();
        for room_id in room_ids {
            let room_id = room_id.trim().to_string();
            if room_id.is_empty() {
                continue;
            }
            if !deduped.iter().any(|existing| existing == &room_id) {
                deduped.push(room_id);
            }
        }
        self.room_ids = deduped;
        self
    }

    pub fn with_start_from_latest(mut self, start_from_latest: bool) -> Self {
        self.start_from_latest = start_from_latest;
        self
    }

    pub fn with_sync_timeout_ms(mut self, sync_timeout_ms: u64) -> Self {
        self.sync_timeout_ms = sync_timeout_ms.max(1);
        self
    }

    fn api_url(&self, path: &str) -> Result<Url> {
        Url::parse(&format!("{}{}", self.homeserver_url, path))
            .map_err(|e| anyhow!("invalid matrix API URL path {path:?}: {e}"))
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for MatrixAdapter {
    fn channel_id(&self) -> &str {
        "matrix"
    }

    async fn start(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        if self.room_ids.is_empty() {
            return Err(anyhow!(
                "matrix adapter requires at least one room id to poll"
            ));
        }
        let adapter = self.clone();
        tokio::spawn(async move {
            if let Err(error) = adapter.run_sync_loop(tx).await {
                tracing::error!(%error, "matrix sync loop exited");
            }
        });
        Ok(())
    }

    async fn send(&self, recipient_id: &str, message: OutboundMessage) -> Result<()> {
        let room_id = recipient_id.trim();
        if room_id.is_empty() {
            return Err(anyhow!("recipient_id (matrix room id) is required"));
        }
        let text = message.content.trim();
        if text.is_empty() {
            return Err(anyhow!("message content is empty"));
        }

        let txn_id = Uuid::new_v4().to_string();
        let url = self.api_url(&format!(
            "/_matrix/client/v3/rooms/{room_id}/send/m.room.message/{txn_id}"
        ))?;
        let mut payload = serde_json::json!({
            "msgtype": "m.text",
            "body": text,
        });
        if let Some(reply_to) = message.reply_to_message_id.as_ref() {
            payload["m.relates_to"] = serde_json::json!({
                "m.in_reply_to": {
                    "event_id": reply_to.as_str(),
                }
            });
        }

        let response = self
            .http
            .put(url)
            .bearer_auth(&self.access_token)
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        let body: MatrixSendResponse = response.json().await?;
        if !status.is_success() {
            return Err(anyhow!(
                "matrix send failed: status={} errcode={} error={}",
                status,
                body.errcode.unwrap_or_else(|| "unknown".to_string()),
                body.error.unwrap_or_else(|| "unknown".to_string())
            ));
        }
        Ok(())
    }

    fn supports_reactions(&self) -> bool {
        true
    }
}

impl MatrixAdapter {
    #[tracing::instrument(level = "info", skip_all)]
    async fn run_sync_loop(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let mut since_token: Option<String> = None;

        if self.start_from_latest {
            let snapshot = self.sync_once(None).await?;
            since_token = Some(snapshot.next_batch);
            tracing::info!("matrix adapter seeded initial sync token");
        }

        loop {
            let sync = self.sync_once(since_token.as_deref()).await?;
            let emitted = self.emit_sync_events(&sync, &tx).await?;
            since_token = Some(sync.next_batch);
            tracing::info!(emitted, "matrix sync cycle complete");
            tokio::time::sleep(self.poll_interval).await;
        }
    }

    async fn sync_once(&self, since: Option<&str>) -> Result<MatrixSyncResponse> {
        let url = self.api_url("/_matrix/client/v3/sync")?;
        let timeout = self.sync_timeout_ms.to_string();
        let mut request = self
            .http
            .get(url)
            .bearer_auth(&self.access_token)
            .query(&[("timeout", timeout.as_str())]);
        if let Some(since) = since {
            request = request.query(&[("since", since)]);
        }

        let response = request.send().await?;
        let status = response.status();
        let body: MatrixSyncResponse = response.json().await?;
        if !status.is_success() {
            return Err(anyhow!(
                "matrix sync failed: status={} next_batch={}",
                status,
                body.next_batch
            ));
        }
        Ok(body)
    }

    async fn emit_sync_events(
        &self,
        sync: &MatrixSyncResponse,
        tx: &mpsc::Sender<InboundMessage>,
    ) -> Result<usize> {
        let configured_rooms = self.room_ids.iter().collect::<HashSet<_>>();
        let mut emitted = 0usize;

        for (room_id, room_state) in &sync.rooms.join {
            if !configured_rooms.contains(room_id) {
                continue;
            }
            for event in &room_state.timeline.events {
                if !should_emit_matrix_event(event, &self.user_id) {
                    continue;
                }
                let Some(event_id) = event.event_id.as_deref() else {
                    continue;
                };
                let Some(sender) = event.sender.as_deref() else {
                    continue;
                };

                if event.event_type == "m.room.message" {
                    let Some(body) = extract_matrix_message_body(&event.content) else {
                        continue;
                    };
                    let inbound = InboundMessage {
                        kind: InboundMessageKind::Message,
                        message_id: event_id.to_string().into(),
                        channel_id: "matrix".into(),
                        sender_id: sender.to_string().into(),
                        thread_id: Some(room_id.clone().into()),
                        is_group: true,
                        content: body,
                        metadata: serde_json::json!({
                            "room_id": room_id,
                            "event": event,
                        }),
                        received_at: Utc::now(),
                    };
                    tx.send(inbound)
                        .await
                        .map_err(|e| anyhow!("matrix inbound queue closed: {e}"))?;
                    emitted += 1;
                    continue;
                }

                if event.event_type == "m.reaction" {
                    let Some(key) = extract_matrix_reaction_key(&event.content) else {
                        continue;
                    };
                    let inbound = InboundMessage {
                        kind: InboundMessageKind::Reaction,
                        message_id: event_id.to_string().into(),
                        channel_id: "matrix".into(),
                        sender_id: sender.to_string().into(),
                        thread_id: Some(room_id.clone().into()),
                        is_group: true,
                        content: key,
                        metadata: serde_json::json!({
                            "room_id": room_id,
                            "event": event,
                        }),
                        received_at: Utc::now(),
                    };
                    tx.send(inbound)
                        .await
                        .map_err(|e| anyhow!("matrix inbound queue closed: {e}"))?;
                    emitted += 1;
                }
            }
        }

        Ok(emitted)
    }
}

fn normalize_homeserver_url(raw: &str) -> Result<String> {
    let normalized = raw.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        return Err(anyhow!("matrix homeserver URL is required"));
    }
    let parsed =
        Url::parse(&normalized).map_err(|e| anyhow!("invalid matrix homeserver URL: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(normalized),
        other => Err(anyhow!(
            "invalid matrix homeserver URL scheme: {other} (expected http or https)"
        )),
    }
}

fn extract_matrix_message_body(content: &serde_json::Value) -> Option<String> {
    let body = content.get("body")?.as_str()?.trim();
    if body.is_empty() {
        return None;
    }
    Some(body.to_string())
}

fn extract_matrix_reaction_key(content: &serde_json::Value) -> Option<String> {
    let key = content.get("m.relates_to")?.get("key")?.as_str()?.trim();
    if key.is_empty() {
        return None;
    }
    Some(key.to_string())
}

fn should_emit_matrix_event(event: &MatrixEvent, self_user_id: &str) -> bool {
    let Some(sender) = event.sender.as_deref() else {
        return false;
    };
    if sender.eq_ignore_ascii_case(self_user_id) {
        return false;
    }
    event.event_type == "m.room.message" || event.event_type == "m.reaction"
}

#[derive(Debug, Deserialize)]
struct MatrixSyncResponse {
    next_batch: String,
    #[serde(default)]
    rooms: MatrixSyncRooms,
}

#[derive(Debug, Default, Deserialize)]
struct MatrixSyncRooms {
    #[serde(default)]
    join: HashMap<String, MatrixSyncJoinedRoom>,
}

#[derive(Debug, Default, Deserialize)]
struct MatrixSyncJoinedRoom {
    #[serde(default)]
    timeline: MatrixSyncTimeline,
}

#[derive(Debug, Default, Deserialize)]
struct MatrixSyncTimeline {
    #[serde(default)]
    events: Vec<MatrixEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MatrixEvent {
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    sender: Option<String>,
    #[serde(rename = "type", default)]
    event_type: String,
    #[serde(default)]
    content: serde_json::Value,
    #[serde(default)]
    origin_server_ts: Option<i64>,
    #[serde(default)]
    unsigned: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct MatrixSendResponse {
    #[serde(default)]
    errcode: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        MatrixEvent, extract_matrix_message_body, extract_matrix_reaction_key,
        normalize_homeserver_url, should_emit_matrix_event,
    };

    #[test]
    fn homeserver_url_requires_http_or_https() {
        assert!(normalize_homeserver_url("https://matrix.org").is_ok());
        assert!(normalize_homeserver_url("http://localhost:8008").is_ok());
        assert!(normalize_homeserver_url("matrix://example").is_err());
    }

    #[test]
    fn extractors_pull_message_body_and_reaction_key() {
        let message = serde_json::json!({ "msgtype": "m.text", "body": "hello world" });
        assert_eq!(
            extract_matrix_message_body(&message).as_deref(),
            Some("hello world")
        );
        let reaction = serde_json::json!({
            "m.relates_to": {
                "event_id": "$abc",
                "rel_type": "m.annotation",
                "key": "🔥"
            }
        });
        assert_eq!(
            extract_matrix_reaction_key(&reaction).as_deref(),
            Some("🔥")
        );
    }

    #[test]
    fn should_emit_filters_self_and_unknown_event_types() {
        let message = MatrixEvent {
            event_id: Some("$event".to_string()),
            sender: Some("@alice:matrix.org".to_string()),
            event_type: "m.room.message".to_string(),
            content: serde_json::json!({ "body": "hello" }),
            origin_server_ts: None,
            unsigned: None,
        };
        assert!(should_emit_matrix_event(&message, "@bot:matrix.org"));
        assert!(!should_emit_matrix_event(&message, "@alice:matrix.org"));

        let presence = MatrixEvent {
            event_type: "m.presence".to_string(),
            ..message
        };
        assert!(!should_emit_matrix_event(&presence, "@bot:matrix.org"));
    }
}
