use crate::traits::ChannelAdapter;
use crate::types::{InboundMessage, InboundMessageKind, OutboundMessage};
use anyhow::{Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use reqwest::Url;
use serde::Deserialize;
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::mpsc;

const GMAIL_API_BASE: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

#[derive(Clone)]
pub struct EmailAdapter {
    http: reqwest::Client,
    gmail_access_token: String,
    poll_interval: Duration,
    query: String,
    start_from_latest: bool,
    mark_processed_as_read: bool,
    max_results: usize,
}

impl EmailAdapter {
    pub fn new(gmail_access_token: &str) -> Result<Self> {
        let token = gmail_access_token.trim();
        if token.is_empty() {
            return Err(anyhow!("gmail access token is required"));
        }

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;

        Ok(Self {
            http,
            gmail_access_token: token.to_string(),
            poll_interval: Duration::from_millis(1500),
            query: "in:inbox is:unread".to_string(),
            start_from_latest: true,
            mark_processed_as_read: true,
            max_results: 25,
        })
    }

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    pub fn with_query(mut self, query: String) -> Self {
        self.query = query;
        self
    }

    pub fn with_start_from_latest(mut self, start_from_latest: bool) -> Self {
        self.start_from_latest = start_from_latest;
        self
    }

    pub fn with_mark_processed_as_read(mut self, mark_processed_as_read: bool) -> Self {
        self.mark_processed_as_read = mark_processed_as_read;
        self
    }

    pub fn with_max_results(mut self, max_results: usize) -> Self {
        self.max_results = max_results;
        self
    }

    fn api_url(&self, path: &str) -> Result<Url> {
        Ok(Url::parse(&format!("{GMAIL_API_BASE}/{path}"))?)
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.bearer_auth(&self.gmail_access_token)
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for EmailAdapter {
    fn channel_id(&self) -> &str {
        "email"
    }

    async fn start(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let adapter = self.clone();
        tokio::spawn(async move {
            if let Err(e) = adapter.run_poll_loop(tx).await {
                tracing::error!(%e, "email poll loop exited");
            }
        });
        Ok(())
    }

    async fn send(&self, recipient_id: &str, message: OutboundMessage) -> Result<()> {
        let recipient = parse_recipient(recipient_id)?;
        let body = message.content.trim();
        if body.is_empty() {
            return Err(anyhow!("message content is empty"));
        }

        let mime = build_mime_message(&recipient.to, "OpenCraw", body);
        let raw = URL_SAFE_NO_PAD.encode(mime.as_bytes());

        let mut payload = serde_json::json!({ "raw": raw });
        if let Some(thread_id) = recipient.thread_id {
            payload["threadId"] = serde_json::Value::String(thread_id);
        }

        let url = self.api_url("messages/send")?;
        let resp = self.auth(self.http.post(url)).json(&payload).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(anyhow!("gmail send failed: status={status} body={text}"));
        }

        tracing::info!(to = %recipient.to, "email message sent");
        Ok(())
    }
}

impl EmailAdapter {
    #[tracing::instrument(level = "info", skip_all)]
    async fn run_poll_loop(&self, tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        let mut seen_message_ids = HashSet::<String>::new();

        if self.start_from_latest {
            let initial = self.fetch_messages().await?;
            for message in initial {
                seen_message_ids.insert(message.message_id);
            }
            tracing::info!(
                seeded_count = seen_message_ids.len(),
                "email adapter seeded initial cursor"
            );
        }

        loop {
            let pulled = self.fetch_messages().await?;
            let mut emitted = 0usize;

            for message in pulled {
                if seen_message_ids.contains(&message.message_id) {
                    continue;
                }

                let inbound = InboundMessage {
                    kind: InboundMessageKind::Message,
                    message_id: message.message_id.clone().into(),
                    channel_id: "email".into(),
                    sender_id: message.sender_id.into(),
                    thread_id: Some(message.thread_id.clone().into()),
                    is_group: false,
                    content: message.content,
                    metadata: message.metadata,
                    received_at: Utc::now(),
                };

                tx.send(inbound)
                    .await
                    .map_err(|e| anyhow!("email inbound queue closed: {e}"))?;
                seen_message_ids.insert(message.message_id.clone());
                emitted += 1;

                if self.mark_processed_as_read {
                    self.mark_as_read(&message.message_id).await?;
                }
            }

            tracing::info!(
                emitted,
                seen_count = seen_message_ids.len(),
                "email poll cycle complete"
            );

            tokio::time::sleep(self.poll_interval).await;
        }
    }

    async fn fetch_messages(&self) -> Result<Vec<ParsedEmailMessage>> {
        let refs = self.list_message_refs().await?;
        let mut out = Vec::with_capacity(refs.len());
        for message_ref in refs {
            out.push(self.fetch_message(&message_ref.id).await?);
        }
        Ok(out)
    }

    async fn list_message_refs(&self) -> Result<Vec<GmailMessageRef>> {
        let url = self.api_url("messages")?;
        let max_results = self.max_results.to_string();
        let resp = self
            .auth(self.http.get(url).query(&[
                ("q", self.query.as_str()),
                ("maxResults", max_results.as_str()),
            ]))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(anyhow!(
                "gmail list messages failed: status={status} body={text}"
            ));
        }

        let payload: GmailListMessagesResponse = resp.json().await?;
        Ok(payload
            .messages
            .into_iter()
            .take(self.max_results)
            .collect())
    }

    async fn fetch_message(&self, message_id: &str) -> Result<ParsedEmailMessage> {
        let url = self.api_url(&format!("messages/{message_id}"))?;
        let resp = self
            .auth(self.http.get(url).query(&[
                ("format", "metadata"),
                ("metadataHeaders", "From"),
                ("metadataHeaders", "Subject"),
            ]))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(anyhow!(
                "gmail get message failed for id={message_id}: status={status} body={text}"
            ));
        }

        let payload: GmailMessage = resp.json().await?;

        let from = payload
            .header_value("From")
            .ok_or_else(|| anyhow!("gmail message {message_id} missing From header"))?;
        let subject = payload
            .header_value("Subject")
            .unwrap_or_else(|| "(no subject)".to_string());
        let sender_id = extract_email_address(&from);

        let snippet = payload
            .snippet
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let content = if snippet.is_empty() {
            format!("From: {from}\nSubject: {subject}")
        } else {
            format!("From: {from}\nSubject: {subject}\n\n{snippet}")
        };

        let thread_id = payload.thread_id.clone();
        Ok(ParsedEmailMessage {
            message_id: payload.id,
            thread_id,
            sender_id,
            content,
            metadata: serde_json::json!({
                "from": from,
                "subject": subject,
                "thread_id": payload.thread_id,
                "snippet": snippet,
            }),
        })
    }

    async fn mark_as_read(&self, message_id: &str) -> Result<()> {
        let url = self.api_url(&format!("messages/{message_id}/modify"))?;
        let payload = serde_json::json!({
            "removeLabelIds": ["UNREAD"]
        });
        let resp = self.auth(self.http.post(url)).json(&payload).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(anyhow!(
                "gmail modify failed for id={message_id}: status={status} body={text}"
            ));
        }

        Ok(())
    }
}

