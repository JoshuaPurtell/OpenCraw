use crate::error::{Result, ToolError};
use crate::traits::{Tool, ToolSpec, optional_string, require_string};
use async_trait::async_trait;
use horizons_core::core_agents::models::RiskLevel;
use regex::Regex;
use reqwest::Url;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::process::Command;

const MAX_HTML_BYTES: usize = 1_000_000;

struct BrowserSession {
    current_url: String,
    status_code: u16,
    title: Option<String>,
    html: String,
}

pub struct BrowserTool {
    http: reqwest::Client,
    sessions: Mutex<HashMap<String, BrowserSession>>,
    next_session_id: AtomicU64,
}

impl BrowserTool {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(Self {
            http,
            sessions: Mutex::new(HashMap::new()),
            next_session_id: AtomicU64::new(1),
        })
    }

    async fn navigate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value> {
        let raw_url = require_string(arguments, "url")?;
        let url = parse_http_url(&raw_url)?;
        let session_id = optional_string(arguments, "session_id")?
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| {
                format!(
                    "browser-{}",
                    self.next_session_id.fetch_add(1, Ordering::Relaxed)
                )
            });

        let response = self
            .http
            .get(url.clone())
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let status_code = response.status().as_u16();
        let final_url = response.url().to_string();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let truncated = bytes.len() > MAX_HTML_BYTES;
        let html_bytes = if truncated {
            &bytes[..MAX_HTML_BYTES]
        } else {
            &bytes
        };
        let html = String::from_utf8_lossy(html_bytes).to_string();
        let title = html_title(&html);

        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| ToolError::ExecutionFailed("browser session lock poisoned".to_string()))?;
        sessions.insert(
            session_id.clone(),
            BrowserSession {
                current_url: final_url.clone(),
                status_code,
                title: title.clone(),
                html: html.clone(),
            },
        );

        Ok(serde_json::json!({
            "session_id": session_id,
            "url": final_url,
            "status_code": status_code,
            "title": title,
            "html_bytes": html.len(),
            "truncated": truncated,
        }))
    }

    fn extract_text(&self, arguments: &serde_json::Value) -> Result<serde_json::Value> {
        let session_id = require_string(arguments, "session_id")?;
        let max_chars = match arguments.get("max_chars") {
            None => 8000usize,
            Some(raw) => {
                let value = raw.as_u64().ok_or_else(|| {
                    ToolError::InvalidArguments("max_chars must be an integer".to_string())
                })?;
                usize::try_from(value).map_err(|_| {
                    ToolError::InvalidArguments("max_chars is out of range".to_string())
                })?
            }
        }
        .min(50_000);

        let sessions = self
            .sessions
            .lock()
            .map_err(|_| ToolError::ExecutionFailed("browser session lock poisoned".to_string()))?;
        let session = sessions.get(&session_id).ok_or_else(|| {
            ToolError::InvalidArguments(format!("unknown session_id: {session_id}"))
        })?;
        let full_text = html_to_text(&session.html);
        let truncated = full_text.len() > max_chars;
        let mut text = full_text.clone();
        if truncated {
            text.truncate(max_chars);
        }
        Ok(serde_json::json!({
            "session_id": session_id,
            "url": session.current_url,
            "status_code": session.status_code,
            "title": session.title,
            "text": text,
            "truncated": truncated,
        }))
    }

    fn find_text(&self, arguments: &serde_json::Value) -> Result<serde_json::Value> {
        let session_id = require_string(arguments, "session_id")?;
        let pattern = require_string(arguments, "pattern")?;
        if pattern.trim().is_empty() {
            return Err(ToolError::InvalidArguments(
                "pattern must not be empty".to_string(),
            ));
        }
        let case_sensitive = arguments
            .get("case_sensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let max_matches = arguments
            .get("max_matches")
            .and_then(|v| v.as_u64())
            .unwrap_or(20)
            .min(200) as usize;

        let sessions = self
            .sessions
            .lock()
            .map_err(|_| ToolError::ExecutionFailed("browser session lock poisoned".to_string()))?;
        let session = sessions.get(&session_id).ok_or_else(|| {
            ToolError::InvalidArguments(format!("unknown session_id: {session_id}"))
        })?;

        let text = html_to_text(&session.html);
        let matches = find_in_text(&text, &pattern, case_sensitive, max_matches);
        Ok(serde_json::json!({
            "session_id": session_id,
            "url": session.current_url,
            "pattern": pattern,
            "case_sensitive": case_sensitive,
            "match_count": matches.len(),
            "matches": matches,
        }))
    }

    fn extract_links(&self, arguments: &serde_json::Value) -> Result<serde_json::Value> {
        let session_id = require_string(arguments, "session_id")?;
        let max_links = arguments
            .get("max_links")
            .and_then(|v| v.as_u64())
            .unwrap_or(200)
            .min(2000) as usize;

        let sessions = self
            .sessions
            .lock()
            .map_err(|_| ToolError::ExecutionFailed("browser session lock poisoned".to_string()))?;
        let session = sessions.get(&session_id).ok_or_else(|| {
            ToolError::InvalidArguments(format!("unknown session_id: {session_id}"))
        })?;

        let links = extract_links_from_html(&session.html, &session.current_url, max_links);
        Ok(serde_json::json!({
            "session_id": session_id,
            "url": session.current_url,
            "link_count": links.len(),
            "links": links,
        }))
    }

    fn query_selector_all(&self, arguments: &serde_json::Value) -> Result<serde_json::Value> {
        let session_id = require_string(arguments, "session_id")?;
        let selector_raw = require_string(arguments, "selector")?;
        if selector_raw.trim().is_empty() {
            return Err(ToolError::InvalidArguments(
                "selector must not be empty".to_string(),
            ));
        }
        let selector = parse_simple_selector(&selector_raw).map_err(ToolError::InvalidArguments)?;
        let max_results = arguments
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(50)
            .min(500) as usize;
        let attr = optional_string(arguments, "attr")?
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let include_html = arguments
            .get("include_html")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let sessions = self
            .sessions
            .lock()
            .map_err(|_| ToolError::ExecutionFailed("browser session lock poisoned".to_string()))?;
        let session = sessions.get(&session_id).ok_or_else(|| {
            ToolError::InvalidArguments(format!("unknown session_id: {session_id}"))
        })?;

        let items = query_selector_all_html(
            &session.html,
            &selector,
            attr.as_deref(),
            include_html,
            max_results,
        )?;

        Ok(serde_json::json!({
            "session_id": session_id,
            "url": session.current_url,
            "selector": selector_raw,
            "match_count": items.len(),
            "items": items,
        }))
    }

    fn list_sessions(&self) -> Result<serde_json::Value> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| ToolError::ExecutionFailed("browser session lock poisoned".to_string()))?;
        let items = sessions
            .iter()
            .map(|(session_id, s)| {
                serde_json::json!({
                    "session_id": session_id,
                    "url": s.current_url,
                    "status_code": s.status_code,
                    "title": s.title,
                })
            })
            .collect::<Vec<_>>();
        Ok(serde_json::json!({ "sessions": items }))
    }

    fn close_session(&self, arguments: &serde_json::Value) -> Result<serde_json::Value> {
        let session_id = require_string(arguments, "session_id")?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| ToolError::ExecutionFailed("browser session lock poisoned".to_string()))?;
        let removed = sessions.remove(&session_id).is_some();
        Ok(serde_json::json!({
            "session_id": session_id,
            "removed": removed,
        }))
    }

    async fn screenshot(&self, arguments: &serde_json::Value) -> Result<serde_json::Value> {
        let raw_url = require_string(arguments, "url")?;
        let url = parse_http_url(&raw_url)?;

        let output_path = optional_string(arguments, "output_path")?
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!(
                    "opencraw-browser-{}.png",
                    self.next_session_id.fetch_add(1, Ordering::Relaxed)
                ))
            });

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        }

        let script = r#"
