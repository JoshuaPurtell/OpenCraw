//! Core assistant agent: LLM + tools + approval gate + memory.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use crate::config::{ApprovalMode, OpenShellConfig};
use crate::session::Session;
use anyhow::Result;
use futures_util::StreamExt;
use horizons_core::core_agents::models::{
    ActionProposal, ActionStatus, ReviewMode, ReviewPolicy, RiskLevel,
};
use horizons_core::core_agents::traits::CoreAgents as _;
use horizons_core::evaluation::engine::EvaluationEngine;
use horizons_core::evaluation::traits::VerificationCase;
use horizons_core::memory::traits::{
    HorizonsMemory, MemoryItem, MemoryType, RetrievalQuery, Scope,
};
use horizons_core::models::{AgentIdentity, OrgId, ProjectDbHandle, ProjectId};
use horizons_core::onboard::traits::{ProjectDb, ProjectDbParam, ProjectDbValue};
use os_channels::{InboundMessage, InboundMessageKind};
use os_llm::{ChatMessage, ChatResponse, Role, StreamChunk, ToolCall, Usage};
use os_tools::{to_llm_tool_def, Tool};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::{self, UnboundedSender};
use uuid::Uuid;

/// Sent by the assistant when a tool call requires human approval.
/// The gateway handles the channel I/O and responds on `response_tx`.
pub struct ApprovalRequest {
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub response_tx: tokio::sync::oneshot::Sender<bool>,
}

pub struct AssistantAgent {
    cfg: OpenShellConfig,
    llm: Option<os_llm::LlmClient>,
    tools: Vec<Arc<dyn Tool>>,
    memory: Option<Arc<dyn HorizonsMemory>>,
    project_db: Arc<dyn ProjectDb>,
    core_agents: Arc<horizons_core::core_agents::executor::CoreAgentsExecutor>,
    org_id: OrgId,
    project_id: ProjectId,
    project_db_handle: ProjectDbHandle,
    evaluation: Option<Arc<EvaluationEngine>>,
}

