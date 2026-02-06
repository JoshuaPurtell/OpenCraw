//! Dev backends for local OpenCraw development.
//!
//! Re-uses Horizons dev backend implementations and wires them into a Horizons
//! `horizons_rs::server::AppState` so the Horizons HTTP API is available
//! alongside OpenCraw routes.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use crate::config::OpenShellConfig;
use crate::setup;
use anyhow::anyhow;
use anyhow::Result;
use chrono::Utc;
use horizons_core::context_refresh::traits::ContextRefresh;
use horizons_core::context_refresh::engine::ContextRefreshEngine;
use horizons_core::core_agents::executor::CoreAgentsExecutor;
use horizons_core::core_agents::traits::{ActionApprover, ReviewDecision};
use horizons_core::evaluation::engine::EvaluationEngine;
use horizons_core::evaluation::traits::{
    Evaluator, RewardSignal, SignalKind, SignalWeight, VerifierConfig,
};
use horizons_core::evaluation::wiring::build_rlm_evaluator;
use horizons_core::events::traits::EventBus;
use horizons_core::memory::traits::{HorizonsMemory, VoyagerMemory};
use horizons_core::memory::wiring::{build_voyager_memory, VoyagerBackedHorizonsMemory};
use horizons_core::models::{AgentIdentity, OrgId, ProjectDbHandle, ProjectId};
use horizons_core::onboard::traits::{
    Cache, CentralDb, Filestore, GraphStore, OrgRecord, ProjectDb, UserRecord, UserRole,
    VectorStore,
};
use horizons_core::optimization::continual::ContinualLearningEngine;
use horizons_core::optimization::engine::OptimizationEngine;
use horizons_core::optimization::traits::{
    ContinualLearning, ExactMatchMetric, LlmClient as MiproLlmClient,
    VariantSampler as MiproVariantSampler,
};
use horizons_core::optimization::wiring::build_mipro_continual_learning;
use horizons_core::pipelines::engine::{CoreAgentsSubagent, DefaultPipelineRunner};
use horizons_core::pipelines::traits::{PipelineRunner, Subagent};
use horizons_rs::dev_backends::{
    DevCache, DevCentralDb, DevEventBus, DevFilestore, DevGraphStore, DevProjectDb, DevVectorStore,
};
use horizons_rs::server::AppState;
use horizons_graph::llm::LlmClient as GraphLlmClient;
use horizons_graph::tools::DefaultToolExecutor as GraphToolExecutor;
use horizons_graph::GraphEngine;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

pub struct DevRuntime {
    pub horizons_state: AppState,
    pub org_id: OrgId,
    pub project_id: ProjectId,
    pub project_db_handle: ProjectDbHandle,
    pub project_db: Arc<dyn ProjectDb>,
    pub core_agents: Arc<CoreAgentsExecutor>,
    pub memory: Option<Arc<dyn HorizonsMemory>>,
    pub evaluation: Option<Arc<EvaluationEngine>>,
}

pub fn dev_org_id() -> OrgId {
    OrgId(Uuid::nil())
}

pub fn dev_project_id() -> ProjectId {
    ProjectId(Uuid::nil())
}

