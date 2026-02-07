use crate::error::{Result, ToolError};
use crate::traits::{optional_string, require_string, Tool, ToolSpec};
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use horizons_core::core_agents::models::RiskLevel;
use reqwest::Url;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

const GMAIL_API_BASE: &str = "https://gmail.googleapis.com/gmail/v1/users/me";
const DEFAULT_MAX_RESULTS: usize = 10;
const MAX_RESULTS_CAP: usize = 50;

#[derive(Clone)]
pub struct EmailTool {
    http: reqwest::Client,
    gmail_access_token: String,
    default_query: String,
}

impl EmailTool {
    pub fn new(gmail_access_token: &str, default_query: String) -> Result<Self> {
        let token = gmail_access_token.trim();
        if token.is_empty() {
            return Err(ToolError::InvalidArguments(
                "gmail access token is required".to_string(),
            ));
        }
        if default_query.trim().is_empty() {
            return Err(ToolError::InvalidArguments(
                "default email query is required".to_string(),
            ));
        }

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(Self {
            http,
            gmail_access_token: token.to_string(),
            default_query,
        })
    }

    fn api_url(&self, path: &str) -> Result<Url> {
        Url::parse(&format!("{GMAIL_API_BASE}/{path}"))
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.bearer_auth(&self.gmail_access_token)
    }

    async fn list_messages(&self, query: &str, max_results: usize) -> Result<Vec<GmailMessageRef>> {
        let url = self.api_url("messages")?;
        let max_results_s = max_results.to_string();
        let resp = self
            .auth(
                self.http
                    .get(url)
                    .query(&[("q", query), ("maxResults", max_results_s.as_str())]),
            )
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read response body>".to_string());
            return Err(ToolError::ExecutionFailed(format!(
                "gmail list messages failed: status={status} body={body}"
            )));
        }

        let payload: GmailListMessagesResponse = resp
            .json()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(payload.messages)
    }

    async fn get_message_metadata(&self, message_id: &str) -> Result<GmailMessageMetadata> {
        if message_id.trim().is_empty() {
            return Err(ToolError::InvalidArguments(
                "message_id is required".to_string(),
            ));
        }

        let path = format!("messages/{message_id}");
        let url = self.api_url(&path)?;
        let resp = self
            .auth(self.http.get(url).query(&[
                ("format", "metadata"),
                ("metadataHeaders", "From"),
                ("metadataHeaders", "To"),
                ("metadataHeaders", "Subject"),
                ("metadataHeaders", "Date"),
            ]))
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read response body>".to_string());
            return Err(ToolError::ExecutionFailed(format!(
                "gmail get message failed for id={message_id}: status={status} body={body}"
            )));
        }

        let payload: GmailMessage = resp
            .json()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(GmailMessageMetadata {
            message_id: payload.id.clone(),
            thread_id: payload.thread_id.clone(),
            from: payload
                .header_value("From")
                .unwrap_or_else(|| "(missing)".to_string()),
            to: payload
                .header_value("To")
                .unwrap_or_else(|| "(missing)".to_string()),
            subject: payload
                .header_value("Subject")
                .unwrap_or_else(|| "(no subject)".to_string()),
            date: payload
                .header_value("Date")
                .unwrap_or_else(|| "(missing)".to_string()),
            snippet: payload.snippet.unwrap_or_default(),
        })
    }

    async fn send_message(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        thread_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        validate_email_like(to)?;
        if subject.trim().is_empty() {
            return Err(ToolError::InvalidArguments(
                "subject must not be empty".to_string(),
            ));
        }
        if body.trim().is_empty() {
            return Err(ToolError::InvalidArguments(
                "body must not be empty".to_string(),
            ));
        }

        let mime = build_mime_message(to, subject, body);
        let raw = URL_SAFE_NO_PAD.encode(mime.as_bytes());

        let mut payload = json!({ "raw": raw });
        if let Some(id) = thread_id {
            let id = id.trim();
            if id.is_empty() {
                return Err(ToolError::InvalidArguments(
                    "thread_id must not be empty when provided".to_string(),
                ));
            }
            payload["threadId"] = serde_json::Value::String(id.to_string());
        }

        let url = self.api_url("messages/send")?;
        let resp = self
            .auth(self.http.post(url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read response body>".to_string());
            return Err(ToolError::ExecutionFailed(format!(
                "gmail send failed: status={status} body={body}"
            )));
        }

        #[derive(Deserialize)]
        struct GmailSendResponse {
            id: String,
            #[serde(rename = "threadId")]
            thread_id: String,
        }

        let sent: GmailSendResponse = resp
            .json()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(json!({
            "status": "sent",
            "message_id": sent.id,
            "thread_id": sent.thread_id,
            "to": to,
            "subject": subject,
        }))
    }
}

