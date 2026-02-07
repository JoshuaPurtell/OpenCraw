# Implementation Roadmap

Generated: 2026-02-07

This roadmap assumes execution inside `/Users/synth/OpenCraw` with Horizons as the backing platform.

## Execution Log

### 2026-02-07 (completed in code)

- Phase 0 / Sprint items delivered:
  - Runtime mode and data directory config (`runtime.mode`, `runtime.data_dir`).
  - Config control-plane endpoints (`config.get`, `config.apply`, `config.patch` with `base_hash`).
  - Session snapshot persistence to runtime data dir with load-on-start.
- Phase 1 / Sprint items delivered:
  - Replaced global serial message handling with lane-aware dispatch.
  - Added per-lane serialization with a global concurrency budget (`queue.max_concurrency`) and lane buffering (`queue.lane_buffer`).
  - Added fixed-window lane debounce (`queue.debounce_ms`) before queue-mode shaping.
- Phase 2 / Sprint items delivered:
  - Added token-aware context windowing controls (`context.max_prompt_tokens`, `context.min_recent_messages`, `context.max_tool_chars`).
  - Implemented context trimming and tool-output truncation behavior in assistant prompt assembly.
- Phase 3 / Sprint items delivered:
  - Switched assistant request path from non-streaming chat calls to streaming chunk ingestion.
  - Added gateway delta forwarding path to channel adapters.
  - Added WebChat delta transport (`type = "delta"`) and explicit capability-gated streaming dispatch.
  - Added WebChat typing signals (`type = "typing"`) during generation.

### 2026-02-07 (strict fail-fast hardening pass)

- Runtime/config hardening:
  - `runtime.mode = "prod"` now fails startup explicitly (no dev fallback path).
  - Config parsing now rejects unknown fields across config sections (`#[serde(deny_unknown_fields)]`).
  - Environment overrides now fail on invalid values instead of being silently ignored.
  - Model/provider/key validation now fails during config validation (no implicit provider fallback, no key-less startup).
- Gateway/session hardening:
  - Removed warn-and-continue behavior in gateway dispatch/lane handlers; failures now propagate.
  - Session persistence is now error-propagating in active request paths.
  - Typing/delta sends are capability-gated and error-propagating when enabled.
- Channel/LLM payload hardening:
  - Telegram/Discord/WebChat outbound non-2xx send failures now return errors.
  - Telegram/Discord/WebChat malformed inbound payloads now fail loudly instead of being silently dropped.
  - LLM client/provider detection is strict and explicit; unsupported models now error.
  - OpenAI/Anthropic stream and response parsing now rejects missing required fields instead of defaulting.
- Validation:
  - `cargo check` and full `cargo test` pass after hardening changes.

### 2026-02-07 (email + linear channel slice)

- Phase 4 / Sprint items delivered:
  - Added `email` channel adapter with strict Gmail API integration:
    - inbound polling via Gmail message list/get
    - outbound send via Gmail `messages/send`
    - optional read-marking after ingest
    - strict recipient parsing (`email` and optional `thread:<thread_id>:<email>` format)
  - Added `linear` channel adapter with strict Linear GraphQL integration:
    - inbound polling from viewer assigned-issue comments
    - optional team filters (`team_ids`)
    - outbound comment posting by issue id
  - Added strict config/env validation and wiring for new channels:
    - new `[channels.email]` and `[channels.linear]` config sections
    - fail-fast token validation for Telegram/Discord (removed soft skip behavior when enabled)
    - env overrides for `GMAIL_ACCESS_TOKEN`, `LINEAR_API_KEY`, and channel tuning knobs
  - Added channel setup runbook for production configuration and verification:
    - `/Users/synth/OpenCraw/plans/10-channel-setup-email-linear.md`
- Validation:
  - `cargo check -p os-channels -p os-app` passes.
  - `cargo test -p os-channels` could not complete due machine disk exhaustion (`No space left on device`) during dependency build.

### 2026-02-07 (known limits after current slice)

- `runtime.mode = "prod"` now uses strict production Horizons backend wiring and fails fast when required environment variables are missing/invalid.
- Session durability now uses Horizons ProjectDb storage (`opencraw_sessions`); envelope-versioning/migration strategy beyond the current schema remains pending.
- Queue modes (`collect`, `followup`, `steer`, `interrupt`) are implemented; per-channel debounce, drop-policy variants, and overflow summarization are still pending.

