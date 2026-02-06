use crate::anthropic::AnthropicClient;
use crate::error::Result;
use crate::openai::OpenAiClient;
use crate::types::{ChatMessage, ChatResponse, StreamChunk, ToolDefinition};
use futures_util::Stream;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::pin::Pin;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    OpenAI,
    Anthropic,
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
    pub fn new(api_key: &str, model: &str) -> Self {
        let provider = detect_provider(model);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!(%e, "reqwest client build failed; falling back to default client");
                reqwest::Client::new()
            });
        Self {
            provider,
            api_key: api_key.to_string(),
            model: model.to_string(),
            client,
        }
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
        match self.provider {
            Provider::OpenAI => {
                let c = OpenAiClient::new(self.client.clone(), &self.api_key, &self.model);
                let (tools_sanitized, forward, reverse) = sanitize_tools_for_openai(tools);
                let messages_sanitized = sanitize_messages_for_openai(messages, &forward);
                let mut resp = c.chat(&messages_sanitized, &tools_sanitized).await?;
                remap_tool_calls_in_response(&mut resp, &reverse);
                Ok(resp)
            }
            Provider::Anthropic => {
                let c = AnthropicClient::new(self.client.clone(), &self.api_key, &self.model);
                c.chat(messages, tools).await
            }
        }
    }

    #[tracing::instrument(level = "info", skip_all)]
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>> {
        match self.provider {
            Provider::OpenAI => {
                let c = OpenAiClient::new(self.client.clone(), &self.api_key, &self.model);
                let (tools_sanitized, forward, reverse) = sanitize_tools_for_openai(tools);
                let messages_sanitized = sanitize_messages_for_openai(messages, &forward);
                let stream = c
                    .chat_stream(&messages_sanitized, &tools_sanitized)
                    .await?;
                Ok(Box::pin(stream.map(move |chunk| match chunk {
                    Ok(StreamChunk::ToolCallStart { id, name }) => Ok(StreamChunk::ToolCallStart {
                        id,
                        name: reverse.get(&name).cloned().unwrap_or(name),
                    }),
                    other => other,
                })))
            }
            Provider::Anthropic => {
                let c = AnthropicClient::new(self.client.clone(), &self.api_key, &self.model);
                c.chat_stream(messages, tools).await
            }
        }
    }
}

fn detect_provider(model: &str) -> Provider {
    let m = model.to_ascii_lowercase();
    if m.starts_with("claude-") {
        return Provider::Anthropic;
    }
    Provider::OpenAI
}

fn sanitize_tools_for_openai(
    tools: &[ToolDefinition],
) -> (Vec<ToolDefinition>, HashMap<String, String>, HashMap<String, String>) {
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut forward: HashMap<String, String> = HashMap::new(); // original -> sanitized
    let mut reverse: HashMap<String, String> = HashMap::new(); // sanitized -> original
    let mut out = Vec::with_capacity(tools.len());

    for t in tools {
        let mut name = sanitize_openai_tool_name(&t.name);
        if let Some(n) = used.get_mut(&name) {
            *n += 1;
            name = format!("{name}_{}", *n);
        } else {
            used.insert(name.clone(), 0);
        }
        forward.insert(t.name.clone(), name.clone());
        reverse.insert(name.clone(), t.name.clone());
        out.push(ToolDefinition {
            name,
            description: t.description.clone(),
            parameters: t.parameters.clone(),
        });
    }

    (out, forward, reverse)
}

fn remap_tool_calls_in_response(resp: &mut ChatResponse, reverse: &HashMap<String, String>) {
    for tc in resp.message.tool_calls.iter_mut() {
        if let Some(orig) = reverse.get(&tc.name) {
            tc.name = orig.clone();
        }
    }
}

fn sanitize_messages_for_openai(
    messages: &[ChatMessage],
    forward: &HashMap<String, String>,
) -> Vec<ChatMessage> {
    let mut out = Vec::with_capacity(messages.len());
    for m in messages {
        let mut m2 = m.clone();
        for tc in m2.tool_calls.iter_mut() {
            if let Some(s) = forward.get(&tc.name) {
                tc.name = s.clone();
            } else {
                // Best-effort: if tool name isn't in the current tool list, still sanitize to
                // satisfy OpenAI validation.
                tc.name = sanitize_openai_tool_name(&tc.name);
            }
        }
        out.push(m2);
    }
    out
}

fn sanitize_openai_tool_name(name: &str) -> String {
    // OpenAI tool names must match: ^[a-zA-Z0-9_-]+$
    // We preserve readability by replacing invalid characters with underscores.
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "tool".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatMessage, Role, ToolCall, ToolDefinition};
    use serde_json::json;

    #[test]
    fn openai_tool_names_are_sanitized_and_unique() {
        let tools = vec![
            ToolDefinition {
                name: "shell.execute".to_string(),
                description: "run shell".to_string(),
                parameters: json!({}),
            },
            ToolDefinition {
                name: "shell_execute".to_string(),
                description: "run shell 2".to_string(),
                parameters: json!({}),
            },
        ];

        let (sanitized, forward, reverse) = sanitize_tools_for_openai(&tools);
        assert_eq!(sanitized.len(), 2);
        assert!(sanitized[0].name.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '_' || c == '-'
        }));
        assert!(sanitized[1].name.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '_' || c == '-'
        }));
        assert_ne!(sanitized[0].name, sanitized[1].name);

        let s1 = forward.get("shell.execute").expect("forward mapping exists");
        let s2 = forward
            .get("shell_execute")
            .expect("forward mapping exists");
        assert_ne!(s1, s2);
        assert_eq!(
            reverse.get(s1).expect("reverse mapping exists"),
            "shell.execute"
        );
        assert_eq!(
            reverse.get(s2).expect("reverse mapping exists"),
            "shell_execute"
        );
    }

    #[test]
    fn openai_messages_tool_calls_are_sanitized_before_send() {
        let tools = vec![ToolDefinition {
            name: "shell.execute".to_string(),
            description: "run shell".to_string(),
            parameters: json!({}),
        }];
        let (_sanitized, forward, _reverse) = sanitize_tools_for_openai(&tools);

        let messages = vec![ChatMessage {
            role: Role::Assistant,
            content: "".to_string(),
            tool_calls: vec![ToolCall {
                id: "tc1".to_string(),
                name: "shell.execute".to_string(),
                arguments: "{}".to_string(),
            }],
            tool_call_id: None,
        }];

        let sanitized = sanitize_messages_for_openai(&messages, &forward);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(sanitized[0].tool_calls.len(), 1);
        assert_eq!(sanitized[0].tool_calls[0].name, "shell_execute");
    }
}