const { chromium } = require('playwright');
const url = process.argv[2];
const out = process.argv[3];
(async () => {
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30000 });
  await page.screenshot({ path: out, fullPage: true });
  await browser.close();
  process.stdout.write('ok');
})().catch((err) => {
  console.error(String(err && err.stack ? err.stack : err));
  process.exit(1);
});
"#;

        let mut cmd = Command::new("node");
        cmd.arg("-e")
            .arg(script)
            .arg(url.as_str())
            .arg(output_path.to_string_lossy().to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(playwright_cwd) = resolve_playwright_cwd() {
            cmd.current_dir(playwright_cwd);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        if !output.status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "screenshot failed (install playwright/chromium in ./web): status={} stderr={}",
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let metadata = tokio::fs::metadata(&output_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(serde_json::json!({
            "status": "ok",
            "url": url.to_string(),
            "output_path": output_path.display().to_string(),
            "bytes": metadata.len(),
        }))
    }
}

#[async_trait]
impl Tool for BrowserTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "browser".to_string(),
            description:
                "Managed browser sessions: navigate pages, extract text, take screenshots, and track session state."
                    .to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["navigate", "extract_text", "find", "extract_links", "query_selector_all", "screenshot", "list_sessions", "close_session"]
                    },
                    "url": { "type": "string" },
                    "session_id": { "type": "string" },
                    "selector": { "type": "string" },
                    "attr": { "type": "string" },
                    "include_html": { "type": "boolean" },
                    "pattern": { "type": "string" },
                    "case_sensitive": { "type": "boolean" },
                    "max_matches": { "type": "integer", "minimum": 1 },
                    "max_links": { "type": "integer", "minimum": 1 },
                    "max_results": { "type": "integer", "minimum": 1 },
                    "max_chars": { "type": "integer", "minimum": 1 },
                    "output_path": { "type": "string" }
                },
                "required": ["action"]
            }),
            risk_level: RiskLevel::Medium,
        }
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<serde_json::Value> {
        let action = require_string(&arguments, "action")?;
        match action.as_str() {
            "navigate" => self.navigate(&arguments).await,
            "extract_text" => self.extract_text(&arguments),
            "find" => self.find_text(&arguments),
            "extract_links" => self.extract_links(&arguments),
            "query_selector_all" => self.query_selector_all(&arguments),
            "screenshot" => self.screenshot(&arguments).await,
            "list_sessions" => self.list_sessions(),
            "close_session" => self.close_session(&arguments),
            other => Err(ToolError::InvalidArguments(format!(
                "unknown action: {other}"
            ))),
        }
    }
}

