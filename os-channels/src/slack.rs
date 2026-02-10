use crate::traits::ChannelAdapter;
use crate::types::{InboundMessage, InboundMessageKind, OutboundMessage};
use anyhow::{Result, anyhow};
use chrono::Utc;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct SlackAdapter {
    http: reqwest::Client,
    bot_token: String,
    poll_interval: Duration,
    channel_ids: Vec<String>,
    start_from_latest: bool,
    history_limit: usize,
}

impl SlackAdapter {
    pub fn new(bot_token: &str) -> Result<Self> {
        let token = bot_token.trim();
        if token.is_empty() {
            return Err(anyhow!("slack bot token is required"));
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            bot_token: token.to_string(),
            poll_interval: Duration::from_millis(3000),
            channel_ids: Vec::new(),
            start_from_latest: true,
            history_limit: 100,
        })
    }

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    pub fn with_channel_ids(mut self, channel_ids: Vec<String>) -> Self {
        let mut deduped = Vec::new();
        for channel_id in channel_ids {
            let channel_id = channel_id.trim().to_string();
            if channel_id.is_empty() {
                continue;
            }
            if !deduped.iter().any(|existing| existing == &channel_id) {
                deduped.push(channel_id);
            }
        }
        self.channel_ids = deduped;
        self
    }

    pub fn with_start_from_latest(mut self, start_from_latest: bool) -> Self {
        self.start_from_latest = start_from_latest;
        self
    }

    pub fn with_history_limit(mut self, history_limit: usize) -> Self {
        self.history_limit = history_limit.clamp(1, 200);
        self
    }

    fn api_url(&self, method: &str) -> Result<Url> {
        Ok(Url::parse(&format!("https://slack.com/api/{method}"))?)
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for SlackAdapter {
    fn channel_id(&self) -> &str {
        "slack"
    }

    async fn start(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        if self.channel_ids.is_empty() {
            return Err(anyhow!(
                "slack adapter requires at least one channel id to poll"
            ));
        }
        let adapter = self.clone();
        tokio::spawn(async move {
            if let Err(error) = adapter.run_poll_loop(tx).await {
                tracing::error!(%error, "slack poll loop exited");
            }
        });
        Ok(())
    }

    async fn send(&self, recipient_id: &str, message: OutboundMessage) -> Result<()> {
        let channel_id = recipient_id.trim();
        if channel_id.is_empty() {
            return Err(anyhow!("recipient_id (slack channel id) is required"));
        }
        let text = message.content.trim();
        if text.is_empty() {
            return Err(anyhow!("message content is empty"));
        }

        let mut payload = serde_json::json!({
            "channel": channel_id,
            "text": text,
        });
        if let Some(reply_to) = message.reply_to_message_id.as_ref() {
            payload["thread_ts"] = serde_json::json!(reply_to.as_str());
        }

        let url = self.api_url("chat.postMessage")?;
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.bot_token)
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        let body: SlackPostMessageResponse = response.json().await?;
        if !status.is_success() || !body.ok {
            return Err(anyhow!(
                "slack chat.postMessage failed: status={} error={}",
                status,
                body.error.unwrap_or_else(|| "unknown".to_string())
            ));
        }

        Ok(())
    }

    fn supports_reactions(&self) -> bool {
        true
    }
}