pub async fn build_dev_runtime(
    cfg: &OpenShellConfig,
    data_dir: impl AsRef<Path>,
) -> Result<DevRuntime> {
    let data_dir = data_dir.as_ref().to_path_buf();
    tokio::fs::create_dir_all(&data_dir).await?;

    let central_db: Arc<dyn CentralDb> = Arc::new(DevCentralDb::new());
    let project_db: Arc<dyn ProjectDb> =
        Arc::new(DevProjectDb::new(data_dir.join("project_dbs")).await?);
    let filestore: Arc<dyn Filestore> = Arc::new(DevFilestore::new(data_dir.join("files")).await?);
    let cache: Arc<dyn Cache> = Arc::new(DevCache::new());
    let graph_store: Arc<dyn GraphStore> = Arc::new(DevGraphStore::new());
    let vector_store: Arc<dyn VectorStore> = Arc::new(DevVectorStore::new());
    let event_bus: Arc<dyn EventBus> = Arc::new(DevEventBus::new());

    let context_refresh: Arc<dyn ContextRefresh> = Arc::new(ContextRefreshEngine::new(
        central_db.clone(),
        project_db.clone(),
        event_bus.clone(),
    ));

    let ai_approver = build_ai_approver(cfg);
    let core_agents = Arc::new(CoreAgentsExecutor::new(
        central_db.clone(),
        project_db.clone(),
        event_bus.clone(),
        ai_approver,
    ));

    // Seed deterministic dev org/user/project.
    let org_id = dev_org_id();
    central_db
        .upsert_org(&OrgRecord {
            org_id,
            name: "opencraw-dev".to_string(),
            created_at: Utc::now(),
        })
        .await?;
    central_db
        .upsert_user(&UserRecord {
            user_id: Uuid::nil(),
            org_id,
            email: "dev@opencraw.invalid".to_string(),
            display_name: Some("Dev User".to_string()),
            role: UserRole::Admin,
            created_at: Utc::now(),
        })
        .await?;

    let project_id = dev_project_id();
    let handle = project_db.provision(org_id, project_id).await?;

    setup::register_subscriptions(&*event_bus, &org_id.to_string()).await?;

    let memory = if cfg.memory.enabled {
        let voyager = build_dev_voyager_memory(graph_store.clone(), vector_store.clone());
        Some(Arc::new(VoyagerBackedHorizonsMemory::new(voyager)) as Arc<dyn HorizonsMemory>)
    } else {
        None
    };

    let evaluator: Arc<dyn Evaluator> = {
        let signals = vec![RewardSignal {
            name: "user_feedback_exact_match".to_string(),
            weight: SignalWeight::new(1.0)
                .map_err(|e| anyhow!("invalid reward signal weight: {e}"))?,
            kind: SignalKind::ExactMatch,
            description: "Maps reactions to pass/fail in v0.1.0.".to_string(),
        }];
        Arc::new(build_rlm_evaluator(VerifierConfig::default(), signals, None)?)
            as Arc<dyn Evaluator>
    };
    let evaluation_engine = Arc::new(EvaluationEngine::new(
        central_db.clone(),
        project_db.clone(),
        filestore.clone(),
        evaluator.clone(),
    ));
    let evaluation = Some(evaluation_engine.clone());

    // Graph engine (used by Horizons verifier graphs and pipeline steps).
    // Configuration is env-driven (LLM keys, tool executor endpoint, python backend, etc).
    let graph_llm = Arc::new(GraphLlmClient::new()) as Arc<dyn horizons_graph::llm::LlmClientApi>;
    let graph_tools =
        Arc::new(GraphToolExecutor::from_env()) as Arc<dyn horizons_graph::tools::ToolExecutor>;
    let graph_engine = Arc::new(GraphEngine::new(graph_llm, graph_tools));

    // Pipelines (used by Horizons endpoints; OpenCraw doesn't depend on them directly).
    let subagent: Arc<dyn Subagent> =
        Arc::new(CoreAgentsSubagent::new(core_agents.clone())) as Arc<dyn Subagent>;
    let pipelines: Arc<dyn PipelineRunner> = Arc::new(DefaultPipelineRunner::new(
        event_bus.clone(),
        subagent,
        None,
        None,
    )) as Arc<dyn PipelineRunner>;

    // Horizons AppState requires these when compiled with horizons_rs feature "all".
    let horizons_memory: Arc<dyn HorizonsMemory> = memory.clone().unwrap_or_else(|| {
        let voyager = build_dev_voyager_memory(graph_store.clone(), vector_store.clone());
        Arc::new(VoyagerBackedHorizonsMemory::new(voyager)) as Arc<dyn HorizonsMemory>
    });

    // Continual learning wiring (required by Horizons `all` feature).
    let mipro_llm: Arc<dyn MiproLlmClient> = Arc::new(MiproLlmAdapter {
        llm: cfg.api_key_for_model().map(|key| os_llm::LlmClient::new(&key, &cfg.general.model)),
    });
    let sampler: Arc<dyn MiproVariantSampler> = Arc::new(mipro_v2::BasicSampler::new());
    let metric: Arc<dyn mipro_v2::EvalMetric> = Arc::new(ExactMatchMetric);
    let cl_impl = Arc::new(build_mipro_continual_learning(mipro_llm, sampler, metric));

    let optimization = Arc::new(OptimizationEngine::new(
        central_db.clone(),
        project_db.clone(),
        filestore.clone(),
        cl_impl.clone() as Arc<dyn ContinualLearning>,
    ));
    let continual_learning = Arc::new(ContinualLearningEngine::new(
        horizons_memory.clone(),
        cl_impl.clone(),
        evaluator.clone(),
        event_bus.clone(),
    ));

    let horizons_state = AppState::new(
        central_db.clone(),
        project_db.clone(),
        filestore.clone(),
        cache.clone(),
        graph_store.clone(),
        vector_store.clone(),
        graph_engine.clone(),
        event_bus.clone(),
        context_refresh.clone(),
        core_agents.clone(),
        pipelines.clone(),
        None,
        None,
        horizons_memory,
        optimization,
        evaluation_engine,
        continual_learning,
    );

    Ok(DevRuntime {
        horizons_state,
        org_id,
        project_id,
        project_db_handle: handle,
        project_db,
        core_agents,
        memory,
        evaluation,
    })
}

struct MiproLlmAdapter {
    llm: Option<os_llm::LlmClient>,
}

#[async_trait::async_trait]
impl mipro_v2::LlmClient for MiproLlmAdapter {
    async fn complete(&self, prompt: &str) -> mipro_v2::Result<String> {
        let Some(llm) = &self.llm else {
            // Keep dev environments usable without wiring optimization endpoints.
            return Ok("noop".to_string());
        };

        let resp = llm
            .chat(
                &[os_llm::ChatMessage {
                    role: os_llm::Role::User,
                    content: prompt.to_string(),
                    tool_calls: vec![],
                    tool_call_id: None,
                }],
                &[],
            )
            .await
            .map_err(|e| mipro_v2::MiproError::Llm(format!("{e}")))?;

        Ok(resp.message.content)
    }

