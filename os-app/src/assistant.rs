//! Core assistant agent: LLM + tools + approval gate + memory.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use crate::config::{ApprovalMode, OpenShellConfig};
use crate::session::{ModelPinningMode, Session};
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
use os_channels::{ChannelAdapter, InboundMessage, InboundMessageKind, OutboundMessage};
use os_llm::{ChatMessage, ChatResponse, Role, StreamChunk, ToolCall, Usage};
use os_tools::{Tool, to_llm_tool_def};
use serde_json::json;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
struct LlmProfileState {
    consecutive_failures: u32,
    cooldown_until: Option<Instant>,
}

pub struct AssistantAgent {
    cfg: OpenShellConfig,
    llm_clients: Vec<os_llm::LlmClient>,
    llm_profile_state: Mutex<Vec<LlmProfileState>>,
    tools: Vec<Arc<dyn Tool>>,
    memory: Option<Arc<dyn HorizonsMemory>>,
    project_db: Arc<dyn ProjectDb>,
    core_agents: Arc<horizons_core::core_agents::executor::CoreAgentsExecutor>,
    org_id: OrgId,
    project_id: ProjectId,
    project_db_handle: ProjectDbHandle,
    evaluation: Option<Arc<EvaluationEngine>>,
    channels: HashMap<String, Arc<dyn ChannelAdapter>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionDecision {
    Approve,
    Deny,
}

const EXECUTION_BEHAVIOR_APPENDIX: &str = r#"
Execution directives:
- If the user gives a clear request and an available tool can satisfy it, execute the tool flow.
- Do not add speculative policy objections or moral commentary not present in configured policy gates.
- For third-party recipients, rely on configured access control and approval gates instead of inventing extra consent requirements.
- Ask a clarifying question only when a required argument is missing.
- For tool/backend failures, report the concrete error and the next actionable step.
- Follow provider safety policies and applicable law.
"#;
const SQLITE_WRITE_RETRY_ATTEMPTS: u32 = 4;
const SQLITE_WRITE_RETRY_BASE_DELAY_MS: u64 = 60;

impl AssistantAgent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: OpenShellConfig,
        llm_clients: Vec<os_llm::LlmClient>,
        tools: Vec<Arc<dyn Tool>>,
        memory: Option<Arc<dyn HorizonsMemory>>,
        project_db: Arc<dyn ProjectDb>,
        core_agents: Arc<horizons_core::core_agents::executor::CoreAgentsExecutor>,
        org_id: OrgId,
        project_id: ProjectId,
        project_db_handle: ProjectDbHandle,
        evaluation: Option<Arc<EvaluationEngine>>,
        channels: HashMap<String, Arc<dyn ChannelAdapter>>,
    ) -> Self {
        let llm_profile_state = vec![LlmProfileState::default(); llm_clients.len()];
        Self {
            cfg,
            llm_clients,
            llm_profile_state: Mutex::new(llm_profile_state),
            tools,
            memory,
            project_db,
            core_agents,
            org_id,
            project_id,
            project_db_handle,
            evaluation,
            channels,
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

    pub async fn nuke_pending_actions_for_sender(
        &self,
        channel_id: &str,
        sender_id: &str,
    ) -> Result<usize> {
        const PAGE_LIMIT: usize = 200;
        let mut offset = 0usize;
        let mut pending: Vec<ActionProposal> = Vec::new();

        loop {
            let page = self
                .core_agents
                .list_pending(
                    self.org_id,
                    self.project_id,
                    &self.project_db_handle,
                    PAGE_LIMIT,
                    offset,
                )
                .await?;
            if page.is_empty() {
                break;
            }
            let count = page.len();
            pending.extend(page);
            if count < PAGE_LIMIT {
                break;
            }
            offset += PAGE_LIMIT;
        }

        let approver_id = format!("{channel_id}:{sender_id}");
        let target_ids: Vec<Uuid> = pending
            .into_iter()
            .filter(|proposal| {
                action_context_matches_sender_scope(&proposal.context, channel_id, sender_id)
            })
            .map(|proposal| proposal.id)
            .collect();

        let mut nuked = 0usize;
        for action_id in target_ids {
            if let Err(error) = self
                .retry_sqlite_write("approval nuke deny", || async {
                    self.core_agents
                        .deny(
                            self.org_id,
                            self.project_id,
                            &self.project_db_handle,
                            action_id,
                            &approver_id,
                            "nuked by user command",
                        )
                        .await
                        .map_err(anyhow::Error::from)
                })
                .await
            {
                tracing::warn!(
                    %error,
                    %action_id,
                    channel_id,
                    sender_id,
                    "failed to nuke pending action"
                );
                continue;
            }
            nuked += 1;
        }

        Ok(nuked)
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
        recipient_id: Option<&str>,
        session: &mut Session,
        user_message: &str,
        stream_tx: Option<UnboundedSender<String>>,
    ) -> Result<String> {
        tracing::info!(
            model = %self.cfg.default_model().unwrap_or(""),
            llm_active_profile = %self.cfg.llm.active_profile,
            session_model_override = ?session.model_override,
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

        if self.llm_clients.is_empty() {
            return Err(anyhow::anyhow!(
                "LLM client is not configured; cannot process user messages"
            ));
        }

        let tool_defs: Vec<os_llm::ToolDefinition> = self
            .tools
            .iter()
            .map(|t| to_llm_tool_def(t.as_ref()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut tool_defs = tool_defs;
        if self.memory.is_some() {
            tool_defs.extend(memory_tool_definitions()?);
        }

        let mut tool_loops = 0usize;
        let tool_loops_max = self.cfg.context.tool_loops_max;
        let max_runtime = Duration::from_secs(self.cfg.context.tool_max_runtime_seconds);
        let run_started = Instant::now();
        let no_progress_limit = self.cfg.context.tool_no_progress_limit;
        let mut last_tool_signature: Option<String> = None;
        let mut consecutive_same_tool_signature = 0usize;

        loop {
            if run_started.elapsed() > max_runtime {
                let elapsed = run_started.elapsed().as_secs();
                tracing::error!(
                    elapsed_seconds = elapsed,
                    max_runtime_seconds = max_runtime.as_secs(),
                    "assistant request runtime limit reached"
                );
                return Ok(format!(
                    "I stopped this request after {elapsed}s (limit: {}s). \
Send /nuke and retry with a narrower request, or raise context.tool_max_runtime_seconds.",
                    max_runtime.as_secs()
                ));
            }
            tool_loops += 1;
            if tool_loops > tool_loops_max {
                tracing::error!(tool_loops_max, "assistant tool loop limit reached");
                return Ok(format!(
                    "I hit the tool-loop cap ({tool_loops_max}) before finishing this request. \
Send /nuke and retry with a narrower request, or raise context.tool_loops_max."
                ));
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
                .chat_with_failover(
                    &messages,
                    &tool_defs,
                    stream_tx.as_ref(),
                    session.model_override.as_deref(),
                    session.model_pinning,
                    channel_id,
                    recipient_id,
                )
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

            let tool_signature = tool_call_signature(&response.message.tool_calls);
            if last_tool_signature.as_ref() == Some(&tool_signature) {
                consecutive_same_tool_signature = consecutive_same_tool_signature.saturating_add(1);
            } else {
                consecutive_same_tool_signature = 1;
                last_tool_signature = Some(tool_signature);
            }
            if consecutive_same_tool_signature >= no_progress_limit {
                tracing::error!(
                    consecutive_same_tool_signature,
                    no_progress_limit,
                    "assistant no-progress breaker tripped"
                );
                return Ok(format!(
                    "I stopped after {consecutive_same_tool_signature} repeated tool plans with no progress. \
Send /nuke and retry, or raise context.tool_no_progress_limit."
                ));
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
                let args: serde_json::Value = match serde_json::from_str(&tool_call.arguments) {
                    Ok(value) => value,
                    Err(e) => {
                        if let Some(last_assistant) = session.history.last_mut() {
                            if last_assistant.role == Role::Assistant {
                                if let Some(candidate) = last_assistant
                                    .tool_calls
                                    .iter_mut()
                                    .find(|candidate| candidate.id == tool_call.id)
                                {
                                    // Keep tool_use/tool_result pairing intact for provider contracts.
                                    candidate.arguments = "{}".to_string();
                                }
                            }
                        }
                        tracing::warn!(
                            tool_call_id = %tool_call.id,
                            tool_name = %tool_call.name,
                            error = %e,
                            "invalid tool arguments; returning tool error result"
                        );
                        session.history.push(ChatMessage {
                            role: Role::Tool,
                            content: json!({
                                "error": format!("invalid tool arguments for {}: {e}", tool_call.name)
                            })
                            .to_string(),
                            tool_calls: vec![],
                            tool_call_id: Some(tool_call.id.clone()),
                        });
                        continue;
                    }
                };

                let memory_tool_result = self
                    .execute_memory_tool_call(&tool_call, channel_id, sender_id, &args)
                    .await;
                let memory_out = match memory_tool_result {
                    Ok(value) => value,
                    Err(e) => {
                        tracing::warn!(
                            tool_call_id = %tool_call.id,
                            tool_name = %tool_call.name,
                            error = %e,
                            "memory tool dispatch failed; returning tool error result"
                        );
                        session.history.push(ChatMessage {
                            role: Role::Tool,
                            content:
                                json!({ "error": format!("memory tool dispatch failed: {e}") })
                                    .to_string(),
                            tool_calls: vec![],
                            tool_call_id: Some(tool_call.id.clone()),
                        });
                        continue;
                    }
                };

                if let Some(memory_out) = memory_out {
                    let risk = RiskLevel::Low;
                    let approved = match self
                        .gate_tool_call(
                            &tool_call,
                            risk,
                            &args,
                            channel_id,
                            sender_id,
                            recipient_id,
                        )
                        .await
                    {
                        Ok(v) => v,
                        Err(e) => {
                            if is_sqlite_approval_write_error(&e) {
                                tracing::warn!(
                                    tool_call_id = %tool_call.id,
                                    tool_name = %tool_call.name,
                                    error = %e,
                                    "approval persistence unavailable; returning fail-fast message"
                                );
                                return Ok(sqlite_approval_write_user_message());
                            }
                            if is_approval_timeout_error(&e) {
                                tracing::warn!(
                                    tool_call_id = %tool_call.id,
                                    tool_name = %tool_call.name,
                                    error = %e,
                                    "tool approval timed out; returning timeout message to user"
                                );
                                return Ok(approval_timeout_user_message(
                                    self.cfg.security.human_approval_timeout_seconds,
                                ));
                            }
                            tracing::warn!(
                                tool_call_id = %tool_call.id,
                                tool_name = %tool_call.name,
                                error = %e,
                                "tool approval failed, treating as denied"
                            );
                            session.history.push(ChatMessage {
                                role: Role::Tool,
                                content: json!({ "error": format!("tool approval failed: {e}") })
                                    .to_string(),
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
                let risk = match effective_risk_level(tool.as_ref(), &args) {
                    Ok(value) => value,
                    Err(e) => {
                        tracing::warn!(
                            tool_call_id = %tool_call.id,
                            tool_name = %tool_call.name,
                            error = %e,
                            "risk level resolution failed; returning tool error result"
                        );
                        session.history.push(ChatMessage {
                            role: Role::Tool,
                            content:
                                json!({ "error": format!("risk level resolution failed: {e}") })
                                    .to_string(),
                            tool_calls: vec![],
                            tool_call_id: Some(tool_call.id.clone()),
                        });
                        continue;
                    }
                };
                let approved = match self
                    .gate_tool_call(&tool_call, risk, &args, channel_id, sender_id, recipient_id)
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        if is_sqlite_approval_write_error(&e) {
                            tracing::warn!(
                                tool_call_id = %tool_call.id,
                                tool_name = %tool_call.name,
                                error = %e,
                                "approval persistence unavailable; returning fail-fast message"
                            );
                            return Ok(sqlite_approval_write_user_message());
                        }
                        if is_approval_timeout_error(&e) {
                            tracing::warn!(
                                tool_call_id = %tool_call.id,
                                tool_name = %tool_call.name,
                                error = %e,
                                "tool approval timed out; returning timeout message to user"
                            );
                            return Ok(approval_timeout_user_message(
                                self.cfg.security.human_approval_timeout_seconds,
                            ));
                        }
                        tracing::warn!(
                            tool_call_id = %tool_call.id,
                            tool_name = %tool_call.name,
                            error = %e,
                            "tool approval failed, treating as denied"
                        );
                        session.history.push(ChatMessage {
                            role: Role::Tool,
                            content: json!({ "error": format!("tool approval failed: {e}") })
                                .to_string(),
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
                let tool_out = match tool.execute(args).await {
                    Ok(value) => value,
                    Err(e) => {
                        tracing::warn!(
                            tool_call_id = %tool_call.id,
                            tool_name = %tool_call.name,
                            error = %e,
                            "tool execution failed; returning tool error result"
                        );
                        session.history.push(ChatMessage {
                            role: Role::Tool,
                            content: json!({ "error": format!("tool execution failed: {e}") })
                                .to_string(),
                            tool_calls: vec![],
                            tool_call_id: Some(tool_call.id.clone()),
                        });
                        return Ok(tool_execution_failed_user_message(&tool_call.name, &e));
                    }
                };
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

    async fn chat_with_failover(
        &self,
        messages: &[ChatMessage],
        tool_defs: &[os_llm::ToolDefinition],
        stream_tx: Option<&UnboundedSender<String>>,
        model_override: Option<&str>,
        model_pinning: ModelPinningMode,
        channel_id: &str,
        recipient_id: Option<&str>,
    ) -> Result<ChatResponse> {
        let attempt_order = self.profile_attempt_order(model_override, model_pinning);
        if attempt_order.is_empty() {
            let requested = model_override.unwrap_or("<none>");
            return Err(anyhow::anyhow!(
                "session model pinning is strict and requested model {requested:?} is unavailable"
            ));
        }
        let mut final_errors: Vec<String> = Vec::new();
        const RATE_LIMIT_RETRY_ROUNDS: usize = 1;

        for round in 0..=RATE_LIMIT_RETRY_ROUNDS {
            let mut errors = Vec::new();
            let mut retry_wait: Option<Duration> = None;
            for idx in &attempt_order {
                let llm = &self.llm_clients[*idx];
                let now = Instant::now();
                if let Some(remaining) = self.profile_cooldown_remaining(*idx, now) {
                    tracing::warn!(
                        profile_index = idx,
                        provider = ?llm.provider(),
                        model = %llm.model(),
                        cooldown_ms = remaining.as_millis() as u64,
                        "llm profile is cooling down; skipping attempt"
                    );
                    retry_wait = Some(min_duration(retry_wait, remaining));
                    errors.push(format!(
                        "profile[{idx}] provider={:?} model={} cooldown_ms={}",
                        llm.provider(),
                        llm.model(),
                        remaining.as_millis()
                    ));
                    continue;
                }

                match self
                    .chat_with_stream(llm, messages, tool_defs, stream_tx)
                    .await
                {
                    Ok(response) => {
                        self.mark_profile_success(*idx);
                        if *idx > 0 {
                            tracing::warn!(
                                profile_index = idx,
                                provider = ?llm.provider(),
                                model = %llm.model(),
                                "llm fallback profile succeeded"
                            );
                        }
                        return Ok(response);
                    }
                    Err(e) => {
                        let cooldown = self.mark_profile_failure(*idx, Instant::now());
                        tracing::warn!(
                            profile_index = idx,
                            provider = ?llm.provider(),
                            model = %llm.model(),
                            cooldown_seconds = cooldown.as_secs(),
                            error = %e,
                            "llm profile attempt failed; trying next profile"
                        );
                        if is_rate_limit_error(&e) {
                            retry_wait = Some(min_duration(retry_wait, cooldown));
                        }
                        errors.push(format!(
                            "profile[{idx}] provider={:?} model={} error={e} cooldown_seconds={}",
                            llm.provider(),
                            llm.model(),
                            cooldown.as_secs()
                        ));
                    }
                }
            }

            final_errors = errors;
            if round < RATE_LIMIT_RETRY_ROUNDS {
                if let Some(wait) = retry_wait {
                    tracing::warn!(
                        retry_round = round + 1,
                        wait_seconds = wait.as_secs(),
                        "all llm profiles unavailable/rate-limited; waiting before retry"
                    );
                    self.notify_rate_limit_backoff(
                        channel_id,
                        recipient_id,
                        wait,
                        round + 1,
                        RATE_LIMIT_RETRY_ROUNDS + 1,
                    )
                    .await;
                    tokio::time::sleep(wait).await;
                    continue;
                }
            }
            break;
        }

        Err(anyhow::anyhow!(
            "all llm profiles failed: {}",
            final_errors.join(" | ")
        ))
    }

    fn profile_attempt_order(
        &self,
        model_override: Option<&str>,
        model_pinning: ModelPinningMode,
    ) -> Vec<usize> {
        let available_models: Vec<&str> = self.llm_clients.iter().map(|c| c.model()).collect();
        let order = compute_profile_attempt_order(&available_models, model_override, model_pinning);
        if order.is_empty() && model_pinning == ModelPinningMode::Strict {
            tracing::warn!(
                requested_model = model_override.map(str::trim).unwrap_or(""),
                available_models = ?available_models,
                "session strict model pinning requested unavailable model"
            );
        } else if model_pinning == ModelPinningMode::Prefer
            && model_override
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .is_some()
            && order.iter().copied().eq(0..self.llm_clients.len())
        {
            tracing::warn!(
                requested_model = model_override.map(str::trim).unwrap_or(""),
                available_models = ?available_models,
                "session model override did not match configured LLM profiles; using default failover order"
            );
        }
        order
    }

    async fn notify_rate_limit_backoff(
        &self,
        channel_id: &str,
        recipient_id: Option<&str>,
        wait: Duration,
        retry_round: usize,
        total_rounds: usize,
    ) {
        let Some(recipient_id) = recipient_id else {
            return;
        };
        let Some(channel) = self.channels.get(channel_id).cloned() else {
            return;
        };
        if channel.supports_streaming_deltas() {
            return;
        }

        let wait_seconds = wait.as_secs().max(1);
        let eta_suffix = format_local_retry_eta(wait)
            .map(|eta| format!(" ETA {eta}."))
            .unwrap_or_default();
        let content = format!(
            "Provider rate limit hit. I will retry automatically in {wait_seconds}s (attempt {retry_round}/{total_rounds}).{eta_suffix}"
        );
        if let Err(error) = channel
            .send(
                recipient_id,
                OutboundMessage {
                    content,
                    reply_to_message_id: None,
                    attachments: vec![],
                    metadata: serde_json::Value::Null,
                },
            )
            .await
        {
            tracing::warn!(
                %error,
                %channel_id,
                %recipient_id,
                wait_seconds,
                "failed to send rate-limit backoff notice"
            );
        }
    }

    fn profile_cooldown_remaining(&self, profile_index: usize, now: Instant) -> Option<Duration> {
        let state = self.llm_profile_state.lock().ok()?;
        let profile = state.get(profile_index)?;
        let until = profile.cooldown_until?;
        if until > now {
            Some(until.duration_since(now))
        } else {
            None
        }
    }

    fn mark_profile_success(&self, profile_index: usize) {
        if let Ok(mut state) = self.llm_profile_state.lock() {
            if let Some(profile) = state.get_mut(profile_index) {
                profile.consecutive_failures = 0;
                profile.cooldown_until = None;
            }
        }
    }

    fn mark_profile_failure(&self, profile_index: usize, now: Instant) -> Duration {
        let base = self.cfg.llm.failover_cooldown_base_seconds.max(1);
        let max = self.cfg.llm.failover_cooldown_max_seconds.max(base);

        let mut cooldown = Duration::from_secs(base);
        if let Ok(mut state) = self.llm_profile_state.lock() {
            if let Some(profile) = state.get_mut(profile_index) {
                profile.consecutive_failures = profile.consecutive_failures.saturating_add(1);
                let exponent = profile.consecutive_failures.saturating_sub(1).min(12);
                let multiplier = 1u64 << exponent;
                let seconds = base.saturating_mul(multiplier).min(max);
                cooldown = Duration::from_secs(seconds);
                profile.cooldown_until = Some(now + cooldown);
            }
        }
        cooldown
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
        system.push_str("\n\n");
        system.push_str(EXECUTION_BEHAVIOR_APPENDIX);
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
            system.push('\n');
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
        channel_id: &str,
        sender_id: &str,
        recipient_id: Option<&str>,
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
        self.retry_sqlite_write("approval policy upsert", || async {
            self.core_agents
                .upsert_policy(
                    self.org_id,
                    self.project_id,
                    &self.project_db_handle,
                    policy.clone(),
                    &identity,
                )
                .await
                .map_err(anyhow::Error::from)
        })
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
            "approval_channel": channel_id,
            "approval_sender": sender_id,
            "approval_recipient": recipient_id,
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

        let action_id = self
            .retry_sqlite_write_with_backoff_notice(
                "approval proposal insert",
                channel_id,
                recipient_id,
                "Temporary local database contention while preparing approval.",
                || async {
                    self.core_agents
                        .propose_action(proposal.clone(), &identity)
                        .await
                        .map_err(anyhow::Error::from)
                },
            )
            .await?;
        if review_mode == ReviewMode::Human {
            self.notify_human_approval_required(
                action_id,
                &tool_call.name,
                arguments,
                channel_id,
                sender_id,
                recipient_id,
            )
            .await;
        }
        let timeout = approval_wait_timeout(self.cfg.security.human_approval_timeout_seconds);
        let status = match wait_for_action_status(
            &*self.project_db,
            self.org_id,
            &self.project_db_handle,
            action_id,
            timeout,
        )
        .await
        {
            Ok(status) => status,
            Err(error) => {
                if review_mode == ReviewMode::Human && is_approval_timeout_error(&error) {
                    self.notify_human_approval_timed_out(
                        action_id,
                        channel_id,
                        sender_id,
                        recipient_id,
                        self.cfg.security.human_approval_timeout_seconds,
                    )
                    .await;
                }
                return Err(error);
            }
        };
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

    async fn retry_sqlite_write<T, F, Fut>(&self, operation: &str, mut op: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let mut attempt = 1u32;
        loop {
            match op().await {
                Ok(value) => return Ok(value),
                Err(error) => {
                    let transient = is_transient_sqlite_write_error(&error);
                    if !transient || attempt >= SQLITE_WRITE_RETRY_ATTEMPTS {
                        if transient {
                            return Err(anyhow::anyhow!(
                                "{operation} failed due to transient sqlite write contention after {attempt} attempts: {error}"
                            ));
                        }
                        return Err(error);
                    }

                    let delay_ms =
                        SQLITE_WRITE_RETRY_BASE_DELAY_MS * (1u64 << ((attempt - 1).min(4)));
                    tracing::warn!(
                        operation,
                        attempt,
                        delay_ms,
                        error = %error,
                        "transient sqlite write failure; retrying"
                    );
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn retry_sqlite_write_with_backoff_notice<T, F, Fut>(
        &self,
        operation: &str,
        channel_id: &str,
        recipient_id: Option<&str>,
        user_notice: &str,
        mut op: F,
    ) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let mut attempt = 1u32;
        loop {
            match op().await {
                Ok(value) => return Ok(value),
                Err(error) => {
                    let transient = is_transient_sqlite_write_error(&error);
                    if !transient || attempt >= SQLITE_WRITE_RETRY_ATTEMPTS {
                        if transient {
                            return Err(anyhow::anyhow!(
                                "{operation} failed due to transient sqlite write contention after {attempt} attempts: {error}"
                            ));
                        }
                        return Err(error);
                    }

                    let delay_ms =
                        SQLITE_WRITE_RETRY_BASE_DELAY_MS * (1u64 << ((attempt - 1).min(4)));
                    tracing::warn!(
                        operation,
                        attempt,
                        delay_ms,
                        error = %error,
                        "transient sqlite write failure; retrying with user notice"
                    );
                    self.notify_sqlite_backoff(
                        channel_id,
                        recipient_id,
                        user_notice,
                        delay_ms,
                        attempt + 1,
                    )
                    .await;
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn notify_sqlite_backoff(
        &self,
        channel_id: &str,
        recipient_id: Option<&str>,
        user_notice: &str,
        delay_ms: u64,
        next_attempt: u32,
    ) {
        let Some(recipient_id) = recipient_id else {
            return;
        };
        let Some(channel) = self.channels.get(channel_id).cloned() else {
            return;
        };
        if channel.supports_streaming_deltas() {
            return;
        }
        let content = format!(
            "{user_notice} Retrying in {delay_ms}ms (attempt {next_attempt}/{SQLITE_WRITE_RETRY_ATTEMPTS})."
        );
        if let Err(error) = channel
            .send(
                recipient_id,
                OutboundMessage {
                    content,
                    reply_to_message_id: None,
                    attachments: vec![],
                    metadata: serde_json::Value::Null,
                },
            )
            .await
        {
            tracing::warn!(
                %error,
                %channel_id,
                %recipient_id,
                delay_ms,
                next_attempt,
                "failed to send sqlite backoff notice"
            );
        }
    }

    async fn notify_human_approval_required(
        &self,
        action_id: Uuid,
        tool_name: &str,
        arguments: &serde_json::Value,
        channel_id: &str,
        sender_id: &str,
        recipient_id: Option<&str>,
    ) {
        let Some(recipient_id) = recipient_id else {
            tracing::warn!(
                %action_id,
                %channel_id,
                %sender_id,
                "cannot send approval prompt without recipient thread"
            );
            return;
        };
        let Some(channel) = self.channels.get(channel_id).cloned() else {
            tracing::warn!(
                %action_id,
                %channel_id,
                "cannot send approval prompt: channel adapter unavailable"
            );
            return;
        };

        let render_arguments = self.enrich_approval_arguments(tool_name, arguments).await;
        let is_telegram = channel_id.eq_ignore_ascii_case("telegram");
        let content = if is_telegram {
            render_telegram_approval_prompt(tool_name, &render_arguments, "pending")
        } else {
            let args_preview = compact_json(&render_arguments, 400);
            format!(
                "Approval required for tool `{tool_name}`.\nAction ID: `{action_id}`\n\nApprove: /approve-action {action_id}\nDeny: /deny-action {action_id}\n\nArguments:\n{args_preview}"
            )
        };
        let metadata = if is_telegram {
            json!({
                "telegram_reply_markup": {
                    "inline_keyboard": [[
                        {
                            "text": "Approve ✅",
                            "callback_data": format!("oc:approve:{action_id}")
                        },
                        {
                            "text": "Deny ❌",
                            "callback_data": format!("oc:deny:{action_id}")
                        }
                    ]]
                }
            })
        } else {
            serde_json::Value::Null
        };

        if let Err(error) = channel
            .send(
                recipient_id,
                OutboundMessage {
                    content,
                    reply_to_message_id: None,
                    attachments: vec![],
                    metadata,
                },
            )
            .await
        {
            tracing::warn!(
                %error,
                %action_id,
                %channel_id,
                %recipient_id,
                "failed to send in-channel approval prompt"
            );
        }
    }

    async fn enrich_approval_arguments(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> serde_json::Value {
        let mut enriched = arguments.clone();
        if tool_name.eq_ignore_ascii_case("linear") {
            self.enrich_linear_approval_arguments(&mut enriched).await;
        }
        enriched
    }

    async fn enrich_linear_approval_arguments(&self, arguments: &mut serde_json::Value) {
        let Some(action) = arguments.get("action").and_then(|value| value.as_str()) else {
            return;
        };
        if normalize_linear_action_name(action) != "update_project" {
            return;
        }
        if non_empty_string_from_keys(arguments, &["project_name"]).is_some() {
            return;
        }
        let Some(project_ref) = non_empty_string_from_keys(
            arguments,
            &["project_id", "projectid", "project_ref", "project"],
        ) else {
            return;
        };
        let Some(project_name) = self
            .resolve_linear_project_name_for_approval(project_ref)
            .await
        else {
            return;
        };
        if let Some(object) = arguments.as_object_mut() {
            object.insert(
                "project_name".to_string(),
                serde_json::Value::String(project_name),
            );
        }
    }

    async fn resolve_linear_project_name_for_approval(&self, project_ref: &str) -> Option<String> {
        let linear = self
            .tools
            .iter()
            .find(|tool| tool.spec().name == "linear")
            .cloned()?;

        if let Ok(direct) = linear
            .execute(json!({
                "action": "get_project",
                "project_id": project_ref
            }))
            .await
        {
            if let Some(project_name) = direct
                .get("project")
                .and_then(|project| project.get("name"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
            {
                return Some(project_name);
            }
        }

        let response = linear
            .execute(json!({
                "action": "list_projects",
                "query": project_ref
            }))
            .await
            .ok()?;
        let projects = response.get("projects")?.as_array()?;
        let exact = projects.iter().find(|project| {
            project
                .get("id")
                .and_then(|value| value.as_str())
                .map(|id| id.eq_ignore_ascii_case(project_ref))
                .unwrap_or(false)
        });
        let selected = exact.or_else(|| projects.first())?;
        selected
            .get("name")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    }

    async fn notify_human_approval_timed_out(
        &self,
        action_id: Uuid,
        channel_id: &str,
        sender_id: &str,
        recipient_id: Option<&str>,
        timeout_seconds: u64,
    ) {
        let Some(recipient_id) = recipient_id else {
            tracing::warn!(
                %action_id,
                %channel_id,
                %sender_id,
                "cannot send approval-timeout notice without recipient thread"
            );
            return;
        };
        let Some(channel) = self.channels.get(channel_id).cloned() else {
            tracing::warn!(
                %action_id,
                %channel_id,
                "cannot send approval-timeout notice: channel adapter unavailable"
            );
            return;
        };

        let timeout_label = if timeout_seconds == 0 {
            "configured wait window".to_string()
        } else {
            format!("{timeout_seconds}s")
        };
        let is_telegram = channel_id.eq_ignore_ascii_case("telegram");
        let content = if is_telegram {
            format!(
                "Approval timed out after {timeout_label}. I stopped this run to avoid hanging.\n\nYou can still decide below, then resend your request."
            )
        } else {
            format!(
                "Approval timed out for action `{action_id}` after {timeout_label}. \
I stopped this run to avoid hanging.\n\nYou can still decide it:\nApprove: /approve-action {action_id}\nDeny: /deny-action {action_id}\n\nThen resend your request."
            )
        };
        let metadata = if is_telegram {
            json!({
                "telegram_reply_markup": {
                    "inline_keyboard": [[
                        {
                            "text": "Approve ✅",
                            "callback_data": format!("oc:approve:{action_id}")
                        },
                        {
                            "text": "Deny ❌",
                            "callback_data": format!("oc:deny:{action_id}")
                        }
                    ]]
                }
            })
        } else {
            serde_json::Value::Null
        };

        if let Err(error) = channel
            .send(
                recipient_id,
                OutboundMessage {
                    content,
                    reply_to_message_id: None,
                    attachments: vec![],
                    metadata,
                },
            )
            .await
        {
            tracing::warn!(
                %error,
                %action_id,
                %channel_id,
                %recipient_id,
                "failed to send in-channel approval-timeout notice"
            );
        }
    }

    pub async fn render_telegram_approval_prompt_snapshot(
        &self,
        action_id: Option<Uuid>,
    ) -> Result<Option<String>> {
        let Some(action_id) = action_id else {
            return Ok(None);
        };
        let Some(record) = self.load_action_record(action_id).await? else {
            return Ok(None);
        };
        let tool_name = record
            .context
            .get("tool")
            .and_then(|value| value.as_str())
            .unwrap_or("tool");
        let arguments = record
            .context
            .get("arguments")
            .unwrap_or(&serde_json::Value::Null);
        let render_arguments = self.enrich_approval_arguments(tool_name, arguments).await;
        Ok(Some(render_telegram_approval_prompt(
            tool_name,
            &render_arguments,
            "pending",
        )))
    }

    pub async fn resolve_action_decision(
        &self,
        channel_id: &str,
        sender_id: &str,
        recipient_id: Option<&str>,
        decision: ActionDecision,
        action_id: Option<Uuid>,
        reason: Option<&str>,
    ) -> Result<String> {
        let action_id = if let Some(action_id) = action_id {
            action_id
        } else {
            let pending = self
                .core_agents
                .list_pending(
                    self.org_id,
                    self.project_id,
                    &self.project_db_handle,
                    100,
                    0,
                )
                .await?;
            let latest = pending
                .into_iter()
                .filter(|proposal| {
                    action_context_matches(&proposal.context, channel_id, sender_id, recipient_id)
                })
                .max_by_key(|proposal| proposal.created_at);
            let Some(latest) = latest else {
                return Ok("No pending action found for this channel/user.".to_string());
            };
            latest.id
        };

        let Some(record) = self.load_action_record(action_id).await? else {
            return Ok(format!("Action `{action_id}` was not found."));
        };

        if !action_context_matches(&record.context, channel_id, sender_id, recipient_id) {
            return Ok(
                "That action is not pending for this channel/user context; refusing decision."
                    .to_string(),
            );
        }

        if record.status != ActionStatus::Proposed {
            return Ok(format!(
                "Action `{action_id}` is already `{}`.",
                action_status_name(record.status)
            ));
        }

        let approver_id = format!("{channel_id}:{sender_id}");
        let reason = reason
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| match decision {
                ActionDecision::Approve => "approved from channel".to_string(),
                ActionDecision::Deny => "denied from channel".to_string(),
            });

        let decision_result = match decision {
            ActionDecision::Approve => {
                self.retry_sqlite_write("approval decision apply", || async {
                    self.core_agents
                        .approve(
                            self.org_id,
                            self.project_id,
                            &self.project_db_handle,
                            action_id,
                            &approver_id,
                            &reason,
                        )
                        .await
                        .map_err(anyhow::Error::from)
                })
                .await
            }
            ActionDecision::Deny => {
                self.retry_sqlite_write("approval decision apply", || async {
                    self.core_agents
                        .deny(
                            self.org_id,
                            self.project_id,
                            &self.project_db_handle,
                            action_id,
                            &approver_id,
                            &reason,
                        )
                        .await
                        .map_err(anyhow::Error::from)
                })
                .await
            }
        };

        match decision_result {
            Ok(()) => Ok(match decision {
                ActionDecision::Approve => {
                    "Approved. Continuing request.\nThis approval is closed.".to_string()
                }
                ActionDecision::Deny => "Denied.\nThis approval is closed.".to_string(),
            }),
            Err(error) => Ok(format!("Failed to apply decision: {error}")),
        }
    }

    async fn load_action_record(&self, action_id: Uuid) -> Result<Option<ActionRecord>> {
        let sql = r#"
SELECT status, context_json
  FROM horizons_action_proposals
 WHERE org_id = ?1 AND id = ?2
 LIMIT 1
"#;
        let params = vec![
            ProjectDbParam::String(self.org_id.to_string()),
            ProjectDbParam::String(action_id.to_string()),
        ];
        let rows = match self
            .project_db
            .query(self.org_id, &self.project_db_handle, sql, &params)
            .await
        {
            Ok(rows) => rows,
            Err(error) if error.to_string().contains("no such table") => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let Some(row) = rows.first() else {
            return Ok(None);
        };

        let status_value = row
            .get("status")
            .and_then(|value| match value {
                ProjectDbValue::String(s) => Some(s.as_str()),
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("action proposal missing status"))?;
        let status = parse_action_status(status_value)?;

        let context_json = row
            .get("context_json")
            .and_then(|value| match value {
                ProjectDbValue::String(s) => Some(s.as_str()),
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("action proposal missing context_json"))?;
        let context = serde_json::from_str(context_json)?;

        Ok(Some(ActionRecord { status, context }))
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

struct ActionRecord {
    status: ActionStatus,
    context: serde_json::Value,
}

fn is_transient_sqlite_write_error(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("sqlite execute")
        || message.contains("database is locked")
        || message.contains("sqlite busy")
        || message.contains("sqlite_busy")
}

fn tool_call_signature(tool_calls: &[ToolCall]) -> String {
    let mut signature_parts = Vec::with_capacity(tool_calls.len());
    for tool_call in tool_calls {
        let normalized_args = serde_json::from_str::<serde_json::Value>(&tool_call.arguments)
            .map(|value| canonicalize_json_for_signature(&value))
            .unwrap_or_else(|_| tool_call.arguments.clone());
        signature_parts.push(format!("{}:{normalized_args}", tool_call.name));
    }
    signature_parts.join("|")
}

fn canonicalize_json_for_signature(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            let mut out = String::from("{");
            for (idx, key) in keys.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                out.push_str(key);
                out.push(':');
                let next = map
                    .get(*key)
                    .map(canonicalize_json_for_signature)
                    .unwrap_or_else(|| "null".to_string());
                out.push_str(&next);
            }
            out.push('}');
            out
        }
        serde_json::Value::Array(values) => {
            let mut out = String::from("[");
            for (idx, entry) in values.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                out.push_str(&canonicalize_json_for_signature(entry));
            }
            out.push(']');
            out
        }
        _ => value.to_string(),
    }
}

fn is_rate_limit_error(error: &anyhow::Error) -> bool {
    let normalized = error.to_string().to_ascii_lowercase();
    normalized.contains("429 too many requests")
        || normalized.contains("rate_limit")
        || normalized.contains("rate limit")
}

fn is_approval_timeout_error(error: &anyhow::Error) -> bool {
    error
        .to_string()
        .to_ascii_lowercase()
        .contains("timed out waiting for action proposal status")
}

fn is_sqlite_approval_write_error(error: &anyhow::Error) -> bool {
    let normalized = error.to_string().to_ascii_lowercase();
    normalized.contains("approval proposal insert failed due to transient sqlite write contention")
        || normalized.contains("backend error: sqlite execute")
}

fn approval_timeout_user_message(timeout_seconds: u64) -> String {
    if timeout_seconds == 0 {
        return "Approval is still pending and this run was paused. Approve or deny the pending action, then resend your request.".to_string();
    }
    format!(
        "Approval timed out after {timeout_seconds}s before a decision was received. \
Please approve/deny the pending action and resend your request. \
If you want longer waits, raise security.human_approval_timeout_seconds (or set 0 to wait indefinitely)."
    )
}

fn sqlite_approval_write_user_message() -> String {
    "I couldn't persist the approval request due to local database contention, so I stopped this run instead of hanging. Please retry in a few seconds. If it keeps happening, restart the dev backend and rerun the request.".to_string()
}

fn tool_execution_failed_user_message(tool_name: &str, error: &dyn std::fmt::Display) -> String {
    format!(
        "I couldn't complete the `{tool_name}` action, so I stopped this run.\n\nTool error: {error}"
    )
}

fn approval_wait_timeout(timeout_seconds: u64) -> Option<Duration> {
    if timeout_seconds == 0 {
        None
    } else {
        Some(Duration::from_secs(timeout_seconds))
    }
}

fn min_duration(current: Option<Duration>, candidate: Duration) -> Duration {
    match current {
        Some(existing) => existing.min(candidate),
        None => candidate,
    }
}

fn format_local_retry_eta(wait: Duration) -> Option<String> {
    let wait = chrono::Duration::from_std(wait).ok()?;
    let eta = chrono::Local::now().checked_add_signed(wait)?;
    Some(eta.format("%H:%M:%S %Z").to_string())
}

fn parse_action_status(status: &str) -> Result<ActionStatus> {
    match status {
        "proposed" => Ok(ActionStatus::Proposed),
        "approved" => Ok(ActionStatus::Approved),
        "denied" => Ok(ActionStatus::Denied),
        "executed" => Ok(ActionStatus::Executed),
        "expired" => Ok(ActionStatus::Expired),
        other => Err(anyhow::anyhow!("unknown action status value: {other}")),
    }
}

fn action_status_name(status: ActionStatus) -> &'static str {
    match status {
        ActionStatus::Proposed => "proposed",
        ActionStatus::Approved => "approved",
        ActionStatus::Denied => "denied",
        ActionStatus::Executed => "executed",
        ActionStatus::Expired => "expired",
    }
}

fn action_context_matches(
    context: &serde_json::Value,
    channel_id: &str,
    sender_id: &str,
    recipient_id: Option<&str>,
) -> bool {
    let ctx_channel = context.get("approval_channel").and_then(|v| v.as_str());
    let ctx_sender = context.get("approval_sender").and_then(|v| v.as_str());
    if ctx_channel != Some(channel_id) || ctx_sender != Some(sender_id) {
        return false;
    }

    let ctx_recipient = context.get("approval_recipient").and_then(|v| v.as_str());
    match (ctx_recipient, recipient_id) {
        (Some(expected), Some(actual)) => expected == actual,
        (Some(_), None) => false,
        (None, _) => true,
    }
}

fn action_context_matches_sender_scope(
    context: &serde_json::Value,
    channel_id: &str,
    sender_id: &str,
) -> bool {
    let ctx_channel = context.get("approval_channel").and_then(|v| v.as_str());
    let ctx_sender = context.get("approval_sender").and_then(|v| v.as_str());
    ctx_channel == Some(channel_id) && ctx_sender == Some(sender_id)
}

fn compact_json(value: &serde_json::Value, max_chars: usize) -> String {
    let rendered = serde_json::to_string_pretty(value)
        .or_else(|_| serde_json::to_string(value))
        .unwrap_or_else(|_| "<failed to serialize arguments>".to_string());
    if rendered.chars().count() <= max_chars {
        return rendered;
    }
    let mut compact: String = rendered.chars().take(max_chars).collect();
    compact.push_str("...");
    compact
}

fn render_telegram_approval_prompt(
    tool_name: &str,
    arguments: &serde_json::Value,
    status: &str,
) -> String {
    let summary = render_tool_approval_summary(tool_name, arguments)
        .map(|value| format!("{value}\n\n"))
        .unwrap_or_default();
    let args_preview = compact_json(arguments, 400);
    format!(
        "Approval required for tool `{tool_name}`.\n\n{summary}Arguments:\n{args_preview}\n\nStatus: {status}"
    )
}

fn render_tool_approval_summary(tool_name: &str, arguments: &serde_json::Value) -> Option<String> {
    if !tool_name.eq_ignore_ascii_case("linear") {
        return None;
    }
    render_linear_approval_summary(arguments)
}

fn render_linear_approval_summary(arguments: &serde_json::Value) -> Option<String> {
    let action = arguments
        .get("action")
        .and_then(|value| value.as_str())
        .map(normalize_linear_action_name)?;
    if action != "update_project" {
        return None;
    }

    let project_name = non_empty_string_from_keys(arguments, &["project_name"]);
    let project_id = non_empty_string_from_keys(arguments, &["project_id", "projectid"]);
    let project_ref = non_empty_string_from_keys(arguments, &["project_ref", "project"]);
    let project_display = match (project_name, project_id.or(project_ref)) {
        (Some(name), Some(reference)) => format!("{name} ({reference})"),
        (Some(name), None) => name.to_string(),
        (None, Some(reference)) => reference.to_string(),
        (None, None) => "<missing>".to_string(),
    };

    let priority = arguments.get("priority").and_then(|value| {
        if let Some(numeric) = value.as_i64() {
            return Some(numeric.to_string());
        }
        value
            .as_str()
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(ToOwned::to_owned)
    });
    let state = non_empty_string_from_keys(arguments, &["state", "status"]);
    let mut lines = vec![
        "Summary:".to_string(),
        format!("- action: {action}"),
        format!("- project: {project_display}"),
    ];
    if let Some(value) = priority {
        lines.push(format!("- priority: {value}"));
    }
    if let Some(value) = state {
        lines.push(format!("- state: {value}"));
    }
    if let Some(value) = non_empty_string_from_keys(arguments, &["name"]) {
        lines.push(format!("- rename_to: {value}"));
    }
    Some(lines.join("\n"))
}

fn non_empty_string_from_keys<'a>(value: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(|entry| entry.as_str())
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
    })
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

fn memory_tool_definitions() -> Result<Vec<os_llm::ToolDefinition>> {
    Ok(vec![
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
        )?,
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
        )?,
    ])
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

fn compute_profile_attempt_order(
    available_models: &[&str],
    model_override: Option<&str>,
    model_pinning: ModelPinningMode,
) -> Vec<usize> {
    let total = available_models.len();
    let default_order: Vec<usize> = (0..total).collect();
    let Some(requested) = model_override.map(str::trim).filter(|v| !v.is_empty()) else {
        return default_order;
    };

    let mut preferred = Vec::new();
    let mut fallback = Vec::new();
    for (idx, model) in available_models.iter().enumerate() {
        if model.eq_ignore_ascii_case(requested) {
            preferred.push(idx);
        } else {
            fallback.push(idx);
        }
    }
    if preferred.is_empty() {
        return match model_pinning {
            ModelPinningMode::Prefer => default_order,
            ModelPinningMode::Strict => Vec::new(),
        };
    }
    if model_pinning == ModelPinningMode::Strict {
        return preferred;
    }
    preferred.extend(fallback);
    preferred
}

fn action_type_for_tool(tool_name: &str, arguments: &serde_json::Value) -> Result<String> {
    match tool_name {
        "shell_execute" => {
            let action = shell_action(arguments);
            let elevated = shell_requires_elevated(arguments)?;
            let mode = if elevated { "elevated" } else { "sandbox" };
            match action {
                "start_background" => Ok(format!("tool.shell.background.{mode}.start")),
                "poll_background" => Ok("tool.shell.background.inspect".to_string()),
                "list_background" => Ok("tool.shell.background.inspect".to_string()),
                "stop_background" => Ok("tool.shell.background.stop".to_string()),
                _ => Ok(format!("tool.shell.execute.{mode}")),
            }
        }
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
        "linear" => {
            let action = linear_action(arguments)?;
            match action {
                "create_issue" => Ok("tool.linear.issue.create".to_string()),
                "create_project" => Ok("tool.linear.project.create".to_string()),
                "update_project" => Ok("tool.linear.project.update".to_string()),
                "update_issue" => Ok("tool.linear.issue.update".to_string()),
                "assign_issue" => Ok("tool.linear.issue.assign".to_string()),
                "comment_issue" => Ok("tool.linear.comment.send".to_string()),
                "graphql_mutation" => Ok("tool.linear.graphql.mutation".to_string()),
                "list_assigned" | "list_users" | "list_teams" | "list_projects" | "get_project"
                | "whoami" | "graphql_query" => Ok("tool.linear.read".to_string()),
                _ => Ok("tool.linear".to_string()),
            }
        }
        "clipboard" => Ok("tool.clipboard".to_string()),
        "browser" => Ok("tool.browser".to_string()),
        "apply_patch" => Ok("tool.apply_patch".to_string()),
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
        "shell_execute" => {
            let action = shell_action(arguments);
            let elevated = shell_requires_elevated(arguments)?;
            match action {
                "list_background" | "poll_background" => Ok(ApprovalMode::Auto),
                "start_background" => Ok(ApprovalMode::Human),
                "stop_background" => Ok(cfg.security.shell_approval),
                _ => {
                    if elevated {
                        Ok(ApprovalMode::Human)
                    } else {
                        Ok(cfg.security.shell_approval)
                    }
                }
            }
        }
        "browser" => Ok(cfg.security.browser_approval),
        "apply_patch" => Ok(cfg.security.filesystem_write_approval),
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
        "linear" => {
            let action = linear_action(arguments)?;
            if matches!(
                action,
                "create_issue"
                    | "create_project"
                    | "update_project"
                    | "update_issue"
                    | "assign_issue"
                    | "comment_issue"
                    | "graphql_mutation"
            ) {
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
        "shell_execute" => {
            let action = shell_action(arguments);
            let elevated = shell_requires_elevated(arguments)?;
            match action {
                "list_background" | "poll_background" => Ok(RiskLevel::Low),
                "stop_background" => Ok(RiskLevel::Medium),
                "start_background" => {
                    if elevated {
                        Ok(RiskLevel::High)
                    } else {
                        Ok(RiskLevel::Medium)
                    }
                }
                _ => {
                    if elevated {
                        Ok(RiskLevel::High)
                    } else {
                        Ok(RiskLevel::Medium)
                    }
                }
            }
        }
        "apply_patch" => Ok(RiskLevel::Medium),
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
        "linear" => {
            let action = linear_action(arguments)?;
            if matches!(
                action,
                "create_issue"
                    | "create_project"
                    | "update_project"
                    | "update_issue"
                    | "assign_issue"
                    | "comment_issue"
                    | "graphql_mutation"
            ) {
                Ok(RiskLevel::High)
            } else {
                Ok(RiskLevel::Low)
            }
        }
        _ => Ok(base),
    }
}

fn shell_action(arguments: &serde_json::Value) -> &str {
    arguments
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("exec")
}

fn shell_requires_elevated(arguments: &serde_json::Value) -> Result<bool> {
    let Some(raw) = arguments.get("sandbox_permissions") else {
        return Ok(false);
    };
    let as_str = raw
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("shell tool sandbox_permissions must be a string"))?;
    match as_str.trim().to_ascii_lowercase().as_str() {
        "sandbox" => Ok(false),
        "require_elevated" | "elevated" => Ok(true),
        other => Err(anyhow::anyhow!(
            "shell tool sandbox_permissions must be 'sandbox' or 'require_elevated', got {other:?}"
        )),
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

fn linear_action(arguments: &serde_json::Value) -> Result<&str> {
    let action = arguments
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("linear tool arguments missing string action"))?;
    Ok(normalize_linear_action_name(action))
}

fn normalize_linear_action_name(action: &str) -> &str {
    let trimmed = action.trim();
    if trimmed.eq_ignore_ascii_case("issuecreate") {
        return "create_issue";
    }
    if trimmed.eq_ignore_ascii_case("projectcreate") {
        return "create_project";
    }
    if trimmed.eq_ignore_ascii_case("projectupdate") {
        return "update_project";
    }
    if trimmed.eq_ignore_ascii_case("issueupdate") {
        return "update_issue";
    }
    if trimmed.eq_ignore_ascii_case("commentcreate") {
        return "comment_issue";
    }
    if trimmed.eq_ignore_ascii_case("viewer") {
        return "whoami";
    }
    if trimmed.eq_ignore_ascii_case("projects") {
        return "list_projects";
    }
    if trimmed.eq_ignore_ascii_case("users") {
        return "list_users";
    }
    if trimmed.eq_ignore_ascii_case("teams") {
        return "list_teams";
    }
    if trimmed.eq_ignore_ascii_case("assignedissues") {
        return "list_assigned";
    }
    if trimmed.eq_ignore_ascii_case("updateproject") {
        return "update_project";
    }
    if trimmed.eq_ignore_ascii_case("createproject") {
        return "create_project";
    }
    if trimmed.eq_ignore_ascii_case("createissue") {
        return "create_issue";
    }
    if trimmed.eq_ignore_ascii_case("updateissue") {
        return "update_issue";
    }
    if trimmed.eq_ignore_ascii_case("assignissue") {
        return "assign_issue";
    }
    if trimmed.eq_ignore_ascii_case("commentissue") {
        return "comment_issue";
    }
    if trimmed.eq_ignore_ascii_case("listprojects") {
        return "list_projects";
    }
    if trimmed.eq_ignore_ascii_case("getproject") {
        return "get_project";
    }
    if trimmed.eq_ignore_ascii_case("listusers") {
        return "list_users";
    }
    if trimmed.eq_ignore_ascii_case("listassigned") {
        return "list_assigned";
    }
    if trimmed.eq_ignore_ascii_case("listteams") {
        return "list_teams";
    }
    if trimmed.eq_ignore_ascii_case("graphqlquery")
        || trimmed.eq_ignore_ascii_case("query")
        || trimmed.eq_ignore_ascii_case("graphql_query")
    {
        return "graphql_query";
    }
    if trimmed.eq_ignore_ascii_case("graphqlmutation")
        || trimmed.eq_ignore_ascii_case("mutation")
        || trimmed.eq_ignore_ascii_case("graphql_mutation")
    {
        return "graphql_mutation";
    }
    trimmed
}

#[tracing::instrument(level = "debug", skip_all)]
async fn wait_for_action_status(
    project_db: &dyn ProjectDb,
    org_id: OrgId,
    handle: &ProjectDbHandle,
    action_id: Uuid,
    timeout: Option<std::time::Duration>,
) -> Result<ActionStatus> {
    let deadline = timeout.map(|value| Instant::now() + value);
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
        if let Some(deadline) = deadline {
            if Instant::now() >= deadline {
                tracing::error!(
                    action_id = %action_id,
                    polls,
                    timeout_ms = timeout.map(|value| value.as_millis() as u64).unwrap_or_default(),
                    "timed out waiting for action status"
                );
                return Err(anyhow::anyhow!(
                    "timed out waiting for action proposal status: {action_id}"
                ));
            }
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
    parse_action_status(raw.as_str())
}

#[cfg(test)]
mod tests {
    use super::{
        approval_timeout_user_message, approval_wait_timeout, compute_profile_attempt_order,
        estimate_history_tokens, is_approval_timeout_error, is_rate_limit_error, min_duration,
        non_empty_string_from_keys, normalize_linear_action_name, parse_memory_search_arguments,
        parse_memory_summarize_arguments, render_compaction_transcript,
        render_telegram_approval_prompt, sqlite_approval_write_user_message,
        tool_execution_failed_user_message,
    };
    use crate::session::ModelPinningMode;
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
    fn rate_limit_error_detection_matches_provider_errors() {
        let err = anyhow::anyhow!(
            "{}",
            r#"http error: anthropic stream status=429 Too Many Requests body={"type":"error","error":{"type":"rate_limit_error"}}"#
        );
        assert!(is_rate_limit_error(&err));
        let other = anyhow::anyhow!("http error: bad request");
        assert!(!is_rate_limit_error(&other));
    }

    #[test]
    fn min_duration_prefers_smaller_wait() {
        let a = std::time::Duration::from_secs(9);
        let b = std::time::Duration::from_secs(4);
        assert_eq!(min_duration(None, a), a);
        assert_eq!(min_duration(Some(a), b), b);
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

    #[test]
    fn strict_model_pinning_only_uses_matching_profiles() {
        let models = vec!["gpt-4o-mini", "claude-sonnet-4-5-20250929", "gpt-4o-mini"];
        let order =
            compute_profile_attempt_order(&models, Some("gpt-4o-mini"), ModelPinningMode::Strict);
        assert_eq!(order, vec![0, 2]);
    }

    #[test]
    fn strict_model_pinning_returns_empty_when_model_missing() {
        let models = vec!["gpt-4o-mini", "claude-sonnet-4-5-20250929"];
        let order =
            compute_profile_attempt_order(&models, Some("o3-mini"), ModelPinningMode::Strict);
        assert!(order.is_empty());
    }

    #[test]
    fn approval_timeout_detection_matches_status_error() {
        let timeout = anyhow::anyhow!("timed out waiting for action proposal status: 123");
        assert!(is_approval_timeout_error(&timeout));
        let other = anyhow::anyhow!("action proposal missing string status");
        assert!(!is_approval_timeout_error(&other));
    }

    #[test]
    fn approval_timeout_config_allows_infinite_wait() {
        assert!(approval_wait_timeout(0).is_none());
        assert_eq!(
            approval_wait_timeout(90),
            Some(std::time::Duration::from_secs(90))
        );
    }

    #[test]
    fn approval_timeout_user_message_mentions_override() {
        let msg = approval_timeout_user_message(300);
        assert!(msg.contains("300s"));
        assert!(msg.contains("human_approval_timeout_seconds"));
    }

    #[test]
    fn sqlite_approval_write_user_message_is_actionable() {
        let msg = sqlite_approval_write_user_message();
        assert!(msg.contains("database contention"));
        assert!(msg.contains("retry"));
    }

    #[test]
    fn tool_execution_failed_user_message_reports_tool_and_error() {
        let err = anyhow::anyhow!("linear graphql returned errors: INVALID_INPUT");
        let msg = tool_execution_failed_user_message("linear", &err);
        assert!(msg.contains("`linear`"));
        assert!(msg.contains("INVALID_INPUT"));
    }

    #[test]
    fn normalize_linear_action_name_accepts_compact_aliases() {
        assert_eq!(
            normalize_linear_action_name("updateproject"),
            "update_project"
        );
        assert_eq!(normalize_linear_action_name("createissue"), "create_issue");
        assert_eq!(
            normalize_linear_action_name("projectUpdate"),
            "update_project"
        );
        assert_eq!(normalize_linear_action_name("mutation"), "graphql_mutation");
    }

    #[test]
    fn non_empty_string_from_keys_prefers_first_present() {
        let value = serde_json::json!({"project_name":"Bible","project_id":"123"});
        assert_eq!(
            non_empty_string_from_keys(&value, &["project_name", "project_id"]),
            Some("Bible")
        );
    }

    #[test]
    fn render_telegram_approval_prompt_adds_linear_update_project_summary() {
        let prompt = render_telegram_approval_prompt(
            "linear",
            &serde_json::json!({
                "action":"updateproject",
                "project_name":"Bible ingestion",
                "projectid":"6bba92d9-6405-4f25-acce-2ca9af17058a",
                "priority":4,
                "state":"In Progress"
            }),
            "pending",
        );
        assert!(prompt.contains("Summary:"));
        assert!(prompt.contains("project: Bible ingestion"));
        assert!(prompt.contains("priority: 4"));
        assert!(prompt.contains("state: In Progress"));
        assert!(prompt.contains("Status: pending"));
    }
}