#[async_trait]
impl Tool for EmailTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "email".to_string(),
            description: "Search/read Gmail messages and send Gmail replies or new emails."
                .to_string(),
            parameters_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "action": { "type": "string", "enum": ["list_inbox", "search", "read", "send"] },
                    "query": { "type": "string" },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 50 },
                    "message_id": { "type": "string" },
                    "thread_id": { "type": "string" },
                    "to": { "type": "string" },
                    "subject": { "type": "string" },
                    "body": { "type": "string" }
                },
                "required": ["action"]
            }),
            risk_level: RiskLevel::High,
        }
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn execute(&self, arguments: serde_json::Value) -> Result<serde_json::Value> {
        let action = require_string(&arguments, "action")?;
        match action.as_str() {
            "list_inbox" => {
                let max_results = parse_max_results(&arguments)?;
                let refs = self.list_messages(&self.default_query, max_results).await?;
                let mut messages = Vec::with_capacity(refs.len());
                for msg in refs {
                    messages.push(self.get_message_metadata(&msg.id).await?);
                }
                Ok(json!({
                    "query": self.default_query,
                    "count": messages.len(),
                    "messages": messages
                }))
            }
            "search" => {
                let query = require_string(&arguments, "query")?;
                let query = query.trim();
                if query.is_empty() {
                    return Err(ToolError::InvalidArguments(
                        "query must not be empty".to_string(),
                    ));
                }

                let max_results = parse_max_results(&arguments)?;
                let refs = self.list_messages(query, max_results).await?;
                let mut messages = Vec::with_capacity(refs.len());
                for msg in refs {
                    messages.push(self.get_message_metadata(&msg.id).await?);
                }
                Ok(json!({
                    "query": query,
                    "count": messages.len(),
                    "messages": messages
                }))
            }
            "read" => {
                let message_id = require_string(&arguments, "message_id")?;
                let metadata = self.get_message_metadata(&message_id).await?;
                Ok(json!({
                    "message": metadata
                }))
            }
            "send" => {
                let to = require_string(&arguments, "to")?;
                let subject = require_string(&arguments, "subject")?;
                let body = require_string(&arguments, "body")?;
                let thread_id = optional_string(&arguments, "thread_id")?;
                self.send_message(&to, &subject, &body, thread_id.as_deref())
                    .await
            }
            other => Err(ToolError::InvalidArguments(format!(
                "unknown action: {other}"
            ))),
        }
    }
}

fn parse_max_results(arguments: &serde_json::Value) -> Result<usize> {
    match arguments.get("max_results") {
        None => Ok(DEFAULT_MAX_RESULTS),
        Some(v) => {
            let n = v.as_u64().ok_or_else(|| {
                ToolError::InvalidArguments("max_results must be an integer".to_string())
            })?;
            let n = usize::try_from(n).map_err(|_| {
                ToolError::InvalidArguments("max_results is out of range".to_string())
            })?;
            if !(1..=MAX_RESULTS_CAP).contains(&n) {
                return Err(ToolError::InvalidArguments(format!(
                    "max_results must be between 1 and {MAX_RESULTS_CAP}"
                )));
            }
            Ok(n)
        }
    }
}

fn validate_email_like(value: &str) -> Result<()> {
    if !value.contains('@') || value.contains(' ') {
        return Err(ToolError::InvalidArguments(format!(
            "invalid email recipient: {value:?}"
        )));
    }
    Ok(())
}

fn build_mime_message(to: &str, subject: &str, body: &str) -> String {
    let normalized = body.replace('\r', "");
    format!(
        "To: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=\"UTF-8\"\r\nMIME-Version: 1.0\r\n\r\n{normalized}\r\n"
    )
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

#[derive(Debug, serde::Serialize)]
struct GmailMessageMetadata {
    message_id: String,
    thread_id: String,
    from: String,
    to: String,
    subject: String,
    date: String,
    snippet: String,
}

#[cfg(test)]
mod tests {
    use super::{build_mime_message, parse_max_results, validate_email_like};

    #[test]
    fn parse_max_results_defaults_to_ten() {
        let got = parse_max_results(&serde_json::json!({})).expect("default max_results");
        assert_eq!(got, 10);
    }

    #[test]
    fn parse_max_results_enforces_bounds() {
        let err = parse_max_results(&serde_json::json!({"max_results": 0}))
            .expect_err("max_results=0 should fail");
        assert!(err.to_string().contains("between 1 and 50"));
    }

    #[test]
    fn recipient_validation_rejects_invalid_emails() {
        let err = validate_email_like("not-an-email").expect_err("invalid address should fail");
        assert!(err.to_string().contains("invalid email recipient"));
    }

    #[test]
    fn mime_builder_writes_required_headers() {
        let mime = build_mime_message("user@example.com", "subject", "hello");
        assert!(mime.contains("To: user@example.com"));
        assert!(mime.contains("Subject: subject"));
        assert!(mime.contains("hello"));
    }
}
