# Work Backlog (Actionable)

Generated: 2026-02-07

This backlog is grouped by epics and written for direct implementation.

## Epic A: Runtime and Persistence Baseline

- [x] Add backend mode switch in OpenCraw config (`dev`/`prod`).
- [x] Refactor `/os-app/src/server.rs` to avoid hardcoded `data` path.
- [x] Implement control-plane config operations (`config.get`, `config.apply`, `config.patch` with `baseHash`).
- [ ] Implement bind/auth/discovery mode matrix (`loopback`, `lan`, `tailnet`, `auto`, `custom`).
- [x] Introduce persistent `SessionStore` backed by Horizons ProjectDb.
- [ ] Implement migration format for session envelopes.
- [ ] Generate and validate env-var/config-key compatibility inventory.
- [x] Add restart recovery test for session continuity.

## Epic B: Queue and Scheduling Semantics

- [x] Replace global serial gateway loop with lane-aware dispatcher.
- [x] Implement per-lane serialization and global concurrency cap.
- [x] Add queue modes: `collect`, `followup`, `steer`, `interrupt`.
- [x] Add global lane debounce configuration (`queue.debounce_ms`).
- [ ] Add per-channel debounce overrides.
- [ ] Add queue drop-policy variants and send-policy routing behavior.
- [ ] Add overflow summarization strategy for saturated lanes.
- [ ] Implement heartbeat scheduler baseline and `HEARTBEAT_OK` processing.
- [ ] Emit queue metrics/events for observability (structured logs are in place; numeric metrics export still pending).

## Epic C: Context Lifecycle and Memory

- [x] Implement token-aware context builder.
- [ ] Add pruning of stale tool outputs.
- [x] Add compaction job with summary checkpoints.
- [x] Add pre-compaction memory flush turn.
- [x] Add assistant-facing Horizons memory tools (`memory.search`, `memory.summarize`).
- [x] Add memory tool API endpoints for external/control-plane usage.
- [ ] Add deterministic bootstrap file loader (`AGENTS/SOUL/TOOLS/BOOTSTRAP/IDENTITY/USER/HEARTBEAT`).
- [ ] Implement lifecycle hooks (`before_tool_call`, `after_tool_call`, compaction, gateway lifecycle).
- [ ] Add quality benchmark scenarios for long sessions.

## Epic D: Tool System Parity

- [ ] Implement tool profiles (`minimal/coding/messaging/full`).
- [ ] Add allow/deny precedence logic and elevated mode policies.
- [ ] Build web search tool abstraction.
- [ ] Build web fetch/readability extraction tool.
- [ ] Add background process execution tooling with lifecycle controls.
- [ ] Add session tools (`sessions_list`, `sessions_history`, `sessions_send`, `sessions_spawn`).
- [ ] Add gateway admin tools for config/restart/diagnostics flows.
- [ ] Add tool telemetry and failure taxonomy.

## Epic E: Model and Streaming Parity

- [ ] Add provider registry and alias resolver.
- [ ] Add failover chain strategy (`ordered`, `priority`, `regional`).
- [ ] Add auth-profile registry and two-stage failover (auth profile -> model fallback).
- [ ] Add cooldown/backoff ladders and billing/auth failure handling policies.
- [ ] Add per-session model override controls.
- [x] Implement end-to-end streaming output pipeline.
- [x] Add capability-gated chunking and typing behavior per adapter.

## Epic F: Channel Expansion

- [x] Add Email (Gmail API) adapter.
- [x] Add Linear (GraphQL API) adapter.
- [ ] Add WhatsApp adapter.
- [ ] Add Slack adapter.
- [ ] Add Signal adapter.
- [ ] Add Matrix adapter.
- [ ] Implement shared channel policy framework (DM/group/mention rules).
- [ ] Add per-channel operational knobs (retries, limits, draft streaming/typing features).
- [ ] Implement cross-channel identity linker.

## Epic G: Access and Safety