### 2026-02-07 (prod runtime + session store slice)

- Phase 0 / Sprint items delivered:
  - Implemented `runtime.mode=prod` wiring with strict env validation and no dev fallback path.
  - Added production backend composition for OpenCraw on Horizons (Postgres, LibSQL, Redis, S3, Helix, PgVector).
- Phase 1 / Sprint items delivered:
  - Replaced local file session persistence with ProjectDb-backed session storage and schema migration.
  - Updated gateway and session API flows to async database persistence semantics.

### 2026-02-07 (queue mode parity slice)

- Phase 1 / Sprint items delivered:
  - Implemented queue mode semantics in gateway lane workers:
    - `followup`: strict FIFO processing.
    - `collect`: burst coalescing into a single assistant run.
    - `steer`: latest-message-wins before each run.
    - `interrupt`: explicit cancellation of in-flight run when newer lane traffic arrives.
  - Added queue reshaping metadata markers (`queue_collected_messages`, `queue_dropped_messages`) on transformed inbound payloads.
  - Added unit coverage for collect/latest/non-message reshaping behavior.

### 2026-02-07 (Horizons memory tools slice)

- Phase 2 / Sprint items delivered:
  - Added assistant-facing memory tool definitions exposed to the model when Horizons memory is enabled:
    - `memory.search` (scoped retrieval against `os.assistant.{channel}.{sender}`)
    - `memory.summarize` (scoped summary generation by horizon)
  - Wired memory tool execution through existing approval/audit flow (`CoreAgents`) with explicit low-risk policy.
  - Added strict argument validation tests for memory tool inputs.
  - Added OpenCraw memory API endpoints backed by Horizons memory:
    - `POST /api/v1/os/memory/search`
    - `POST /api/v1/os/memory/summarize`

### 2026-02-07 (context compaction + pre-flush slice)

- Phase 2 / Sprint items delivered:
  - Added Horizons-backed pre-compaction memory flush in assistant runtime before history rewrite.
  - Added session compaction trigger pipeline with summary checkpoint insertion and retained-recent-message policy.
  - Added strict compaction config controls in context config (`compaction_enabled`, trigger/retain/horizon/flush limits).
  - Added fail-fast validation: compaction cannot be enabled without memory.

## Phase 0: Foundation Hardening (1-2 weeks)

## Objectives

- Remove dev-only wiring assumptions.
- Establish production-grade runtime contracts.

## Deliverables

- Runtime backend mode switch (`dev` vs `prod`) in OpenCraw.
- Replace hardcoded `data` path with configurable data/runtime storage.
- Document and enforce org/project scoping model.
- Add control-plane config lifecycle (`config.get`, `config.apply`, `config.patch` with `baseHash`).
- Define bind/auth/discovery compatibility matrix (`loopback`, `lan`, `tailnet`, `auto`, `custom`).
- Publish initial env-var and config-key compatibility inventory.
- Confirm compile + smoke tests in CI for OpenCraw main workflows.

## Exit criteria

- OpenCraw can run against non-dev Horizons backends.
- No hardcoded nil org/project in production path.

## Phase 1: Queue + Session Core Parity (2-4 weeks)

## Objectives

- Reach baseline reliability semantics for messaging.

## Deliverables

- Lane-aware queue with per-session serialization and global budget.
- Queue modes: `collect`, `followup`, `steer`, `interrupt`.
- Debounce and overflow summarization behavior.
- Queue drop-policy variants and send-policy routing controls.
- Persistent session store and `SessionKey` model.
- Session reset policy engine (daily + idle).
- Heartbeat contract and `HEARTBEAT_OK` handling baseline.

## Exit criteria

- High-concurrency chat traffic no longer blocks globally.
- Session continuity survives restart.

## Phase 2: Context Engine Parity (2-3 weeks)

## Objectives

- Implement OpenClaw-like memory and context lifecycle.

## Deliverables

- Context pruning policy (tool-output trimming and turn windowing).
- Compaction pipeline triggered near token thresholds.
- Pre-compaction memory flush hook.
- Dedicated memory tools (`memory_search`, `memory_get`-style behavior).
- Bootstrap/prompt file loading contract (`AGENTS/SOUL/TOOLS/BOOTSTRAP/IDENTITY/USER/HEARTBEAT`).
- Hook lifecycle parity (`before_tool_call`, `after_tool_call`, compaction hooks, gateway hooks).

