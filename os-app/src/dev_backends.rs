//! Dev backends for local OpenCraw development.
//!
//! Re-uses Horizons dev backend implementations and wires them into a Horizons
//! `horizons_rs::server::AppState` so the Horizons HTTP API is available
//! alongside OpenCraw routes.
//!
//! See: specifications/openshell/implementation_v0_1_0.md

use crate::config::{OpenShellConfig, RuntimeMode};
use crate::setup;
use anyhow::Result;
use anyhow::anyhow;
use chrono::Utc;
use horizons_core::context_refresh::engine::ContextRefreshEngine;
use horizons_core::context_refresh::traits::ContextRefresh;
use horizons_core::core_agents::executor::CoreAgentsExecutor;
use horizons_core::core_agents::traits::{ActionApprover, ReviewDecision};
use horizons_core::evaluation::engine::EvaluationEngine;
use horizons_core::evaluation::traits::{
    Evaluator, RewardSignal, SignalKind, SignalWeight, VerifierConfig,
};
use horizons_core::evaluation::wiring::build_rlm_evaluator;
use horizons_core::events::bus::RedisEventBus;
use horizons_core::events::config::EventSyncConfig;
use horizons_core::events::traits::EventBus;
use horizons_core::memory::traits::{HorizonsMemory, VoyagerMemory};
use horizons_core::memory::wiring::{VoyagerBackedHorizonsMemory, build_voyager_memory};
use horizons_core::models::{AgentIdentity, OrgId, ProjectDbHandle, ProjectId};
use horizons_core::onboard::config::{
    HelixConfig, PostgresConfig, ProjectDbConfig, RedisConfig, S3Config,
};
use horizons_core::onboard::helix_client::HelixGraphStore;
use horizons_core::onboard::postgres::PostgresCentralDb;
use horizons_core::onboard::redis::RedisCache;
use horizons_core::onboard::s3::S3Filestore;
use horizons_core::onboard::traits::{
    Cache, CentralDb, Filestore, GraphStore, OrgRecord, ProjectDb, UserRecord, UserRole,
    VectorStore,
};
use horizons_core::onboard::turso::LibsqlProjectDb;
use horizons_core::optimization::continual::ContinualLearningEngine;
use horizons_core::optimization::engine::OptimizationEngine;
use horizons_core::optimization::traits::{
    ContinualLearning, ExactMatchMetric, LlmClient as MiproLlmClient,
    VariantSampler as MiproVariantSampler,
};
use horizons_core::optimization::wiring::build_mipro_continual_learning;
use horizons_core::pipelines::engine::{CoreAgentsSubagent, DefaultPipelineRunner};
use horizons_core::pipelines::traits::{PipelineRunner, Subagent};
use horizons_graph::GraphEngine;
use horizons_graph::llm::LlmClient as GraphLlmClient;
use horizons_graph::tools::DefaultToolExecutor as GraphToolExecutor;
use horizons_integrations::vector::pgvector::PgVectorStore;
use horizons_rs::dev_backends::{
    DevCache, DevCentralDb, DevEventBus, DevFilestore, DevGraphStore, DevProjectDb, DevVectorStore,
};
use horizons_rs::server::AppState;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use uuid::Uuid;