- [ ] Implement pairing flows and access tier states.
- [ ] Integrate CoreAgents policy templates for tool/channel risk categories.
- [ ] Add optional sandbox execution path for high-risk tools.
- [ ] Add plugin trust policy (allow/deny/provenance) and enforcement checks.
- [ ] Add `security audit` parity command surface (`--deep`, `--fix`) and reports.
- [ ] Write incident response workflow (detect/contain/rotate/recover/verify/postmortem).
- [ ] Add policy simulation command for dry-run approval behavior.

## Epic H: Automation Surfaces

- [ ] Implement cron job entity and execution runtime in OpenCraw layer.
- [ ] Support main-session vs isolated-session automation modes.
- [ ] Implement webhook trigger endpoint and auth model.
- [ ] Implement poll-style trigger via context-refresh connectors.
- [ ] Add heartbeat and auth-monitor automation flows with explicit exit semantics.
- [ ] Add delivery routing options for automation outputs.

## Epic I: Horizons Differentiators

- [ ] Capture structured verification cases from conversation outcomes.
- [ ] Schedule optimization cycles and policy candidate generation.
- [ ] Add promotion gate based on eval score thresholds.
- [ ] Introduce graph templates for high-complexity workflows.
- [ ] Add dashboard/report endpoints for eval and optimization runs.

## Epic J: Ops and Productization

- [ ] Define production deployment profile for OpenCraw + Horizons dependencies.
- [ ] Add runbooks for channel credentials and incident handling.
- [ ] Define full slash/CLI command-group matrix and command registration behavior.
- [ ] Add SLOs for latency, queue depth, and failed runs.
- [ ] Add release gates: parity regression suite + safety checks.

## Epic K: Scope Decisions (Parity Boundary)

- [ ] Decide launch scope for voice/TTS parity; if in scope, define provider and interrupt behavior.
- [ ] Decide launch scope for node/device capability parity; if deferred, publish explicit deferral criteria.

## Suggested First Sprint (highest leverage)

- [x] A1 backend mode switch
- [x] A2 config control-plane primitives
- [x] A3 session persistence
- [x] B1 lane dispatcher
- [x] B2 per-lane serialization
- [x] C1 token-aware context builder
- [x] E4 streaming response path

## Progress Notes

