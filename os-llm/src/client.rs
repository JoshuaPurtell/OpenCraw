use crate::anthropic::AnthropicClient;
use crate::error::LlmError;
use crate::error::Result;
use crate::openai::OpenAiClient;
use crate::types::{ChatMessage, ChatResponse, Role, StreamChunk, ToolCall, ToolDefinition};
use futures_util::Stream;
use std::collections::HashSet;
use std::pin::Pin;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    OpenAI,
    Anthropic,
}

const ALL_PROVIDERS: &[Provider] = &[Provider::OpenAI, Provider::Anthropic];

pub struct ToolNameConstraints {
    pub pattern: &'static str,
    pub max_length: usize,
}

impl Provider {
    pub fn tool_name_constraints(&self) -> ToolNameConstraints {
        match self {
            Provider::OpenAI => ToolNameConstraints {
                pattern: "^[a-zA-Z0-9_-]+$",
                max_length: 64,
            },
            Provider::Anthropic => ToolNameConstraints {
                pattern: "^[a-zA-Z0-9_-]+$",
                max_length: 128,
            },
        }
    }
}

fn matches_tool_name_pattern(name: &str, _pattern: &str) -> bool {
    // All current provider patterns are ^[a-zA-Z0-9_-]+$
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Validate a tool name against every provider's constraints.
/// Returns `Ok(())` if the name satisfies all providers, or an error describing the violation.
pub fn validate_tool_name_all_providers(name: &str) -> Result<()> {
    for provider in ALL_PROVIDERS {
        let c = provider.tool_name_constraints();
        if name.len() > c.max_length {
            return Err(LlmError::InvalidInput(format!(
                "tool name '{name}' exceeds {provider:?} max length {} (got {})",
                c.max_length,
                name.len()
            )));
        }
        if !matches_tool_name_pattern(name, c.pattern) {
            return Err(LlmError::InvalidInput(format!(
                "tool name '{name}' does not match {provider:?} pattern {}",
                c.pattern
            )));
        }
    }
    Ok(())
}

fn validate_request_payload(messages: &[ChatMessage], tools: &[ToolDefinition]) -> Result<()> {
    let mut tool_names: HashSet<&str> = HashSet::with_capacity(tools.len());
    for tool in tools {
        validate_tool_name_all_providers(&tool.name)?;
        if !tool_names.insert(tool.name.as_str()) {
            return Err(LlmError::InvalidInput(format!(
                "duplicate tool definition name '{}'",
                tool.name
            )));
        }
    }

    let mut active_tool_call_ids: HashSet<&str> = HashSet::new();
    let mut active_tool_turn = false;
    for (message_idx, message) in messages.iter().enumerate() {
        if message.role == Role::Assistant {
            active_tool_call_ids.clear();
            for tool_call in &message.tool_calls {
                active_tool_call_ids.insert(tool_call.id.as_str());
            }
            active_tool_turn = !active_tool_call_ids.is_empty();
        } else if message.role != Role::Tool {
            active_tool_call_ids.clear();
            active_tool_turn = false;
        }

        if message.role == Role::Tool {
            let tool_call_id = message.tool_call_id.as_deref().ok_or_else(|| {
                LlmError::InvalidInput(format!(
                    "message[{message_idx}] is a tool result missing tool_call_id"
                ))
            })?;
            if !active_tool_turn {
                return Err(LlmError::InvalidInput(format!(
                    "message[{message_idx}] is a tool result without a preceding assistant tool_use"
                )));
            }
            if !active_tool_call_ids.remove(tool_call_id) {
                return Err(LlmError::InvalidInput(format!(
                    "message[{message_idx}] tool_result references unknown or duplicate tool_call_id '{}'",
                    tool_call_id
                )));
            }
        }

        for (tool_call_idx, tool_call) in message.tool_calls.iter().enumerate() {
            if tool_call.id.trim().is_empty() {
                return Err(LlmError::InvalidInput(format!(
                    "message[{message_idx}] tool_calls[{tool_call_idx}] has empty id"
                )));
            }

            validate_tool_name_all_providers(&tool_call.name)?;
            if !tool_names.contains(tool_call.name.as_str()) {
                return Err(LlmError::InvalidInput(format!(
                    "message[{message_idx}] tool_calls[{tool_call_idx}] references unknown tool '{}'",
                    tool_call.name
                )));
            }

            serde_json::from_str::<serde_json::Value>(&tool_call.arguments).map_err(|e| {
                LlmError::InvalidInput(format!(
                    "message[{message_idx}] tool_calls[{tool_call_idx}] has invalid JSON arguments for '{}': {e}",
                    tool_call.name
                ))
            })?;
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct CanonicalToolExchange {
    call: ToolCall,
    result_content: String,
}

#[derive(Debug, Clone)]
enum CanonicalMessage {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: String,
        tool_exchanges: Vec<CanonicalToolExchange>,
    },
}

#[derive(Debug, Clone, Default)]
struct CanonicalConversation {
    messages: Vec<CanonicalMessage>,
    dropped_orphan_tool_results: usize,
    stripped_orphan_assistant_calls: usize,
    normalized_invalid_tool_arguments: usize,
}

fn sanitize_request_messages(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let canonical = canonicalize_messages(messages);
    if canonical.dropped_orphan_tool_results > 0
        || canonical.stripped_orphan_assistant_calls > 0
        || canonical.normalized_invalid_tool_arguments > 0
    {
        tracing::debug!(
            dropped_orphan_tool_results = canonical.dropped_orphan_tool_results,
            stripped_orphan_assistant_calls = canonical.stripped_orphan_assistant_calls,
            normalized_invalid_tool_arguments = canonical.normalized_invalid_tool_arguments,
            "sanitized tool-call contract before LLM request"
        );
    }

    materialize_canonical_messages(&canonical.messages)
}

fn canonicalize_messages(messages: &[ChatMessage]) -> CanonicalConversation {
    let mut out = CanonicalConversation::default();
    let mut idx = 0usize;

    while idx < messages.len() {
        let message = &messages[idx];
        match message.role {
            Role::System => {
                out.messages.push(CanonicalMessage::System {
                    content: message.content.clone(),
                });
                idx += 1;
            }
            Role::User => {
                out.messages.push(CanonicalMessage::User {
                    content: message.content.clone(),
                });
                idx += 1;
            }
            Role::Assistant => {
                let mut lookahead = idx + 1;
                let mut tool_results: Vec<&ChatMessage> = Vec::new();
                while lookahead < messages.len() && messages[lookahead].role == Role::Tool {
                    tool_results.push(&messages[lookahead]);
                    lookahead += 1;
                }

                let (tool_exchanges, dropped_results, stripped_calls, normalized_args) =
                    canonicalize_assistant_tools(&message.tool_calls, &tool_results);
                out.dropped_orphan_tool_results += dropped_results;
                out.stripped_orphan_assistant_calls += stripped_calls;
                out.normalized_invalid_tool_arguments += normalized_args;

                out.messages.push(CanonicalMessage::Assistant {
                    content: message.content.clone(),
                    tool_exchanges,
                });
                idx = lookahead;
            }
            Role::Tool => {
                out.dropped_orphan_tool_results += 1;
                idx += 1;
            }
        }
    }

    out
}

fn canonicalize_assistant_tools(
    tool_calls: &[ToolCall],
    tool_results: &[&ChatMessage],
) -> (Vec<CanonicalToolExchange>, usize, usize, usize) {
    let mut normalized_calls: Vec<ToolCall> = Vec::with_capacity(tool_calls.len());
    let mut normalized_invalid_tool_arguments = 0usize;
    for tool_call in tool_calls {
        let mut normalized = tool_call.clone();
        if serde_json::from_str::<serde_json::Value>(&normalized.arguments).is_err() {
            normalized.arguments = "{}".to_string();
            normalized_invalid_tool_arguments += 1;
        }
        normalized_calls.push(normalized);
    }

    let mut consumed_results = vec![false; tool_results.len()];
    let mut exchanges = Vec::new();
    let mut stripped_orphan_assistant_calls = 0usize;

    for tool_call in normalized_calls {
        let result_idx = tool_results
            .iter()
            .enumerate()
            .find_map(|(idx, candidate)| {
                if consumed_results[idx] {
                    return None;
                }
                if candidate.tool_call_id.as_deref() == Some(tool_call.id.as_str()) {
                    Some(idx)
                } else {
                    None
                }
            });
        let Some(result_idx) = result_idx else {
            stripped_orphan_assistant_calls += 1;
            continue;
        };
        consumed_results[result_idx] = true;
        exchanges.push(CanonicalToolExchange {
            call: tool_call,
            result_content: tool_results[result_idx].content.clone(),
        });
    }

    let dropped_orphan_tool_results = consumed_results.iter().filter(|used| !**used).count();

    (
        exchanges,
        dropped_orphan_tool_results,
        stripped_orphan_assistant_calls,
        normalized_invalid_tool_arguments,
    )
}

fn materialize_canonical_messages(messages: &[CanonicalMessage]) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::new();
    for message in messages {
        match message {
            CanonicalMessage::System { content } => out.push(ChatMessage {
                role: Role::System,
                content: content.clone(),
                tool_calls: vec![],
                tool_call_id: None,
            }),
            CanonicalMessage::User { content } => out.push(ChatMessage {
                role: Role::User,
                content: content.clone(),
                tool_calls: vec![],
                tool_call_id: None,
            }),
            CanonicalMessage::Assistant {
                content,
                tool_exchanges,
            } => {
                out.push(ChatMessage {
                    role: Role::Assistant,
                    content: content.clone(),
                    tool_calls: tool_exchanges
                        .iter()
                        .map(|exchange| exchange.call.clone())
                        .collect(),
                    tool_call_id: None,
                });
                for exchange in tool_exchanges {
                    out.push(ChatMessage {
                        role: Role::Tool,
                        content: exchange.result_content.clone(),
                        tool_calls: vec![],
                        tool_call_id: Some(exchange.call.id.clone()),
                    });
                }
            }
        }
    }
    out
}

#[derive(Clone)]
pub struct LlmClient {
    provider: Provider,
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl LlmClient {
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn new(api_key: &str, model: &str) -> Result<Self> {
        let provider = detect_provider(model)?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        tracing::info!(
            provider = ?provider,
            model = %model,
            http_timeout_seconds = 60,
            "llm client initialized"
        );
        Ok(Self {
            provider,
            api_key: api_key.to_string(),
            model: model.to_string(),
            client,
        })
    }

    pub fn provider(&self) -> Provider {
        self.provider
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    #[tracing::instrument(level = "info", skip_all)]
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ChatResponse> {
        let sanitized_messages = sanitize_request_messages(messages);
        validate_request_payload(&sanitized_messages, tools)?;
        tracing::info!(
            provider = ?self.provider,
            model = %self.model,
            message_count = sanitized_messages.len(),
            tool_count = tools.len(),
            "llm chat request started"
        );
        let started = Instant::now();
        let resp = match self.provider {
            Provider::OpenAI => {
                let c = OpenAiClient::new(self.client.clone(), &self.api_key, &self.model);
                c.chat(&sanitized_messages, tools).await?
            }
            Provider::Anthropic => {
                let c = AnthropicClient::new(self.client.clone(), &self.api_key, &self.model);
                c.chat(&sanitized_messages, tools).await?
            }
        };
        tracing::info!(
            provider = ?self.provider,
            model = %self.model,
            latency_ms = started.elapsed().as_millis() as u64,
            prompt_tokens = resp.usage.prompt_tokens,
            completion_tokens = resp.usage.completion_tokens,
            tool_calls = resp.message.tool_calls.len(),
            finish_reason = %resp.finish_reason,
            "llm chat request completed"
        );
        Ok(resp)
    }

    #[tracing::instrument(level = "info", skip_all)]
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>> {
        let sanitized_messages = sanitize_request_messages(messages);
        validate_request_payload(&sanitized_messages, tools)?;
        tracing::info!(
            provider = ?self.provider,
            model = %self.model,
            message_count = sanitized_messages.len(),
            tool_count = tools.len(),
            "llm stream request started"
        );
        let started = Instant::now();
        let stream = match self.provider {
            Provider::OpenAI => {
                let c = OpenAiClient::new(self.client.clone(), &self.api_key, &self.model);
                c.chat_stream(&sanitized_messages, tools).await?
            }
            Provider::Anthropic => {
                let c = AnthropicClient::new(self.client.clone(), &self.api_key, &self.model);
                c.chat_stream(&sanitized_messages, tools).await?
            }
        };
        tracing::info!(
            provider = ?self.provider,
            model = %self.model,
            latency_ms = started.elapsed().as_millis() as u64,
            "llm stream request established"
        );
        Ok(stream)
    }
}

fn detect_provider(model: &str) -> Result<Provider> {
    let m = model.to_ascii_lowercase();
    if m.starts_with("claude-") {
        return Ok(Provider::Anthropic);
    }
    if m.starts_with("gpt-") || m.starts_with("o1") || m.starts_with("o3") || m.starts_with("o4") {
        return Ok(Provider::OpenAI);
    }
    Err(LlmError::InvalidInput(format!(
        "unsupported model '{model}': provider cannot be inferred"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatMessage, Role, ToolCall, ToolDefinition};
    use serde_json::json;

    #[test]
    fn valid_tool_name_passes_all_providers() {
        validate_tool_name_all_providers("shell_execute").unwrap();
        validate_tool_name_all_providers("memory-search").unwrap();
        validate_tool_name_all_providers("a").unwrap();
    }

    #[test]
    fn dot_in_tool_name_is_rejected() {
        let err =
            validate_tool_name_all_providers("shell.execute").expect_err("dots should be rejected");
        assert!(err.to_string().contains("does not match"));
    }

    #[test]
    fn empty_tool_name_is_rejected() {
        let err = validate_tool_name_all_providers("").expect_err("empty name should be rejected");
        assert!(err.to_string().contains("does not match"));
    }

    #[test]
    fn too_long_tool_name_is_rejected() {
        let long = "a".repeat(65);
        let err = validate_tool_name_all_providers(&long)
            .expect_err("name exceeding 64 chars should be rejected");
        assert!(err.to_string().contains("max length"));
    }

    #[test]
    fn duplicate_tool_names_are_rejected() {
        let tools = vec![
            ToolDefinition {
                name: "shell_execute".to_string(),
                description: "run shell".to_string(),
                parameters: json!({}),
            },
            ToolDefinition {
                name: "shell_execute".to_string(),
                description: "run shell again".to_string(),
                parameters: json!({}),
            },
        ];

        let err = validate_request_payload(&[], &tools)
            .expect_err("duplicate tool names should be rejected");
        assert!(
            err.to_string()
                .contains("duplicate tool definition name 'shell_execute'")
        );
    }

    #[test]
    fn tool_call_unknown_name_is_rejected() {
        let tools = vec![ToolDefinition {
            name: "filesystem".to_string(),
            description: "filesystem".to_string(),
            parameters: json!({}),
        }];
        let messages = vec![ChatMessage {
            role: Role::Assistant,
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "tc1".to_string(),
                name: "shell_execute".to_string(),
                arguments: "{}".to_string(),
            }],
            tool_call_id: None,
        }];

        let err = validate_request_payload(&messages, &tools)
            .expect_err("unknown tool call name should be rejected");
        assert!(
            err.to_string()
                .contains("references unknown tool 'shell_execute'")
        );
    }

    #[test]
    fn detect_provider_openai_models() {
        assert_eq!(detect_provider("gpt-4").unwrap(), Provider::OpenAI);
        assert_eq!(detect_provider("o1-preview").unwrap(), Provider::OpenAI);
        assert_eq!(detect_provider("o3-mini").unwrap(), Provider::OpenAI);
        assert_eq!(detect_provider("o4-mini").unwrap(), Provider::OpenAI);
    }

    #[test]
    fn detect_provider_anthropic_models() {
        assert_eq!(
            detect_provider("claude-3-sonnet").unwrap(),
            Provider::Anthropic
        );
    }

    #[test]
    fn sanitize_request_messages_drops_orphan_tool_result() {
        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: "hello".to_string(),
                tool_calls: vec![],
                tool_call_id: None,
            },
            ChatMessage {
                role: Role::Tool,
                content: "{\"ok\":true}".to_string(),
                tool_calls: vec![],
                tool_call_id: Some("tool_missing".to_string()),
            },
        ];

        let sanitized = sanitize_request_messages(&messages);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(sanitized[0].role, Role::User);
    }

    #[test]
    fn sanitize_request_messages_clears_orphan_assistant_tool_call_and_result() {
        let messages = vec![
            ChatMessage {
                role: Role::Assistant,
                content: "calling tool".to_string(),
                tool_calls: vec![ToolCall {
                    id: "tool_1".to_string(),
                    name: "filesystem".to_string(),
                    arguments: "{}".to_string(),
                }],
                tool_call_id: None,
            },
            ChatMessage {
                role: Role::User,
                content: "new turn".to_string(),
                tool_calls: vec![],
                tool_call_id: None,
            },
            ChatMessage {
                role: Role::Tool,
                content: "{\"late\":true}".to_string(),
                tool_calls: vec![],
                tool_call_id: Some("tool_1".to_string()),
            },
        ];

        let sanitized = sanitize_request_messages(&messages);
        assert_eq!(sanitized.len(), 2);
        assert_eq!(sanitized[0].role, Role::Assistant);
        assert!(sanitized[0].tool_calls.is_empty());
        assert_eq!(sanitized[1].role, Role::User);
    }

    #[test]
    fn validate_request_payload_rejects_orphan_tool_result_contract() {
        let tools = vec![ToolDefinition {
            name: "filesystem".to_string(),
            description: "filesystem".to_string(),
            parameters: json!({}),
        }];
        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: "hello".to_string(),
                tool_calls: vec![],
                tool_call_id: None,
            },
            ChatMessage {
                role: Role::Tool,
                content: "{\"ok\":true}".to_string(),
                tool_calls: vec![],
                tool_call_id: Some("tool_1".to_string()),
            },
        ];

        let err = validate_request_payload(&messages, &tools)
            .expect_err("orphan tool_result should be rejected");
        assert!(
            err.to_string()
                .contains("without a preceding assistant tool_use")
        );
    }

    #[test]
    fn validate_request_payload_allows_multiple_tool_results_for_single_assistant_turn() {
        let tools = vec![ToolDefinition {
            name: "filesystem".to_string(),
            description: "filesystem".to_string(),
            parameters: json!({}),
        }];
        let messages = vec![
            ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: vec![
                    ToolCall {
                        id: "tool_1".to_string(),
                        name: "filesystem".to_string(),
                        arguments: "{}".to_string(),
                    },
                    ToolCall {
                        id: "tool_2".to_string(),
                        name: "filesystem".to_string(),
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

        validate_request_payload(&messages, &tools)
            .expect("multiple tool results from one assistant turn should validate");
    }

    #[test]
    fn validate_request_payload_rejects_duplicate_tool_result_for_same_call() {
        let tools = vec![ToolDefinition {
            name: "filesystem".to_string(),
            description: "filesystem".to_string(),
            parameters: json!({}),
        }];
        let messages = vec![
            ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: vec![ToolCall {
                    id: "tool_1".to_string(),
                    name: "filesystem".to_string(),
                    arguments: "{}".to_string(),
                }],
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
                tool_call_id: Some("tool_1".to_string()),
            },
        ];

        let err = validate_request_payload(&messages, &tools)
            .expect_err("duplicate tool_result for the same tool_call_id must fail");
        assert!(
            err.to_string()
                .contains("unknown or duplicate tool_call_id 'tool_1'")
        );
    }

    #[test]
    fn sanitize_then_validate_supports_multi_tool_result_contract() {
        let tools = vec![ToolDefinition {
            name: "filesystem".to_string(),
            description: "filesystem".to_string(),
            parameters: json!({}),
        }];
        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: "run it".to_string(),
                tool_calls: vec![],
                tool_call_id: None,
            },
            ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: vec![
                    ToolCall {
                        id: "tool_1".to_string(),
                        name: "filesystem".to_string(),
                        arguments: "{}".to_string(),
                    },
                    ToolCall {
                        id: "tool_2".to_string(),
                        name: "filesystem".to_string(),
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

        let sanitized = sanitize_request_messages(&messages);
        validate_request_payload(&sanitized, &tools)
            .expect("sanitized payload should keep a valid multi-tool-result contract");
    }
}
