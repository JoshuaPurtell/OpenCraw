use crate::error::{Result, ToolError};
use crate::traits::{Tool, ToolSpec, optional_string, require_string};
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use horizons_core::core_agents::models::RiskLevel;
use reqwest::Url;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use std::time::Duration;

const GMAIL_API_BASE: &str = "https://gmail.googleapis.com/gmail/v1/users/me";
const DEFAULT_MAX_RESULTS: usize = 10;
const MAX_RESULTS_CAP: usize = 50;

#[derive(Clone, Copy, Debug)]
pub struct EmailActionToggles {
    pub list_labels: bool,
    pub list_inbox: bool,
    pub search: bool,
    pub read: bool,
    pub send: bool,
}

impl EmailActionToggles {
    pub fn all_enabled() -> Self {
        Self {
            list_labels: true,
            list_inbox: true,
            search: true,
            read: true,
            send: true,
        }
    }

    fn allows(self, action: &str) -> bool {
        match action {
            "list_labels" => self.list_labels,
            "list_inbox" => self.list_inbox,
            "search" => self.search,
            "read" => self.read,
            "send" => self.send,
            _ => false,
        }
    }
}

#[derive(Clone)]
pub struct EmailTool {
    http: reqwest::Client,
    gmail_access_token: String,
    default_query: String,
    action_toggles: EmailActionToggles,
}

