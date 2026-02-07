use crate::anthropic::AnthropicClient;
use crate::error::LlmError;
use crate::error::Result;
use crate::openai::OpenAiClient;
use crate::types::{ChatMessage, ChatResponse, StreamChunk, ToolDefinition};
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

    for (message_idx, message) in messages.iter().enumerate() {
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
        validate_request_payload(messages, tools)?;
        tracing::info!(
            provider = ?self.provider,
            model = %self.model,
            message_count = messages.len(),
            tool_count = tools.len(),
            "llm chat request started"
        );
        let started = Instant::now();
        let resp = match self.provider {
            Provider::OpenAI => {
                let c = OpenAiClient::new(self.client.clone(), &self.api_key, &self.model);
                c.chat(messages, tools).await?
            }
            Provider::Anthropic => {
                let c = AnthropicClient::new(self.client.clone(), &self.api_key, &self.model);
                c.chat(messages, tools).await?
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
        validate_request_payload(messages, tools)?;
        tracing::info!(
            provider = ?self.provider,
            model = %self.model,
            message_count = messages.len(),
            tool_count = tools.len(),
            "llm stream request started"
        );
        let started = Instant::now();
        let stream = match self.provider {
            Provider::OpenAI => {
                let c = OpenAiClient::new(self.client.clone(), &self.api_key, &self.model);
                c.chat_stream(messages, tools).await?
            }
            Provider::Anthropic => {
                let c = AnthropicClient::new(self.client.clone(), &self.api_key, &self.model);
                c.chat_stream(messages, tools).await?
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
        assert!(err
            .to_string()
            .contains("duplicate tool definition name 'shell_execute'"));
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
        assert!(err
            .to_string()
            .contains("references unknown tool 'shell_execute'"));
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
}