impl AssistantAgent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: OpenShellConfig,
        llm: Option<os_llm::LlmClient>,
        tools: Vec<Arc<dyn Tool>>,
        memory: Option<Arc<dyn HorizonsMemory>>,
        project_db: Arc<dyn ProjectDb>,
        core_agents: Arc<horizons_core::core_agents::executor::CoreAgentsExecutor>,
        org_id: OrgId,
        project_id: ProjectId,
        project_db_handle: ProjectDbHandle,
        evaluation: Option<Arc<EvaluationEngine>>,
    ) -> Self {
        Self {
            cfg,
            llm,
            tools,
            memory,
            project_db,
            core_agents,
            org_id,
            project_id,
            project_db_handle,
            evaluation,
        }
    }

    pub async fn on_reaction(&self, inbound: &InboundMessage) -> Result<()> {
        if inbound.kind != InboundMessageKind::Reaction {
            return Ok(());
        }
        let Some(eval) = self.evaluation.as_ref() else {
            return Ok(());
        };

        // Minimal v0.1.0 wiring: map 👍 to pass, 👎 to fail.
        let (output, expected) = match inbound.content.as_str() {
            "👍" | "❤️" | "✅" => ("positive".to_string(), Some("positive".to_string())),
            "👎" | "❌" => ("negative".to_string(), Some("positive".to_string())),
            _ => return Ok(()),
        };

        let case = VerificationCase::new(
            format!("reaction:{}:{}", inbound.channel_id, inbound.sender_id),
            output,
            expected,
        );
        let identity = AgentIdentity::System {
            name: "openshell.feedback".to_string(),
        };
        let _ = eval
            .run(
                self.org_id,
                self.project_id,
                &self.project_db_handle,
                &identity,
                case,
            )
            .await?;
        Ok(())
    }

    #[tracing::instrument(
        level = "info",
        skip_all,
        fields(channel_id = %channel_id, sender_id = %sender_id)
    )]
    pub async fn run(
        &self,
        channel_id: &str,
        sender_id: &str,
        session: &mut Session,
        user_message: &str,
        stream_tx: Option<UnboundedSender<String>>,
        approval_tx: Option<mpsc::Sender<ApprovalRequest>>,
    ) -> Result<String> {
        tracing::info!(
            model = %self.cfg.general.model,
            prior_history_messages = session.history.len(),
            tools_registered = self.tools.len(),
            memory_enabled = self.memory.is_some(),
            stream_enabled = stream_tx.is_some(),
            "assistant run started"
        );
        session.history.push(ChatMessage {
            role: Role::User,
            content: user_message.to_string(),
            tool_calls: vec![],
            tool_call_id: None,
        });

        let Some(llm) = self.llm.as_ref() else {
            return Err(anyhow::anyhow!(
                "LLM client is not configured; cannot process user messages"
            ));
        };

        let tool_defs: Vec<os_llm::ToolDefinition> = self
            .tools
            .iter()
            .map(|t| to_llm_tool_def(t.as_ref()))
            .collect();
        let mut tool_defs = tool_defs;
        if self.memory.is_some() {
            tool_defs.extend(memory_tool_definitions());
        }

        let mut tool_loops = 0usize;
        let tool_loops_max = 4usize;

        loop {
            tool_loops += 1;
            if tool_loops > tool_loops_max {
                tracing::error!(tool_loops_max, "assistant tool loop limit reached");
                return Ok("Tool loop limit reached.".to_string());
            }
            tracing::info!(
                tool_loop = tool_loops,
                "assistant tool loop iteration started"
            );

            self.compact_session_if_needed(channel_id, sender_id, session)
                .await?;

            let system_message = ChatMessage {
                role: Role::System,
                content: self
                    .build_system_prompt(channel_id, sender_id, user_message)
                    .await?,
                tool_calls: vec![],
                tool_call_id: None,
            };
            let mut messages = vec![system_message.clone()];
            messages.extend(self.build_context_window(&system_message, &session.history));
            tracing::debug!(
                tool_loop = tool_loops,
                prompt_messages = messages.len(),
                history_messages = session.history.len(),
                "assistant prompt assembled"
            );

            let llm_started = Instant::now();
            let response = self
                .chat_with_stream(llm, &messages, &tool_defs, stream_tx.as_ref())
                .await?;
            tracing::info!(
                tool_loop = tool_loops,
                latency_ms = llm_started.elapsed().as_millis() as u64,
                prompt_tokens = response.usage.prompt_tokens,
                completion_tokens = response.usage.completion_tokens,
                tool_calls = response.message.tool_calls.len(),
                content_len = response.message.content.len(),
                "assistant llm call completed"
            );
            session.usage_totals.prompt_tokens += response.usage.prompt_tokens;
            session.usage_totals.completion_tokens += response.usage.completion_tokens;
            tracing::debug!(
                total_prompt_tokens = session.usage_totals.prompt_tokens,
                total_completion_tokens = session.usage_totals.completion_tokens,
                "assistant cumulative usage updated"
            );

            if response.message.tool_calls.is_empty() {
                let content = response.message.content.clone();
                session.history.push(ChatMessage {
                    role: Role::Assistant,
                    content: content.clone(),
                    tool_calls: vec![],
                    tool_call_id: None,
                });
                session.last_assistant_message_id = Some(Uuid::new_v4().to_string());

                if let Some(mem) = self.memory.as_ref() {
                    self.append_memory(mem, channel_id, sender_id, user_message, &content)
                        .await?;
                }

                tracing::info!(
                    response_len = content.len(),
                    session_history_messages = session.history.len(),
                    "assistant run completed without tool calls"
                );
                return Ok(content);
            }

            session.history.push(response.message.clone());
            tracing::info!(
                tool_calls = response.message.tool_calls.len(),
                "assistant received tool calls from llm"
            );

            for tool_call in response.message.tool_calls {
                tracing::info!(
                    tool_call_id = %tool_call.id,
                    tool_name = %tool_call.name,
                    arguments_len = tool_call.arguments.len(),
                    "assistant handling tool call"
                );
                let args: serde_json::Value =
                    serde_json::from_str(&tool_call.arguments).map_err(|e| {
                        anyhow::anyhow!("invalid tool arguments for {}: {e}", tool_call.name)
                    })?;

                if let Some(memory_out) = self
                    .execute_memory_tool_call(&tool_call, channel_id, sender_id, &args)
                    .await?
                {
                    let risk = RiskLevel::Low;
                    let approved = match self.gate_tool_call(&tool_call, risk, &args, approval_tx.as_ref()).await {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!(
                                tool_call_id = %tool_call.id,
                                tool_name = %tool_call.name,
                                error = %e,
                                "tool approval failed, treating as denied"
                            );
                            session.history.push(ChatMessage {
                                role: Role::Tool,
                                content: json!({ "error": format!("tool approval failed: {e}") }).to_string(),
                                tool_calls: vec![],
                                tool_call_id: Some(tool_call.id.clone()),
                            });
                            continue;
                        }
                    };
                    if !approved {
                        tracing::warn!(
                            tool_call_id = %tool_call.id,
                            tool_name = %tool_call.name,
                            risk = ?risk,
                            "tool call denied by approval flow"
                        );
                        session.history.push(ChatMessage {
                            role: Role::Tool,
                            content: json!({ "error": "tool call denied" }).to_string(),
                            tool_calls: vec![],
                            tool_call_id: Some(tool_call.id.clone()),
                        });
                        continue;
                    }

                    let tool_out_json = memory_out.to_string();
                    tracing::info!(
                        tool_call_id = %tool_call.id,
                        tool_name = %tool_call.name,
                        output_len = tool_out_json.len(),
                        "horizons memory tool call executed"
                    );
                    session.history.push(ChatMessage {
                        role: Role::Tool,
                        content: tool_out_json,
                        tool_calls: vec![],
                        tool_call_id: Some(tool_call.id.clone()),
                    });
                    continue;
                }

                let tool = self
                    .tools
                    .iter()
                    .find(|t| t.spec().name == tool_call.name)
                    .cloned();
                let Some(tool) = tool else {
                    tracing::error!(
                        tool_call_id = %tool_call.id,
                        tool_name = %tool_call.name,
                        "tool call referenced unknown tool"
                    );
                    session.history.push(ChatMessage {
                        role: Role::Tool,
                        content: json!({ "error": "unknown tool" }).to_string(),
                        tool_calls: vec![],
                        tool_call_id: Some(tool_call.id.clone()),
                    });
                    continue;
                };
                let risk = effective_risk_level(tool.as_ref(), &args)?;
                let approved = match self.gate_tool_call(&tool_call, risk, &args, approval_tx.as_ref()).await {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(
                            tool_call_id = %tool_call.id,
                            tool_name = %tool_call.name,
                            error = %e,
                            "tool approval failed, treating as denied"
                        );
                        session.history.push(ChatMessage {
                            role: Role::Tool,
                            content: json!({ "error": format!("tool approval failed: {e}") }).to_string(),
                            tool_calls: vec![],
                            tool_call_id: Some(tool_call.id.clone()),
                        });
                        continue;
                    }
                };
                if !approved {
                    tracing::warn!(
                        tool_call_id = %tool_call.id,
                        tool_name = %tool_call.name,
                        risk = ?risk,
                        "tool call denied by approval flow"
                    );
                    session.history.push(ChatMessage {
                        role: Role::Tool,
                        content: json!({ "error": "tool call denied" }).to_string(),
                        tool_calls: vec![],
                        tool_call_id: Some(tool_call.id.clone()),
                    });
                    continue;
                }

                let execute_started = Instant::now();
                let tool_out = tool.execute(args).await?;
                let tool_out_json = tool_out.to_string();
                tracing::info!(
                    tool_call_id = %tool_call.id,
                    tool_name = %tool_call.name,
                    latency_ms = execute_started.elapsed().as_millis() as u64,
                    output_len = tool_out_json.len(),
                    "tool call executed"
                );
                session.history.push(ChatMessage {
                    role: Role::Tool,
                    content: tool_out_json,
                    tool_calls: vec![],
                    tool_call_id: Some(tool_call.id.clone()),
                });
            }
        }
    }

    async fn compact_session_if_needed(
        &self,
        channel_id: &str,
        sender_id: &str,
        session: &mut Session,
    ) -> Result<()> {
        if !self.cfg.context.compaction_enabled {
            return Ok(());
        }

        let total_history_tokens = estimate_history_tokens(&session.history);
        if total_history_tokens < self.cfg.context.compaction_trigger_tokens {
            return Ok(());
        }

        if session.history.len() <= self.cfg.context.compaction_retain_messages {
            return Ok(());
        }

        let Some(memory) = self.memory.as_ref() else {
            return Err(anyhow::anyhow!(
                "context.compaction_enabled=true requires configured memory backend"
            ));
        };

        let retain_messages = self.cfg.context.compaction_retain_messages;
        let split_index = session.history.len().saturating_sub(retain_messages);
        let archived_messages = session.history[..split_index].to_vec();
        self.flush_pre_compaction_memory(memory, channel_id, sender_id, &archived_messages)
            .await?;

        let agent_id = format!("os.assistant.{channel_id}.{sender_id}");
        let summary = memory
            .summarize(self.org_id, &agent_id, &self.cfg.context.compaction_horizon)
            .await?;
        let summary_message = ChatMessage {
            role: Role::Assistant,
            content: format!(
                "[context compaction summary horizon={}]\n{}",
                self.cfg.context.compaction_horizon, summary.text
            ),
            tool_calls: vec![],
            tool_call_id: None,
        };

        let mut compacted_history = Vec::with_capacity(1 + retain_messages);
        compacted_history.push(summary_message);
        compacted_history.extend_from_slice(&session.history[split_index..]);
        session.history = compacted_history;

        tracing::info!(
            archived_messages = archived_messages.len(),
            retained_messages = retain_messages,
            total_history_tokens,
            compaction_trigger_tokens = self.cfg.context.compaction_trigger_tokens,
            compaction_horizon = %self.cfg.context.compaction_horizon,
            "session history compacted"
        );
        Ok(())
    }

    async fn flush_pre_compaction_memory(
        &self,
        memory: &Arc<dyn HorizonsMemory>,
        channel_id: &str,
        sender_id: &str,
        archived_messages: &[ChatMessage],
    ) -> Result<()> {
        let scope = Scope::new(
            self.org_id.to_string(),
            format!("os.assistant.{channel_id}.{sender_id}"),
        );
        let transcript = render_compaction_transcript(
            archived_messages,
            self.cfg.context.compaction_flush_max_chars,
        );
        let message_count = archived_messages.len();
        let token_estimate = estimate_history_tokens(archived_messages);
        let item = MemoryItem::new(
            &scope,
            MemoryType::observation(),
            json!({
                "kind": "pre_compaction_flush",
                "channel_id": channel_id,
                "sender_id": sender_id,
                "message_count": message_count,
                "token_estimate": token_estimate,
                "transcript": transcript,
            }),
            chrono::Utc::now(),
        )
        .with_importance(0.9)
        .with_index_text(format!(
            "pre_compaction_flush messages={message_count} tokens={token_estimate}\n{}",
            transcript
        ));
        memory.append_item(self.org_id, item).await?;
        tracing::debug!(
            channel_id = %channel_id,
            sender_id = %sender_id,
            message_count,
            token_estimate,
            "pre-compaction memory flush appended"
        );
        Ok(())
    }

    async fn chat_with_stream(
        &self,
        llm: &os_llm::LlmClient,
        messages: &[ChatMessage],
        tool_defs: &[os_llm::ToolDefinition],
        stream_tx: Option<&UnboundedSender<String>>,
    ) -> Result<ChatResponse> {
        tracing::debug!(
            provider = ?llm.provider(),
            model = %llm.model(),
            message_count = messages.len(),
            tool_count = tool_defs.len(),
            stream_forwarding = stream_tx.is_some(),
            "starting llm streaming call"
        );
        let started = Instant::now();
        let mut stream = llm.chat_stream(messages, tool_defs).await?;
        let mut content = String::new();
        let mut usage = Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
        };
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut delta_chunks = 0usize;
        let mut delta_chars = 0usize;
        let mut tool_call_delta_chars = 0usize;
        let mut tool_call_start_events = 0usize;
        let mut done_events = 0usize;

        while let Some(chunk) = stream.next().await {
            match chunk? {
                StreamChunk::Delta { content: delta } => {
                    delta_chunks = delta_chunks.saturating_add(1);
                    delta_chars = delta_chars.saturating_add(delta.len());
                    content.push_str(&delta);
                    if let Some(tx) = stream_tx {
                        tx.send(delta).map_err(|e| {
                            anyhow::anyhow!("stream delta channel send failed: {e}")
                        })?;
                    }
                }
                StreamChunk::ToolCallStart { id, name } => {
                    tool_call_start_events = tool_call_start_events.saturating_add(1);
                    tracing::debug!(tool_call_id = %id, tool_name = %name, "llm tool call started");
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments: String::new(),
                    });
                }
                StreamChunk::ToolCallDelta { arguments } => {
                    tool_call_delta_chars = tool_call_delta_chars.saturating_add(arguments.len());
                    if let Some(last) = tool_calls.last_mut() {
                        last.arguments.push_str(&arguments);
                    }
                }
                StreamChunk::Done { usage: done_usage } => {
                    done_events = done_events.saturating_add(1);
                    usage = done_usage;
                }
            }
        }
        tracing::info!(
            provider = ?llm.provider(),
            model = %llm.model(),
            latency_ms = started.elapsed().as_millis() as u64,
            delta_chunks,
            delta_chars,
            tool_call_start_events,
            tool_call_delta_chars,
            done_events,
            prompt_tokens = usage.prompt_tokens,
            completion_tokens = usage.completion_tokens,
            "llm streaming call finished"
        );

        Ok(ChatResponse {
            message: ChatMessage {
                role: Role::Assistant,
                content,
                tool_calls,
                tool_call_id: None,
            },
            usage,
            finish_reason: "stream_done".to_string(),
        })
    }

    fn build_context_window(
        &self,
        system_message: &ChatMessage,
        history: &[ChatMessage],
    ) -> Vec<ChatMessage> {
        let mut budget = self.cfg.context.max_prompt_tokens;
        let system_tokens = estimate_tokens(system_message);
        budget = budget.saturating_sub(system_tokens);

        let mut selected_reversed: Vec<ChatMessage> = Vec::new();
        for msg in history.iter().rev() {
            let normalized = self.normalize_history_message(msg);
            let message_tokens = estimate_tokens(&normalized);
            let must_keep = selected_reversed.len() < self.cfg.context.min_recent_messages;
            if must_keep || message_tokens <= budget {
                selected_reversed.push(normalized);
                budget = budget.saturating_sub(message_tokens);
                continue;
            }
            break;
        }

        selected_reversed.reverse();
        tracing::debug!(
            kept_messages = selected_reversed.len(),
            total_messages = history.len(),
            max_prompt_tokens = self.cfg.context.max_prompt_tokens,
            min_recent_messages = self.cfg.context.min_recent_messages,
            "context window prepared"
        );
        selected_reversed
    }

    fn normalize_history_message(&self, msg: &ChatMessage) -> ChatMessage {
        if msg.role != Role::Tool {
            return msg.clone();
        }
        if msg.content.chars().count() <= self.cfg.context.max_tool_chars {
            return msg.clone();
        }

        let max_chars = self.cfg.context.max_tool_chars;
        let truncated: String = msg.content.chars().take(max_chars).collect();
        let total_chars = msg.content.chars().count();
        let dropped_chars = total_chars.saturating_sub(max_chars);
        let mut out = msg.clone();
        out.content =
            format!("{truncated}\n...[tool output truncated: dropped {dropped_chars} chars]");
        tracing::debug!(
            max_tool_chars = self.cfg.context.max_tool_chars,
            dropped_chars,
            "tool message truncated for prompt budget"
        );
        out
    }

    async fn build_system_prompt(
        &self,
        channel_id: &str,
        sender_id: &str,
        user_message: &str,
    ) -> Result<String> {
        let mut system = self.cfg.general.system_prompt.clone();
        let Some(mem) = self.memory.as_ref() else {
            return Ok(system);
        };

        let agent_scope = format!("os.assistant.{channel_id}.{sender_id}");
        let query = RetrievalQuery::new(user_message.to_string(), 5);
        let items = mem.retrieve(self.org_id, &agent_scope, query).await?;
        if items.is_empty() {
            tracing::debug!("no relevant memory retrieved for system prompt");
            return Ok(system);
        }
        tracing::debug!(
            retrieved_items = items.len(),
            "relevant memory retrieved for system prompt"
        );

        system.push_str("\n\nRelevant memory:\n");
        for item in items {
            system.push_str("- ");
            system.push_str(&item.content_as_text());
            system.push_str("\n");
        }
        Ok(system)
    }

    async fn append_memory(
        &self,
        mem: &Arc<dyn HorizonsMemory>,
        channel_id: &str,
        sender_id: &str,
        user_message: &str,
        assistant_message: &str,
    ) -> Result<()> {
        let agent_id = format!("os.assistant.{channel_id}.{sender_id}");
        let scope = Scope::new(self.org_id.to_string(), agent_id);

        let importance = if assistant_message.contains("tool") {
            0.8
        } else {
            0.3
        };
        let content = json!({
            "channel": channel_id,
            "sender": sender_id,
            "user": user_message,
            "assistant": assistant_message,
        });

        let item = MemoryItem::new(
            &scope,
            MemoryType::observation(),
            content,
            chrono::Utc::now(),
        )
        .with_importance(importance)
        .with_index_text(format!("{user_message}\n{assistant_message}"));

        mem.append_item(self.org_id, item).await?;
        tracing::debug!(
            channel_id = %channel_id,
            sender_id = %sender_id,
            importance,
            "memory item appended"
        );
        Ok(())
    }

    async fn gate_tool_call(
        &self,
        tool_call: &ToolCall,
        risk: RiskLevel,
        arguments: &serde_json::Value,
    ) -> Result<bool> {
        let approval_mode = approval_mode_for_tool(&self.cfg, &tool_call.name, risk, arguments)?;
        let review_mode = match approval_mode {
            ApprovalMode::Auto => ReviewMode::Auto,
            ApprovalMode::Ai => ReviewMode::Ai,
            ApprovalMode::Human => ReviewMode::Human,
        };
        tracing::info!(
            tool_call_id = %tool_call.id,
            tool_name = %tool_call.name,
            risk = ?risk,
            approval_mode = ?approval_mode,
            review_mode = ?review_mode,
            "evaluating tool approval policy"
        );

        let action_type = action_type_for_tool(&tool_call.name, arguments)?;
        let policy = ReviewPolicy {
            action_type: action_type.clone(),
            risk_level: risk,
            review_mode,
            mcp_scopes: None,
            ttl_seconds: 60 * 60,
        };

        let identity = AgentIdentity::System {
            name: "openshell".to_string(),
        };
        self.core_agents
            .upsert_policy(
                self.org_id,
                self.project_id,
                &self.project_db_handle,
                policy,
                &identity,
            )
            .await?;

        if review_mode == ReviewMode::Auto {
            tracing::info!(
                tool_call_id = %tool_call.id,
                tool_name = %tool_call.name,
                "tool approval resolved automatically"
            );
            return Ok(true);
        }

        let handle_json = serde_json::to_value(&self.project_db_handle)?;

        let context = json!({
            "_project_db_handle": handle_json,
            "tool": tool_call.name,
            "arguments": arguments,
        });

        let proposal = ActionProposal::new(
            self.org_id,
            self.project_id,
            "os.assistant".to_string(),
            action_type,
            json!({ "tool_call_id": tool_call.id, "arguments": arguments }),
            risk,
            Some(format!("os_tool:{}", Uuid::new_v4())),
            context,
            chrono::Utc::now(),
            60 * 60,
        )?;

        let action_id = self.core_agents.propose_action(proposal, &identity).await?;
        let status = wait_for_action_status(
            &*self.project_db,
            self.org_id,
            &self.project_db_handle,
            action_id,
            std::time::Duration::from_secs(60),
        )
        .await?;
        let approved = matches!(status, ActionStatus::Approved | ActionStatus::Executed);
        tracing::info!(
            tool_call_id = %tool_call.id,
            tool_name = %tool_call.name,
            action_id = %action_id,
            final_status = ?status,
            approved,
            "tool approval flow completed"
        );
        Ok(approved)
    }

    async fn execute_memory_tool_call(
        &self,
        tool_call: &ToolCall,
        channel_id: &str,
        sender_id: &str,
        arguments: &serde_json::Value,
    ) -> Result<Option<serde_json::Value>> {
        let Some(memory) = self.memory.as_ref() else {
            return Ok(None);
        };

        let agent_id = format!("os.assistant.{channel_id}.{sender_id}");
        match tool_call.name.as_str() {
            "memory_search" => {
                let (query, limit) = parse_memory_search_arguments(arguments)?;
                let items = memory
                    .retrieve(
                        self.org_id,
                        &agent_id,
                        RetrievalQuery::new(query.clone(), limit),
                    )
                    .await?;
                Ok(Some(json!({
                    "query": query,
                    "limit": limit,
                    "agent_id": agent_id,
                    "items": items,
                })))
            }
            "memory_summarize" => {
                let horizon = parse_memory_summarize_arguments(arguments)?;
                let summary = memory.summarize(self.org_id, &agent_id, &horizon).await?;
                Ok(Some(json!({
                    "horizon": horizon,
                    "agent_id": agent_id,
                    "summary": summary,
                })))
            }
            _ => Ok(None),
        }
    }
}

