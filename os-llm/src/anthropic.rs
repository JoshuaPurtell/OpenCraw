use crate::error::{LlmError, Result};
use crate::types::{ChatMessage, ChatResponse, Role, StreamChunk, ToolCall, ToolDefinition, Usage};
use bytes::Bytes;
use futures_util::Stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::time::Instant;

const ANTHROPIC_MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Clone)]
pub struct AnthropicClient {
    http: reqwest::Client,
    api_key: String,
    model: String,
}

impl AnthropicClient {
    pub fn new(http: reqwest::Client, api_key: &str, model: &str) -> Self {
        Self {
            http,
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }

    #[tracing::instrument(level = "info", skip_all)]
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ChatResponse> {
        tracing::debug!(
            model = %self.model,
            message_count = messages.len(),
            tool_count = tools.len(),
            "anthropic chat request build started"
        );
        let started = Instant::now();
        let req = AnthropicRequest::new(&self.model, messages, tools, false)?;

        let response = self
            .http
            .post(ANTHROPIC_MESSAGES_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&req)
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            tracing::error!(
                model = %self.model,
                status = %status,
                body_len = body.len(),
                latency_ms = started.elapsed().as_millis() as u64,
                "anthropic chat request failed"
            );
            return Err(LlmError::Http(format!(
                "anthropic chat status={status} body={body}"
            )));
        }

        let parsed: AnthropicResponse = serde_json::from_str(&body)?;
        let chat_response: ChatResponse = parsed.try_into()?;
        tracing::info!(
            model = %self.model,
            status = %status,
            latency_ms = started.elapsed().as_millis() as u64,
            prompt_tokens = chat_response.usage.prompt_tokens,
            completion_tokens = chat_response.usage.completion_tokens,
            tool_calls = chat_response.message.tool_calls.len(),
            finish_reason = %chat_response.finish_reason,
            "anthropic chat request completed"
        );
        Ok(chat_response)
    }

    #[tracing::instrument(level = "info", skip_all)]
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>> {
        tracing::debug!(
            model = %self.model,
            message_count = messages.len(),
            tool_count = tools.len(),
            "anthropic stream request build started"
        );
        let started = Instant::now();
        let req = AnthropicRequest::new(&self.model, messages, tools, true)?;

        let response = self
            .http
            .post(ANTHROPIC_MESSAGES_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&req)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await?;
            tracing::error!(
                model = %self.model,
                status = %status,
                body_len = body.len(),
                latency_ms = started.elapsed().as_millis() as u64,
                "anthropic stream request failed"
            );
            return Err(LlmError::Http(format!(
                "anthropic stream status={status} body={body}"
            )));
        }
        tracing::info!(
            model = %self.model,
            status = %status,
            latency_ms = started.elapsed().as_millis() as u64,
            "anthropic stream request established"
        );

        let sse = Box::pin(decode_sse(response.bytes_stream()));
        let state = AnthropicStreamState::new();

        let stream = futures_util::stream::unfold(
            (sse, state),
            |(mut sse, mut state)| async move {
                loop {
                    let next = sse.as_mut().next().await?;
                    let (event_name, data) = match next {
                        Ok(v) => v,
                        Err(e) => return Some((Err(e), (sse, state))),
                    };

                    match event_name.as_str() {
                        "message_start" => {
                            let v: AnthropicMessageStart = match serde_json::from_str(&data) {
                                Ok(v) => v,
                                Err(e) => {
                                    return Some((
                                        Err(LlmError::StreamParse(format!(
                                            "anthropic message_start json error={e} data={data}"
                                        ))),
                                        (sse, state),
                                    ));
                                }
                            };
                            state.usage.prompt_tokens = v.message.usage.input_tokens as u32;
                            state.usage.completion_tokens = v.message.usage.output_tokens as u32;
                        }
                        "content_block_start" => {
                            let v: AnthropicContentBlockStart = match serde_json::from_str(&data) {
                                Ok(v) => v,
                                Err(e) => {
                                    return Some((
                                        Err(LlmError::StreamParse(format!(
                                            "anthropic content_block_start json error={e} data={data}"
                                        ))),
                                        (sse, state),
                                    ));
                                }
                            };
                            if let AnthropicContentBlock::ToolUse { id, name, .. } = v.content_block
                            {
                                state.tool_started.insert(id.clone(), true);
                                return Some((
                                    Ok(StreamChunk::ToolCallStart { id, name }),
                                    (sse, state),
                                ));
                            }
                        }
                        "content_block_delta" => {
                            let v: AnthropicContentBlockDelta = match serde_json::from_str(&data) {
                                Ok(v) => v,
                                Err(e) => {
                                    return Some((
                                        Err(LlmError::StreamParse(format!(
                                            "anthropic delta json error={e} data={data}"
                                        ))),
                                        (sse, state),
                                    ));
                                }
                            };
                            match v.delta {
                                AnthropicDelta::TextDelta { text } => {
                                    if !text.is_empty() {
                                        return Some((
                                            Ok(StreamChunk::Delta { content: text }),
                                            (sse, state),
                                        ));
                                    }
                                }
                                AnthropicDelta::InputJsonDelta { partial_json } => {
                                    if !partial_json.is_empty() {
                                        return Some((
                                            Ok(StreamChunk::ToolCallDelta {
                                                arguments: partial_json,
                                            }),
                                            (sse, state),
                                        ));
                                    }
                                }
                            }
                        }
                        "message_delta" => {
                            let v: AnthropicMessageDelta = match serde_json::from_str(&data) {
                                Ok(v) => v,
                                Err(e) => {
                                    return Some((
                                        Err(LlmError::StreamParse(format!(
                                            "anthropic message_delta json error={e} data={data}"
                                        ))),
                                        (sse, state),
                                    ));
                                }
                            };
                            if let Some(u) = v.usage {
                                state.usage.prompt_tokens = u.input_tokens as u32;
                                state.usage.completion_tokens = u.output_tokens as u32;
                            }
                        }
                        "message_stop" => {
                            let usage = state.usage.clone();
                            return Some((Ok(StreamChunk::Done { usage }), (sse, state)));
                        }
                        _ => {}
                    }
                }
            },
        );