pub struct DevRuntime {
    pub horizons_state: AppState,
    pub org_id: OrgId,
    pub project_id: ProjectId,
    pub project_db_handle: ProjectDbHandle,
    pub project_db: Arc<dyn ProjectDb>,
    pub event_bus: Arc<dyn EventBus>,
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

pub async fn build_runtime(
    cfg: &OpenShellConfig,
    data_dir: impl AsRef<Path>,
) -> Result<DevRuntime> {
    match cfg.runtime.mode {
        RuntimeMode::Dev => build_dev_runtime(cfg, data_dir).await,
        RuntimeMode::Prod => build_prod_runtime(cfg, data_dir).await,
    }
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

    let ai_approver = Some(build_ai_approver(cfg)?);
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

    let voyager = build_dev_voyager_memory(graph_store.clone(), vector_store.clone());
    let horizons_memory =
        Arc::new(VoyagerBackedHorizonsMemory::new(voyager)) as Arc<dyn HorizonsMemory>;
    let memory = if cfg.memory.enabled {
        Some(horizons_memory.clone())
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
        Arc::new(build_rlm_evaluator(
            VerifierConfig::default(),
            signals,
            None,
        )?) as Arc<dyn Evaluator>
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
    let graph_llm =
        Arc::new(SafeGraphLlmClient::new()) as Arc<dyn horizons_graph::llm::LlmClientApi>;
    let graph_tools =
        Arc::new(SafeGraphToolExecutor::new()) as Arc<dyn horizons_graph::tools::ToolExecutor>;
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

    // Continual learning wiring (required by Horizons `all` feature).
    let default_model = cfg.default_model()?.to_string();
    let mipro_llm: Arc<dyn MiproLlmClient> = Arc::new(MiproLlmAdapter {
        llm: os_llm::LlmClient::new(&cfg.api_key_for_active_profile_model()?, &default_model)?,
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
        event_bus,
        core_agents,
        memory,
        evaluation,
    })
}

struct ProdRuntimeEnv {
    central_db_url: String,
    central_db_max_connections: u32,
    central_db_acquire_timeout_ms: u64,
    redis_url: String,
    redis_key_prefix: Option<String>,
    s3_bucket: String,
    s3_region: String,
    s3_endpoint: Option<String>,
    s3_access_key_id: String,
    s3_secret_access_key: String,
    s3_prefix: Option<String>,
    helix_url: String,
    helix_api_key: Option<String>,
    helix_timeout_ms: u64,
    vector_dim: usize,
    org_id: OrgId,
    project_id: ProjectId,
}

impl ProdRuntimeEnv {
    fn from_env() -> Result<Self> {
        let org_id_raw = required_env("OPENCRAW_ORG_ID")?;
        let project_id_raw = required_env("OPENCRAW_PROJECT_ID")?;
        let org_id = OrgId::from_str(&org_id_raw)
            .map_err(|e| anyhow!("invalid OPENCRAW_ORG_ID={org_id_raw:?}: {e}"))?;
        let project_id = ProjectId::from_str(&project_id_raw)
            .map_err(|e| anyhow!("invalid OPENCRAW_PROJECT_ID={project_id_raw:?}: {e}"))?;

        Ok(Self {
            central_db_url: required_env("HORIZONS_CENTRAL_DB_URL")?,
            central_db_max_connections: parse_env_u32("HORIZONS_CENTRAL_DB_MAX_CONNECTIONS", 10)?,
            central_db_acquire_timeout_ms: parse_env_u64(
                "HORIZONS_CENTRAL_DB_ACQUIRE_TIMEOUT_MS",
                5_000,
            )?,
            redis_url: required_env("HORIZONS_REDIS_URL")?,
            redis_key_prefix: optional_env("HORIZONS_REDIS_KEY_PREFIX"),
            s3_bucket: required_env("HORIZONS_S3_BUCKET")?,
            s3_region: required_env("HORIZONS_S3_REGION")?,
            s3_endpoint: optional_env("HORIZONS_S3_ENDPOINT"),
            s3_access_key_id: required_env("HORIZONS_S3_ACCESS_KEY_ID")?,
            s3_secret_access_key: required_env("HORIZONS_S3_SECRET_ACCESS_KEY")?,
            s3_prefix: optional_env("HORIZONS_S3_PREFIX"),
            helix_url: required_env("HORIZONS_HELIX_URL")?,
            helix_api_key: optional_env("HORIZONS_HELIX_API_KEY"),
            helix_timeout_ms: parse_env_u64("HORIZONS_HELIX_TIMEOUT_MS", 10_000)?,
            vector_dim: parse_env_usize("HORIZONS_VECTOR_DIM", 64)?,
            org_id,
            project_id,
        })
    }
}

fn required_env(name: &str) -> Result<String> {
    let value = std::env::var(name)
        .map_err(|_| anyhow!("required environment variable is not set: {name}"))?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("required environment variable is empty: {name}"));
    }
    Ok(trimmed.to_string())
}

fn optional_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn parse_env_u32(name: &str, default: u32) -> Result<u32> {
    let Some(raw) = optional_env(name) else {
        return Ok(default);
    };
    raw.parse::<u32>()
        .map_err(|e| anyhow!("invalid {name}={raw:?}: expected u32 ({e})"))
}

fn parse_env_u64(name: &str, default: u64) -> Result<u64> {
    let Some(raw) = optional_env(name) else {
        return Ok(default);
    };
    raw.parse::<u64>()
        .map_err(|e| anyhow!("invalid {name}={raw:?}: expected u64 ({e})"))
}

fn parse_env_usize(name: &str, default: usize) -> Result<usize> {
    let Some(raw) = optional_env(name) else {
        return Ok(default);
    };
    raw.parse::<usize>()
        .map_err(|e| anyhow!("invalid {name}={raw:?}: expected usize ({e})"))
}