    fn name(&self) -> &'static str {
        "opencraw-mipro"
    }
}

fn build_dev_voyager_memory(
    graph: Arc<dyn GraphStore>,
    vectors: Arc<dyn VectorStore>,
) -> VoyagerMemory {
    let embedder: Arc<dyn voyager::EmbeddingModel> = Arc::new(SimpleHashEmbedder::new(256));
    let summarizer: Arc<dyn voyager::SummarizationModel> = Arc::new(SimpleSummarizer);
    let cfg = voyager::config::VoyagerConfig::default();
    build_voyager_memory(graph, vectors, embedder, summarizer, cfg)
}

fn build_ai_approver(cfg: &OpenShellConfig) -> Option<Arc<dyn ActionApprover>> {
    let key = cfg.api_key_for_model()?;
    let llm = os_llm::LlmClient::new(&key, &cfg.general.model);
    Some(Arc::new(LlmSafetyApprover { llm }))
}

struct LlmSafetyApprover {
    llm: os_llm::LlmClient,
}

#[async_trait::async_trait]
impl ActionApprover for LlmSafetyApprover {
    async fn review(
        &self,
        _policy: &horizons_core::core_agents::models::ReviewPolicy,
        proposal: &horizons_core::core_agents::models::ActionProposal,
        _identity: &AgentIdentity,
    ) -> horizons_core::Result<ReviewDecision> {
        let prompt = format!(
            "You are a safety reviewer for tool calls.\n\nAction type: {}\nRisk: {:?}\n\nPayload:\n{}\n\nDecide approve or deny. Respond with JSON: {{\"decision\":\"approve\"|\"deny\",\"reason\":\"...\"}}.",
            proposal.action_type,
            proposal.risk_level,
            proposal.payload
        );

        let resp = self
            .llm
            .chat(
                &[
                    os_llm::ChatMessage {
                        role: os_llm::Role::System,
                        content: "You review tool calls for safety.".to_string(),
                        tool_calls: vec![],
                        tool_call_id: None,
                    },
                    os_llm::ChatMessage {
                        role: os_llm::Role::User,
                        content: prompt,
                        tool_calls: vec![],
                        tool_call_id: None,
                    },
                ],
                &[],
            )
            .await
            .map_err(|e| {
                horizons_core::Error::BackendMessage(format!("ai approver llm error: {e}"))
            })?;

        let v: serde_json::Value = serde_json::from_str(&resp.message.content).unwrap_or_else(
            |_| serde_json::json!({ "decision": "deny", "reason": "invalid json" }),
        );
        let decision = v.get("decision").and_then(|d| d.as_str()).unwrap_or("deny");
        let reason = v
            .get("reason")
            .and_then(|r| r.as_str())
            .unwrap_or("no reason")
            .to_string();
        Ok(match decision {
            "approve" => ReviewDecision::Approved { reason },
            _ => ReviewDecision::Denied { reason },
        })
    }

    fn name(&self) -> &'static str {
        "openshell_llm_safety"
    }
}

struct SimpleHashEmbedder {
    dims: usize,
}

impl SimpleHashEmbedder {
    fn new(dims: usize) -> Self {
        Self { dims }
    }
}

#[async_trait::async_trait]
impl voyager::EmbeddingModel for SimpleHashEmbedder {
    async fn embed(&self, _scope: &voyager::Scope, text: &str) -> voyager::Result<Vec<f32>> {
        let mut v = vec![0.0f32; self.dims];
        let mut steps = 0usize;
        let steps_max = 50_000usize;
        for token in text.split_whitespace() {
            steps += 1;
            if steps >= steps_max {
                break;
            }
            let mut h = 0u64;
            for b in token.as_bytes() {
                h = h.wrapping_mul(131).wrapping_add(*b as u64);
            }
            let idx = (h as usize) % self.dims;
            v[idx] += 1.0;
        }
        Ok(v)
    }

    fn name(&self) -> &'static str {
        "simple_hash"
    }
}

struct SimpleSummarizer;

#[async_trait::async_trait]
impl voyager::SummarizationModel for SimpleSummarizer {
    async fn summarize(
        &self,
        _scope: &voyager::Scope,
        items: &[voyager::models::MemoryItem],
    ) -> voyager::Result<String> {
        // Minimal deterministic summarizer for dev: concatenate item text and truncate.
        // Real deployments should use an LLM summarizer.
        let mut out = String::new();
        let max_items = 64usize;
        let mut seen = 0usize;
        for it in items.iter() {
            seen += 1;
            if seen > max_items {
                break;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            // Prefer a "text" field if present; otherwise fall back to JSON.
            let line = it
                .content
                .get("text")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| it.content.to_string());
            out.push_str(&line);
        }
        if out.len() > 800 {
            out.truncate(800);
            out.push_str("...");
        }
        Ok(out)
    }

    fn name(&self) -> &'static str {
        "simple_truncate"
    }
}
