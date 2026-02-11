# OpenCraw OpenClaw Production Parity Master Plan (Horizons-First)

Generated: 2026-02-09  
Last Updated: 2026-02-10  
Repository: (this repo)
Companion platform repo: (Horizons companion repo)

## Mission

Build OpenCraw to match OpenClaw functionality while using Horizons as the execution platform, and ship it as a production-grade product showcase.

This is not a demo plan. This is an operational delivery plan with hard quality gates.

## Source of Truth

- Feature baseline and parity target:
  - `docs/product/feature-inventory.md`
  - `(openclaw)/README.md`
  - `(openclaw)/docs`
- Existing parity docs:
  - (historical) `plans/02-parity-gap-matrix.md`
  - (historical) `plans/03-target-architecture.md`
  - (historical) `plans/04-implementation-roadmap.md`
- Code quality and investigation principles:
  - `~/.claude/CLAUDE.md`

## Scope Decision (2026-02-10): Core-First Parity

Decision: for the next release milestone, OpenCraw targets OpenClaw core runtime parity first and explicitly defers native apps and dashboard polish.

Evidence anchors:

1. OpenClaw baseline includes broad platform/app surfaces (Voice Wake, Talk Mode, Canvas, macOS/iOS/Android nodes) plus channels/tools/runtime:
   - `(openclaw)/README.md:121`
   - `(openclaw)/README.md:151`
   - `(openclaw)/README.md:158`
2. OpenClaw operator and automation surface is CLI-heavy and can run headless:
   - `(openclaw)/README.md:140`
   - `(openclaw)/docs/cli/index.md`
   - `(openclaw)/docs/automation/cron-jobs.md`
3. OpenCraw currently exposes a reduced slice and minimal CLI/UI surface:
   - `README.md:7`
   - `os-app/src/main.rs:40`
   - `web/src/App.tsx:122`

Core parity scope (Milestone M1, launch-blocking):

1. Gateway control plane, auth, and secure runtime boundaries.
2. Channel runtime parity for core + plugin-driven channels (headless first).
3. Tool runtime parity (browser, apply_patch, sandbox/elevated, policy model).
4. Pairing/approval lifecycle for DM and device trust boundaries.
5. Automation parity (cron/webhook/poll/hook), including idempotency and observability.
6. Skills trust pipeline and model failover/auth rotation.
7. Operator control via CLI and API; minimal web control surface is acceptable.

Deferred scope (Milestone M2, not launch-blocking for M1):

1. Native app parity (macOS/iOS/Android app-specific UX flows).
2. Voice-first and node-rich product experiences (Voice Wake, Talk Mode, Canvas-centric UX).
3. Dashboard/TUI polish beyond core operator workflows.

## Tiered Core Delivery Model (M1)

Channel/runtime delivery is intentionally tiered. "Parity" is not claimed globally until each tier is certified.

Tier order:

1. Tier T1: Telegram must be rock-solid first.
   - Baseline anchor: `docs/providers/telegram.md:10`
2. Tier T2: Gmail/Email workflows must be rock-solid second.
   - Baseline anchors:
     - `(openclaw)/docs/automation/gmail-pubsub.md:11`
     - `README.md:14`
3. Tier T3: Core multi-channel expansion (Slack, Discord, Matrix, WhatsApp, Signal).
   - Baseline anchors:
     - `(openclaw)/README.md:148`
     - `docs/providers/slack.md:7`
4. Tier T4: Long-tail plugin channels and ecosystem breadth.
   - Baseline anchor: `(openclaw)/README.md:148`

Rock-solid means all are true for a channel tier:

1. Auth, pairing, and allowlist/approval boundaries are enforced by default.
2. Delivery is deterministic and replay-safe (idempotent ingest, dedupe, bounded retries/backoff).
3. Session routing invariants hold (DM vs group isolation, mention/reply behavior, ordering guarantees).
4. Operator diagnostics are complete (`status`, logs, health, channel-specific probes).
5. Tier acceptance tests pass and stay green in CI.

## Parallel Agent Execution Model (Provider Lanes)

Yes, multi-agent parallelization is possible, but only with explicit overlap controls.

Current overlap hotspots (single-owner integration files):

1. `os-app/src/channel_plugins.rs`
2. `os-app/src/config.rs`
3. `os-app/src/server.rs`
4. `os-app/src/main.rs`

These files currently couple all providers, so parallel provider agents should not edit them directly except through an integration queue.

Lane model:

1. Provider lane agents (parallel):
   - Own `os-channels/src/<provider>.rs` and provider-specific tests.
   - Own provider documentation and provider acceptance cases.
   - Must not modify global registry/config wiring directly.
2. Core lane agent (parallel):
   - Owns pairing/auth/automation/skills/model/runtime invariants shared by all providers.
3. Integration lane agent (single threaded):
   - Merges provider lanes into hotspot files (`channel_plugins.rs`, `config.rs`, `server.rs`, CLI routing).
   - Resolves schema/validation conflicts and keeps boot/config surfaces coherent.
4. Certification lane agent:
   - Maintains WS-15 tier certification harness and signs off T1->T2->T3->T4 promotion.

Provider lane ownership for current tiers:

1. T1: Telegram lane + Pairing lane + Integration lane + Certification lane.
2. T2: Gmail/Email lane + Automation lane + Integration lane + Certification lane.
3. T3: Slack, Discord, Matrix, WhatsApp, Signal lanes (parallel) + Integration lane + Certification lane.
4. T4: External plugin lanes (parallel) + Integration lane + Certification lane.