fn estimate_tokens(message: &ChatMessage) -> usize {
    let mut chars = message.content.chars().count();
    for tc in &message.tool_calls {
        chars = chars.saturating_add(tc.name.chars().count());
        chars = chars.saturating_add(tc.arguments.chars().count());
    }
    // Simple estimate: ~4 chars/token for English-heavy text.
    (chars / 4).max(1)
}

fn estimate_history_tokens(history: &[ChatMessage]) -> usize {
    history.iter().map(estimate_tokens).sum()
}

fn render_compaction_transcript(history: &[ChatMessage], max_chars: usize) -> String {
    let mut out = String::new();
    for msg in history {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push('[');
        out.push_str(match &msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        });
        out.push_str("] ");
        out.push_str(msg.content.trim());

        if out.chars().count() >= max_chars {
            let truncated: String = out.chars().take(max_chars).collect();
            return format!("{truncated}\n...[pre-compaction transcript truncated]");
        }
    }
    out
}

fn memory_tool_definitions() -> Vec<os_llm::ToolDefinition> {
    vec![
        os_llm::ToolDefinition::validated(
            "memory_search",
            "Search Horizons memory for this conversation scope.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
                },
                "required": ["query"]
            }),
        )
        .expect("memory_search tool name must be valid"),
        os_llm::ToolDefinition::validated(
            "memory_summarize",
            "Summarize Horizons memory for this conversation scope and time horizon.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "horizon": { "type": "string" }
                },
                "required": ["horizon"]
            }),
        )
        .expect("memory_summarize tool name must be valid"),
    ]
}

