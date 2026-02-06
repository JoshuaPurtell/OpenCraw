use crate::error::{LlmError, Result};
use crate::types::{ChatMessage, ChatResponse, Role, StreamChunk, ToolCall, ToolDefinition, Usage};
use bytes::Bytes;
use futures_util::Stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;

const OPENAI_CHAT_COMPLETIONS_URL: &str = "https://api.openai.com/v1/chat/completions";

#[derive(Clone)]
pub struct OpenAiClient {
    http: reqwest::Client,
    api_key: String,
    model: String,
}

impl OpenAiClient {
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
        let req = OpenAiChatRequest::new(&self.model, messages, tools, false);

        let response = self
            .http
            .post(OPENAI_CHAT_COMPLETIONS_URL)
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(LlmError::Http(format!(
                "openai chat status={status} body={body}"
            )));
        }

        let parsed: OpenAiChatResponse = serde_json::from_str(&body)?;
        parsed.try_into()
    }

    #[tracing::instrument(level = "info", skip_all)]
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>> {
        let req = OpenAiChatRequest::new(&self.model, messages, tools, true);

        let response = self
            .http
            .post(OPENAI_CHAT_COMPLETIONS_URL)
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::Http(format!(
                "openai stream status={status} body={body}"
            )));
        }

        let state = OpenAiStreamState::new();
        let sse = Box::pin(decode_sse(response.bytes_stream()));

        let stream =
            futures_util::stream::unfold((sse, state), |(mut sse, mut state)| async move {
                loop {
                    let next = sse.as_mut().next().await?;
                    match next {
                        Ok(SseEvent::Data(data)) => {
                            if data.trim() == "[DONE]" {
                                let usage = state.usage.clone().unwrap_or(Usage {
                                    prompt_tokens: 0,
                                    completion_tokens: 0,
                                });
                                return Some((Ok(StreamChunk::Done { usage }), (sse, state)));
                            }

                            let chunk: OpenAiStreamResponseChunk = match serde_json::from_str(&data)
                            {
                                Ok(v) => v,
                                Err(e) => {
                                    return Some((
                                        Err(LlmError::StreamParse(format!(
                                            "openai chunk json error={e} data={data}"
                                        ))),
                                        (sse, state),
                                    ));
                                }
                            };

                            if let Some(u) = chunk.usage.as_ref() {
                                state.usage = Some(Usage {
                                    prompt_tokens: u.prompt_tokens.unwrap_or(0) as u32,
                                    completion_tokens: u.completion_tokens.unwrap_or(0) as u32,
                                });
                            }

                            let Some(choice) = chunk.choices.first() else {
                                continue;
                            };
                            let delta = &choice.delta;
                            if let Some(content) = delta.content.as_ref() {
                                if !content.is_empty() {
                                    return Some((
                                        Ok(StreamChunk::Delta {
                                            content: content.clone(),
                                        }),
                                        (sse, state),
                                    ));
                                }
                            }

                            if let Some(tool_calls) = delta.tool_calls.as_ref() {
                                for tc in tool_calls {
                                    let idx = tc.index.unwrap_or(0);
                                    let entry = state.tool_calls.entry(idx).or_default();
                                    if entry.id.is_none() {
                                        entry.id = tc.id.clone();
                                    }
                                    if entry.name.is_none() {
                                        entry.name =
                                            tc.function.as_ref().and_then(|f| f.name.clone());
                                    }

                                    if !entry.started {
                                        let Some(id) = entry.id.clone() else {
                                            continue;
                                        };
                                        let Some(name) = entry.name.clone() else {
                                            continue;
                                        };
                                        entry.started = true;
                                        return Some((
                                            Ok(StreamChunk::ToolCallStart { id, name }),
                                            (sse, state),
                                        ));
                                    }

                                    let arguments = tc
                                        .function
                                        .as_ref()
                                        .and_then(|f| f.arguments.clone())
                                        .unwrap_or_default();
                                    if !arguments.is_empty() {
                                        return Some((
                                            Ok(StreamChunk::ToolCallDelta { arguments }),
                                            (sse, state),
                                        ));
                                    }
                                }
                            }
                        }
                        Ok(SseEvent::Other) => continue,
                        Err(e) => return Some((Err(e), (sse, state))),
                    }
                }
            });

        Ok(Box::pin(stream))
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OpenAiTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<OpenAiStreamOptions>,
}

#[derive(Debug, Serialize)]
struct OpenAiStreamOptions {
    include_usage: bool,
}