fn parse_http_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        other => Err(ToolError::InvalidArguments(format!(
            "unsupported scheme: {other}"
        ))),
    }
}

fn html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start_tag = "<title";
    let start_idx = lower.find(start_tag)?;
    let after_start = &html[start_idx..];
    let gt = after_start.find('>')?;
    let content_start = start_idx + gt + 1;
    let remaining = &html[content_start..];
    let end_rel = remaining.to_ascii_lowercase().find("</title>")?;
    Some(remaining[..end_rel].trim().to_string())
}

fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut inside_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => {
                inside_tag = true;
                out.push(' ');
            }
            '>' => inside_tag = false,
            _ => {
                if !inside_tag {
                    out.push(ch);
                }
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_whitespace(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone)]
struct SimpleSelector {
    tag: Option<String>,
    class: Option<String>,
    id: Option<String>,
}

impl SimpleSelector {
    fn matches_tag(&self, tag: &str) -> bool {
        self.tag
            .as_deref()
            .map(|expected| expected.eq_ignore_ascii_case(tag))
            .unwrap_or(true)
    }
}

fn parse_simple_selector(raw: &str) -> std::result::Result<SimpleSelector, String> {
    let selector = raw.trim();
    if selector.is_empty() {
        return Err("selector must not be empty".to_string());
    }
    if selector.contains(char::is_whitespace)
        || selector.contains('>')
        || selector.contains('+')
        || selector.contains('~')
        || selector.contains('[')
        || selector.contains(':')
    {
        return Err(
            "selector currently supports only simple forms: tag, .class, #id, tag.class, tag#id"
                .to_string(),
        );
    }

    let mut tag = None;
    let mut class = None;
    let mut id = None;

    let mut cursor = 0usize;
    let bytes = selector.as_bytes();
    while cursor < bytes.len() {
        let marker = bytes[cursor] as char;
        if marker == '.' || marker == '#' {
            let start = cursor + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] as char != '.' && bytes[end] as char != '#' {
                end += 1;
            }
            if start == end {
                return Err("selector contains empty class/id component".to_string());
            }
            let value = selector[start..end].to_string();
            if marker == '.' {
                if class.is_some() {
                    return Err("selector supports a single .class component".to_string());
                }
                class = Some(value);
            } else {
                if id.is_some() {
                    return Err("selector supports a single #id component".to_string());
                }
                id = Some(value);
            }
            cursor = end;
            continue;
        }

        let mut end = cursor;
        while end < bytes.len() && bytes[end] as char != '.' && bytes[end] as char != '#' {
            end += 1;
        }
        if end == cursor {
            return Err("selector parse error".to_string());
        }
        let value = selector[cursor..end].to_ascii_lowercase();
        if value.is_empty() {
            return Err("selector tag component is empty".to_string());
        }
        if tag.is_some() {
            return Err("selector supports a single tag component".to_string());
        }
        tag = Some(value);
        cursor = end;
    }

    if tag.is_none() && class.is_none() && id.is_none() {
        return Err("selector must include tag, class, or id".to_string());
    }

    Ok(SimpleSelector { tag, class, id })
}