fn parse_memory_search_arguments(arguments: &serde_json::Value) -> Result<(String, usize)> {
    let query = arguments
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("memory.search requires string query"))?
        .trim()
        .to_string();
    if query.is_empty() {
        return Err(anyhow::anyhow!("memory.search query must not be empty"));
    }

    let limit = match arguments.get("limit") {
        None => 5usize,
        Some(v) => {
            let n = v
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("memory.search limit must be an integer"))?;
            usize::try_from(n)
                .map_err(|_| anyhow::anyhow!("memory.search limit is out of range"))?
        }
    };
    if !(1..=50).contains(&limit) {
        return Err(anyhow::anyhow!(
            "memory.search limit must be between 1 and 50"
        ));
    }

    Ok((query, limit))
}

fn parse_memory_summarize_arguments(arguments: &serde_json::Value) -> Result<String> {
    let horizon = arguments
        .get("horizon")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("memory.summarize requires string horizon"))?
        .trim()
        .to_string();
    if horizon.is_empty() {
        return Err(anyhow::anyhow!(
            "memory.summarize horizon must not be empty"
        ));
    }
    Ok(horizon)
}

fn action_type_for_tool(tool_name: &str, arguments: &serde_json::Value) -> Result<String> {
    match tool_name {
        "shell_execute" => Ok("tool.shell.execute".to_string()),
        "filesystem" => {
            let action = filesystem_action(arguments)?;
            if action == "write_file" {
                Ok("tool.filesystem.write".to_string())
            } else {
                Ok("tool.filesystem.read".to_string())
            }
        }
        "email" => {
            let action = email_action(arguments)?;
            if action == "send" {
                Ok("tool.email.send".to_string())
            } else {
                Ok("tool.email.read".to_string())
            }
        }
        "imessage" => {
            let action = imessage_action(arguments)?;
            if action == "send" {
                Ok("tool.imessage.send".to_string())
            } else {
                Ok("tool.imessage.read".to_string())
            }
        }
        "clipboard" => Ok("tool.clipboard".to_string()),
        "browser" => Ok("tool.browser".to_string()),
        other => Ok(format!("tool.{other}")),
    }
}