Branching and merge contract:

1. One branch per lane (`codex/lane-<tier>-<provider>`).
2. Provider branches rebase only onto integration branch tip.
3. Hotspot files are touched only by integration branch, except pre-approved refactor commits.
4. Tier promotion only occurs when certification lane checks are green and merged.

## Checklist Dashboard (At-a-Glance)

This section is the fastest status view. Keep it in sync with the backlog table below.

Milestones and tiers:

- [ ] M1 Core parity certified
- [ ] T1 Telegram rock-solid certified
- [ ] T2 Gmail/Email rock-solid certified
- [ ] T3 Core multi-channel expansion certified
- [ ] T4 Long-tail plugin channel expansion certified
- [ ] M2 Full product parity certified (native apps + UX)

Workstreams:

- [x] WS-01 Build/test/clippy recovery (Completed)
- [ ] WS-02 Control-plane auth hardening (In Progress)
- [ ] WS-03 Bind/discovery policy matrix (In Progress)
- [ ] WS-04 Tool profile + sandbox/elevated split (In Progress)
- [ ] WS-05 Browser/apply_patch parity (In Progress)
- [ ] WS-06 Plugin channel architecture (In Progress)
- [ ] WS-07 Priority channel expansion (In Progress)
- [ ] WS-08 Pairing lifecycle v2 (Planned)
- [ ] WS-09 Automation scheduler and webhooks (In Progress)
- [ ] WS-10 Skills trust pipeline (In Progress)
- [ ] WS-11 Model failover and auth rotation (In Progress)
- [ ] WS-12 CLI surface parity expansion (Planned)
- [ ] WS-13 Control UI/dashboard/TUI parity (Deferred)
- [ ] WS-14 Horizons alignment/pin strategy (Planned)
- [ ] WS-15 Parity acceptance suite (Planned)
- [ ] WS-16 Native apps + voice/canvas parity (Deferred)

## Current State Snapshot (2026-02-10)

## Build and lint status

