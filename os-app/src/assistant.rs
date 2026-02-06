//! Core assistant agent: LLM + tools + approval gate + memory.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use crate::config::{ApprovalMode, OpenShellConfig};
use crate::session::Session;
use anyhow::Result;
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
use os_llm::{ChatMessage, Role, ToolCall};
use os_tools::{to_llm_tool_def, Tool};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

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

    #[tracing::instrument(level = "info", skip_all)]
    pub async fn run(
        &self,
        channel_id: &str,
        sender_id: &str,
        session: &mut Session,
        user_message: &str,
    ) -> Result<String> {
        session.history.push(ChatMessage {
            role: Role::User,
            content: user_message.to_string(),
            tool_calls: vec![],
            tool_call_id: None,
        });

        let Some(llm) = self.llm.as_ref() else {
            let reply = format!("echo: {user_message}");
            session.history.push(ChatMessage {
                role: Role::Assistant,
                content: reply.clone(),
                tool_calls: vec![],
                tool_call_id: None,
            });
            return Ok(reply);
        };

        let tool_defs: Vec<os_llm::ToolDefinition> = self
            .tools
            .iter()
            .map(|t| to_llm_tool_def(t.as_ref()))
            .collect();

        let mut tool_loops = 0usize;
        let tool_loops_max = 4usize;

        loop {
            tool_loops += 1;
            if tool_loops > tool_loops_max {
                return Ok("Tool loop limit reached.".to_string());
            }

            let mut messages = Vec::new();
            messages.push(ChatMessage {
                role: Role::System,
                content: self
                    .build_system_prompt(channel_id, sender_id, user_message)
                    .await,
                tool_calls: vec![],
                tool_call_id: None,
            });
            messages.extend(session.history.clone());

            let response = llm.chat(&messages, &tool_defs).await?;
            session.usage_totals.prompt_tokens += response.usage.prompt_tokens;
            session.usage_totals.completion_tokens += response.usage.completion_tokens;

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
                        .await;
                }

                return Ok(content);
            }

            session.history.push(response.message.clone());

            for tool_call in response.message.tool_calls {
                let tool = self
                    .tools
                    .iter()
                    .find(|t| t.spec().name == tool_call.name)
                    .cloned();
                let Some(tool) = tool else {
                    session.history.push(ChatMessage {
                        role: Role::Tool,
                        content: json!({ "error": "unknown tool" }).to_string(),
                        tool_calls: vec![],
                        tool_call_id: Some(tool_call.id.clone()),
                    });
                    continue;
                };

                let args: serde_json::Value =
                    serde_json::from_str(&tool_call.arguments).unwrap_or_else(|_| json!({}));
                let risk = effective_risk_level(tool.as_ref(), &args);
                let approved = self.gate_tool_call(&tool_call, risk, &args).await?;
                if !approved {
                    session.history.push(ChatMessage {
                        role: Role::Tool,
                        content: json!({ "error": "tool call denied" }).to_string(),
                        tool_calls: vec![],
                        tool_call_id: Some(tool_call.id.clone()),
                    });
                    continue;
                }

                let tool_out = tool.execute(args).await?;
                session.history.push(ChatMessage {
                    role: Role::Tool,
                    content: tool_out.to_string(),
                    tool_calls: vec![],
                    tool_call_id: Some(tool_call.id.clone()),
                });
            }
        }
    }

    async fn build_system_prompt(
        &self,
        channel_id: &str,
        sender_id: &str,
        user_message: &str,
    ) -> String {
        let mut system = self.cfg.general.system_prompt.clone();
        let Some(mem) = self.memory.as_ref() else {
            return system;
        };

        let agent_scope = format!("os.assistant.{channel_id}.{sender_id}");
        let query = RetrievalQuery::new(user_message.to_string(), 5);
        let items = mem
            .retrieve(self.org_id, &agent_scope, query)
            .await
            .unwrap_or_default();
        if items.is_empty() {
            return system;
        }

        system.push_str("\n\nRelevant memory:\n");
        for item in items {
            system.push_str("- ");
            system.push_str(&item.content_as_text());
            system.push_str("\n");
        }
        system
    }

    async fn append_memory(
        &self,
        mem: &Arc<dyn HorizonsMemory>,
        channel_id: &str,
        sender_id: &str,
        user_message: &str,
        assistant_message: &str,
    ) {
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

        let _ = mem.append_item(self.org_id, item).await;
    }

    async fn gate_tool_call(
        &self,
        tool_call: &ToolCall,
        risk: RiskLevel,
        arguments: &serde_json::Value,
    ) -> Result<bool> {
        let approval_mode = approval_mode_for_tool(&self.cfg, &tool_call.name, risk, arguments);
        let review_mode = match approval_mode {
            ApprovalMode::Auto => ReviewMode::Auto,
            ApprovalMode::Ai => ReviewMode::Ai,
            ApprovalMode::Human => ReviewMode::Human,
        };

        let policy = ReviewPolicy {
            action_type: action_type_for_tool(&tool_call.name, arguments),
            risk_level: risk,
            review_mode,
            mcp_scopes: None,
            ttl_seconds: 60 * 60,
        };

        let identity = AgentIdentity::System {
            name: "openshell".to_string(),
        };
        let _ = self
            .core_agents
            .upsert_policy(
                self.org_id,
                self.project_id,
                &self.project_db_handle,
                policy,
                &identity,
            )
            .await;

        if review_mode == ReviewMode::Auto {
            return Ok(true);
        }

        let handle_json =
            serde_json::to_value(&self.project_db_handle).unwrap_or_else(|_| json!(null));

        let context = json!({
            "_project_db_handle": handle_json,
            "tool": tool_call.name,
            "arguments": arguments,
        });

        let proposal = ActionProposal::new(
            self.org_id,
            self.project_id,
            "os.assistant".to_string(),
            action_type_for_tool(&tool_call.name, arguments),
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

        Ok(matches!(
            status,
            ActionStatus::Approved | ActionStatus::Executed
        ))
    }
}