fn approval_mode_for_tool(
    cfg: &OpenShellConfig,
    tool_name: &str,
    risk: RiskLevel,
    arguments: &serde_json::Value,
) -> Result<ApprovalMode> {
    match tool_name {
        "shell_execute" => Ok(cfg.security.shell_approval),
        "browser" => Ok(cfg.security.browser_approval),
        "filesystem" => {
            let action = filesystem_action(arguments)?;
            if action == "write_file" {
                Ok(cfg.security.filesystem_write_approval)
            } else {
                Ok(ApprovalMode::Auto)
            }
        }
        "email" => {
            let action = email_action(arguments)?;
            if action == "send" {
                Ok(ApprovalMode::Human)
            } else {
                Ok(ApprovalMode::Auto)
            }
        }
        "imessage" => {
            let action = imessage_action(arguments)?;
            if action == "send" {
                Ok(ApprovalMode::Human)
            } else {
                Ok(ApprovalMode::Auto)
            }
        }
        _ => Ok(match risk {
            RiskLevel::Low => ApprovalMode::Auto,
            RiskLevel::Medium => ApprovalMode::Ai,
            RiskLevel::High | RiskLevel::Critical => ApprovalMode::Human,
        }),
    }
}

fn effective_risk_level(tool: &dyn Tool, arguments: &serde_json::Value) -> Result<RiskLevel> {
    let base = tool.spec().risk_level;
    match tool.spec().name.as_str() {
        "filesystem" => {
            let action = filesystem_action(arguments)?;
            match action {
                "read_file" | "list_dir" | "search_files" => Ok(RiskLevel::Low),
                "write_file" => Ok(RiskLevel::Medium),
                _ => Ok(base),
            }
        }
        "email" => {
            let action = email_action(arguments)?;
            if action == "send" {
                Ok(RiskLevel::High)
            } else {
                Ok(RiskLevel::Low)
            }
        }
        "imessage" => {
            let action = imessage_action(arguments)?;
            if action == "send" {
                Ok(RiskLevel::High)
            } else {
                Ok(RiskLevel::Low)
            }
        }
        _ => Ok(base),
    }
}

