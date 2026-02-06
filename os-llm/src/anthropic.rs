use crate::error::{LlmError, Result};
use crate::types::{ChatMessage, ChatResponse, Role, StreamChunk, ToolCall, ToolDefinition, Usage};
use bytes::Bytes;
use futures_util::Stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;

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
            return Err(LlmError::Http(format!(
                "anthropic chat status={status} body={body}"
            )));
        }

        let parsed: AnthropicResponse = serde_json::from_str(&body)?;
        parsed.try_into()
    }

    #[tracing::instrument(level = "info", skip_all)]
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>> {
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
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::Http(format!(
                "anthropic stream status={status} body={body}"
            )));
        }

        let sse = Box::pin(decode_sse(response.bytes_stream()));
        let state = AnthropicStreamState::new();

        let stream =
            futures_util::stream::unfold((sse, state), |(mut sse, mut state)| async move {
                loop {
                    let next = sse.as_mut().next().await?;
                    let (event_name, data) = match next {
                        Ok(v) => v,
                        Err(e) => return Some((Err(e), (sse, state))),
                    };

                    match event_name.as_str() {
                        "message_start" => {
                            if let Ok(v) = serde_json::from_str::<AnthropicMessageStart>(&data) {
                                state.usage.prompt_tokens = v.message.usage.input_tokens as u32;
                                state.usage.completion_tokens =
                                    v.message.usage.output_tokens as u32;
                            }
                        }
                        "content_block_start" => {
                            if let Ok(v) = serde_json::from_str::<AnthropicContentBlockStart>(&data)
                            {
                                if let AnthropicContentBlock::ToolUse { id, name, .. } =
                                    v.content_block
                                {
                                    state.tool_started.insert(id.clone(), true);
                                    return Some((
                                        Ok(StreamChunk::ToolCallStart { id, name }),
                                        (sse, state),
                                    ));
                                }
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
                            if let Ok(v) = serde_json::from_str::<AnthropicMessageDelta>(&data) {
                                if let Some(u) = v.usage {
                                    state.usage.prompt_tokens = u.input_tokens as u32;
                                    state.usage.completion_tokens = u.output_tokens as u32;
                                }
                            }
                        }
                        "message_stop" => {
                            let usage = state.usage.clone();
                            return Some((Ok(StreamChunk::Done { usage }), (sse, state)));
                        }
                        _ => {}
                    }
                }
            });

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
        let mut system = String::new();
        let mut out_messages = Vec::new();

        for m in messages {
            match m.role {
                Role::System => {
                    if !system.is_empty() {
                        system.push_str("\n");
                    }
                    system.push_str(m.content.trim());
                }
                Role::User => out_messages.push(to_anthropic_user_message(m)),
                Role::Assistant => out_messages.push(to_anthropic_assistant_message(m)?),
                Role::Tool => out_messages.push(to_anthropic_tool_result_message(m)),
            }
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

fn to_anthropic_tool_result_message(m: &ChatMessage) -> AnthropicMessage {
    let tool_use_id = m.tool_call_id.clone().unwrap_or_default();
    AnthropicMessage {
        role: "user".to_string(),
        content: vec![AnthropicContentBlock::ToolResult {
            tool_use_id,
            content: m.content.clone(),
        }],
    }
}

fn to_anthropic_assistant_message(m: &ChatMessage) -> Result<AnthropicMessage> {
    let mut blocks = Vec::new();
    if !m.content.trim().is_empty() {
        blocks.push(AnthropicContentBlock::Text {
            text: m.content.clone(),
        });
    }
    for tc in &m.tool_calls {
        let input: serde_json::Value = match serde_json::from_str(&tc.arguments) {
            Ok(v) => v,
            Err(_) => serde_json::json!({}),
        };
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

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    stop_reason: String,
    #[serde(default)]
    usage: AnthropicUsage,
}

#[derive(Debug, Default, Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
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
                        return Some((Err(LlmError::Http(e.to_string())), (stream, buffer)))
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
