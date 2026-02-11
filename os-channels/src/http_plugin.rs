use crate::traits::ChannelAdapter;
use crate::types::{InboundMessage, InboundMessageKind, OutboundMessage};
use anyhow::{Result, anyhow};
use chrono::Utc;
use reqwest::Url;
use serde::Deserialize;
use std::collections::{HashSet, VecDeque};
use std::time::Duration;
use tokio::sync::mpsc;

const RECENT_EVENT_ID_CAPACITY: usize = 4096;

#[derive(Clone)]
pub struct HttpPluginAdapter {
    http: reqwest::Client,
    channel_id: String,
    send_url: String,
    poll_url: Option<String>,
    auth_token: Option<String>,
    poll_interval: Duration,
    start_from_latest: bool,
    supports_streaming_deltas: bool,
    supports_typing_events: bool,
    supports_reactions: bool,
}

impl HttpPluginAdapter {
    pub fn new(channel_id: &str, send_url: &str) -> Result<Self> {
        let channel_id = normalize_plugin_channel_id(channel_id)?;
        let send_url = normalize_http_url(send_url, "send_url")?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            channel_id,
            send_url,
            poll_url: None,
            auth_token: None,
            poll_interval: Duration::from_millis(3000),
            start_from_latest: true,
            supports_streaming_deltas: false,
            supports_typing_events: false,
            supports_reactions: false,
        })
    }

    pub fn with_poll_url(mut self, poll_url: Option<String>) -> Result<Self> {
        self.poll_url = match poll_url {
            Some(url) if !url.trim().is_empty() => Some(normalize_http_url(&url, "poll_url")?),
            _ => None,
        };
        Ok(self)
    }

    pub fn with_auth_token(mut self, auth_token: Option<String>) -> Self {
        self.auth_token = auth_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(ToOwned::to_owned);
        self
    }

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    pub fn with_start_from_latest(mut self, start_from_latest: bool) -> Self {
        self.start_from_latest = start_from_latest;
        self
    }

    pub fn with_capabilities(
        mut self,
        supports_streaming_deltas: bool,
        supports_typing_events: bool,
        supports_reactions: bool,
    ) -> Self {
        self.supports_streaming_deltas = supports_streaming_deltas;
        self.supports_typing_events = supports_typing_events;
        self.supports_reactions = supports_reactions;
        self
    }

    fn authorized_request(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.auth_token.as_deref() {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for HttpPluginAdapter {
    fn channel_id(&self) -> &str {
        &self.channel_id
    }

    async fn start(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        if self.poll_url.is_none() {
            return Ok(());
        }

        let adapter = self.clone();
        tokio::spawn(async move {
            if let Err(error) = adapter.run_poll_loop(tx).await {
                tracing::error!(channel_id = %adapter.channel_id, %error, "http plugin poll loop exited");
            }
        });
        Ok(())
    }

    async fn send(&self, recipient_id: &str, message: OutboundMessage) -> Result<()> {
        let recipient_id = recipient_id.trim();
        if recipient_id.is_empty() {
            return Err(anyhow!("recipient_id is required"));
        }
        let content = message.content.trim();
        if content.is_empty() {
            return Err(anyhow!("message content is empty"));
        }

        let payload = serde_json::json!({
            "channel_id": self.channel_id,
            "recipient_id": recipient_id,
            "message": {
                "content": content,
                "reply_to_message_id": message.reply_to_message_id,
                "attachments": message.attachments,
            }
        });
        let url = Url::parse(&self.send_url).map_err(|e| {
            anyhow!(
                "invalid plugin send_url for channel {}: {e}",
                self.channel_id
            )
        })?;
        let response = self
            .authorized_request(self.http.post(url))
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(anyhow!(
                "http plugin send failed for channel {}: status={} body={}",
                self.channel_id,
                status,
                body
            ));
        }
        Ok(())
    }

    fn supports_streaming_deltas(&self) -> bool {
        self.supports_streaming_deltas
    }

    fn supports_typing_events(&self) -> bool {
        self.supports_typing_events
    }

    fn supports_reactions(&self) -> bool {
        self.supports_reactions
    }
}

impl HttpPluginAdapter {
    async fn run_poll_loop(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let mut last_timestamp_millis: Option<i64> = None;
        let mut recent_event_ids = VecDeque::<String>::new();
        let mut recent_event_id_set = HashSet::<String>::new();

        if self.start_from_latest {
            let events = self.poll_once().await?;
            for (index, event) in events.iter().enumerate() {
                let Some(normalized) = normalize_inbound_event(&self.channel_id, event, index)
                else {
                    continue;
                };
                if let Some(timestamp) = normalized.timestamp_millis {
                    match last_timestamp_millis {
                        Some(current) if current >= timestamp => {}
                        _ => last_timestamp_millis = Some(timestamp),
                    }
                }
                remember_event_id(
                    normalized.event_id,
                    &mut recent_event_ids,
                    &mut recent_event_id_set,
                    RECENT_EVENT_ID_CAPACITY,
                );
            }
            tracing::info!(
                channel_id = %self.channel_id,
                seed_cursor = ?last_timestamp_millis,
                seeded_ids = recent_event_ids.len(),
                "http plugin seeded poll cursor"
            );
        }

        loop {
            let events = self.poll_once().await?;
            let mut emitted = 0usize;
            let mut newest = last_timestamp_millis;

            for (index, event) in events.into_iter().enumerate() {
                let Some(normalized) = normalize_inbound_event(&self.channel_id, &event, index)
                else {
                    continue;
                };

                if recent_event_id_set.contains(&normalized.event_id) {
                    continue;
                }

                if let Some(timestamp) = normalized.timestamp_millis {
                    if last_timestamp_millis.is_some_and(|cursor| timestamp <= cursor) {
                        remember_event_id(
                            normalized.event_id,
                            &mut recent_event_ids,
                            &mut recent_event_id_set,
                            RECENT_EVENT_ID_CAPACITY,
                        );
                        continue;
                    }
                    match newest {
                        Some(current) if current >= timestamp => {}
                        _ => newest = Some(timestamp),
                    }
                }

                let event_id = normalized.event_id.clone();
                let inbound = normalized.into_inbound_message();
                tx.send(inbound)
                    .await
                    .map_err(|e| anyhow!("http plugin inbound queue closed: {e}"))?;
                emitted += 1;
                remember_event_id(
                    event_id,
                    &mut recent_event_ids,
                    &mut recent_event_id_set,
                    RECENT_EVENT_ID_CAPACITY,
                );
            }

            last_timestamp_millis = newest;
            tracing::info!(
                channel_id = %self.channel_id,
                emitted,
                cursor = ?last_timestamp_millis,
                "http plugin poll cycle complete"
            );
            tokio::time::sleep(self.poll_interval).await;
        }
    }

    async fn poll_once(&self) -> Result<Vec<HttpPluginInboundEnvelope>> {
        let Some(poll_url) = self.poll_url.as_deref() else {
            return Ok(Vec::new());
        };
        let response = self
            .authorized_request(self.http.get(poll_url))
            .send()
            .await?;
        let status = response.status();
        let body: serde_json::Value = response.json().await?;
        if !status.is_success() {
            return Err(anyhow!(
                "http plugin poll failed for channel {}: status={} body={}",
                self.channel_id,
                status,
                body
            ));
        }
        Ok(extract_poll_events(body))
    }
}

fn normalize_plugin_channel_id(raw: &str) -> Result<String> {
    let channel_id = raw.trim().to_ascii_lowercase();
    if channel_id.is_empty() {
        return Err(anyhow!("plugin channel id is required"));
    }
    if !channel_id
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        return Err(anyhow!(
            "invalid plugin channel id {:?}: use [a-z0-9_-]+",
            raw
        ));
    }
    Ok(channel_id)
}