fn filesystem_action(arguments: &serde_json::Value) -> Result<&str> {
    arguments
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("filesystem tool arguments missing string action"))
}

fn email_action(arguments: &serde_json::Value) -> Result<&str> {
    arguments
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("email tool arguments missing string action"))
}

fn imessage_action(arguments: &serde_json::Value) -> Result<&str> {
    arguments
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("imessage tool arguments missing string action"))
}

#[tracing::instrument(level = "debug", skip_all)]
async fn wait_for_action_status(
    project_db: &dyn ProjectDb,
    org_id: OrgId,
    handle: &ProjectDbHandle,
    action_id: Uuid,
    timeout: std::time::Duration,
) -> Result<ActionStatus> {
    let deadline = Instant::now() + timeout;
    let poll_interval = std::time::Duration::from_millis(250);
    let started = Instant::now();
    let mut polls = 0usize;

    loop {
        polls = polls.saturating_add(1);
        let status = read_action_status(project_db, org_id, handle, action_id).await?;
        match status {
            ActionStatus::Proposed => {}
            other => {
                tracing::debug!(
                    action_id = %action_id,
                    polls,
                    latency_ms = started.elapsed().as_millis() as u64,
                    final_status = ?other,
                    "action status resolved"
                );
                return Ok(other);
            }
        }
        if Instant::now() >= deadline {
            tracing::error!(
                action_id = %action_id,
                polls,
                timeout_ms = timeout.as_millis() as u64,
                "timed out waiting for action status"
            );
            return Err(anyhow::anyhow!(
                "timed out waiting for action proposal status: {action_id}"
            ));
        }
        tokio::time::sleep(poll_interval).await;
    }
}