impl EmailTool {
    pub fn new(
        gmail_access_token: &str,
        default_query: String,
        action_toggles: EmailActionToggles,
    ) -> Result<Self> {
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
            action_toggles,
        })
    }

    fn ensure_action_enabled(&self, action: &str) -> Result<()> {
        if self.action_toggles.allows(action) {
            return Ok(());
        }
        Err(ToolError::Unauthorized(format!(
            "email action {action:?} is disabled by channels.email.actions.{action}"
        )))
    }

    fn api_url(&self, path: &str) -> Result<Url> {
        Url::parse(&format!("{GMAIL_API_BASE}/{path}"))
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.bearer_auth(&self.gmail_access_token)
    }

    async fn list_messages(
        &self,
        query: Option<&str>,
        max_results: usize,
        label_ids: &[String],
    ) -> Result<Vec<GmailMessageRef>> {
        let url = self.api_url("messages")?;
        let max_results_s = max_results.to_string();
        let mut req = self.auth(self.http.get(url));
        req = req.query(&[("maxResults", max_results_s.as_str())]);
        if let Some(query) = query.map(str::trim).filter(|value| !value.is_empty()) {
            req = req.query(&[("q", query)]);
        }
        for label_id in label_ids {
            req = req.query(&[("labelIds", label_id.as_str())]);
        }
        let resp = req
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

    async fn fetch_labels(&self) -> Result<Vec<GmailLabel>> {
        let url = self.api_url("labels")?;
        let resp = self
            .auth(self.http.get(url))
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
                "gmail list labels failed: status={status} body={body}"
            )));
        }

        let mut payload: GmailListLabelsResponse = resp
            .json()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        payload.labels.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(payload.labels)
    }

    async fn list_labels(
        &self,
        query: Option<&str>,
        max_results: usize,
        include_system: bool,
        include_user: bool,
    ) -> Result<serde_json::Value> {
        if !include_system && !include_user {
            return Err(ToolError::InvalidArguments(
                "at least one of include_system/include_user must be true".to_string(),
            ));
        }

        let query = query
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(normalize_for_compare);

        let labels = self
            .fetch_labels()
            .await?
            .into_iter()
            .filter(|label| include_system || !label.label_type.eq_ignore_ascii_case("system"))
            .filter(|label| include_user || !label.label_type.eq_ignore_ascii_case("user"))
            .filter(|label| {
                let Some(query) = query.as_deref() else {
                    return true;
                };
                normalize_for_compare(label.id.as_str()).contains(query)
                    || normalize_for_compare(label.name.as_str()).contains(query)
            })
            .take(max_results)
            .collect::<Vec<_>>();

        Ok(json!({
            "count": labels.len(),
            "labels": labels
        }))
    }

    async fn resolve_label_references(&self, refs: &[String]) -> Result<Vec<GmailLabel>> {
        if refs.is_empty() {
            return Ok(Vec::new());
        }
        let labels = self.fetch_labels().await?;
        if labels.is_empty() {
            return Err(ToolError::ExecutionFailed(
                "gmail labels list returned no labels".to_string(),
            ));
        }

        let mut resolved = Vec::new();
        let mut seen = HashSet::new();
        for reference in refs {
            let reference = reference.trim();
            if reference.is_empty() {
                continue;
            }
            let normalized_reference = normalize_for_compare(reference);
            let exact_id = labels
                .iter()
                .find(|label| label.id.eq_ignore_ascii_case(reference))
                .cloned();
            let exact_name = labels
                .iter()
                .find(|label| normalize_for_compare(label.name.as_str()) == normalized_reference)
                .cloned();
            let resolved_label = if let Some(label) = exact_id.or(exact_name) {
                label
            } else {
                let partial_matches = labels
                    .iter()
                    .filter(|label| {
                        normalize_for_compare(label.name.as_str())
                            .contains(normalized_reference.as_str())
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if partial_matches.len() == 1 {
                    partial_matches.into_iter().next().ok_or_else(|| {
                        ToolError::ExecutionFailed("gmail label resolution failed".to_string())
                    })?
                } else if partial_matches.len() > 1 {
                    let matches = partial_matches
                        .iter()
                        .map(|label| format!("{} ({})", label.name, label.id))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(ToolError::InvalidArguments(format!(
                        "label reference {reference:?} matched multiple labels: {matches}; pass exact label id or exact label name"
                    )));
                } else {
                    let available = labels
                        .iter()
                        .map(|label| format!("{} ({})", label.name, label.id))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(ToolError::InvalidArguments(format!(
                        "label reference {reference:?} was not found; available labels: {available}"
                    )));
                }
            };

            if seen.insert(resolved_label.id.clone()) {
                resolved.push(resolved_label);
            }
        }

        Ok(resolved)
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
            label_ids: payload.label_ids.clone(),
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
                    "action": { "type": "string", "enum": ["list_labels", "list_inbox", "search", "read", "send"] },
                    "query": { "type": "string" },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 50 },
                    "labels": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional Gmail label references (label id or exact label name)."
                    },
                    "label_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Alias for labels using label ids."
                    },
                    "label_names": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Alias for labels using exact label names."
                    },
                    "include_system": { "type": "boolean" },
                    "include_user": { "type": "boolean" },
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
        self.ensure_action_enabled(&action)?;
        match action.as_str() {
            "list_labels" => {
                let max_results = parse_max_results(&arguments)?;
                let query = optional_string(&arguments, "query")?;
                let include_system = optional_bool(&arguments, "include_system")?.unwrap_or(true);
                let include_user = optional_bool(&arguments, "include_user")?.unwrap_or(true);
                self.list_labels(query.as_deref(), max_results, include_system, include_user)
                    .await
            }
            "list_inbox" => {
                let max_results = parse_max_results(&arguments)?;
                let label_refs = parse_optional_label_refs(&arguments)?;
                let label_filters = self.resolve_label_references(label_refs.as_slice()).await?;
                let label_ids = label_filters
                    .iter()
                    .map(|label| label.id.clone())
                    .collect::<Vec<_>>();
                let refs = self
                    .list_messages(Some(&self.default_query), max_results, &label_ids)
                    .await?;
                let mut messages = Vec::with_capacity(refs.len());
                for msg in refs {
                    messages.push(self.get_message_metadata(&msg.id).await?);
                }
                Ok(json!({
                    "query": self.default_query,
                    "label_filters": label_filters,
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
                let label_refs = parse_optional_label_refs(&arguments)?;
                let label_filters = self.resolve_label_references(label_refs.as_slice()).await?;
                let label_ids = label_filters
                    .iter()
                    .map(|label| label.id.clone())
                    .collect::<Vec<_>>();
                let refs = self
                    .list_messages(Some(query), max_results, &label_ids)
                    .await?;
                let mut messages = Vec::with_capacity(refs.len());
                for msg in refs {
                    messages.push(self.get_message_metadata(&msg.id).await?);
                }
                Ok(json!({
                    "query": query,
                    "label_filters": label_filters,
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

fn optional_bool(arguments: &serde_json::Value, key: &str) -> Result<Option<bool>> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    let value = value.as_bool().ok_or_else(|| {
        ToolError::InvalidArguments(format!("key {key} must be boolean, got {value:?}"))
    })?;
    Ok(Some(value))
}

fn optional_string_list(arguments: &serde_json::Value, key: &str) -> Result<Option<Vec<String>>> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    let values = value.as_array().ok_or_else(|| {
        ToolError::InvalidArguments(format!("key {key} must be an array of strings"))
    })?;
    let mut items = Vec::with_capacity(values.len());
    for value in values {
        let Some(item) = value.as_str() else {
            return Err(ToolError::InvalidArguments(format!(
                "key {key} must be an array of strings"
            )));
        };
        let item = item.trim();
        if item.is_empty() {
            return Err(ToolError::InvalidArguments(format!(
                "key {key} cannot contain empty strings"
            )));
        }
        items.push(item.to_string());
    }
    Ok(Some(items))
}

fn parse_optional_label_refs(arguments: &serde_json::Value) -> Result<Vec<String>> {
    let mut values = Vec::new();
    for key in ["labels", "label_ids", "label_names"] {
        if let Some(items) = optional_string_list(arguments, key)? {
            values.extend(items);
        }
    }
    for key in ["label", "label_id", "label_name"] {
        if let Some(value) = optional_string(arguments, key)? {
            let value = value.trim();
            if value.is_empty() {
                return Err(ToolError::InvalidArguments(format!(
                    "key {key} must not be empty"
                )));
            }
            values.push(value.to_string());
        }
    }
    values.sort();
    values.dedup();
    Ok(values)
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

fn normalize_for_compare(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

#[derive(Debug, Deserialize)]
struct GmailListMessagesResponse {
    #[serde(default)]
    messages: Vec<GmailMessageRef>,
}

#[derive(Debug, Deserialize)]
struct GmailListLabelsResponse {
    #[serde(default)]
    labels: Vec<GmailLabel>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct GmailLabel {
    id: String,
    name: String,
    #[serde(rename = "type", default)]
    label_type: String,
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
    #[serde(rename = "labelIds", default)]
    label_ids: Vec<String>,
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
    #[serde(default)]
    label_ids: Vec<String>,
    from: String,
    to: String,
    subject: String,
    date: String,
    snippet: String,
}

#[cfg(test)]
mod tests {
    use super::{
        build_mime_message, optional_bool, parse_max_results, parse_optional_label_refs,
        validate_email_like,
    };

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

    #[test]
    fn parse_optional_label_refs_merges_aliases_and_dedupes() {
        let args = serde_json::json!({
            "labels": ["INBOX", "Finance"],
            "label_ids": ["Label_123"],
            "label_name": "Finance",
            "label_id": "Label_123"
        });
        let refs = parse_optional_label_refs(&args).expect("label refs");
        assert_eq!(
            refs,
            vec![
                "Finance".to_string(),
                "INBOX".to_string(),
                "Label_123".to_string()
            ]
        );
    }

    #[test]
    fn parse_optional_label_refs_rejects_empty_singular_label() {
        let args = serde_json::json!({"label":"   "});
        let err = parse_optional_label_refs(&args).expect_err("empty label");
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn optional_bool_rejects_non_boolean() {
        let args = serde_json::json!({"include_system":"yes"});
        let err = optional_bool(&args, "include_system").expect_err("invalid bool");
        assert!(err.to_string().contains("must be boolean"));
    }
}