        Ok(Box::pin(stream))
    }
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<AnthropicTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

impl AnthropicRequest {
    fn new(
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        stream: bool,
    ) -> Result<Self> {
        let (system, out_messages) = build_anthropic_messages(messages)?;
        let (out_messages, dropped_tool_results) =
            sanitize_anthropic_tool_result_contract(out_messages);
        if dropped_tool_results > 0 {
            tracing::warn!(
                dropped_tool_results,
                "sanitized anthropic tool_result contract before request"
            );
        }

        Ok(Self {
            model: model.to_string(),
            max_tokens: 2048,
            system,
            messages: out_messages,
            tools: tools.iter().map(to_anthropic_tool).collect(),
            stream: if stream { Some(true) } else { None },
        })
    }
}

fn build_anthropic_messages(messages: &[ChatMessage]) -> Result<(String, Vec<AnthropicMessage>)> {
    let mut system = String::new();
    let mut out_messages = Vec::new();
    let mut idx = 0usize;

    while idx < messages.len() {
        let message = &messages[idx];
        match message.role {
            Role::System => {
                if !system.is_empty() {
                    system.push('\n');
                }
                system.push_str(message.content.trim());
                idx += 1;
            }
            Role::User => {
                out_messages.push(to_anthropic_user_message(message));
                idx += 1;
            }
            Role::Assistant => {
                out_messages.push(to_anthropic_assistant_message(message)?);
                idx += 1;
            }
            Role::Tool => {
                // Anthropic requires tool_result blocks to be carried in a single user message
                // following the assistant tool_use turn.
                let mut blocks = Vec::new();
                while idx < messages.len() && messages[idx].role == Role::Tool {
                    blocks.push(to_anthropic_tool_result_block(&messages[idx])?);
                    idx += 1;
                }
                if !blocks.is_empty() {
                    out_messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: blocks,
                    });
                }
            }
        }
    }

    Ok((system, out_messages))
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