impl OpenAiChatRequest {
    fn new(model: &str, messages: &[ChatMessage], tools: &[ToolDefinition], stream: bool) -> Self {
        let mut out = Self {
            model: model.to_string(),
            messages: messages.iter().map(to_openai_message).collect(),
            tools: tools.iter().map(to_openai_tool).collect(),
            tool_choice: None,
            stream: None,
            stream_options: None,
        };

        if !out.tools.is_empty() {
            out.tool_choice = Some("auto".to_string());
        }

        if stream {
            out.stream = Some(true);
            out.stream_options = Some(OpenAiStreamOptions {
                include_usage: true,
            });
        }

        out
    }
}

#[derive(Debug, Serialize)]
struct OpenAiTool {
    r#type: String,
    function: OpenAiToolFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiToolFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

fn to_openai_tool(t: &ToolDefinition) -> OpenAiTool {
    OpenAiTool {
        r#type: "function".to_string(),
        function: OpenAiToolFunction {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t.parameters.clone(),
        },
    }
}

#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<OpenAiToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAiToolCall {
    id: String,
    r#type: String,
    function: OpenAiToolFunctionCall,
}

#[derive(Debug, Serialize)]
struct OpenAiToolFunctionCall {
    name: String,
    arguments: String,
}

fn to_openai_message(m: &ChatMessage) -> OpenAiMessage {
    let role = match m.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };
    OpenAiMessage {
        role: role.to_string(),
        content: Some(m.content.clone()).filter(|s| !s.is_empty()),
        tool_calls: m
            .tool_calls
            .iter()
            .map(|tc| OpenAiToolCall {
                id: tc.id.clone(),
                r#type: "function".to_string(),
                function: OpenAiToolFunctionCall {
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                },
            })
            .collect(),
        tool_call_id: m.tool_call_id.clone(),
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiChoiceMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoiceMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<OpenAiChoiceToolCall>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoiceToolCall {
    id: String,
    #[serde(default)]
    function: OpenAiChoiceToolCallFunction,
}

#[derive(Debug, Deserialize, Default)]
struct OpenAiChoiceToolCallFunction {
    #[serde(default)]
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

impl TryFrom<OpenAiChatResponse> for ChatResponse {
    type Error = LlmError;

    fn try_from(v: OpenAiChatResponse) -> Result<Self> {
        let choice = v.choices.into_iter().next().ok_or_else(|| {
            LlmError::ResponseFormat("openai response missing choices".to_string())
        })?;

        let usage = v.usage.unwrap_or(OpenAiUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
        });

        let tool_calls = choice
            .message
            .tool_calls
            .into_iter()
            .map(|tc| ToolCall {
                id: tc.id,
                name: tc.function.name,
                arguments: tc.function.arguments,
            })
            .collect();

        Ok(ChatResponse {
            message: ChatMessage {
                role: Role::Assistant,
                content: choice.message.content.unwrap_or_default(),
                tool_calls,
                tool_call_id: None,
            },
            usage: Usage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
            },
            finish_reason: choice
                .finish_reason
                .unwrap_or_else(|| "unknown".to_string()),
        })
    }
}

#[derive(Debug)]
enum SseEvent {
    Data(String),
    Other,
}

fn decode_sse<S>(bytes_stream: S) -> impl Stream<Item = Result<SseEvent>> + Send
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

                    let mut data_lines = Vec::new();
                    for line in raw.lines() {
                        let line = line.trim_end();
                        if let Some(rest) = line.strip_prefix("data:") {
                            data_lines.push(rest.trim_start().to_string());
                        }
                    }
                    if data_lines.is_empty() {
                        return Some((Ok(SseEvent::Other), (stream, buffer)));
                    }
                    return Some((Ok(SseEvent::Data(data_lines.join("\n"))), (stream, buffer)));
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

#[derive(Debug, Deserialize)]
struct OpenAiStreamResponseChunk {
    #[serde(default)]
    choices: Vec<OpenAiStreamChoice>,
    #[serde(default)]
    usage: Option<OpenAiStreamUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamUsage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChoice {
    delta: OpenAiStreamDelta,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiStreamDeltaToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamDeltaToolCall {
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<OpenAiStreamDeltaToolFunction>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamDeltaToolFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Default)]
struct OpenAiStreamToolCallState {
    id: Option<String>,
    name: Option<String>,
    started: bool,
}

#[derive(Debug)]
struct OpenAiStreamState {
    tool_calls: HashMap<u32, OpenAiStreamToolCallState>,
    usage: Option<Usage>,
}

impl OpenAiStreamState {
    fn new() -> Self {
        Self {
            tool_calls: HashMap::new(),
            usage: None,
        }
    }
}