- 2026-02-07:
  - Implemented runtime config and control-plane config API surfaces in code.
  - Implemented session snapshot persistence (local data-dir backed) with reload on startup.
  - Reworked gateway ingress from globally serialized handling to lane-aware queue workers.
  - Added token-aware context windowing with recent-message preservation and long tool-output trimming.
  - Implemented streaming response path (`chat_stream` -> gateway delta channel -> adapter) with explicit adapter capability checks.
  - Added WebChat typing events (`active=true/false`) and baseline adapter hook for typing support.
  - Added automated tests for config `base_hash` handling and session persistence reload behavior.
  - Enforced strict fail-fast behavior across runtime/config/gateway/channel/LLM paths:
    - Removed implicit provider and client-construction fallbacks.
    - Replaced warn-and-continue dispatch behavior with hard error propagation.
    - Replaced silent payload/config defaulting with explicit validation failures.
    - Tightened request payload schemas for config/message/skill APIs with `deny_unknown_fields`.
  - Added structured observability hardening across runtime, HTTP, gateway, assistant, and provider layers:
    - Added tracing bootstrap modes (`OPENCRAW_LOG_FORMAT=json|pretty|compact`) and panic-hook logging in `/Users/synth/OpenCraw/os-app/src/main.rs`.
    - Added request-id propagation and HTTP request lifecycle tracing in `/Users/synth/OpenCraw/os-app/src/server.rs`.
    - Added message lifecycle logging (ingress/lane dispatch/assistant run/egress) in `/Users/synth/OpenCraw/os-app/src/gateway.rs`.
    - Added assistant phase logging (context build, tool loops, approval outcomes, tool execution, memory append, stream summary) in `/Users/synth/OpenCraw/os-app/src/assistant.rs`.
    - Added provider/client request lifecycle logs (OpenAI/Anthropic sync and stream) in:
      - `/Users/synth/OpenCraw/os-llm/src/client.rs`
      - `/Users/synth/OpenCraw/os-llm/src/openai.rs`
      - `/Users/synth/OpenCraw/os-llm/src/anthropic.rs`
  - Implemented production runtime backend wiring for `runtime.mode=prod`:
    - Added strict env-driven Horizons backend bootstrapping in `/Users/synth/OpenCraw/os-app/src/dev_backends.rs`:
      - Postgres central DB (`PostgresCentralDb`)
      - LibSQL project DB (`LibsqlProjectDb`)
      - Redis cache + event bus (`RedisCache`, `RedisEventBus`)
      - S3 filestore (`S3Filestore`)
      - Helix graph store (`HelixGraphStore`)
      - PgVector vector store (`PgVectorStore`)
    - Added fail-fast runtime env validation (`OPENCRAW_ORG_ID`, `OPENCRAW_PROJECT_ID`, and required `HORIZONS_*` vars).
  - Replaced local JSON session snapshots with Horizons ProjectDb-backed persistence:
    - Added `opencraw_sessions` table migration/load/save/delete logic in `/Users/synth/OpenCraw/os-app/src/session.rs`.
    - Updated gateway/session routes to async DB persistence calls.
  - Implemented queue mode parity in `/Users/synth/OpenCraw/os-app/src/gateway.rs`:
    - Added configurable mode handling (`followup`, `collect`, `steer`, `interrupt`) with deterministic lane shaping.
    - Added in-flight run interruption for `interrupt` mode using lane-scoped watch signals.
    - Added unit tests for burst collect/latest-mode behavior and non-message invariants.
  - Added queue debounce baseline in `/Users/synth/OpenCraw/os-app/src/gateway.rs` and `/Users/synth/OpenCraw/os-app/src/config.rs`:
    - Added fixed-window lane debounce (`queue.debounce_ms`) before queue-mode shaping.
    - Added queue metadata marker (`queue_debounced_messages`) for debounced bursts.
    - Added env override support via `OPENSHELL_QUEUE_DEBOUNCE_MS`.
  - Implemented Horizons memory tool wrappers in `/Users/synth/OpenCraw/os-app/src/assistant.rs`:
    - Added model-facing tool definitions for `memory.search` and `memory.summarize` when memory is enabled.
    - Added scoped execution against `os.assistant.{channel}.{sender}` agent memory.
    - Added strict argument-validation unit tests for memory tool payloads.
  - Implemented Horizons-backed memory API routes in `/Users/synth/OpenCraw/os-app/src/routes/memory.rs`:
    - Added `POST /api/v1/os/memory/search` with strict scope/query/limit validation.
    - Added `POST /api/v1/os/memory/summarize` with strict scope/horizon validation.
    - Added route-unit tests for scope and validation behavior.
  - Implemented Horizons-backed context compaction in `/Users/synth/OpenCraw/os-app/src/assistant.rs`:
    - Added pre-compaction memory flush entries (`kind=pre_compaction_flush`) with transcript truncation controls.
    - Added compaction trigger/rewrite path using `memory.summarize(...)` checkpoints.
    - Added strict compaction config validation in `/Users/synth/OpenCraw/os-app/src/config.rs` (`context.compaction_enabled` requires `memory.enabled`).
  - Implemented Email + Linear channels in `/Users/synth/OpenCraw/os-channels/src`:
    - Added strict Gmail adapter (`email`) with polling/send/read-mark flow.
    - Added strict Linear adapter (`linear`) with assigned-issue comment polling and comment send.
  - Extended strict config/runtime wiring in `/Users/synth/OpenCraw/os-app/src/config.rs` and `/Users/synth/OpenCraw/os-app/src/server.rs`:
    - Added `[channels.email]` and `[channels.linear]` configs plus env overrides.
    - Removed soft skip behavior for enabled Telegram/Discord channels when tokens are missing.
    - Added startup registration and one-shot send support for Email/Linear.
  - Added channel runbook `/Users/synth/OpenCraw/plans/10-channel-setup-email-linear.md`.
  - Validation:
    - `cargo check -p os-channels -p os-app` passes.
    - `cargo test -p os-channels` blocked by local disk exhaustion (`No space left on device`).

## Definition of Done (per task)

- Code merged with docs and config updates.
- Telemetry emitted for new runtime behavior.
- Regression test or integration test added where practical.
- No fallback/shim behavior introduced; invalid input/config fails loudly and early.