pub async fn build_prod_runtime(
    cfg: &OpenShellConfig,
    data_dir: impl AsRef<Path>,
) -> Result<DevRuntime> {
    let env = ProdRuntimeEnv::from_env()?;
    let data_dir = data_dir.as_ref().to_path_buf();
    tokio::fs::create_dir_all(&data_dir).await?;

    let postgres_cfg = PostgresConfig {
        url: env.central_db_url.clone(),
        max_connections: env.central_db_max_connections,
        acquire_timeout: Duration::from_millis(env.central_db_acquire_timeout_ms),
    };
    let central_pg = PostgresCentralDb::connect(&postgres_cfg).await?;
    central_pg.migrate().await?;
    let central_db: Arc<dyn CentralDb> = Arc::new(central_pg.clone());

    let project_db_cfg = ProjectDbConfig {
        root_dir: data_dir.join("project_dbs"),
    };
    let project_db: Arc<dyn ProjectDb> = Arc::new(LibsqlProjectDb::new(
        central_pg.pool().clone(),
        &project_db_cfg,
    ));

    let s3_cfg = S3Config {
        bucket: env.s3_bucket,
        region: env.s3_region,
        endpoint: env.s3_endpoint,
        access_key_id: env.s3_access_key_id,
        secret_access_key: env.s3_secret_access_key,
        prefix: env.s3_prefix,
    };
    let filestore: Arc<dyn Filestore> = Arc::new(S3Filestore::new(&s3_cfg).await?);

    let redis_cfg = RedisConfig {
        url: env.redis_url.clone(),
        key_prefix: env.redis_key_prefix,
    };
    let cache: Arc<dyn Cache> = Arc::new(RedisCache::new(&redis_cfg).await?);

    let helix_cfg = HelixConfig {
        url: env.helix_url,
        api_key: env.helix_api_key,
        timeout: Duration::from_millis(env.helix_timeout_ms),
    };
    let graph_store: Arc<dyn GraphStore> = Arc::new(HelixGraphStore::new(&helix_cfg)?);

    let vector_store: Arc<dyn VectorStore> = Arc::new(PgVectorStore::new(
        central_pg.pool().clone(),
        env.vector_dim,
    ));

    let event_cfg = EventSyncConfig {
        postgres_url: env.central_db_url.clone(),
        redis_url: env.redis_url.clone(),
        ..EventSyncConfig::default()
    };
    let event_bus: Arc<dyn EventBus> = Arc::new(RedisEventBus::connect(event_cfg).await?);

    let context_refresh: Arc<dyn ContextRefresh> = Arc::new(ContextRefreshEngine::new(
        central_db.clone(),
        project_db.clone(),
        event_bus.clone(),
    ));

    let ai_approver = Some(build_ai_approver(cfg)?);
    let core_agents = Arc::new(CoreAgentsExecutor::new(
        central_db.clone(),
        project_db.clone(),
        event_bus.clone(),
        ai_approver,
    ));

    central_db
        .upsert_org(&OrgRecord {
            org_id: env.org_id,
            name: "opencraw-prod".to_string(),
            created_at: Utc::now(),
        })
        .await?;

    let handle = if let Some(existing) = project_db.get_handle(env.org_id, env.project_id).await? {
        existing
    } else {
        project_db.provision(env.org_id, env.project_id).await?
    };

    setup::register_subscriptions(&*event_bus, &env.org_id.to_string()).await?;

    let voyager = build_dev_voyager_memory(graph_store.clone(), vector_store.clone());
    let horizons_memory =
        Arc::new(VoyagerBackedHorizonsMemory::new(voyager)) as Arc<dyn HorizonsMemory>;
    let memory = if cfg.memory.enabled {
        Some(horizons_memory.clone())
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
        Arc::new(build_rlm_evaluator(
            VerifierConfig::default(),
            signals,
            None,
        )?) as Arc<dyn Evaluator>
    };
    let evaluation_engine = Arc::new(EvaluationEngine::new(
        central_db.clone(),
        project_db.clone(),
        filestore.clone(),
        evaluator.clone(),
    ));
    let evaluation = Some(evaluation_engine.clone());

    let graph_llm =
        Arc::new(SafeGraphLlmClient::new()) as Arc<dyn horizons_graph::llm::LlmClientApi>;
    let graph_tools =
        Arc::new(SafeGraphToolExecutor::new()) as Arc<dyn horizons_graph::tools::ToolExecutor>;
    let graph_engine = Arc::new(GraphEngine::new(graph_llm, graph_tools));

    let subagent: Arc<dyn Subagent> =
        Arc::new(CoreAgentsSubagent::new(core_agents.clone())) as Arc<dyn Subagent>;
    let pipelines: Arc<dyn PipelineRunner> = Arc::new(DefaultPipelineRunner::new(
        event_bus.clone(),
        subagent,
        None,
        None,
    )) as Arc<dyn PipelineRunner>;

    let default_model = cfg.default_model()?.to_string();
    let mipro_llm: Arc<dyn MiproLlmClient> = Arc::new(MiproLlmAdapter {
        llm: os_llm::LlmClient::new(&cfg.api_key_for_active_profile_model()?, &default_model)?,
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
        org_id: env.org_id,
        project_id: env.project_id,
        project_db_handle: handle,
        project_db,
        event_bus,
        core_agents,
        memory,
        evaluation,
    })
}