fn normalize_http_url(raw: &str, field: &str) -> Result<String> {
    let normalized = raw.trim().to_string();
    if normalized.is_empty() {
        return Err(anyhow!("{field} is required"));
    }
    let parsed = Url::parse(&normalized).map_err(|e| anyhow!("invalid {field}: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(normalized),
        other => Err(anyhow!("invalid {field} scheme: {other}")),
    }
}

fn extract_poll_events(body: serde_json::Value) -> Vec<HttpPluginInboundEnvelope> {
    match body {
        serde_json::Value::Array(events) => events
            .into_iter()
            .filter_map(|event| serde_json::from_value(event).ok())
            .collect(),
        serde_json::Value::Object(mut obj) => {
            if let Some(events) = obj
                .remove("events")
                .and_then(|value| value.as_array().cloned())
            {
                return events
                    .into_iter()
                    .filter_map(|event| serde_json::from_value(event).ok())
                    .collect();
            }
            serde_json::from_value(serde_json::Value::Object(obj))
                .ok()
                .into_iter()
                .collect()
        }
        _ => Vec::new(),
    }
}

#[derive(Debug)]
struct NormalizedInboundEvent {
    event_id: String,
    kind: InboundMessageKind,
    sender_id: String,
    thread_id: Option<String>,
    is_group: bool,
    content: String,
    metadata: serde_json::Value,
    timestamp_millis: Option<i64>,
    channel_id: String,
}

impl NormalizedInboundEvent {
    fn into_inbound_message(self) -> InboundMessage {
        InboundMessage {
            kind: self.kind,
            message_id: self.event_id.into(),
            channel_id: self.channel_id.into(),
            sender_id: self.sender_id.into(),
            thread_id: self.thread_id.map(Into::into),
            is_group: self.is_group,
            content: self.content,
            metadata: self.metadata,
            received_at: Utc::now(),
        }
    }
}

fn normalize_inbound_event(
    channel_id: &str,
    event: &HttpPluginInboundEnvelope,
    fallback_index: usize,
) -> Option<NormalizedInboundEvent> {
    let sender_id = event
        .sender_id
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if sender_id.is_empty() {
        return None;
    }
    let content = event.content.as_deref().map(str::trim).unwrap_or_default();
    if content.is_empty() {
        return None;
    }
    let timestamp_millis = event.timestamp_ms;
    let thread_id = event
        .thread_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned);
    let kind = match event
        .kind
        .as_deref()
        .unwrap_or("message")
        .to_ascii_lowercase()
        .as_str()
    {
        "reaction" => InboundMessageKind::Reaction,
        _ => InboundMessageKind::Message,
    };
    let fallback_timestamp = timestamp_millis.unwrap_or_else(|| Utc::now().timestamp_millis());
    let event_id = event
        .message_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            format!(
                "{}:{}:{}:{}",
                channel_id, sender_id, fallback_timestamp, fallback_index
            )
        });

    Some(NormalizedInboundEvent {
        event_id,
        kind,
        sender_id: sender_id.to_string(),
        thread_id,
        is_group: event.is_group.unwrap_or(false),
        content: content.to_string(),
        metadata: event.metadata.clone().unwrap_or(serde_json::Value::Null),
        timestamp_millis,
        channel_id: channel_id.to_string(),
    })
}