fn action_type_for_tool(tool_name: &str, arguments: &serde_json::Value) -> String {
    match tool_name {
        "shell.execute" => "tool.shell.execute".to_string(),
        "filesystem" => {
            let action = arguments
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if action == "write_file" {
                "tool.filesystem.write".to_string()
            } else {
                "tool.filesystem.read".to_string()
            }
        }
        "clipboard" => "tool.clipboard".to_string(),
        "browser" => "tool.browser".to_string(),
        other => format!("tool.{other}"),
    }
}

fn approval_mode_for_tool(
    cfg: &OpenShellConfig,
    tool_name: &str,
    risk: RiskLevel,
    arguments: &serde_json::Value,
) -> ApprovalMode {
    match tool_name {
        "shell.execute" => cfg.security.shell_approval,
        "browser" => cfg.security.browser_approval,
        "filesystem" => {
            let action = arguments
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if action == "write_file" {
                cfg.security.filesystem_write_approval
            } else {
                ApprovalMode::Auto
            }
        }
        _ => match risk {
            RiskLevel::Low => ApprovalMode::Auto,
            RiskLevel::Medium => ApprovalMode::Ai,
            RiskLevel::High | RiskLevel::Critical => ApprovalMode::Human,
        },
    }
}

fn effective_risk_level(tool: &dyn Tool, arguments: &serde_json::Value) -> RiskLevel {
    let base = tool.spec().risk_level;
    if tool.spec().name != "filesystem" {
        return base;
    }
    let action = arguments
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    match action {
        "read_file" | "list_dir" | "search_files" => RiskLevel::Low,
        "write_file" => RiskLevel::Medium,
        _ => base,
    }
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

    loop {
        let status = read_action_status(project_db, org_id, handle, action_id).await?;
        match status {
            ActionStatus::Proposed => {}
            other => return Ok(other),
        }
        if Instant::now() >= deadline {
            return Ok(ActionStatus::Proposed);
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
        return Ok(ActionStatus::Proposed);
    }
    let raw = rows[0]
        .get("status")
        .and_then(|v| match v {
            ProjectDbValue::String(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "proposed".to_string());
    Ok(match raw.as_str() {
        "proposed" => ActionStatus::Proposed,
        "approved" => ActionStatus::Approved,
        "denied" => ActionStatus::Denied,
        "expired" => ActionStatus::Expired,
        "executed" => ActionStatus::Executed,
        _ => ActionStatus::Proposed,
    })
}
