# Target Architecture: Full OpenClaw on Horizons

Generated: 2026-02-07

## Goal

Build OpenCraw as a product gateway layer on top of Horizons so that:

1. OpenCraw reaches OpenClaw operational parity for messaging-agent behavior.
2. Horizons capabilities (evaluation, optimization, graph, pipelines, context refresh, sandbox runtime) are first-class differentiators, not side experiments.

## Architecture Principles

1. Keep OpenCraw thin: channel + session + tool orchestration + UX behavior.
2. Delegate infrastructure to Horizons: durability, approvals, memory/eval/opt, workflows, eventing.
3. Make parity deterministic first (queue/session/memory/tool profiles), then optimize.
4. Keep every side effect idempotent and auditable.

## Proposed Runtime Topology

## OpenCraw App (product runtime)

Responsibilities:

- Channel adapters and channel policy behavior
- Session key resolution and identity linking
- Message ingress and per-session queueing
- Assistant loop orchestration (streaming, tool invocation, retries)
- OpenCraw-specific APIs/UI

## Horizons Platform (backplane)

Responsibilities:

- CoreAgents policy and approvals
- Project/central storage abstractions
- Voyager memory APIs
- Pipelines and graph execution
- Evaluation and optimization cycles
- Context refresh (connectors/polls/webhooks)
- Event bus, audit, optional sandbox engine/MCP gateway

## Data and Control Planes

## Data plane (messages and responses)

1. Channel adapter normalizes inbound message.
2. Session resolver computes canonical `SessionKey`.
3. Message queue enqueues by lane/session.
4. Worker executes assistant loop for that session.
5. Response streams in chunks to channel adapter.
6. Artifacts/events persisted via Horizons routes.

## Control plane (policy and automation)

- Approvals: CoreAgents policies/action proposals.
- Schedules: Horizons scheduler + cron abstraction.
- External triggers: `/events/inbound`, context refresh triggers.
- Optimization loop: evaluation reports feed optimization cycles.

## Gateway Control-Plane Compatibility Layer

OpenCraw needs an explicit control-plane compatibility module so parity is not only behavioral but operational:

- Config lifecycle APIs and semantics:
  - `config.get`
  - `config.apply` (full apply with validation and dry-run mode)
  - `config.patch` with optimistic `baseHash` protection
- Bind and exposure modes:
  - `loopback`, `lan`, `tailnet`, `auto`, `custom`
  - auth/discovery coupling rules enforced in one policy layer
- Discovery and remote access:
  - mDNS/Bonjour toggles
  - tailnet serve/funnel mode handling
- Idempotent operator actions:
  - every control-plane write has request IDs and repeat-safe behavior

## Core Domain Model Changes for OpenCraw

## Session identity model

Replace `(channel_id, sender_id)` with:

- `IdentityId` (cross-channel person identity)
- `ConversationScope` (`dm`, `group`, `topic`, `cron`, `webhook`)
- `SessionKey` (`identity + scope + agent binding`)
- `SessionState` persisted in ProjectDb

## Queue model

- Queue lanes per `SessionKey`
- Global worker budget + per-lane serialization
- Queue modes: `collect`, `followup`, `steer`, `interrupt`
- Debounce windows and overflow strategy

## Tool policy model

- `ToolProfile` (`minimal`, `coding`, `messaging`, `full`)
- Tool groups + allow/deny lists (`deny` precedence)
- Elevated mode gate by channel/identity policy

## Bootstrap and Prompt Assembly model

- Bootstrap sources loaded in deterministic order:
  - `AGENTS.md`, `SOUL.md`, `TOOLS.md`, `BOOTSTRAP.md`, `IDENTITY.md`, `USER.md`, `HEARTBEAT.md`
- Prompt modes:
  - `full`, `minimal`, `none`
- Hook lifecycle support:
  - gateway startup/shutdown hooks
  - `before_tool_call` and `after_tool_call`
  - context compaction hooks
  - heartbeat hooks
- Token accounting:
  - per-component prompt budget attribution for compaction decisions

## Memory model

- Short-term: rolling conversation context
- Mid-term: daily summaries (OpenClaw-style operational memory)
- Long-term: Voyager memory store with semantic retrieval
- Pre-compaction flush hook before context trimming

## OpenCraw <-> Horizons integration contracts

## Required contracts

1. Session persistence contract
   - OpenCraw writes/reads session envelope and checkpoints in ProjectDb.
2. Memory contract
   - OpenCraw performs retrieval with typed filters and writes observations/reflections.
3. Approval contract
   - Tool and high-risk actions proposed through CoreAgents policies.
4. Event contract
   - All inbound/outbound message events published to Horizons event bus.
5. Automation contract
   - Cron/poll/webhook routes trigger OpenCraw assistant jobs through queue.
6. Config compatibility contract
   - Control-plane config operations (`get/apply/patch`) are versioned, hash-checked, and auditable.
7. Heartbeat contract
   - Heartbeat jobs and `HEARTBEAT_OK` response semantics are typed and monitorable.
8. Security and incident contract
   - Security audit actions, incident stages, and recovery events are captured with immutable audit trails.

## Recommended contracts

- Evaluation contract: reaction/outcome events produce verification cases.
- Optimization contract: scheduled cycles propose prompt/policy revisions.
- Graph contract: high-complexity tasks route to graph templates.
- Command-surface contract: slash/CLI command registry is generated from the same typed action definitions.

## Channel Layer Expansion Strategy

Prioritize channel additions by user impact and integration complexity:

1. WhatsApp
2. Slack
3. Signal
4. Matrix
5. Teams / Mattermost / Google Chat

Each adapter should support:

- DM/group policy gating
- Mention and prefix behavior
- Typing and chunked delivery
- Reaction handling
- Stable sender identity extraction for cross-channel linking

## Safety and Security Model

1. Default-deny for external channels until pairing/allow policy permits.
2. Risk-aware tool approvals via CoreAgents.
3. Optional sandbox execution path for high-risk tool groups.
4. Full audit events for all action proposals and tool executions.
5. Prompt-injection mitigation rules at tool boundary and fetch boundary.
6. Plugin trust policy with allow/deny and provenance checks.
7. Incident-response workflow (detect, contain, rotate, recover, verify, postmortem).

## Model/Auth Orchestration Details

- Provider registry includes both model aliases and auth profiles.
- Two-stage failover:
  1. rotate to next healthy auth profile for the same model/provider;
  2. then fallback through model/provider chain.
- Cooldown ladder and billing-failure guards:
  - temporary quarantine of failing auth profiles
  - explicit behavior for auth-expired vs quota-exceeded vs provider outage.
- Session pinning policy:
  - pinned sessions retain configured model/auth where possible; fallback behavior is explicit and logged.

## Ops and Command Surface

- Full slash + CLI command taxonomy mapped to typed internal actions.
- Gateway admin commands for queue/session/config inspection and controlled restart.
- Compatibility matrix for env vars and config keys published with each release.

## Differentiators (Horizons-native)

After parity baseline, activate:

- Continuous evaluation (RLM)
- Periodic prompt/policy optimization (MiPRO)
- Graph orchestration for complex workflows
- Event-driven pipelines for long-running tasks
- Context refresh connectors for proactive assistant context

## Non-goals (initial delivery)

- Skills marketplace
- Full node companion ecosystem parity is phase-gated pending product scope decision.
- Voice/TTS parity is phase-gated pending launch segment requirements.
- Custom protocol extensions not needed for first parity release.