fn remember_event_id(
    event_id: String,
    order: &mut VecDeque<String>,
    set: &mut HashSet<String>,
    max_capacity: usize,
) {
    if set.insert(event_id.clone()) {
        order.push_back(event_id);
    }
    while order.len() > max_capacity {
        if let Some(evicted) = order.pop_front() {
            set.remove(&evicted);
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct HttpPluginInboundEnvelope {
    message_id: Option<String>,
    kind: Option<String>,
    sender_id: Option<String>,
    thread_id: Option<String>,
    is_group: Option<bool>,
    content: Option<String>,
    metadata: Option<serde_json::Value>,
    timestamp_ms: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::{
        HttpPluginInboundEnvelope, extract_poll_events, normalize_http_url,
        normalize_inbound_event, normalize_plugin_channel_id,
    };

    #[test]
    fn normalize_plugin_channel_id_enforces_identifier_policy() {
        assert_eq!(
            normalize_plugin_channel_id("custom_alerts").expect("valid id"),
            "custom_alerts"
        );
        assert!(normalize_plugin_channel_id("bad id").is_err());
    }

    #[test]
    fn normalize_http_url_requires_http_or_https() {
        assert_eq!(
            normalize_http_url("https://plugins.example.com/send", "send_url")
                .expect("https url should be valid"),
            "https://plugins.example.com/send"
        );
        assert!(normalize_http_url("ftp://plugins.example.com/send", "send_url").is_err());
    }

    #[test]
    fn extract_poll_events_supports_array_and_wrapped_shape() {
        let array_payload = serde_json::json!([
            {
                "message_id": "m1",
                "sender_id": "alice",
                "content": "hi",
                "timestamp_ms": 100
            }
        ]);
        let wrapped_payload = serde_json::json!({
            "events": [
                {
                    "message_id": "m2",
                    "sender_id": "bob",
                    "content": "hello",
                    "timestamp_ms": 101
                }
            ]
        });

        let from_array = extract_poll_events(array_payload);
        let from_wrapped = extract_poll_events(wrapped_payload);
        assert_eq!(from_array.len(), 1);
        assert_eq!(from_wrapped.len(), 1);
        assert_eq!(from_array[0].message_id.as_deref(), Some("m1"));
        assert_eq!(from_wrapped[0].message_id.as_deref(), Some("m2"));
    }

    #[test]
    fn normalize_inbound_event_maps_message_kind_and_defaults() {
        let event = HttpPluginInboundEnvelope {
            sender_id: Some("alice".to_string()),
            content: Some("hello".to_string()),
            timestamp_ms: Some(123),
            ..HttpPluginInboundEnvelope::default()
        };

        let normalized =
            normalize_inbound_event("custom", &event, 0).expect("event should normalize");
        assert_eq!(normalized.event_id, "custom:alice:123:0");
        assert_eq!(normalized.kind, crate::InboundMessageKind::Message);
        assert_eq!(normalized.sender_id, "alice");
    }

    #[test]
    fn normalize_inbound_event_maps_reaction_kind() {
        let event = HttpPluginInboundEnvelope {
            message_id: Some("evt-1".to_string()),
            kind: Some("reaction".to_string()),
            sender_id: Some("alice".to_string()),
            content: Some("🔥".to_string()),
            timestamp_ms: Some(123),
            ..HttpPluginInboundEnvelope::default()
        };

        let normalized =
            normalize_inbound_event("custom", &event, 0).expect("event should normalize");
        assert_eq!(normalized.event_id, "evt-1");
        assert_eq!(normalized.kind, crate::InboundMessageKind::Reaction);
    }
}