fn to_anthropic_tool(t: &ToolDefinition) -> AnthropicTool {
    AnthropicTool {
        name: t.name.clone(),
        description: t.description.clone(),
        input_schema: t.parameters.clone(),
    }
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

fn to_anthropic_user_message(m: &ChatMessage) -> AnthropicMessage {
    AnthropicMessage {
        role: "user".to_string(),
        content: vec![AnthropicContentBlock::Text {
            text: m.content.clone(),
        }],
    }
}

fn to_anthropic_tool_result_block(m: &ChatMessage) -> Result<AnthropicContentBlock> {
    let tool_use_id = m.tool_call_id.clone().ok_or_else(|| {
        LlmError::InvalidInput("anthropic tool result message is missing tool_call_id".to_string())
    })?;
    Ok(AnthropicContentBlock::ToolResult {
        tool_use_id,
        content: m.content.clone(),
    })
}

fn to_anthropic_assistant_message(m: &ChatMessage) -> Result<AnthropicMessage> {
    let mut blocks = Vec::new();
    if !m.content.trim().is_empty() {
        blocks.push(AnthropicContentBlock::Text {
            text: m.content.clone(),
        });
    }
    for tc in &m.tool_calls {
        let input: serde_json::Value = serde_json::from_str(&tc.arguments).map_err(|e| {
            LlmError::InvalidInput(format!(
                "anthropic tool call arguments are not valid JSON for {}: {e}",
                tc.name
            ))
        })?;
        blocks.push(AnthropicContentBlock::ToolUse {
            id: tc.id.clone(),
            name: tc.name.clone(),
            input,
        });
    }
    Ok(AnthropicMessage {
        role: "assistant".to_string(),
        content: blocks,
    })
}

fn sanitize_anthropic_tool_result_contract(
    messages: Vec<AnthropicMessage>,
) -> (Vec<AnthropicMessage>, usize) {
    let mut out: Vec<AnthropicMessage> = Vec::with_capacity(messages.len());
    let mut dropped_tool_results = 0usize;

    for mut message in messages {
        let has_tool_result = message
            .content
            .iter()
            .any(|block| matches!(block, AnthropicContentBlock::ToolResult { .. }));
        if !has_tool_result {
            if !message.content.is_empty() {
                out.push(message);
            }
            continue;
        }

        let mut allowed_ids: HashSet<String> = out
            .last()
            .filter(|prev| prev.role == "assistant")
            .map(|prev| {
                prev.content
                    .iter()
                    .filter_map(|block| match block {
                        AnthropicContentBlock::ToolUse { id, .. } => Some(id.clone()),
                        _ => None,
                    })
                    .collect::<HashSet<String>>()
            })
            .unwrap_or_default();

        message.content.retain(|block| match block {
            AnthropicContentBlock::ToolResult { tool_use_id, .. } => {
                if allowed_ids.remove(tool_use_id) {
                    true
                } else {
                    dropped_tool_results = dropped_tool_results.saturating_add(1);
                    false
                }
            }
            _ => true,
        });

        if !message.content.is_empty() {
            out.push(message);
        }
    }

    (out, dropped_tool_results)
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    stop_reason: String,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u64,
    output_tokens: u64,
}

impl TryFrom<AnthropicResponse> for ChatResponse {
    type Error = LlmError;

    fn try_from(v: AnthropicResponse) -> Result<Self> {
        let mut content = String::new();
        let mut tool_calls = Vec::new();

        for block in v.content {
            match block {
                AnthropicContentBlock::Text { text } => content.push_str(&text),
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments: serde_json::to_string(&input)?,
                    });
                }
                AnthropicContentBlock::ToolResult { .. } => {}
            }
        }

        Ok(ChatResponse {
            message: ChatMessage {
                role: Role::Assistant,
                content,
                tool_calls,
                tool_call_id: None,
            },
            usage: Usage {
                prompt_tokens: v.usage.input_tokens as u32,
                completion_tokens: v.usage.output_tokens as u32,
            },
            finish_reason: v.stop_reason,
        })
    }
}

type SseItem = (String, String);

fn decode_sse<S>(bytes_stream: S) -> impl Stream<Item = Result<SseItem>> + Send
where
    S: Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Send + Unpin + 'static,
{
    futures_util::stream::unfold(
        (bytes_stream, String::new()),
        |(mut stream, mut buffer)| async move {
            loop {
                if let Some(idx) = buffer.find("\n\n") {
                    let raw = buffer[..idx].to_string();
                    buffer = buffer[idx + 2..].to_string();

                    let mut event = String::new();
                    let mut data_lines = Vec::new();

                    for line in raw.lines() {
                        let line = line.trim_end();
                        if let Some(rest) = line.strip_prefix("event:") {
                            event = rest.trim_start().to_string();
                            continue;
                        }
                        if let Some(rest) = line.strip_prefix("data:") {
                            data_lines.push(rest.trim_start().to_string());
                        }
                    }

                    let data = data_lines.join("\n");
                    if event.is_empty() && data.is_empty() {
                        continue;
                    }
                    if event.is_empty() {
                        event = "message".to_string();
                    }
                    return Some((Ok((event, data)), (stream, buffer)));
                }

                match stream.next().await {
                    Some(Ok(chunk)) => {
                        buffer.push_str(&String::from_utf8_lossy(&chunk));
                        continue;
                    }
                    Some(Err(e)) => {
                        return Some((Err(LlmError::Http(e.to_string())), (stream, buffer)));
                    }
                    None => return None,
                }
            }
        },
    )
}

#[derive(Debug)]
struct AnthropicStreamState {
    usage: Usage,
    tool_started: HashMap<String, bool>,
}