async fn read_action_status(
    project_db: &dyn ProjectDb,
    org_id: OrgId,
    handle: &ProjectDbHandle,
    action_id: Uuid,
) -> Result<ActionStatus> {
    let sql = r#"
SELECT status
  FROM horizons_action_proposals
 WHERE org_id = ?1 AND id = ?2
 LIMIT 1
"#;
    let params = vec![
        ProjectDbParam::String(org_id.to_string()),
        ProjectDbParam::String(action_id.to_string()),
    ];
    let rows = project_db.query(org_id, handle, sql, &params).await?;
    if rows.is_empty() {
        return Err(anyhow::anyhow!(
            "action proposal not found in database: {action_id}"
        ));
    }
    let raw = rows[0]
        .get("status")
        .and_then(|v| match v {
            ProjectDbValue::String(s) => Some(s.clone()),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("action proposal missing string status"))?;
    match raw.as_str() {
        "proposed" => Ok(ActionStatus::Proposed),
        "approved" => Ok(ActionStatus::Approved),
        "denied" => Ok(ActionStatus::Denied),
        "expired" => Ok(ActionStatus::Expired),
        "executed" => Ok(ActionStatus::Executed),
        other => Err(anyhow::anyhow!("unknown action status value: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        estimate_history_tokens, parse_memory_search_arguments, parse_memory_summarize_arguments,
        render_compaction_transcript,
    };
    use os_llm::{ChatMessage, Role};

    #[test]
    fn parse_memory_search_accepts_valid_args() {
        let (query, limit) = parse_memory_search_arguments(
            &serde_json::json!({"query":"project history","limit":7}),
        )
        .expect("parse valid memory.search args");
        assert_eq!(query, "project history");
        assert_eq!(limit, 7);
    }

    #[test]
    fn parse_memory_search_rejects_invalid_limit() {
        let err = parse_memory_search_arguments(&serde_json::json!({"query":"x","limit":0}))
            .expect_err("limit=0 should be rejected");
        assert!(err.to_string().contains("between 1 and 50"));
    }

    #[test]
    fn parse_memory_summarize_requires_non_empty_horizon() {
        let err = parse_memory_summarize_arguments(&serde_json::json!({"horizon":"   "}))
            .expect_err("empty horizon should be rejected");
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn estimate_history_tokens_sums_messages() {
        let history = vec![
            ChatMessage {
                role: Role::User,
                content: "hello world".to_string(),
                tool_calls: vec![],
                tool_call_id: None,
            },
            ChatMessage {
                role: Role::Assistant,
                content: "response".to_string(),
                tool_calls: vec![],
                tool_call_id: None,
            },
        ];
        let tokens = estimate_history_tokens(&history);
        assert!(tokens >= 2);
    }

    #[test]
    fn render_compaction_transcript_truncates_to_limit() {
        let history = vec![
            ChatMessage {
                role: Role::User,
                content: "a".repeat(200),
                tool_calls: vec![],
                tool_call_id: None,
            },
            ChatMessage {
                role: Role::Assistant,
                content: "b".repeat(200),
                tool_calls: vec![],
                tool_call_id: None,
            },
        ];
        let out = render_compaction_transcript(&history, 80);
        assert!(out.contains("pre-compaction transcript truncated"));
    }
}