fn query_selector_all_html(
    html: &str,
    selector: &SimpleSelector,
    attr_name: Option<&str>,
    include_html: bool,
    max_results: usize,
) -> Result<Vec<serde_json::Value>> {
    if max_results == 0 {
        return Ok(Vec::new());
    }

    let open_tag = Regex::new(r#"(?is)<([a-zA-Z][a-zA-Z0-9:_-]*)([^>]*)>"#)
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let lower_html = html.to_ascii_lowercase();
    let mut items = Vec::new();

    for capture in open_tag.captures_iter(html) {
        if items.len() >= max_results {
            break;
        }

        let Some(full_match) = capture.get(0) else {
            continue;
        };
        let Some(tag_match) = capture.get(1) else {
            continue;
        };
        let attrs = capture.get(2).map(|m| m.as_str()).unwrap_or_default();
        let tag = tag_match.as_str().to_ascii_lowercase();

        if !selector.matches_tag(&tag) {
            continue;
        }
        if let Some(expected_id) = selector.id.as_deref() {
            let matched = extract_attr_value(attrs, "id")
                .as_deref()
                .map(|v| v == expected_id)
                .unwrap_or(false);
            if !matched {
                continue;
            }
        }
        if let Some(expected_class) = selector.class.as_deref() {
            let matched = extract_attr_value(attrs, "class")
                .as_deref()
                .map(|v| v.split_whitespace().any(|cls| cls == expected_class))
                .unwrap_or(false);
            if !matched {
                continue;
            }
        }

        let content_start = full_match.end();
        let close_marker = format!("</{}>", tag);
        let content_end = lower_html[content_start..]
            .find(&close_marker)
            .map(|offset| content_start + offset);
        let inner_html = content_end
            .map(|end| &html[content_start..end])
            .unwrap_or_default();
        let text = normalize_whitespace(&html_to_text(inner_html));
        let mut item = serde_json::json!({
            "tag": tag,
            "text": text,
        });

        if let Some(attr_name) = attr_name {
            item["attr"] = serde_json::json!(attr_name);
            item["attr_value"] = serde_json::json!(extract_attr_value(attrs, attr_name));
        }

        if include_html {
            let html_fragment = if let Some(end) = content_end {
                &html[full_match.start()..end + close_marker.len()]
            } else {
                full_match.as_str()
            };
            item["html"] = serde_json::json!(html_fragment);
        }

        items.push(item);
    }

    Ok(items)
}

fn extract_attr_value(attrs: &str, attr_name: &str) -> Option<String> {
    if attr_name.trim().is_empty() {
        return None;
    }
    let pattern = format!(
        r#"(?is)\b{}\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+))"#,
        regex::escape(attr_name)
    );
    let regex = Regex::new(&pattern).ok()?;
    let captures = regex.captures(attrs)?;
    for index in 1..=3 {
        if let Some(value) = captures.get(index) {
            return Some(value.as_str().to_string());
        }
    }
    None
}

fn find_in_text(
    haystack: &str,
    pattern: &str,
    case_sensitive: bool,
    max_matches: usize,
) -> Vec<serde_json::Value> {
    if max_matches == 0 {
        return Vec::new();
    }
    let (search_haystack, search_pattern) = if case_sensitive {
        (haystack.to_string(), pattern.to_string())
    } else {
        (haystack.to_ascii_lowercase(), pattern.to_ascii_lowercase())
    };
    let mut cursor = 0usize;
    let mut matches = Vec::new();
    while cursor < search_haystack.len() && matches.len() < max_matches {
        let Some(relative_idx) = search_haystack[cursor..].find(&search_pattern) else {
            break;
        };
        let start = cursor + relative_idx;
        let end = start + search_pattern.len();
        let snippet_start = start.saturating_sub(60);
        let snippet_end = (end + 60).min(haystack.len());
        matches.push(serde_json::json!({
            "start": start,
            "end": end,
            "snippet": haystack[snippet_start..snippet_end].to_string(),
        }));
        cursor = end;
    }
    matches
}

fn extract_links_from_html(html: &str, base_url: &str, max_links: usize) -> Vec<String> {
    if max_links == 0 {
        return Vec::new();
    }
    let base = Url::parse(base_url).ok();
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0usize;
    let mut links = Vec::new();
    let mut seen = HashSet::new();

    while cursor < lower.len() && links.len() < max_links {
        let Some(relative_idx) = lower[cursor..].find("href=") else {
            break;
        };
        let href_idx = cursor + relative_idx + "href=".len();
        let bytes = html.as_bytes();
        if href_idx >= bytes.len() {
            break;
        }

        let quote = bytes[href_idx];
        let (value_start, value_end) = if quote == b'"' || quote == b'\'' {
            let value_start = href_idx + 1;
            let mut value_end = value_start;
            while value_end < bytes.len() && bytes[value_end] != quote {
                value_end += 1;
            }
            (value_start, value_end)
        } else {
            let value_start = href_idx;
            let mut value_end = value_start;
            while value_end < bytes.len()
                && !matches!(bytes[value_end], b' ' | b'\t' | b'\n' | b'\r' | b'>')
            {
                value_end += 1;
            }
            (value_start, value_end)
        };

        cursor = value_end;
        if value_start >= bytes.len() || value_start >= value_end {
            continue;
        }
        let raw_href = html[value_start..value_end].trim();
        if raw_href.is_empty() {
            continue;
        }
        if raw_href.starts_with('#')
            || raw_href.to_ascii_lowercase().starts_with("javascript:")
            || raw_href.to_ascii_lowercase().starts_with("mailto:")
        {
            continue;
        }

        let normalized = if let Ok(absolute) = Url::parse(raw_href) {
            absolute.to_string()
        } else if let Some(base) = base.as_ref() {
            match base.join(raw_href) {
                Ok(joined) => joined.to_string(),
                Err(_) => continue,
            }
        } else {
            continue;
        };
        if seen.insert(normalized.clone()) {
            links.push(normalized);
        }
    }

    links
}

fn resolve_playwright_cwd() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let web_dir = cwd.join("web");
    if web_dir.join("node_modules").exists() {
        Some(web_dir)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extract_links_from_html, find_in_text, html_title, html_to_text, parse_http_url,
        parse_simple_selector, query_selector_all_html,
    };

    #[test]
    fn parse_http_url_rejects_non_http_scheme() {
        assert!(parse_http_url("file:///tmp/example").is_err());
        assert!(parse_http_url("https://example.com").is_ok());
    }

    #[test]
    fn html_title_extracts_title() {
        let html = "<html><head><title>OpenCraw</title></head><body>ok</body></html>";
        assert_eq!(html_title(html).as_deref(), Some("OpenCraw"));
    }

    #[test]
    fn html_to_text_strips_tags() {
        let html = "<p>Hello <strong>world</strong></p>";
        assert_eq!(html_to_text(html), "Hello world");
    }

    #[test]
    fn extract_links_from_html_normalizes_relative_and_dedupes() {
        let html = r#"
            <a href="/docs">Docs</a>
            <a href="https://example.com/docs">Docs2</a>
            <a href="/docs">Dup</a>
            <a href="mailto:test@example.com">Mail</a>
        "#;
        let links = extract_links_from_html(html, "https://example.com/base", 10);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], "https://example.com/docs");
    }

    #[test]
    fn find_in_text_returns_bounded_match_snippets() {
        let text = "alpha beta gamma beta delta";
        let matches = find_in_text(text, "beta", false, 10);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].get("start").and_then(|v| v.as_u64()), Some(6));
        assert_eq!(matches[1].get("start").and_then(|v| v.as_u64()), Some(17));
    }

    #[test]
    fn selector_query_extracts_text_and_attr() {
        let html = r#"
            <ul>
              <li><a href="/a"> Alpha </a></li>
              <li><a href="/b">Beta</a></li>
            </ul>
        "#;
        let selector = parse_simple_selector("a").expect("selector should parse");
        let items = query_selector_all_html(html, &selector, Some("href"), false, 10)
            .expect("selector query should succeed");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].get("text").and_then(|v| v.as_str()), Some("Alpha"));
        assert_eq!(
            items[0].get("attr_value").and_then(|v| v.as_str()),
            Some("/a")
        );
        assert_eq!(items[1].get("text").and_then(|v| v.as_str()), Some("Beta"));
        assert_eq!(
            items[1].get("attr_value").and_then(|v| v.as_str()),
            Some("/b")
        );
    }

    #[test]
    fn simple_selector_parser_rejects_complex_combinators() {
        let err = parse_simple_selector("div a").expect_err("combinators should fail");
        assert!(err.contains("simple forms"));
    }
}