## Exit criteria

- Long-running sessions remain performant and coherent.
- Memory retrieval quality demonstrably improves tool and response relevance.

## Phase 3: Tool and Model Parity (2-4 weeks)

## Objectives

- Close practical agent capability gaps.

## Deliverables

- Tool profile system with allow/deny and elevated mode.
- Web search/fetch toolchain and background process management.
- Session and gateway admin tool surface parity (list/history/send/spawn/config actions).
- Provider alias/failover chain support and per-session model override.
- Two-stage failover (auth-profile rotation, then model fallback) with cooldown ladders.
- Streaming output pipeline with chunking and typing indicators.

## Exit criteria

- OpenCraw handles major day-to-day agent tasks at OpenClaw-like UX quality.

## Phase 4: Channel Expansion + Access Policy (3-6 weeks)

## Objectives

- Close channel surface gap and hardened access controls.

## Deliverables

- New adapters: WhatsApp, Slack, Signal, Matrix.
- DM pairing/access tiers and richer channel policy matrix.
- Per-channel operational knobs (retry/limits/chunk/typing policy differences).
- Cross-channel identity linking.

## Exit criteria

- Core channel parity target reached for primary user segments.

## Phase 5: Automation and Workflow Parity (2-3 weeks)

## Objectives

- Deliver cron/poll/webhook behavior integrated with queue/session model.

## Deliverables

- OpenCraw automation API using Horizons events/context-refresh primitives.
- Cron job execution modes (main vs isolated).
- Webhook ingestion -> queue trigger path.
- Heartbeat and auth-monitor automation patterns with explicit exit semantics.

## Exit criteria

- Scheduled and event-driven workflows run reliably with auditability.

## Phase 6: Horizons Differentiators (ongoing)

## Objectives

- Move beyond parity and ship a self-improving assistant platform.

## Deliverables

- Evaluation pipelines from user feedback and sampled transcripts.
- Regular optimization cycles with guarded policy promotion.
- Graph-driven workflow templates for complex tasks.
- Observability and quality dashboards.

## Phase 7: Ops, Security, and Command Surface Parity (2-3 weeks)

## Objectives

- Ensure parity claims include operational and incident readiness, not only core runtime behavior.

## Deliverables

- Slash/CLI command-group parity for critical operations.
- Security audit command equivalents and hardening routines.
- Plugin trust policy (allow/deny/provenance) and credential path checks.
- Incident-response runbook with telemetry and recovery checkpoints.

## Exit criteria

- On-call and operators can execute parity-critical operations without undocumented manual steps.

## Exit criteria

- Continuous quality loop is operational with measurable improvements.

## Critical Dependencies

1. Horizons production vector store and connector completeness (per `/Users/synth/horizons/issues.txt`).
2. Stable contract for engine/sandbox and MCP behavior across SDKs.
3. Channel credentials + ops runbooks for each newly supported network.
4. Alignment on whether voice/TTS and node capabilities are required in parity scope for launch.

## Risk Register

- Risk: Scope explosion from channel expansion.
  - Mitigation: enforce minimal viable channel feature set before deep integrations.
- Risk: Context quality regressions during pruning/compaction rollout.
  - Mitigation: gated rollout + evaluation benchmarks.
- Risk: Prompt/policy optimization degrades live behavior.
  - Mitigation: canary and rollback gates with score thresholds.
- Risk: Security drift from wider tool/channel surface.
  - Mitigation: default-deny policy, approval gates, audit-first design.
- Risk: Parity drift caused by config/env incompatibilities.
  - Mitigation: release-gated compatibility matrix and config conformance tests.
- Risk: Operator surface mismatch (CLI/slash/admin commands) causes incident delays.
  - Mitigation: typed command registry and runbook rehearsal.

## Suggested Milestone Sequencing

1. M1: Production runtime mode and persistent sessions.
2. M2: Queue semantics and context lifecycle parity.
3. M3: Tools/model failover/streaming parity.
4. M4: Core channel expansion.
5. M5: Automation + heartbeat parity.
6. M6: Ops/security/command-surface parity.
7. M7: Differentiator loop (eval/opt/graph).
