# OpenCraw System Audit

Generated: 2026-02-07

## Scope

This audit covers:

- Existing plan: `/Users/synth/Desktop/opencraw-plan.md`
- OpenCraw source: `/Users/synth/OpenCraw`
- Horizons platform source: `/Users/synth/horizons`
- OpenClaw docs baseline: `https://docs.openclaw.ai` (including `llms.txt` and `llms-full.txt`)

## Source Inventory

### OpenCraw reviewed

- Runtime wiring: `/Users/synth/OpenCraw/os-app/src/server.rs`
- Gateway message loop: `/Users/synth/OpenCraw/os-app/src/gateway.rs`
- Assistant/tool loop: `/Users/synth/OpenCraw/os-app/src/assistant.rs`
- Sessions: `/Users/synth/OpenCraw/os-app/src/session.rs`
- Config and model routing: `/Users/synth/OpenCraw/os-app/src/config.rs`
- Horizons wiring layer: `/Users/synth/OpenCraw/os-app/src/dev_backends.rs`
- Tools: `/Users/synth/OpenCraw/os-tools/src/*.rs`
- Channels: `/Users/synth/OpenCraw/os-channels/src/*.rs`
- LLM client: `/Users/synth/OpenCraw/os-llm/src/*.rs`
- Deployment/config: `/Users/synth/OpenCraw/config.example.toml`, `/Users/synth/OpenCraw/docker-compose.yml`, `/Users/synth/OpenCraw/Dockerfile`

### Horizons reviewed

- API surface: `/Users/synth/horizons/horizons_server/src/routes/*.rs`
- Core agents and scheduler: `/Users/synth/horizons/horizons_core/src/core_agents/*.rs`
- Memory/optimization/evaluation wiring:
  - `/Users/synth/horizons/horizons_core/src/memory/*.rs`
  - `/Users/synth/horizons/horizons_core/src/optimization/*.rs`
  - `/Users/synth/horizons/horizons_server/src/routes/optimization.rs`
  - `/Users/synth/horizons/horizons_server/src/routes/evaluation.rs`
- Pipelines/graph/context-refresh/events:
  - `/Users/synth/horizons/horizons_server/src/routes/pipelines.rs`
  - `/Users/synth/horizons/horizons_server/src/routes/graph.rs`
  - `/Users/synth/horizons/horizons_server/src/routes/context_refresh.rs`
  - `/Users/synth/horizons/horizons_server/src/routes/events.rs`
- Sandbox engine:
  - `/Users/synth/horizons/horizons_server/src/routes/engine.rs`
  - `/Users/synth/horizons/horizons_core/src/engine/*.rs`
- Known platform issues list: `/Users/synth/horizons/issues.txt`

### OpenClaw docs reviewed

- Docs index: `https://docs.openclaw.ai/llms.txt`
- Architecture: `https://docs.openclaw.ai/concepts/architecture`
- Queue: `https://docs.openclaw.ai/concepts/queue`
- Memory: `https://docs.openclaw.ai/concepts/memory`
- Session management: `https://docs.openclaw.ai/concepts/session`
- Model failover: `https://docs.openclaw.ai/concepts/model-failover`
- Channels index: `https://docs.openclaw.ai/channels`
- Features overview: `https://docs.openclaw.ai/concepts/features`
- Tools configuration: `https://docs.openclaw.ai/gateway/configuration`
- Providers: `https://docs.openclaw.ai/concepts/model-providers`
- Multi-agent: `https://docs.openclaw.ai/concepts/multi-agent`
- Streaming/chunking: `https://docs.openclaw.ai/concepts/streaming`
- Cron jobs: `https://docs.openclaw.ai/automation/cron-jobs`

## Verified Current State

## 1) OpenCraw is currently a dev-mode gateway, not a production Horizons app

Evidence:

- Runtime always uses `build_dev_runtime` and local `data` directory (`server.rs` lines 67-68).
- `build_dev_runtime` instantiates `DevCentralDb`, `DevProjectDb`, `DevEventBus`, `DevVectorStore` (`dev_backends.rs` lines 77-85).
- Org/project are fixed nil UUIDs (`dev_backends.rs` lines 62-68, 100-121).

Implication:

- OpenCraw currently runs Horizons capabilities in a local/dev composition, not in a production tenancy/runtime model.

## 2) Compose starts Postgres/Redis, but OpenCraw does not consume them yet

Evidence:

- `docker-compose.yml` defines `postgres` and `redis` services.
- OpenCraw runtime wiring uses `Dev*` backends (in-memory/local file) and does not bind to compose Postgres/Redis in `server.rs` and `dev_backends.rs`.

Implication:

- Current deployment shape implies production dependencies but code path is still local-dev infrastructure.

## 3) Message handling is globally serialized

Evidence:

- Gateway run loop receives one message at a time and processes synchronously (`gateway.rs` lines 55-69, 119-147).

Implication:

- No lane/session-aware concurrency. High-latency tool calls can block unrelated senders/channels.

## 4) Sessions are in-memory only

Evidence:

- Session storage is `DashMap<(channel_id, sender_id), Session>` with no persistence hooks (`session.rs` lines 52-72).

Implication:

- Restart wipes sessions, usage totals, and conversational continuity.

## 5) Assistant loop is basic and non-streaming

Evidence:

- Tool loop capped at 4 rounds (`assistant.rs` lines 135-142).
- Calls `llm.chat(...)` (non-stream path), not `chat_stream(...)` (`assistant.rs` line 155).
- No compaction/pruning branch before prompt assembly (`assistant.rs` lines 144-155).

Implication:

- No partial streaming UX, no adaptive context compression, no overflow strategy.

## 6) Memory integration exists but is minimal

Evidence:

- Optional retrieval/append path in assistant (`assistant.rs` lines 220-283).
- Memory disabled by default in config (`config.example.toml` `[memory] enabled = false`).
- Dev embedder/summarizer are placeholder implementations (`dev_backends.rs` lines 260-267 and below).

Implication:

- Memory exists but not yet at OpenClaw-level operational behavior (daily logs + long-term memory tooling + compaction lifecycle).

## 7) Tooling has core primitives but limited parity

Evidence:

- `shell.execute` runs foreground shell command with timeout (`shell.rs` lines 36-57).
- Filesystem supports read/write/list/search under root with traversal guard (`filesystem.rs` lines 29-52, 145-197).
- Browser tool is explicit placeholder (`browser.rs` lines 6-9, 36-44).

Implication:

- Missing mature web/media/background-process stacks expected in OpenClaw baseline.

## 8) Channel support is a useful starter, not parity coverage

Evidence:

- Implemented channels: webchat, telegram, discord, imessage.
- Discord includes mention gate in guilds (`discord.rs` lines 185-195).
- iMessage has group prefixes and chat.db poller (`imessage.rs` lines 33-35, 233-239).

Implication:

- Useful but far below OpenClaw channel breadth and advanced policy behavior.

## 9) Model provider support is narrow

Evidence:

- Provider detection: only `claude-*` => Anthropic; everything else => OpenAI-compatible (`os-llm/src/client.rs` lines 103-109).

Implication:

- No first-class provider catalog/failover behavior comparable to OpenClaw provider matrix.

## 10) Build health

- `cargo check -p os-app` succeeds (2026-02-07).
- Warnings indicate unused optimization config and unused fields/functions (config/server dead code warnings).

## Horizons Capability Reality (for OpenCraw leverage)

Horizons already exposes production-grade primitives that OpenCraw can consume instead of rebuilding:

- Memory API: `/memory`, `/memory/summarize` (`horizons_server/src/routes/memory.rs` lines 43-47)
- Optimization APIs: `/optimization/run|status|reports|cycles` (`optimization.rs` lines 52-58)
- Evaluation APIs: `/eval/run`, `/eval/reports` (`evaluation.rs` lines 30-34)
- Graph APIs: `/graph/*` including validate/normalize/execute (`graph.rs` lines 57-65)
- Pipelines APIs: `/pipelines/*` with approval/cancel (`pipelines.rs` lines 20-26)
- Context refresh and connectors: `/context-refresh/*`, `/connectors` (`context_refresh.rs` lines 60-65)
- Sandbox engine APIs: `/engine/run|start|events|release|health` (`engine.rs` lines 84-91)
- MCP gateway APIs: `/mcp/config|tools|call` (`mcp.rs` lines 52-57)

Important caveat from Horizons repo itself:

- `/Users/synth/horizons/issues.txt` flags gaps like missing production vector store, missing SDK engine endpoints, missing outbound webhook delivery, and incomplete connector coverage.

## OpenClaw Baseline Confirmation

From official docs (2026-02 snapshot):

- OpenClaw gateway is a single long-lived daemon over WS with typed protocol and multi-surface routing.
- Queue supports lane-aware behavior and modes (`steer`, `followup`, `collect`, `steer-backlog`, `interrupt`) with debounce and overflow controls.
- Memory architecture includes long-term `MEMORY.md`, daily logs, and hybrid retrieval.
- Sessions include scope hierarchy, reset policies, pruning/compaction controls, and model/thinking inheritance.
- Model failover has ordered fallback strategies and health tracking.
- Channel support spans significantly more channels than current OpenCraw.

## OpenClaw Detail-Level Gaps (from updated feature inventory)

The updated `/Users/synth/Desktop/openclaw-features.md` adds operational details that were not explicitly represented in the first pass:

1. Gateway control plane details
   - Config lifecycle (`config.get`, `config.apply`, `config.patch` with `baseHash`), bind modes (`loopback`/`lan`/`tailnet`/`auto`/`custom`), auth/discovery coupling, and idempotency expectations.
2. Bootstrap and prompt-assembly contract
   - Full file set (`AGENTS.md`, `SOUL.md`, `TOOLS.md`, `BOOTSTRAP.md`, `IDENTITY.md`, `USER.md`, `HEARTBEAT.md`) plus mode controls (`full`/`minimal`/`none`) and hook lifecycle coverage.
3. Queue/session/streaming knob depth
   - Drop-policy variants, overflow summarize behaviors, chunk/coalesce modes, send-policy rules, and heartbeat contract details.
4. Tools and command surface depth
   - Background exec lifecycle/cleanup, richer browser/session/gateway management tools, elevated gating hierarchy, and full slash/CLI command groups.
5. Models and auth operations
   - Two-stage failover (auth-profile rotation first, then model fallback), cooldown ladders, billing-failure handling, and session pinning.
6. Security and incident operations
   - Security audit command semantics, credential-path hardening expectations, plugin trust policy, and incident-response procedures.
7. Platform extensions with parity decisions needed
   - Voice/TTS behavior and nodes/device capability matrix (whether included in parity claim or explicitly deferred).
8. Config/env compatibility inventory
   - Broader environment/config key compatibility matrix required for operational parity claims.

## Bottom Line

The original thesis in `/Users/synth/Desktop/opencraw-plan.md` is directionally correct:

- OpenCraw should be the thin product/gateway layer.
- Horizons should carry infrastructure-heavy concerns.

But current code is still early-stage and dev-mode. The path to "full OpenClaw on Horizons" is feasible, provided the next steps prioritize:

1. Message/session runtime correctness and durability in OpenCraw.
2. Replacing dev-only wiring with production Horizons backends and tenancy model.
3. Implementing OpenClaw parity surface (queue/session/memory/channel/tool/model behavior).
4. Adding OpenClaw-compatible control-plane and ops surfaces (config patch/apply, CLI/slash parity, security/incident procedures).
5. Then activating Horizons differentiators (eval/opt/graph/pipeline loops) as product multipliers.