impl AnthropicStreamState {
    fn new() -> Self {
        Self {
            usage: Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
            },
            tool_started: HashMap::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageStart {
    message: AnthropicMessageStartMessage,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageStartMessage {
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlockStart {
    content_block: AnthropicContentBlock,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlockDelta {
    delta: AnthropicDelta,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicDelta {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageDelta {
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[cfg(test)]
mod tests {
    use super::{
        AnthropicContentBlock, AnthropicMessage, build_anthropic_messages,
        sanitize_anthropic_tool_result_contract,
    };
    use crate::types::{ChatMessage, Role, ToolCall};

    #[test]
    fn sanitize_anthropic_contract_drops_orphan_tool_result_not_after_assistant() {
        let messages = vec![
            AnthropicMessage {
                role: "user".to_string(),
                content: vec![AnthropicContentBlock::Text {
                    text: "hi".to_string(),
                }],
            },
            AnthropicMessage {
                role: "user".to_string(),
                content: vec![AnthropicContentBlock::ToolResult {
                    tool_use_id: "tool_1".to_string(),
                    content: "{\"ok\":true}".to_string(),
                }],
            },
        ];
        let (sanitized, dropped) = sanitize_anthropic_tool_result_contract(messages);
        assert_eq!(dropped, 1);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(sanitized[0].role, "user");
    }

    #[test]
    fn sanitize_anthropic_contract_drops_tool_result_with_unknown_tool_use_id() {
        let messages = vec![
            AnthropicMessage {
                role: "assistant".to_string(),
                content: vec![AnthropicContentBlock::ToolUse {
                    id: "tool_1".to_string(),
                    name: "linear".to_string(),
                    input: serde_json::json!({}),
                }],
            },
            AnthropicMessage {
                role: "user".to_string(),
                content: vec![
                    AnthropicContentBlock::ToolResult {
                        tool_use_id: "tool_2".to_string(),
                        content: "{\"ok\":false}".to_string(),
                    },
                    AnthropicContentBlock::Text {
                        text: "note".to_string(),
                    },
                ],
            },
        ];
        let (sanitized, dropped) = sanitize_anthropic_tool_result_contract(messages);
        assert_eq!(dropped, 1);
        assert_eq!(sanitized.len(), 2);
        assert_eq!(sanitized[1].role, "user");
        assert!(matches!(
            sanitized[1].content[0],
            AnthropicContentBlock::Text { .. }
        ));
    }

    #[test]
    fn sanitize_anthropic_contract_keeps_valid_tool_result() {
        let messages = vec![
            AnthropicMessage {
                role: "assistant".to_string(),
                content: vec![AnthropicContentBlock::ToolUse {
                    id: "tool_1".to_string(),
                    name: "linear".to_string(),
                    input: serde_json::json!({}),
                }],
            },
            AnthropicMessage {
                role: "user".to_string(),
                content: vec![AnthropicContentBlock::ToolResult {
                    tool_use_id: "tool_1".to_string(),
                    content: "{\"ok\":true}".to_string(),
                }],
            },
        ];
        let (sanitized, dropped) = sanitize_anthropic_tool_result_contract(messages);
        assert_eq!(dropped, 0);
        assert_eq!(sanitized.len(), 2);
        assert_eq!(sanitized[1].role, "user");
        assert_eq!(sanitized[1].content.len(), 1);
    }

    #[test]
    fn build_anthropic_messages_coalesces_consecutive_tool_results() {
        let messages = vec![
            ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: vec![
                    ToolCall {
                        id: "tool_1".to_string(),
                        name: "linear".to_string(),
                        arguments: "{}".to_string(),
                    },
                    ToolCall {
                        id: "tool_2".to_string(),
                        name: "linear".to_string(),
                        arguments: "{}".to_string(),
                    },
                ],
                tool_call_id: None,
            },
            ChatMessage {
                role: Role::Tool,
                content: "{\"ok\":true}".to_string(),
                tool_calls: vec![],
                tool_call_id: Some("tool_1".to_string()),
            },
            ChatMessage {
                role: Role::Tool,
                content: "{\"ok\":true}".to_string(),
                tool_calls: vec![],
                tool_call_id: Some("tool_2".to_string()),
            },
        ];

        let (_system, anthropic_messages) =
            build_anthropic_messages(&messages).expect("build anthropic messages");
        assert_eq!(anthropic_messages.len(), 2);
        assert_eq!(anthropic_messages[0].role, "assistant");
        assert_eq!(anthropic_messages[1].role, "user");
        assert_eq!(anthropic_messages[1].content.len(), 2);
        assert!(matches!(
            anthropic_messages[1].content[0],
            AnthropicContentBlock::ToolResult { .. }
        ));
        assert!(matches!(
            anthropic_messages[1].content[1],
            AnthropicContentBlock::ToolResult { .. }
        ));
    }
}