#[derive(Debug)]
struct ParsedEmailMessage {
    message_id: String,
    thread_id: String,
    sender_id: String,
    content: String,
    metadata: serde_json::Value,
}

#[derive(Debug)]
struct ParsedRecipient {
    to: String,
    thread_id: Option<String>,
}

fn parse_recipient(recipient_id: &str) -> Result<ParsedRecipient> {
    let trimmed = recipient_id.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("recipient_id is required"));
    }

    if let Some(rest) = trimmed.strip_prefix("thread:") {
        let mut parts = rest.splitn(2, ':');
        let thread_id = parts.next().map(str::trim).ok_or_else(|| {
            anyhow!("invalid recipient format, expected thread:<thread_id>:<email>")
        })?;
        let to = parts.next().map(str::trim).ok_or_else(|| {
            anyhow!("invalid recipient format, expected thread:<thread_id>:<email>")
        })?;

        if thread_id.is_empty() || to.is_empty() {
            return Err(anyhow!(
                "invalid recipient format, expected thread:<thread_id>:<email>"
            ));
        }

        validate_email_like(to)?;
        return Ok(ParsedRecipient {
            to: to.to_string(),
            thread_id: Some(thread_id.to_string()),
        });
    }

    validate_email_like(trimmed)?;
    Ok(ParsedRecipient {
        to: trimmed.to_string(),
        thread_id: None,
    })
}

fn validate_email_like(value: &str) -> Result<()> {
    if !value.contains('@') || value.contains(' ') {
        return Err(anyhow!("invalid email recipient: {value:?}"));
    }
    Ok(())
}

fn build_mime_message(to: &str, subject: &str, body: &str) -> String {
    let normalized = body.replace('\r', "");
    format!(
        "To: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=\"UTF-8\"\r\nMIME-Version: 1.0\r\n\r\n{normalized}\r\n"
    )
}

fn extract_email_address(from: &str) -> String {
    let trimmed = from.trim();
    if let Some(start) = trimmed.find('<') {
        if let Some(end) = trimmed[start + 1..].find('>') {
            let candidate = trimmed[start + 1..start + 1 + end].trim();
            if candidate.contains('@') {
                return candidate.to_string();
            }
        }
    }
    trimmed.to_string()
}

#[derive(Debug, Deserialize)]
struct GmailListMessagesResponse {
    #[serde(default)]
    messages: Vec<GmailMessageRef>,
}

#[derive(Debug, Deserialize)]
struct GmailMessageRef {
    id: String,
}

#[derive(Debug, Deserialize)]
struct GmailMessage {
    id: String,
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(default)]
    snippet: Option<String>,
    #[serde(default)]
    payload: Option<GmailPayload>,
}

impl GmailMessage {
    fn header_value(&self, name: &str) -> Option<String> {
        let headers = self.payload.as_ref()?.headers.as_slice();
        headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.clone())
    }
}

#[derive(Debug, Deserialize)]
struct GmailPayload {
    #[serde(default)]
    headers: Vec<GmailHeader>,
}

#[derive(Debug, Deserialize)]
struct GmailHeader {
    name: String,
    value: String,
}

#[cfg(test)]
mod tests {
    use super::{build_mime_message, extract_email_address, parse_recipient};

    #[test]
    fn recipient_parser_supports_thread_format() {
        let parsed = parse_recipient("thread:abc123:user@example.com").expect("parse recipient");
        assert_eq!(parsed.to, "user@example.com");
        assert_eq!(parsed.thread_id.as_deref(), Some("abc123"));
    }

    #[test]
    fn recipient_parser_rejects_invalid_email() {
        let err = parse_recipient("not-an-email").expect_err("expected invalid email error");
        assert!(err.to_string().contains("invalid email recipient"));
    }

    #[test]
    fn extracts_address_from_rfc_5322_from_header() {
        let sender = extract_email_address("Jane Doe <jane@example.com>");
        assert_eq!(sender, "jane@example.com");
    }

    #[test]
    fn builds_basic_mime_message() {
        let mime = build_mime_message("jane@example.com", "OpenCraw", "hello");
        assert!(mime.contains("To: jane@example.com"));
        assert!(mime.contains("Subject: OpenCraw"));
        assert!(mime.contains("hello"));
    }
}
