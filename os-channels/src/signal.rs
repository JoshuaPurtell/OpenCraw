use crate::traits::ChannelAdapter;
use crate::types::{InboundMessage, InboundMessageKind, OutboundMessage};
use anyhow::{Result, anyhow};
use chrono::Utc;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct SignalAdapter {
    http: reqwest::Client,
    api_base_url: String,
    account: String,
    api_token: Option<String>,
    poll_interval: Duration,
    start_from_latest: bool,
    receive_timeout_seconds: u64,
}

impl SignalAdapter {
    pub fn new(api_base_url: &str, account: &str) -> Result<Self> {
        let api_base_url = normalize_signal_api_base_url(api_base_url)?;
        let account = account.trim();
        if account.is_empty() {
            return Err(anyhow!("signal account is required"));
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            api_base_url,
            account: account.to_string(),
            api_token: None,
            poll_interval: Duration::from_millis(3000),
            start_from_latest: true,
            receive_timeout_seconds: 5,
        })
    }

    pub fn with_api_token(mut self, api_token: Option<String>) -> Self {
        self.api_token = api_token
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

    pub fn with_receive_timeout_seconds(mut self, receive_timeout_seconds: u64) -> Self {
        self.receive_timeout_seconds = receive_timeout_seconds.max(1);
        self
    }

    fn api_url(&self, path: &str) -> Result<Url> {
        Url::parse(&format!("{}{}", self.api_base_url, path))
            .map_err(|e| anyhow!("invalid signal API URL path {path:?}: {e}"))
    }

    fn authorized_request(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.api_token.as_deref() {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for SignalAdapter {
    fn channel_id(&self) -> &str {
        "signal"
    }

    async fn start(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let adapter = self.clone();
        tokio::spawn(async move {
            if let Err(error) = adapter.run_poll_loop(tx).await {
                tracing::error!(%error, "signal poll loop exited");
            }
        });
        Ok(())
    }

    async fn send(&self, recipient_id: &str, message: OutboundMessage) -> Result<()> {
        let recipient_id = recipient_id.trim();
        if recipient_id.is_empty() {
            return Err(anyhow!(
                "recipient_id (signal phone number or group:<group_id>) is required"
            ));
        }
        let text = message.content.trim();
        if text.is_empty() {
            return Err(anyhow!("message content is empty"));
        }

        let url = self.api_url("/v2/send")?;
        let mut payload = serde_json::json!({
            "number": self.account,
            "message": text,
        });
        if let Some(group_id) = recipient_id
            .strip_prefix("group:")
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            payload["groupId"] = serde_json::json!(group_id);
        } else {
            payload["recipients"] = serde_json::json!([recipient_id]);
        }

        let response = self
            .authorized_request(self.http.post(url))
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(anyhow!(
                "signal send failed: status={} body={}",
                status,
                body
            ));
        }
        Ok(())
    }

    fn supports_reactions(&self) -> bool {
        true
    }
}

impl SignalAdapter {
    #[tracing::instrument(level = "info", skip_all)]
    async fn run_poll_loop(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let mut cursor_millis: Option<i64> = None;

        if self.start_from_latest {
            let seed = self.receive_once().await?;
            cursor_millis = seed.iter().filter_map(envelope_timestamp_millis).max();
            tracing::info!(cursor_millis = ?cursor_millis, "signal adapter seeded cursor");
        }

        loop {
            let envelopes = self.receive_once().await?;
            let mut newest_seen = cursor_millis;
            let mut emitted = 0usize;

            for envelope in envelopes {
                let timestamp_millis = envelope_timestamp_millis(&envelope)
                    .unwrap_or_else(|| Utc::now().timestamp_millis());

                if cursor_millis.is_some_and(|cursor| timestamp_millis <= cursor) {
                    continue;
                }

                if let Some(inbound) = convert_signal_envelope(&envelope, timestamp_millis) {
                    tx.send(inbound)
                        .await
                        .map_err(|e| anyhow!("signal inbound queue closed: {e}"))?;
                    emitted += 1;
                }

                match newest_seen {
                    Some(current) if current >= timestamp_millis => {}
                    _ => newest_seen = Some(timestamp_millis),
                }
            }

            cursor_millis = newest_seen;
            tracing::info!(emitted, cursor_millis = ?cursor_millis, "signal poll cycle complete");
            tokio::time::sleep(self.poll_interval).await;
        }
    }

    async fn receive_once(&self) -> Result<Vec<SignalEnvelope>> {
        let path = format!("/v1/receive/{}", self.account);
        let url = self.api_url(&path)?;
        let response = self
            .authorized_request(self.http.get(url))
            .query(&[("timeout", self.receive_timeout_seconds)])
            .send()
            .await?;
        let status = response.status();
        let body: serde_json::Value = response.json().await?;
        if !status.is_success() {
            return Err(anyhow!(
                "signal receive failed: status={} body={}",
                status,
                body
            ));
        }
        Ok(parse_signal_receive_payload(body))
    }
}

fn normalize_signal_api_base_url(raw: &str) -> Result<String> {
    let normalized = raw.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        return Err(anyhow!("signal api_base_url is required"));
    }
    let parsed =
        Url::parse(&normalized).map_err(|e| anyhow!("invalid signal api_base_url: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(normalized),
        other => Err(anyhow!(
            "invalid signal api_base_url scheme: {other} (expected http or https)"
        )),
    }
}

fn parse_signal_receive_payload(body: serde_json::Value) -> Vec<SignalEnvelope> {
    match body {
        serde_json::Value::Array(values) => values
            .into_iter()
            .filter_map(extract_signal_envelope)
            .collect(),
        serde_json::Value::Object(map) => {
            if let Some(messages) = map.get("messages").and_then(|value| value.as_array()) {
                return messages
                    .iter()
                    .cloned()
                    .filter_map(extract_signal_envelope)
                    .collect();
            }
            extract_signal_envelope(serde_json::Value::Object(map))
                .into_iter()
                .collect()
        }
        _ => Vec::new(),
    }
}

fn extract_signal_envelope(raw: serde_json::Value) -> Option<SignalEnvelope> {
    let envelope_value = if let Some(envelope) = raw.get("envelope") {
        envelope.clone()
    } else {
        raw
    };
    serde_json::from_value(envelope_value).ok()
}

fn envelope_timestamp_millis(envelope: &SignalEnvelope) -> Option<i64> {
    envelope.timestamp.or_else(|| {
        envelope
            .data_message
            .as_ref()
            .and_then(|message| message.timestamp)
    })
}

fn convert_signal_envelope(
    envelope: &SignalEnvelope,
    timestamp_millis: i64,
) -> Option<InboundMessage> {
    let sender_id = envelope_sender_id(envelope)?;
    let data_message = envelope.data_message.as_ref()?;
    let group_id = envelope_group_id(envelope);
    let thread_id = group_id
        .as_deref()
        .map(|id| format!("signal-group:{id}"))
        .unwrap_or_else(|| format!("signal-dm:{sender_id}"));
    let is_group = group_id.is_some();

    if let Some(reaction) = data_message.reaction.as_ref() {
        if reaction.remove.unwrap_or(false) {
            return None;
        }
        let emoji = reaction
            .emoji
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let message_id = reaction
            .target_timestamp
            .map(|target_ts| format!("signal:{sender_id}:{timestamp_millis}:reaction:{target_ts}"))
            .unwrap_or_else(|| format!("signal:{sender_id}:{timestamp_millis}:reaction"));
        return Some(InboundMessage {
            kind: InboundMessageKind::Reaction,
            message_id: message_id.into(),
            channel_id: "signal".into(),
            sender_id: sender_id.into(),
            thread_id: Some(thread_id.into()),
            is_group,
            content: emoji.to_string(),
            metadata: serde_json::json!({
                "provider": "signal",
                "envelope": envelope,
            }),
            received_at: Utc::now(),
        });
    }

    let content = data_message
        .message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let message_id = format!("signal:{sender_id}:{timestamp_millis}:message");
    Some(InboundMessage {
        kind: InboundMessageKind::Message,
        message_id: message_id.into(),
        channel_id: "signal".into(),
        sender_id: sender_id.into(),
        thread_id: Some(thread_id.into()),
        is_group,
        content: content.to_string(),
        metadata: serde_json::json!({
            "provider": "signal",
            "envelope": envelope,
        }),
        received_at: Utc::now(),
    })
}

fn envelope_sender_id(envelope: &SignalEnvelope) -> Option<String> {
    for sender in [
        envelope.source.as_deref(),
        envelope.source_number.as_deref(),
        envelope.source_uuid.as_deref(),
    ] {
        let candidate = sender.map(str::trim).unwrap_or_default();
        if !candidate.is_empty() {
            return Some(candidate.to_string());
        }
    }
    None
}

fn envelope_group_id(envelope: &SignalEnvelope) -> Option<String> {
    let group_id = envelope
        .data_message
        .as_ref()?
        .group_info
        .as_ref()?
        .group_id
        .as_deref()?
        .trim();
    if group_id.is_empty() {
        return None;
    }
    Some(group_id.to_string())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SignalEnvelope {
    source: Option<String>,
    source_number: Option<String>,
    source_uuid: Option<String>,
    timestamp: Option<i64>,
    data_message: Option<SignalDataMessage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SignalDataMessage {
    timestamp: Option<i64>,
    message: Option<String>,
    reaction: Option<SignalReaction>,
    group_info: Option<SignalGroupInfo>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SignalReaction {
    emoji: Option<String>,
    target_author: Option<String>,
    target_timestamp: Option<i64>,
    remove: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SignalGroupInfo {
    group_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        SignalDataMessage, SignalEnvelope, SignalGroupInfo, SignalReaction,
        convert_signal_envelope, envelope_timestamp_millis, normalize_signal_api_base_url,
        parse_signal_receive_payload,
    };

    #[test]
    fn normalize_signal_api_base_url_requires_http_or_https() {
        assert_eq!(
            normalize_signal_api_base_url("https://signal-gateway.local/")
                .expect("https URL should normalize"),
            "https://signal-gateway.local"
        );
        assert!(normalize_signal_api_base_url("ssh://signal-gateway.local").is_err());
    }

    #[test]
    fn parse_signal_receive_payload_supports_array_and_messages_wrapper() {
        let array_payload = serde_json::json!([
            {
                "envelope": {
                    "source": "+15551234567",
                    "timestamp": 100,
                    "dataMessage": {
                        "message": "hello"
                    }
                }
            }
        ]);
        let wrapped_payload = serde_json::json!({
            "messages": [
                {
                    "source": "+15559876543",
                    "timestamp": 200,
                    "dataMessage": {
                        "message": "world"
                    }
                }
            ]
        });

        let from_array = parse_signal_receive_payload(array_payload);
        let from_wrapper = parse_signal_receive_payload(wrapped_payload);
        assert_eq!(from_array.len(), 1);
        assert_eq!(from_wrapper.len(), 1);
        assert_eq!(from_array[0].source.as_deref(), Some("+15551234567"));
        assert_eq!(from_wrapper[0].source.as_deref(), Some("+15559876543"));
    }

    #[test]
    fn convert_signal_text_message_to_inbound_event() {
        let envelope = SignalEnvelope {
            source: Some("+15551234567".to_string()),
            timestamp: Some(1234),
            data_message: Some(SignalDataMessage {
                message: Some("hello".to_string()),
                ..SignalDataMessage::default()
            }),
            ..SignalEnvelope::default()
        };
        let inbound = convert_signal_envelope(&envelope, 1234).expect("message should convert");
        assert_eq!(inbound.channel_id.as_str(), "signal");
        assert_eq!(inbound.sender_id.as_str(), "+15551234567");
        assert_eq!(inbound.content, "hello");
        assert!(!inbound.is_group);
    }

    #[test]
    fn convert_signal_reaction_message_to_inbound_event() {
        let envelope = SignalEnvelope {
            source: Some("+15551234567".to_string()),
            timestamp: Some(2345),
            data_message: Some(SignalDataMessage {
                reaction: Some(SignalReaction {
                    emoji: Some("🔥".to_string()),
                    target_timestamp: Some(1000),
                    ..SignalReaction::default()
                }),
                group_info: Some(SignalGroupInfo {
                    group_id: Some("group-1".to_string()),
                }),
                ..SignalDataMessage::default()
            }),
            ..SignalEnvelope::default()
        };
        let inbound = convert_signal_envelope(&envelope, 2345).expect("reaction should convert");
        assert_eq!(inbound.content, "🔥");
        assert_eq!(inbound.kind, crate::InboundMessageKind::Reaction);
        assert!(inbound.is_group);
    }

    #[test]
    fn envelope_timestamp_prefers_top_level_then_data_message() {
        let envelope = SignalEnvelope {
            timestamp: Some(10),
            data_message: Some(SignalDataMessage {
                timestamp: Some(20),
                ..SignalDataMessage::default()
            }),
            ..SignalEnvelope::default()
        };
        assert_eq!(envelope_timestamp_millis(&envelope), Some(10));

        let envelope = SignalEnvelope {
            timestamp: None,
            data_message: Some(SignalDataMessage {
                timestamp: Some(20),
                ..SignalDataMessage::default()
            }),
            ..SignalEnvelope::default()
        };
        assert_eq!(envelope_timestamp_millis(&envelope), Some(20));
    }
}