impl SlackAdapter {
    #[tracing::instrument(level = "info", skip_all)]
    async fn run_poll_loop(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let mut cursor_by_channel: HashMap<String, String> = HashMap::new();

        if self.start_from_latest {
            for channel_id in &self.channel_ids {
                let messages = self.fetch_channel_messages(channel_id).await?;
                let latest = messages
                    .iter()
                    .filter_map(|message| message.ts.as_deref())
                    .max_by(|left, right| compare_slack_timestamps(left, right))
                    .map(ToOwned::to_owned);
                if let Some(latest) = latest {
                    cursor_by_channel.insert(channel_id.clone(), latest);
                }
            }
            tracing::info!(
                seeded_channels = cursor_by_channel.len(),
                "slack adapter seeded initial cursors"
            );
        }

        loop {
            for channel_id in &self.channel_ids {
                let mut messages = self.fetch_channel_messages(channel_id).await?;
                messages.sort_by(|left, right| {
                    let left_ts = left.ts.as_deref().unwrap_or_default();
                    let right_ts = right.ts.as_deref().unwrap_or_default();
                    compare_slack_timestamps(left_ts, right_ts)
                });

                let channel_cursor = cursor_by_channel.get(channel_id).cloned();
                let mut newest_seen = channel_cursor.clone();
                let mut emitted = 0usize;

                for message in messages {
                    if !should_emit_message(&message, channel_cursor.as_deref()) {
                        continue;
                    }
                    let Some(ts) = message.ts.clone() else {
                        continue;
                    };
                    let sender_id = message
                        .user
                        .clone()
                        .or(message.bot_id.clone())
                        .unwrap_or_else(|| "unknown".to_string());
                    let content = message.text.clone().unwrap_or_default();
                    if content.trim().is_empty() {
                        continue;
                    }

                    let inbound = InboundMessage {
                        kind: InboundMessageKind::Message,
                        message_id: ts.clone().into(),
                        channel_id: "slack".into(),
                        sender_id: sender_id.into(),
                        thread_id: Some(
                            message
                                .thread_ts
                                .clone()
                                .unwrap_or_else(|| ts.clone())
                                .into(),
                        ),
                        is_group: true,
                        content,
                        metadata: serde_json::to_value(&message)?,
                        received_at: Utc::now(),
                    };
                    tx.send(inbound)
                        .await
                        .map_err(|e| anyhow!("slack inbound queue closed: {e}"))?;
                    emitted += 1;
                    match newest_seen.as_deref() {
                        Some(current)
                            if compare_slack_timestamps(current, &ts) != Ordering::Less => {}
                        _ => newest_seen = Some(ts),
                    }
                }

                if let Some(newest) = newest_seen {
                    cursor_by_channel.insert(channel_id.clone(), newest);
                }
                tracing::info!(channel_id, emitted, "slack poll cycle complete");
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }

    async fn fetch_channel_messages(&self, channel_id: &str) -> Result<Vec<SlackMessage>> {
        let url = self.api_url("conversations.history")?;
        let response = self
            .http
            .get(url)
            .bearer_auth(&self.bot_token)
            .query(&[
                ("channel", channel_id),
                ("limit", &self.history_limit.to_string()),
                ("inclusive", "true"),
            ])
            .send()
            .await?;
        let status = response.status();
        let body: SlackHistoryResponse = response.json().await?;
        if !status.is_success() || !body.ok {
            return Err(anyhow!(
                "slack conversations.history failed: status={} channel={} error={}",
                status,
                channel_id,
                body.error.unwrap_or_else(|| "unknown".to_string())
            ));
        }
        Ok(body.messages)
    }
}

fn should_emit_message(message: &SlackMessage, channel_cursor: Option<&str>) -> bool {
    let Some(ts) = message.ts.as_deref() else {
        return false;
    };
    if message.subtype.as_deref().is_some() {
        return false;
    }
    if let Some(cursor) = channel_cursor {
        return compare_slack_timestamps(ts, cursor) == Ordering::Greater;
    }
    true
}

fn compare_slack_timestamps(left: &str, right: &str) -> Ordering {
    match (parse_slack_timestamp(left), parse_slack_timestamp(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SlackTimestamp {
    seconds: i64,
    micros: i64,
    raw: String,
}

fn parse_slack_timestamp(raw: &str) -> Option<SlackTimestamp> {
    let mut parts = raw.trim().split('.');
    let seconds = parts.next()?.parse::<i64>().ok()?;
    let micros_raw = parts.next().unwrap_or("0");
    let micros_digits = micros_raw.chars().take(6).collect::<String>();
    let micros_padded = format!("{micros_digits:0<6}");
    let micros = micros_padded.parse::<i64>().ok()?;
    Some(SlackTimestamp {
        seconds,
        micros,
        raw: raw.to_string(),
    })
}

#[derive(Debug, Deserialize)]
struct SlackHistoryResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    messages: Vec<SlackMessage>,
}

#[derive(Debug, Deserialize)]
struct SlackPostMessageResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SlackMessage {
    #[serde(default)]
    ts: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    bot_id: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thread_ts: Option<String>,
    #[serde(default)]
    subtype: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        SlackMessage, compare_slack_timestamps, parse_slack_timestamp, should_emit_message,
    };
    use std::cmp::Ordering;

    #[test]
    fn parse_slack_timestamp_normalizes_fractional_precision() {
        let ts = parse_slack_timestamp("1716572940.000123").expect("timestamp should parse");
        assert_eq!(ts.seconds, 1716572940);
        assert_eq!(ts.micros, 123);
    }

    #[test]
    fn compare_slack_timestamps_orders_newer_values() {
        assert_eq!(
            compare_slack_timestamps("1716572940.000100", "1716572940.000099"),
            Ordering::Greater
        );
        assert_eq!(
            compare_slack_timestamps("1716572940.000100", "1716572941.000000"),
            Ordering::Less
        );
    }

    #[test]
    fn should_emit_message_respects_cursor_and_subtype() {
        let message = SlackMessage {
            ts: Some("1716572940.000101".to_string()),
            user: Some("U123".to_string()),
            bot_id: None,
            text: Some("hello".to_string()),
            thread_ts: None,
            subtype: None,
        };
        assert!(should_emit_message(&message, Some("1716572940.000100")));
        assert!(!should_emit_message(&message, Some("1716572940.000101")));

        let mut subtype_message = message.clone();
        subtype_message.subtype = Some("message_changed".to_string());
        assert!(!should_emit_message(
            &subtype_message,
            Some("1716572940.000100")
        ));
    }
}