- `cargo check --workspace --all-targets --locked` passes.
- `cargo test --workspace --all-targets --locked` passes.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` passes.

Baseline recovery completed:

1. Signature/call-site drift removed across assistant and gateway:
   - `os-app/src/assistant.rs`
   - `os-app/src/gateway.rs`
2. Config/test data structure alignment fixed for channel config expansion:
   - `os-app/src/pairing.rs`
3. Strict lint-denied issues resolved in `os-llm`, `os-channels`, and `os-tools`.
4. Foundation hardening tranche landed:
   - Workspace/toolchain contract aligned (`edition=2024`, `resolver=3`, explicit `rust-version`).
   - Graceful shutdown and cancellation-token drain wiring for server/gateway/automation runtime.
   - HTTP timeout and global concurrency envelope for control/API routes.
   - CI supply-chain gates (`cargo-audit`, `cargo-deny`) and weekly latest-deps lane.
   - Data-first channel identity newtypes (`ChannelId`, `SenderId`, `ThreadId`, `MessageId`) wired through adapters and gateway.

## Production parity blockers

1. Tooling parity and safety gaps:
   - Shell now supports sandbox/elevated mode, background lifecycle, and a `horizons_docker` sandbox backend; remaining parity work is stronger container mount/isolation policy and full background parity in docker backend:
     - `os-tools/src/shell.rs`
     - `os-app/src/config.rs`
   - Browser now has managed session/screenshot paths, but parity-grade browser-login/chrome-extension flows remain pending:
     - `os-tools/src/browser.rs`
   - `apply_patch` tool is now present, but full policy simulation/operator workflow is still pending:
     - `os-tools/src/apply_patch.rs`
2. Skills pipeline lacks trust/scanning lifecycle:
   - Baseline trust lifecycle now includes policy scans, scan history, active/quarantine state, operator approval, and rescan APIs; signature trust roots and revocation workflow remain pending:
     - `os-app/src/skills_runtime.rs`
     - `os-app/src/routes/skills.rs`
3. Channel/plugin breadth incomplete vs baseline:
   - Plugin loading registry and capability schema are now in app runtime, but external plugin package loading and expanded channel set remain pending:
     - `os-app/src/channel_plugins.rs`
     - `os-app/src/server.rs`
4. Automation layer missing at app level (cron/webhook/heartbeat orchestration):
   - Baseline heartbeat scheduler + webhook/status API + poll ingress + event publishing are now present; adapterized poll collectors and richer hook orchestration remain pending:
     - `os-app/src/automation_runtime.rs`
     - `os-app/src/routes/automation.rs`
5. Model failover and auth-profile rotation not implemented:
   - Deterministic model chain + auth-profile rotation + cooldown ladders are now wired; session-level model pinning/overrides still pending:
     - `os-app/src/config.rs`
     - `os-app/src/server.rs`
     - `os-app/src/assistant.rs`
6. Operator surface parity incomplete (CLI breadth + minimal operator UI):
   - `os-app/src/main.rs:35`
   - `web/src/App.tsx:95`
7. Pairing lifecycle is still allowlist-centric rather than explicit approval lifecycle:
   - `os-app/src/pairing.rs:7`
   - `(openclaw)/docs/channels/pairing.md:12`
8. Horizons pin drift:
   - `Cargo.lock:2208`
   - `.cargo/config.toml.example:1`

Security/runtime hardening progress:

1. Mutating `/api/v1/os/*` routes now use centralized auth policy middleware:
   - `os-app/src/http_auth.rs`
   - `os-app/src/server.rs`
2. Bearer token control-plane policy added:
   - `os-app/src/config.rs` (`security.control_api_key`)
3. Runtime bind policy data model added (`loopback`, `lan`, `tailnet`, `auto`, `custom`):
   - `os-app/src/config.rs`
   - `os-app/src/server.rs`
   - Expanded bind matrix now includes `tailnet` and `auto` modes with conservative loopback defaults.
4. Tool-policy data model foundations added (`allow`/`deny` with deny precedence):
   - `os-app/src/config.rs`
   - `os-app/src/server.rs`
5. Tool profile + execution policy slice landed:
  - `tools.profile` (`minimal/coding/messaging/full`)
  - `tools.shell_policy` (`default_mode`, `allow_elevated`, `sandbox_backend`, `sandbox_root`, `sandbox_image`, `max_background_processes`)
  - `os-app/src/config.rs`
  - `os-app/src/server.rs`

## Definition of Done

Milestone M1 (Core parity) is done when all are true:

1. Build/test/lint are green in CI for workspace targets.
2. Security posture is production-default safe for control-plane and tool execution.
3. Feature parity acceptance suite demonstrates pass coverage against the required OpenClaw inventory set.
4. Documentation and runbooks are sufficient for repeatable deployment and operation.
5. Core operator workflows are complete via CLI/API + essential web controls.
6. No unresolved P0/P1 blockers in M1 scope.
7. Tier certifications complete in order: T1 (Telegram) -> T2 (Gmail/Email) -> T3 -> T4.

Milestone M2 (Full product parity) extends M1 with native apps and polished dashboard/TUI parity.

## Program Structure

Use workstreams that can run in parallel but converge through strict phase gates.

## Phase Gate 0: Repo Recovery and Quality Baseline

Goal: restore reliable engineering velocity.

Deliverables:

1. Fix compile/test blockers and clippy denies.
2. Add CI-required checks and fail-fast policy.
3. Freeze baseline artifacts: parity matrix, known gaps, risk register.

Exit criteria:

1. `cargo check --workspace --all-targets --locked` passes.
2. `cargo test --workspace --all-targets --locked` passes.
3. `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` passes.

## Phase Gate 1: Security and Runtime Boundary Hardening

Goal: move from dev-safe assumptions to production-safe defaults.

Deliverables:

1. Authn/authz middleware for all mutating `/api/v1/os/*` routes.
2. Bind-mode matrix and network policy (`loopback`, `lan`, `tailnet`, `custom`) with explicit secure defaults.
3. Approval UX and audit trail for risky tool calls.
4. Security runbook and incident response flow.

Exit criteria:

1. All mutating control-plane calls require verified identity or explicit policy override.
2. Default startup mode prevents accidental broad exposure.
3. Audit records capture actor, action, resource, and outcome.

## Phase Gate 2: Tool Runtime Parity and Safety Model

Goal: match OpenClaw-grade operational tooling with explicit risk controls.

Deliverables:

1. Replace placeholder browser tool with fully functional managed-browser path.
2. Add structured patch/edit flow parity (`apply_patch`-style capability where applicable).
3. Introduce tool profiles, allow/deny precedence, and per-provider policy.
4. Split sandbox vs elevated execution with operator-visible decisioning.
5. Add background process management semantics.

Exit criteria:

1. Tool capabilities and safety controls match required parity profile.
2. Tool policy behavior is deterministic and test-covered.

## Phase Gate 3: Channel and Plugin Platform Expansion

Goal: move beyond fixed adapters to scalable channel surface.

Deliverables:

1. Plugin channel loading framework and configuration contract.
2. Tiered channel hardening sequence:
   - T1 Telegram
   - T2 Gmail/Email
   - T3 Slack/Discord/Matrix/WhatsApp/Signal
   - T4 plugin long-tail channels
3. Channel capability schema (typing, streaming, attachments, reactions, retry controls).
4. Pairing ownership/approval lifecycle beyond static allowlist.

Exit criteria:

1. Channel matrix reaches agreed parity target.
2. New channel onboarding no longer requires core architectural surgery.

## Phase Gate 4: Automation and Event-Driven Workflows

Goal: deliver cron/heartbeat/webhook/poll/hook capabilities on Horizons primitives.

Deliverables:

1. First-class automation scheduler and job lifecycle.
2. Webhook ingress + validation + routing.
3. Poll adapters and hook contracts.
4. Heartbeat semantics and monitoring (`HEARTBEAT_OK`-style contract).
5. Safe execution boundaries for automated actions.

Exit criteria:

1. Reliable scheduled and event-driven workflows run in production mode.
2. Automation failures are visible, retriable, and auditable.

## Phase Gate 5: Skills System and Trust Pipeline

Goal: production-grade skill lifecycle with security controls.

Deliverables:

1. Real skill packaging/metadata validation pipeline.
2. Deterministic artifact identity and policy evaluation.
3. Scanning hooks and publish outcomes (`approve`, `warn`, `block` semantics).
4. Versioning, revocation, and continuous rescanning workflow.

Exit criteria:

1. Skill install/publish path is policy-enforced and auditable.
2. Untrusted artifacts cannot silently enter runtime.

## Phase Gate 6: Model Control Plane and Failover

Goal: robust provider/model operations in production.

Deliverables:

1. Provider registry expansion and normalized config surface.
2. Model fallback chains with cooldown ladders.
3. Auth profile rotation before model failover where configured.
4. Session/agent-level model override controls with guardrails.

Exit criteria:

1. Provider failures degrade gracefully by policy.
2. Failover behavior is reproducible and tested.

## Phase Gate 7: Operator Surfaces (CLI-First) and Product Readiness

Goal: make OpenCraw an operational showcase, not just a runtime.

Deliverables:

1. Expanded CLI domains for operations and diagnostics.
2. Essential web control surface for auth, status, approvals, and diagnostics.
3. Deployment, backup, and restore runbooks.
4. Operational observability (health, traces, usage, error budgets).

Exit criteria:

1. Operator can manage OpenCraw end-to-end without code changes.
2. Incident diagnosis is possible from shipped tooling.

## Phase Gate 8: Parity Certification and Launch Readiness

Goal: lock parity claims to executable evidence.

Deliverables:

1. Feature-by-feature parity acceptance suite linked to inventory.
2. Regression suite for queue/session/tool/auth/security invariants.
3. Load and resilience test outcomes against SLOs.
4. Final launch checklist and risk sign-off.

Exit criteria:

1. Parity acceptance suite pass rate meets threshold.
2. No unresolved P0/P1 blockers.

## Phase Gate 9: Native Apps and UX Parity (Deferred)

Goal: match OpenClaw native app and rich UX surfaces after core runtime parity ships.

Deliverables:

1. macOS/iOS/Android app parity backlog (core user journeys + reliability).
2. Voice Wake, Talk Mode, and Canvas-first UX parity slices.
3. Dashboard/TUI product polish and non-essential UX affordances.

Exit criteria:

1. Native app journeys and voice/canvas workflows pass parity acceptance.
2. UI/UX parity does not regress M1 core runtime guarantees.

## Workstream Backlog (Centralized)

Use this table as the living tracker for the long run.

| ID | Workstream | Priority | Status | Phase Gate | Owner | Notes |
|---|---|---|---|---|---|---|
| WS-01 | Build/test/clippy recovery | P0 | Completed | 0 | Codex | Workspace check/test/clippy now green |
| WS-02 | Control-plane auth hardening | P0 | In Progress | 1 | Codex | Mutating-route auth middleware + rotating scoped token pool + route-scope enforcement + config redaction landed |
| WS-03 | Bind/discovery policy matrix | P0 | In Progress | 1 | Codex | Added typed runtime network policy + auth/discovery coupling enforcement + network/discovery introspection endpoints + active mDNS/tailnet probe runtime |
| WS-04 | Tool profile + sandbox/elevated split | P0 | In Progress | 2 | Codex | Profiles + shell execution policy + background lifecycle landed; `horizons_docker` sandbox backend added, remaining work is stronger mount/isolation policy parity |
| WS-05 | Browser/apply_patch parity | P0 | In Progress | 2 | Codex | Managed browser sessions + screenshot path + apply_patch tool landed; browser `find`, `extract_links`, and `query_selector_all` actions now wired; deeper interactive DOM parity still pending |
| WS-06 | Plugin channel architecture | P0 | In Progress | 3 | Codex | Plugin registry + capability schema loader + dynamic external HTTP plugin loading landed; signed package attestation pipeline still pending |
| WS-07 | Priority channel expansion | P0 | In Progress | 3 | Codex | Tiered rollout: T1 Telegram hardening -> T2 Gmail/Email hardening -> T3 Slack/Discord/Matrix/WhatsApp/Signal -> T4 long-tail plugin channels |
| WS-08 | Pairing lifecycle v2 | P0 | Planned | 3 | TBD | Ownership, approval, revocation; align to OpenClaw explicit pairing model |
| WS-09 | Automation scheduler and webhooks | P0 | In Progress | 4 | Codex | Added poll ingest contract + source interval gating + metrics + Horizons event publishing + event-id replay dedupe + rich ingest envelope contract; adapterized poll collectors remain pending |
| WS-10 | Skills trust pipeline | P0 | In Progress | 5 | Codex | Added scan history + active/quarantine lifecycle + operator approve/rescan/revoke APIs + digest/trusted-root policy controls; trust-key distribution/attestation pipeline still pending |
| WS-11 | Model failover and auth rotation | P0 | In Progress | 6 | Codex | Deterministic model+key failover + cooldown ladders landed; session-level strict/prefer model pinning controls now wired, agent-level override plane still pending |
| WS-12 | CLI surface parity expansion | P0 | Planned | 7 | TBD | Operational command groups (primary operator path for M1) |
| WS-13 | Control UI/dashboard/TUI parity | P2 | Deferred | 9 | TBD | UI polish deferred until after core runtime parity |
| WS-14 | Horizons alignment/pin strategy | P1 | Planned | 0-8 | TBD | Choose and lock baseline |
| WS-15 | Parity acceptance suite | P0 | Planned | 8 | TBD | Executable parity evidence + per-tier certification gates |
| WS-16 | Native apps + voice/canvas parity | P2 | Deferred | 9 | TBD | macOS/iOS/Android + Voice Wake/Talk/Canvas parity |

## Next Execution Window (Core-First)

1. Tier T1 certification: Telegram rock-solid.
   - Complete WS-08 pairing v2 requirements used by Telegram default DM policy.
   - Add Telegram-specific reliability checks (ordering, dedupe, retries, group mention routing) into WS-15.
2. Tier T2 certification: Gmail/Email rock-solid.
   - Close WS-09 poll/webhook adapterization for email/Gmail trigger paths.
   - Add end-to-end Gmail/Email acceptance coverage (ingest -> routing -> delivery -> observability) in WS-15.
3. Tier T3 expansion: Slack/Discord/Matrix/WhatsApp/Signal hardening after T1/T2 pass.
4. Finish WS-12 CLI parity tranche for channel diagnostics and tier certification workflows.
5. Close WS-10 trust-key distribution and artifact attestation pipeline.
6. Keep WS-13 and WS-16 deferred except for blocking operator essentials.

## Required Engineering Discipline

Use these as non-negotiable constraints during execution:

1. Every claim in review/planning docs cites code with file:line.
2. No "done" state without passing tests and quality gates.
3. Prefer root-cause refactors over compatibility shims where internal call sites can be fixed directly.
4. Avoid demo-only shortcuts that compromise production behavior.
5. Keep this file updated as the canonical status artifact.

## Update Procedure for This File

When work progresses, update:

1. `Checklist Dashboard (At-a-Glance)` checkbox states.
2. `Current State Snapshot`
3. `Workstream Backlog` status/owner/notes
4. `First 2-Week Execution Plan` (rolling)
5. Add dated entries below.

## Execution Log

### 2026-02-09

- Initial production parity master plan created.
- Baseline blockers and phase-gate structure captured from current audit evidence.
- WS-01 completed:
  - Fixed assistant/gateway call-shape drift and test config drift.
  - Cleared strict clippy-denied findings across workspace crates.
  - Verified green gates for check/test/clippy on workspace targets.
- WS-02 in progress:
  - Added centralized mutating-route auth policy data structure and middleware.
  - Added `security.control_api_key` config + redaction + env override + prod validation.
- WS-02 advanced:
  - Added explicit mutating-auth exemption policy data model (`security.mutating_auth_exempt_prefixes`) with defaults for automation ingest routes.
  - Updated mutating auth middleware to bypass bearer-token checks only for configured exempt prefixes (while preserving route-level webhook-secret enforcement).
  - Added middleware unit test for exemption prefix matching (`mutating_path_exempt_prefix_matches_webhook_and_poll`).
- WS-03 in progress:
  - Added runtime bind policy data structure (`bind_mode`, `bind_addr`) and server wiring.
  - Added `tailnet` and `auto` bind-mode values with secure loopback defaults.
- WS-03 advanced:
  - Added typed runtime network policy model (`RuntimeNetworkPolicy`, `RuntimeExposure`, `DiscoveryMode`) and centralized resolution in config.
  - Added bind/auth/discovery coupling enforcement:
    - public bind (`lan` or non-loopback `custom`) now requires `security.control_api_key` unless `runtime.allow_public_bind_without_auth=true`.
    - `runtime.discovery_mode=mdns` requires public bind target.
    - `runtime.discovery_mode=tailnet_serve|tailnet_funnel` requires `runtime.bind_mode=tailnet|auto`.
    - `runtime.discovery_mode=tailnet_funnel` requires `security.control_api_key`.
  - Added runtime network policy control-plane endpoint:
    - `GET /api/v1/os/config/network`
  - Updated startup binding path to use resolved network policy rather than ad-hoc bind-mode branching.
  - Added unit tests for public-bind auth gating and discovery-mode coupling.
- WS-04 started:
  - Added centralized tool allow/deny policy data structure and deny precedence wiring for server tool registration.
- WS-04 advanced:
  - Added tool profiles (`minimal`, `coding`, `messaging`, `full`) and profile-aware enablement.
  - Added shell execution policy data structure (`tools.shell_policy`) and env overrides.
  - Refactored `shell_execute` tool for sandbox/elevated mode split and background process lifecycle (`start/list/poll/stop`).
  - Updated approval/risk mapping in assistant for elevated/background shell semantics.
  - Added `tools.shell_policy.sandbox_backend` (`host_constrained`, `horizons_docker`) and optional `sandbox_image`.
  - Wired sandbox execution path through Horizons local Docker adapter for foreground shell commands.
  - Preserved explicit guardrails: elevated remains policy-gated, and docker sandbox backend currently blocks background process mode.
- WS-06 started:
  - Added channel plugin registry module with typed plugin IDs and capability schema matrix.
  - Refactored server channel startup to plugin-registry loading flow (decoupling startup from hardcoded per-channel wiring).
  - Added plugin router merge path so channel-specific HTTP routes (e.g., webchat websocket) load through the same registry surface.
  - Extended `/api/v1/os/channels` response with per-channel capability schema for operator inspection.
- WS-09 started:
  - Added app-level automation runtime with heartbeat scheduler and in-memory status model.
  - Added automation API surface:
    - `GET /api/v1/os/automation/status`
    - `POST /api/v1/os/automation/webhook/{source}`
  - Added automation config surface and env overrides (`automation.enabled`, heartbeat interval, webhook secret).
  - Added config redaction for automation webhook secret in config API responses.
- WS-05 started:
  - Replaced browser placeholder with managed session functionality (`navigate`, `extract_text`, `list_sessions`, `close_session`) and screenshot path via Playwright.
  - Added first-class `apply_patch` tool with structured patch-path safety validation.
  - Wired new tools through server registration + config + policy surface.
- WS-11 started:
  - Added model fallback chain config (`general.fallback_models`) and provider key-rotation lists (`keys.openai_api_keys`, `keys.anthropic_api_keys`).
  - Added deterministic LLM profile chain assembly in server startup (model + auth profile ordering).
  - Added assistant runtime failover behavior: on LLM failure, retry next configured profile and continue tool loop.
  - Added cooldown ladder controls (`general.failover_cooldown_base_seconds`, `general.failover_cooldown_max_seconds`) and profile state tracking to avoid immediate hot-loop retries of failing profiles.
- WS-11 advanced:
  - Added session-level model pinning mode with explicit data model:
    - `model_pinning = prefer | strict` on session state.
  - Updated assistant failover ordering to honor pinning mode:
    - `prefer`: keep current behavior (preferred model first, then full fallback chain).
    - `strict`: only attempt profiles matching `model_override`; fail fast if unavailable.
  - Extended sessions control-plane model route to accept pinning mode updates and return persisted model selection state.
  - Added assistant unit tests for strict pinning ordering and strict-missing-model behavior.
- Validation:
  - `cargo check --workspace --all-targets --locked` passes.
  - `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` passes.
  - `cargo test --workspace --all-targets --locked` passes.

### 2026-02-10

- Plan re-baselined against local OpenClaw repository snapshot (`(openclaw)` repo) rather than only web inventory artifacts.
- Scope decision recorded: Milestone M1 is core runtime parity first; native apps and UI polish moved to deferred Gate 9.
- Master plan execution model changed to tiered channel certification:
  - T1 Telegram first
  - T2 Gmail/Email second
  - T3 core channel expansion third
  - T4 long-tail plugin channels last
- Workstream priorities updated to reflect core-first execution:
  - WS-08 elevated to P0 (pairing lifecycle).
  - WS-12 elevated to P0 (CLI as primary operator interface).
  - WS-13/WS-16 marked deferred (UI polish and native app parity).
- WS-02 advanced (scoped rotating token tranche):
  - Added normalized rotating control-plane token pool projection (`control_api_key_pool`) over:
    - legacy `security.control_api_key`
    - new `[[security.control_api_keys]]`
  - Added dedupe/validation coupling so duplicate token values across legacy + rotating fields are rejected.
  - Updated runtime public-bind/tailnet-funnel auth checks to treat either legacy or rotating tokens as valid control-plane auth configuration.
  - Refactored mutating auth middleware to enforce per-route scopes for rotating tokens:
    - `config:write`, `sessions:write`, `automation:write`, `skills:write`, `messages:write`, `channels:write`
    - `control:write` and `*` grant broad access
    - empty scope list remains full mutating access for compatibility
  - Preserved configured mutating-path exemptions for machine-ingest routes (`/automation/webhook/*`, `/automation/poll/*`).
  - Added config API redaction for `security.control_api_keys[*].token`.
  - Added route-level redaction regression test to prevent control token leakage in config API snapshots.
  - Expanded config API secret redaction to channel provider credentials:
    - `channels.telegram.bot_token`
    - `channels.discord.bot_token`
    - `channels.slack.bot_token`
    - `channels.matrix.access_token`
    - `channels.email.gmail_access_token`
    - `channels.linear.api_key`
  - Updated `config-templates/config.toml` with rotating-token docs and scope catalog.
  - Added/updated tests for:
    - duplicate legacy+rotating token rejection
    - rotating-token satisfaction of runtime public-bind auth requirements
    - route-prefix scope matrix and token scope grant behavior in mutating auth middleware
- WS-03 advanced (active discovery runtime tranche):
  - Added discovery runtime data model and lifecycle manager (`DiscoveryRuntime`, `DiscoveryStatus`) with deterministic snapshot API.
  - Added active discovery probes by mode:
    - `mdns`: periodic mDNS PTR query probe for `_opencraw._tcp.local`
    - `tailnet_serve|tailnet_funnel`: periodic `tailscale status --json` probe + parsed health snapshot
  - Wired discovery runtime startup/shutdown into server lifecycle so discovery state is managed with process cancellation.
  - Added control-plane discovery status endpoint:
    - `GET /api/v1/os/config/discovery`
  - Updated runtime config example docs to reflect active probe behavior.
  - Added discovery runtime unit test for mDNS packet encoding invariant.
- WS-05 advanced (browser extraction tranche):
  - Expanded browser tool action surface with bounded session-aware operators:
    - `find` (pattern search with case sensitivity and bounded match count)
    - `extract_links` (normalized/deduped link extraction with bounded output)
    - `query_selector_all` (simple selector-based structured extraction with attribute capture)
  - Updated browser tool schema and dispatch routing for new actions.
  - Added browser tool unit tests for link normalization/deduplication, bounded pattern matches, and selector parsing/query extraction behavior.
- WS-07 started (Slack baseline tranche):
  - Added first-class Slack channel adapter with:
    - outbound send (`chat.postMessage`)
    - inbound poll loop (`conversations.history`) over configured channel IDs
    - bounded per-channel timestamp cursor model (replay-safe, no unbounded seen-set growth)
  - Added Slack config model + validation + env overrides:
    - `[channels.slack]` (`enabled`, `bot_token`, `poll_interval_ms`, `channel_ids`, `start_from_latest`)
    - `SLACK_BOT_TOKEN`
    - `OPENSHELL_SLACK_POLL_INTERVAL_MS`
    - `OPENSHELL_SLACK_CHANNEL_IDS`
    - `OPENSHELL_SLACK_START_FROM_LATEST`
  - Wired Slack into:
    - channel plugin registry loading path
    - one-shot send CLI path (`send --channel slack ...`)
    - startup configuration logging and example config docs
  - Added Slack adapter unit tests for timestamp ordering and cursor/subtype emission rules.
- WS-07 advanced (Matrix baseline tranche):
  - Added first-class Matrix channel adapter with:
    - outbound send (`/_matrix/client/v3/rooms/{roomId}/send/m.room.message/{txnId}`)
    - inbound incremental sync (`/_matrix/client/v3/sync`) with `next_batch` cursor model
    - message + reaction ingest mapping to typed OpenCraw inbound events
  - Added Matrix config model + validation + env overrides:
    - `[channels.matrix]` (`enabled`, `homeserver_url`, `access_token`, `user_id`, `poll_interval_ms`, `room_ids`, `start_from_latest`)
    - `MATRIX_HOMESERVER_URL`
    - `MATRIX_ACCESS_TOKEN`
    - `MATRIX_USER_ID`
    - `OPENSHELL_MATRIX_POLL_INTERVAL_MS`
    - `OPENSHELL_MATRIX_ROOM_IDS`
    - `OPENSHELL_MATRIX_START_FROM_LATEST`
  - Wired Matrix into:
    - channel plugin registry loading path
    - one-shot send CLI path (`send --channel matrix ...`)
    - startup configuration logging and example config docs
  - Added Matrix adapter unit tests for homeserver URL policy, message/reaction extraction, and self-event filtering.
- WS-07 advanced (WhatsApp baseline tranche):
  - Added first-class WhatsApp Cloud adapter and ingress surface with:
    - outbound send (`https://graph.facebook.com/v20.0/{phone_number_id}/messages`)
    - webhook verification route (`GET /api/v1/os/channels/whatsapp/webhook`)
    - webhook ingest route (`POST /api/v1/os/channels/whatsapp/webhook`)
    - optional `x-hub-signature-256` validation via configured app secret
  - Added WhatsApp config model + validation + env overrides:
    - `[channels.whatsapp]` (`enabled`, `access_token`, `phone_number_id`, `webhook_verify_token`, `app_secret`)
    - `WHATSAPP_ACCESS_TOKEN`
    - `WHATSAPP_PHONE_NUMBER_ID`
    - `WHATSAPP_WEBHOOK_VERIFY_TOKEN`
    - `WHATSAPP_APP_SECRET`
  - Wired WhatsApp into:
    - channel plugin registry loading path
    - one-shot send CLI path (`send --channel whatsapp ...`)
    - startup configuration logging and example config docs
  - Added webhook/unit tests for conversion + signature verification.
- WS-07 advanced (Signal baseline tranche):
  - Added first-class Signal adapter with:
    - outbound send (`POST /v2/send`) with direct-recipient and `group:<group_id>` addressing
    - inbound poll loop (`GET /v1/receive/{account}`) with timestamp cursoring and replay-safe startup seeding
    - message + reaction mapping to typed OpenCraw inbound events
  - Added Signal config model + validation + env overrides:
    - `[channels.signal]` (`enabled`, `api_base_url`, `account`, `api_token`, `poll_interval_ms`, `start_from_latest`, `receive_timeout_seconds`)
    - `SIGNAL_API_BASE_URL`
    - `SIGNAL_ACCOUNT`
    - `SIGNAL_API_TOKEN`
    - `OPENSHELL_SIGNAL_POLL_INTERVAL_MS`
    - `OPENSHELL_SIGNAL_START_FROM_LATEST`
    - `OPENSHELL_SIGNAL_RECEIVE_TIMEOUT_SECONDS`
  - Wired Signal into:
    - channel plugin registry loading path
    - one-shot send CLI path (`send --channel signal ...`)
    - startup configuration logging and example config docs
  - Added config redaction for `channels.signal.api_token`.
  - Added Signal adapter unit tests for URL policy, payload extraction variants, timestamp precedence, and message/reaction conversion.
- WS-06 advanced (dynamic external plugin tranche):
  - Added generic external HTTP channel adapter (`HttpPluginAdapter`) in `os-channels` with:
    - outbound send contract (`send_url`) mapped to OpenCraw outbound message shape
    - optional inbound poll contract (`poll_url`) with replay-safe cursoring + bounded event-id dedupe
    - capability toggles (`supports_streaming_deltas`, `supports_typing_events`, `supports_reactions`)
  - Added first-class external plugin config model:
    - `[[channels.external_plugins]]` entries with `id`, `enabled`, `send_url`, `poll_url`, `auth_token`, polling controls, and capability flags
  - Wired dynamic external plugin loading into channel registry startup:
    - enabled entries are loaded without core code changes
    - plugin IDs are validated/normalized and surfaced through `/api/v1/os/channels` capability matrix
  - Extended one-shot CLI send routing so `opencraw send` can target enabled external plugin channels.
  - Added config validation invariants for external plugins:
    - unique IDs, built-in ID conflict prevention, URL scheme policy (`http|https`), non-empty secrets/intervals
  - Added config redaction for `channels.external_plugins[*].auth_token`.
  - Added unit tests for:
    - external plugin config validation policy
    - HTTP plugin ID/URL contracts, payload extraction variants, and message/reaction normalization
- Validation:
  - `cargo check --workspace --all-targets --locked` passes.
  - `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` passes.
  - `cargo test --workspace --all-targets --locked` passes.
  - `bun run build` passes in `web`.
- Rust foundation conformance audit completed against `standards/rust.md`:
  - Added `(historical) plans/18-rust-foundation-conformance-audit.md`.
  - Identified P0 gaps in toolchain policy, graceful shutdown/cancellation model, HTTP guardrails, and CI supply-chain gates.
- Foundation remediation tranche executed:
  - Upgraded workspace policy to Rust 2024 + resolver v3 + explicit MSRV and propagated `rust-version` to all workspace crates.
  - Added graceful shutdown signal handling and cancellation-driven task drain across server, gateway, and automation loops.
  - Added HTTP guardrails (`TimeoutLayer`, `GlobalConcurrencyLimitLayer`) with explicit runtime config controls.
  - Added CI security/supply-chain lanes and policy artifacts (`cargo-audit`, `cargo-deny`, weekly latest-deps job, `FOUNDATION_GATES.md`).
  - Removed production-path panic assumptions in config/tool definition/browser construction paths by converting to fallible constructors and propagated `Result` errors.
- Data-first modeling tranche executed:
  - Added typed channel identity primitives in `os-channels` (`ChannelId`, `SenderId`, `ThreadId`, `MessageId`) and migrated inbound/outbound message contracts.
  - Updated channel adapters and gateway orchestration to use typed identities while preserving interoperability at adapter boundaries.
  - Replaced raw tuple session keys with typed `SessionScope` structure in session persistence/runtime indexing.
  - Added strict boundary validation for `/api/v1/os/messages/send` (required channel/recipient/message) before adapter dispatch.
- WS-09 advanced:
  - Added first-class poll ingest API contract:
    - `POST /api/v1/os/automation/poll/{source}`
  - Added runtime poll receipt/metrics tracking (`poll_events`, `last_poll_*`) in automation status.
  - Added poll interval gating on source-triggered poll jobs and shared ingest-secret validation for webhook/poll ingress.
  - Narrowed scheduler-trigger set so poll jobs are event-driven by poll ingress rather than background tick execution.
  - Added Horizons event-bus coupling for automation ingress + job execution lifecycle topics (`os.automation.*`).
  - Added automation runtime tests for poll interval gating and ingest secret enforcement.
- WS-09 advanced (idempotency tranche):
  - Added event-id replay protection for ingress:
    - `x-opencraw-event-id` header is accepted on webhook and poll ingest routes.
    - Ingest events are persisted in `opencraw_automation_ingest_events` with uniqueness on `(ingest_kind, source, event_id)`.
    - Duplicate ingress events are accepted but do not re-execute jobs.
  - Extended automation status and receipts with duplicate-ingest telemetry:
    - `webhook_duplicate_events`, `poll_duplicate_events`
    - receipt fields `event_id` and `duplicate_event`
  - Added automation runtime test covering replay dedupe behavior (`webhook_ingest_dedupes_replayed_event_id`).
  - Spec alignment note:
    - Directly maps to `standards/foundation-gates.md` event invariants (`mutating commands idempotent`, `side effects replay-safe`).
- WS-09 advanced (contract tranche):
  - Added rich ingest envelope contract support for webhook/poll endpoints with backward compatibility for raw JSON payloads.
  - New envelope schema key:
    - `schema = "opencraw_ingest_envelope_v1"`
  - Envelope fields supported:
    - `event_id` (body-level idempotency key)
    - `occurred_at` (event timestamp)
    - `metadata` (provider/source metadata object)
    - `payload` (actual event payload)
  - Header/body `event_id` mismatch is explicitly rejected to prevent ambiguous dedupe semantics.
  - Runtime receipts now include `occurred_at` + `metadata`, and event-bus payload includes these fields for downstream consumers.
- WS-10 started:
  - Added skills trust lifecycle fields (`active`, `approved_by_operator`, `scan_count`, `last_scan_at`) and persisted scan records.
  - Added scan history persistence table (`opencraw_skill_scans`) with index for skill/time retrieval.
  - Added operator lifecycle methods in runtime:
    - `approve(skill_id, note)`
    - `rescan(skill_id)`
    - `list_scans(skill_id, limit)`
  - Added skills control-plane API expansion:
    - `GET /api/v1/os/skills/{skill_id}`
    - `POST /api/v1/os/skills/{skill_id}/approve`
    - `POST /api/v1/os/skills/{skill_id}/rescan`
    - `GET /api/v1/os/skills/{skill_id}/scans`
  - Added skills runtime tests for warn-approval activation, blocked-approval rejection, and scan persistence.
  - Added deterministic signature check rule:
    - `signature = "sha256:<digest>"` is verified against computed artifact digest.
    - Digest mismatch is policy-blocked with explicit reason.
- WS-10 advanced:
  - Added operator revocation lifecycle:
    - Runtime method: `revoke(skill_id, note)`
    - Control-plane API: `POST /api/v1/os/skills/{skill_id}/revoke`
    - Audit scan event: `operator.revoke`
  - Added first-class skills trust policy data model in config (`[skills]`):
    - `require_source_provenance`
    - `require_https_source`
    - `require_trusted_source`
    - `trusted_source_prefixes`
    - `require_sha256_signature`
  - Wired skills trust policy into runtime evaluation so install/rescan decisions use configured provenance/trust-root/signature constraints.
  - Added policy tests for trust-root blocking and required `sha256:<digest>` signature format enforcement.
- Validation:
  - `cargo check --workspace --all-targets --locked` passes.
  - `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` passes.
  - `cargo test --workspace --all-targets --locked` passes.