struct MiproLlmAdapter {
    llm: os_llm::LlmClient,
}

#[async_trait::async_trait]
impl mipro_v2::LlmClient for MiproLlmAdapter {
    async fn complete(&self, prompt: &str) -> mipro_v2::Result<String> {
        let resp = self
            .llm
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

fn build_ai_approver(cfg: &OpenShellConfig) -> Result<Arc<dyn ActionApprover>> {
    let key = cfg.api_key_for_active_profile_model()?;
    let llm = os_llm::LlmClient::new(&key, cfg.default_model()?)?;
    Ok(Arc::new(LlmSafetyApprover { llm }))
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
            proposal.action_type, proposal.risk_level, proposal.payload
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

        let v: serde_json::Value = serde_json::from_str(&resp.message.content).map_err(|e| {
            horizons_core::Error::BackendMessage(format!(
                "ai approver invalid json response: {e}; body={}",
                resp.message.content
            ))
        })?;
        let decision = v.get("decision").and_then(|d| d.as_str()).ok_or_else(|| {
            horizons_core::Error::BackendMessage(
                "ai approver response missing string decision".to_string(),
            )
        })?;
        let reason = v
            .get("reason")
            .and_then(|r| r.as_str())
            .ok_or_else(|| {
                horizons_core::Error::BackendMessage(
                    "ai approver response missing string reason".to_string(),
                )
            })?
            .to_string();
        match decision {
            "approve" => Ok(ReviewDecision::Approved { reason }),
            "deny" => Ok(ReviewDecision::Denied { reason }),
            other => Err(horizons_core::Error::BackendMessage(format!(
                "ai approver returned unsupported decision: {other}"
            ))),
        }
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
            let line = it.content.to_string();
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

struct SafeGraphLlmClient {
    inner: OnceLock<std::result::Result<GraphLlmClient, String>>,
}

impl SafeGraphLlmClient {
    fn new() -> Self {
        Self {
            inner: OnceLock::new(),
        }
    }

    fn get(&self) -> horizons_graph::Result<&GraphLlmClient> {
        let inner = self.inner.get_or_init(|| {
            std::panic::catch_unwind(GraphLlmClient::new).map_err(|panic_payload| {
                let panic_message = if let Some(msg) = panic_payload.downcast_ref::<&str>() {
                    msg.to_string()
                } else if let Some(msg) = panic_payload.downcast_ref::<String>() {
                    msg.clone()
                } else {
                    "unknown panic payload".to_string()
                };
                format!("graph llm client initialization panicked: {panic_message}")
            })
        });

        match inner {
            Ok(client) => Ok(client),
            Err(message) => Err(horizons_graph::error::GraphError::internal(message.clone())),
        }
    }
}

#[async_trait::async_trait]
impl horizons_graph::llm::LlmClientApi for SafeGraphLlmClient {
    async fn send(
        &self,
        request: &horizons_graph::llm::LlmRequest,
        on_chunk: Option<horizons_graph::llm::LlmChunkCallback>,
    ) -> horizons_graph::Result<horizons_graph::llm::LlmResponse> {
        let client = self.get()?;
        client.send(request, on_chunk).await
    }
}

struct SafeGraphToolExecutor {
    inner: OnceLock<std::result::Result<GraphToolExecutor, String>>,
}

impl SafeGraphToolExecutor {
    fn new() -> Self {
        Self {
            inner: OnceLock::new(),
        }
    }

    fn get(&self) -> horizons_graph::Result<&GraphToolExecutor> {
        let inner = self.inner.get_or_init(|| {
            std::panic::catch_unwind(GraphToolExecutor::from_env).map_err(|panic_payload| {
                let panic_message = if let Some(msg) = panic_payload.downcast_ref::<&str>() {
                    msg.to_string()
                } else if let Some(msg) = panic_payload.downcast_ref::<String>() {
                    msg.clone()
                } else {
                    "unknown panic payload".to_string()
                };
                format!("graph tool executor initialization panicked: {panic_message}")
            })
        });

        match inner {
            Ok(executor) => Ok(executor),
            Err(message) => Err(horizons_graph::error::GraphError::internal(message.clone())),
        }
    }
}

#[async_trait::async_trait]
impl horizons_graph::tools::ToolExecutor for SafeGraphToolExecutor {
    async fn execute(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        context: &serde_json::Value,
        local_state: Option<&horizons_graph::tools::SharedLocalToolState>,
        graph_inputs: Option<&serde_json::Value>,
    ) -> horizons_graph::Result<serde_json::Value> {
        let executor = self.get()?;
        executor
            .execute(tool_name, args, context, local_state, graph_inputs)
            .await
    }
}
